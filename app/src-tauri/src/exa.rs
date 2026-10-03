//! Exa-Client: baut Anfragen, liest Antworten, kennt Kosten.
//!
//! Dieses Modul spricht nur `POST /search` und `POST /contents` (Formate laut
//! Exa-API-Referenz, geprüft am 2026-09-29). Es prüft **keine** Freigaben: das
//! tut `pa_policy::egress::authorize_exa` im aufrufenden Anschluss. Der
//! Schlüssel liegt nur im Speicher dieses Objekts, wird beim Freigeben
//! überschrieben und erscheint in keiner Fehlermeldung.

use std::{sync::Arc, time::Duration};

use pa_core::workflow::{ExaContentsOutcome, ExaSearchOutcome, SearchHit};
use pa_policy::egress::{Connector, ExaEndpoint, PublicText, EXA_HOST};
use serde::Deserialize;
use thiserror::Error;
use zeroize::Zeroize;

use crate::net::{self, Method, NetError, Request, SystemTransport, Transport};

/// Ist die Kostenangabe der Antwort nicht lesbar, rechnet IAP vorsichtig
/// mit diesem Betrag, damit das Kostenlimit nie unterlaufen wird.
pub const ASSUMED_COST_USD: f64 = 0.01;

const TIMEOUT: Duration = Duration::from_secs(60);
const MAX_RESPONSE: usize = 4 * 1024 * 1024;
const MAX_HIT_TEXT: usize = 20_000;

