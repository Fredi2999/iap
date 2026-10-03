//! Web-Konnektoren neben Exa: Wikipedia, Open-Meteo und Brave Search.
//!
//! Gemeinsam ist ihnen: reine `GET`-Anfragen mit fester Host-Tabelle (`pa_policy::egress::Connector`),
//! Query durch den Client kodiert, Antwortgröße begrenzt, nichts Privates (nur `PublicText`).
//! Die Freigabe prüft der aufrufende Anschluss (`authorize_connector`); diese Module sprechen nur
//! das jeweilige Protokoll. So lassen sie sich ohne Netz mit einem Fake-Transport testen.

pub mod brave;
pub mod open_meteo;
pub mod wikipedia;

use std::time::Duration;

use pa_policy::egress::Connector;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::net::{self, Method, NetError, Request, Transport};

/// Zeitgrenze je Anfrage.
const TIMEOUT: Duration = Duration::from_secs(30);
/// Größte erlaubte Antwort.
const MAX_RESPONSE: usize = 2 * 1024 * 1024;

/// Fehler der Web-Konnektoren.
#[derive(Debug, Error)]
pub enum ConnectorError {
    #[error("{0}")]
    Net(#[from] NetError),
    /// Fehlerstatus des Dienstes, mit Hinweis für den Nutzer.
    #[error("{service}: {advice} (Status {status})")]
    Status {
        service: &'static str,
        advice: &'static str,
        status: u16,
    },
    #[error("Die Antwort von {0} ist nicht lesbar")]
    BadResponse(&'static str),
    #[error("{0}")]
    NotFound(String),
}

/// Übersetzt einen Fehlerstatus in einen Hinweis. Enthält nie Schlüssel oder Antworttext.
fn advice(status: u16) -> &'static str {
    match status {
        401 | 403 => "Der Zugang wurde abgelehnt. Prüfe den Schlüssel unter Konnektoren.",
        402 => "Das Guthaben ist aufgebraucht oder der Zugang nicht freigeschaltet.",
        404 => "Nichts gefunden.",
        429 => "Zu viele Anfragen. Warte kurz und versuche es erneut.",
        400 | 422 => "Die Anfrage wurde nicht akzeptiert.",
        500..=599 => "Der Dienst ist gerade nicht erreichbar. Versuche es später erneut.",
        _ => "Unerwarteter Status.",
    }
}

/// Eine `GET`-Anfrage mit JSON-Antwort.
pub(crate) fn get_json<T: DeserializeOwned>(
    transport: &dyn Transport,
    connector: Connector,
    service: &'static str,
    host: &str,
    path: &str,
    query: &[(&str, &str)],
    headers: &[(&str, &str)],
) -> Result<T, ConnectorError> {
    let response = net::request_with(
        transport,
        &Request {
            connector,
            host,
            method: Method::Get,
            path,
            query,
            headers,
            body: &[],
            timeout: TIMEOUT,
            max_response: MAX_RESPONSE,
        },
    )?;
    if !(200..300).contains(&response.status) {
        return Err(ConnectorError::Status {
            service,
            advice: advice(response.status),
            status: response.status,
        });
    }
    serde_json::from_slice(&response.body).map_err(|_| ConnectorError::BadResponse(service))
}

/// Kürzt auf höchstens `limit` Zeichen.
pub(crate) fn clip(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// Nur gewöhnliche Web-Adressen ohne Steuer- und Leerzeichen.
pub(crate) fn safe_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Entfernt HTML-Marken und löst die wenigen üblichen Zeichenverweise auf (Suchauszüge).
pub(crate) fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
pub(crate) mod testing {
    //! Fake-Transport für die Konnektor-Tests.
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::net::HttpsResponse;

    pub type Seen = Vec<(String, Method, String, Vec<(String, String)>)>;

    /// Beantwortet jede Anfrage der Reihe nach mit `(Status, Antwort)` und merkt sich die Anfragen.
    pub struct Fake {
        replies: Mutex<Vec<(u16, String)>>,
        pub seen: Mutex<Seen>,
    }

    impl Fake {
        pub fn new(replies: &[(u16, &str)]) -> Arc<Self> {
            Arc::new(Self {
                replies: Mutex::new(
                    replies
                        .iter()
                        .rev()
                        .map(|(s, b)| (*s, (*b).to_owned()))
                        .collect(),
                ),
                seen: Mutex::new(Vec::new()),
            })
        }
    }

    impl Transport for Fake {
        fn send(
            &self,
            host: &str,
            method: Method,
            target: &str,
            headers: &[(&str, &str)],
            _body: &[u8],
            _timeout: Duration,
            _max: usize,
        ) -> Result<HttpsResponse, NetError> {
            self.seen.lock().unwrap().push((
                host.to_owned(),
                method,
                target.to_owned(),
                headers
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
            ));
            let (status, body) = self
                .replies
                .lock()
                .unwrap()
                .pop()
                .expect("unerwartete Anfrage");
            Ok(HttpsResponse {
                status,
                body: body.into_bytes(),
            })
        }
    }

    /// Hält den globalen Air Gap für die Dauer eines Tests offen.
    pub fn open_net() -> std::sync::MutexGuard<'static, ()> {
        let guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        guard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_is_stripped_and_entities_are_resolved() {
        assert_eq!(
            strip_html(
                r#"Der <span class="searchmatch">Mond</span> &amp; die &quot;Erde&quot;&nbsp;sind"#
            ),
            r#"Der Mond & die "Erde" sind"#
        );
        assert_eq!(strip_html("a <b>b</b>\n\n c"), "a b c");
    }

    #[test]
    fn only_plain_web_urls_pass() {
        assert!(safe_url("https://de.wikipedia.org/wiki/Rom"));
        for bad in [
            "javascript:x",
            "file:///x",
            "https://a b",
            "https://a\nb",
            "",
        ] {
            assert!(!safe_url(bad), "{bad}");
        }
    }

    #[test]
    fn statuses_explain_what_to_do_and_carry_no_body() {
        assert!(advice(401).contains("Schlüssel"));
        assert!(advice(429).contains("Zu viele"));
        assert!(advice(503).contains("nicht erreichbar"));
        let error = ConnectorError::Status {
            service: "Brave",
            advice: advice(401),
            status: 401,
        };
        assert!(error.to_string().starts_with("Brave: "));
    }
}