/// Fehler des Exa-Clients.
#[derive(Debug, Error)]
pub enum ExaError {
    #[error("{0}")]
    Net(#[from] NetError),
    #[error("{}", status_text(*status, detail))]
    Status { status: u16, detail: String },
    #[error("Die Antwort von Exa ist nicht lesbar")]
    BadResponse,
}

#[derive(Debug, Deserialize, Default)]
struct Cost {
    total: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct RawHit {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    highlights: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawResponse {
    #[serde(default)]
    results: Vec<RawHit>,
    #[serde(default, rename = "costDollars")]
    cost: Option<Cost>,
}

#[derive(Debug, Deserialize)]
struct RawError {
    #[serde(default)]
    error: Option<String>,
}

/// Erklärt einen Fehlerstatus so, dass der Nutzer weiß, was zu tun ist. Der Schlüssel
/// kommt darin nie vor; `detail` ist die (gekürzte) Meldung von Exa.
fn status_text(status: u16, detail: &str) -> String {
    let advice = match status {
        401 | 403 => "Exa hat den Schlüssel abgelehnt. Prüfe ihn unter Konnektoren.",
        402 => "Das Exa-Guthaben ist aufgebraucht oder der Zugang ist nicht freigeschaltet.",
        429 => "Exa meldet zu viele Anfragen. Warte kurz und versuche es erneut.",
        400 | 422 => "Exa hat die Anfrage nicht akzeptiert.",
        500..=599 => "Exa ist gerade nicht erreichbar. Versuche es später erneut.",
        _ => "Exa hat mit einem unerwarteten Status geantwortet.",
    };
    format!("{advice} (Status {status}{detail})")
}

/// Client mit Schlüssel.
pub struct ExaClient {
    api_key: String,
    transport: Arc<dyn Transport>,
}

impl Drop for ExaClient {
    fn drop(&mut self) {
        self.api_key.zeroize();
    }
}

fn clip(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn safe_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

impl ExaClient {
    /// Neuer Client. Der Schlüssel wird getrimmt.
    pub fn new(api_key: &str) -> Self {
        Self::with_transport(api_key, Arc::new(SystemTransport))
    }

    /// Client mit eigenem Transport (Tests prüfen so die Anfrage ohne Netz).
    pub fn with_transport(api_key: &str, transport: Arc<dyn Transport>) -> Self {
        Self {
            api_key: api_key.trim().to_owned(),
            transport,
        }
    }

    fn call(
        &self,
        endpoint: ExaEndpoint,
        body: &serde_json::Value,
    ) -> Result<RawResponse, ExaError> {
        let bytes = serde_json::to_vec(body).map_err(|_| ExaError::BadResponse)?;
        let response = net::request_with(
            self.transport.as_ref(),
            &Request {
                connector: Connector::Exa,
                host: EXA_HOST,
                method: Method::Post,
                path: endpoint.path(),
                query: &[],
                headers: &[
                    ("Content-Type", "application/json"),
                    ("Accept", "application/json"),
                    ("x-api-key", &self.api_key),
                ],
                body: &bytes,
                timeout: TIMEOUT,
                max_response: MAX_RESPONSE,
            },
        )?;
        if !(200..300).contains(&response.status) {
            let detail = serde_json::from_slice::<RawError>(&response.body)
                .ok()
                .and_then(|e| e.error)
                .map(|e| format!(": {}", clip(&e, 200)))
                .unwrap_or_default();
            return Err(ExaError::Status {
                status: response.status,
                detail,
            });
        }
        serde_json::from_slice(&response.body).map_err(|_| ExaError::BadResponse)
    }

    /// `POST /search` mit Kurzauszügen (Highlights).
    ///
    /// # Errors
    /// Netz-, Status- und Formatfehler.
    pub fn search(
        &self,
        query: &PublicText,
        num_results: u32,
    ) -> Result<ExaSearchOutcome, ExaError> {
        let body = serde_json::json!({
            "query": query.as_str(),
            "numResults": num_results.clamp(1, 10),
            "type": "auto",
            "contents": { "highlights": true },
        });
        let raw = self.call(ExaEndpoint::Search, &body)?;
        let cost_usd = raw.cost.and_then(|c| c.total).unwrap_or(ASSUMED_COST_USD);
        let hits = raw
            .results
            .into_iter()
            .filter(|hit| safe_url(&hit.url))
            .map(|hit| {
                let text = if hit.highlights.is_empty() {
                    hit.text.unwrap_or_default()
                } else {
                    hit.highlights.join("\n")
                };
                SearchHit {
                    title: clip(hit.title.as_deref().unwrap_or(""), 300),
                    url: hit.url,
                    text: clip(&text, MAX_HIT_TEXT),
                }
            })
            .collect();
        Ok(ExaSearchOutcome { hits, cost_usd })
    }

    /// `POST /contents` für bereits gefundene Adressen.
    ///
    /// # Errors
    /// Netz-, Status- und Formatfehler.
    pub fn contents(
        &self,
        urls: &[String],
        max_characters: u32,
    ) -> Result<ExaContentsOutcome, ExaError> {
        let body = serde_json::json!({
            "urls": urls,
            "text": { "maxCharacters": max_characters },
        });
        let raw = self.call(ExaEndpoint::Contents, &body)?;
        let cost_usd = raw.cost.and_then(|c| c.total).unwrap_or(ASSUMED_COST_USD);
        let texts = raw
            .results
            .into_iter()
            .filter(|hit| safe_url(&hit.url))
            .map(|hit| (hit.url, clip(&hit.text.unwrap_or_default(), MAX_HIT_TEXT)))
            .collect();
        Ok(ExaContentsOutcome { texts, cost_usd })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_http_urls_are_accepted() {
        assert!(safe_url("https://example.org/a?b=1"));
        for bad in [
            "javascript:alert(1)",
            "file:///c:/x",
            "https://a b",
            "https://a\nb",
            "",
        ] {
            assert!(!safe_url(bad), "{bad}");
        }
    }

    #[test]
    fn the_key_never_appears_in_errors_and_debug_is_not_derived() {
        let client = ExaClient::new("  geheim-123  ");
        assert_eq!(client.api_key, "geheim-123");
        let error = ExaError::Status {
            status: 401,
            detail: String::new(),
        };
        assert!(!error.to_string().contains("geheim"));
    }

    #[test]
    fn responses_parse_with_missing_fields() {
        let raw: RawResponse = serde_json::from_str(
            r#"{"results":[{"url":"https://x.org","highlights":["a","b"]},{"url":"https://y.org","title":"T","text":"Inhalt"}],"costDollars":{"total":0.007}}"#,
        )
        .unwrap();
        assert_eq!(raw.results.len(), 2);
        assert_eq!(raw.cost.unwrap().total, Some(0.007));
        let raw: RawResponse = serde_json::from_str(r#"{"results":[]}"#).unwrap();
        assert!(raw.cost.is_none());
    }

    use std::sync::Mutex;

    /// Antwortet mit einer festen Antwort und merkt sich die Anfrage.
    type Seen = (String, String, Vec<(String, String)>, String);

    struct Fake {
        status: u16,
        reply: &'static str,
        seen: Mutex<Vec<Seen>>,
    }

    impl Fake {
        fn new(status: u16, reply: &'static str) -> Arc<Self> {
            Arc::new(Self {
                status,
                reply,
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
            body: &[u8],
            _t: Duration,
            _m: usize,
        ) -> Result<net::HttpsResponse, NetError> {
            assert_eq!(method, Method::Post);
            self.seen.lock().unwrap().push((
                format!("{host}{target}"),
                String::from_utf8_lossy(body).into_owned(),
                headers
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
                String::new(),
            ));
            Ok(net::HttpsResponse {
                status: self.status,
                body: self.reply.as_bytes().to_vec(),
            })
        }
    }

    fn public(text: &str) -> PublicText {
        PublicText::new(text, pa_policy::egress::DataOrigin::UserPublic).unwrap()
    }

    #[test]
    fn a_search_sends_the_documented_request_and_reads_hits_and_cost() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        let fake = Fake::new(
            200,
            r#"{"results":[{"title":"Everest","url":"https://example.org/a","highlights":["8849 m"]},{"url":"javascript:x"}],"costDollars":{"total":0.007}}"#,
        );
        let client = ExaClient::with_transport("  sk-test  ", fake.clone());
        let outcome = client.search(&public("Höhe Everest"), 99).expect("Antwort");
        assert_eq!(outcome.hits.len(), 1, "unsichere Adresse wird verworfen");
        assert_eq!(outcome.hits[0].text, "8849 m");
        assert!((outcome.cost_usd - 0.007).abs() < 1e-9);

        let seen = fake.seen.lock().unwrap();
        let (url, body, headers, _) = &seen[0];
        assert_eq!(url, "api.exa.ai/search");
        let json: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(json["query"], "Höhe Everest");
        assert_eq!(json["numResults"], 10, "auf 10 begrenzt");
        assert_eq!(json["type"], "auto");
        assert_eq!(json["contents"]["highlights"], true);
        assert!(headers.contains(&("x-api-key".to_owned(), "sk-test".to_owned())));
        assert!(headers.contains(&("Content-Type".to_owned(), "application/json".to_owned())));
    }

    #[test]
    fn contents_sends_urls_and_a_character_limit() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        let fake = Fake::new(
            200,
            r#"{"results":[{"url":"https://example.org/a","text":"Volltext"}]}"#,
        );
        let client = ExaClient::with_transport("k", fake.clone());
        let outcome = client
            .contents(&["https://example.org/a".to_owned()], 3000)
            .expect("Antwort");
        assert_eq!(
            outcome.texts,
            vec![("https://example.org/a".to_owned(), "Volltext".to_owned())]
        );
        assert!(
            (outcome.cost_usd - ASSUMED_COST_USD).abs() < 1e-9,
            "fehlende Kosten werden vorsichtig angesetzt"
        );
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen[0].0, "api.exa.ai/contents");
        let json: serde_json::Value = serde_json::from_str(&seen[0].1).unwrap();
        assert_eq!(json["urls"][0], "https://example.org/a");
        assert_eq!(json["text"]["maxCharacters"], 3000);
    }

    #[test]
    fn error_statuses_explain_what_to_do_without_leaking_the_key() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        for (status, expect) in [
            (401, "Schlüssel"),
            (403, "Schlüssel"),
            (402, "Guthaben"),
            (429, "zu viele"),
            (503, "nicht erreichbar"),
        ] {
            let fake = Fake::new(status, r#"{"error":"nope sk-geheim"}"#);
            let client = ExaClient::with_transport("sk-geheim", fake);
            let message = client.search(&public("x"), 3).unwrap_err().to_string();
            assert!(message.contains(expect), "{status}: {message}");
            assert!(message.contains(&format!("Status {status}")));
        }
        // Die Meldung von Exa wird gekürzt mitgegeben, der Schlüssel selbst nie vom Client eingefügt.
        let fake = Fake::new(401, r#"{"error":"invalid api key"}"#);
        let message = ExaClient::with_transport("sk-geheim", fake)
            .search(&public("x"), 3)
            .unwrap_err()
            .to_string();
        assert!(message.contains("invalid api key") && !message.contains("sk-geheim"));
    }

    #[test]
    fn garbage_answers_are_a_clear_error_and_the_air_gap_still_blocks() {
        let _guard = net::TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        net::set_air_gap(false);
        let fake = Fake::new(200, "<html>kein JSON</html>");
        assert!(matches!(
            ExaClient::with_transport("k", fake).search(&public("x"), 1),
            Err(ExaError::BadResponse)
        ));
        net::set_air_gap(true);
        let fake = Fake::new(200, "{}");
        let client = ExaClient::with_transport("k", fake.clone());
        assert!(matches!(
            client.search(&public("x"), 1),
            Err(ExaError::Net(NetError::AirGap))
        ));
        assert!(fake.seen.lock().unwrap().is_empty());
        net::set_air_gap(false);
    }
}
