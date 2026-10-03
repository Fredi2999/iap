//! Brave Search: Websuche mit eigenem Schlüssel.
//!
//! Format geprüft am 2026-10-01 (Brave-Dokumentation): `GET https://api.search.brave.com/res/v1/web/search?q=…&count=…`
//! mit Kopfzeile `X-Subscription-Token: <Schlüssel>` → `{"web":{"results":[{title,url,description,extra_snippets?}]}}`.
//!
//! Preis laut Anbieter: 5 US-Dollar je 1000 Anfragen, dazu monatlich ein Gratisguthaben. IAP rechnet
//! pauschal [`COST_PER_REQUEST_USD`] je Aufruf ins Kostenlimit des Laufs.

use std::sync::Arc;

use pa_core::workflow::{ExaSearchOutcome, SearchHit};
use pa_policy::egress::{Connector, PublicText};
use serde::Deserialize;
use zeroize::Zeroize;

use super::{clip, get_json, safe_url, strip_html, ConnectorError};
use crate::net::{SystemTransport, Transport};

const SERVICE: &str = "Brave";
const HOST: &str = "api.search.brave.com";
const MAX_TEXT: usize = 4_000;

/// Pauschale Kosten je Anfrage (5 USD je 1000).
pub const COST_PER_REQUEST_USD: f64 = 0.005;

#[derive(Debug, Deserialize)]
struct Response {
    #[serde(default)]
    web: Option<Web>,
}

#[derive(Debug, Deserialize)]
struct Web {
    #[serde(default)]
    results: Vec<Result>,
}

#[derive(Debug, Deserialize)]
struct Result {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    extra_snippets: Vec<String>,
}

/// Client mit Schlüssel. Der Schlüssel erscheint in keiner Fehlermeldung und wird beim
/// Freigeben überschrieben.
pub struct BraveClient {
    api_key: String,
    transport: Arc<dyn Transport>,
}

impl Drop for BraveClient {
    fn drop(&mut self) {
        self.api_key.zeroize();
    }
}

impl BraveClient {
    pub fn new(api_key: &str) -> Self {
        Self::with_transport(api_key, Arc::new(SystemTransport))
    }

    pub fn with_transport(api_key: &str, transport: Arc<dyn Transport>) -> Self {
        Self {
            api_key: api_key.trim().to_owned(),
            transport,
        }
    }

    /// Websuche.
    ///
    /// # Errors
    /// Netz-, Status- und Formatfehler.
    pub fn search(
        &self,
        query: &PublicText,
        count: u32,
    ) -> std::result::Result<ExaSearchOutcome, ConnectorError> {
        let count = count.clamp(1, 10).to_string();
        let response: Response = get_json(
            self.transport.as_ref(),
            Connector::Brave,
            SERVICE,
            HOST,
            "/res/v1/web/search",
            &[
                ("q", query.as_str()),
                ("count", &count),
                ("extra_snippets", "true"),
            ],
            &[
                ("Accept", "application/json"),
                ("X-Subscription-Token", &self.api_key),
            ],
        )?;
        let hits = response
            .web
            .map(|web| web.results)
            .unwrap_or_default()
            .into_iter()
            .filter(|r| safe_url(&r.url))
            .map(|r| {
                let mut text = strip_html(r.description.as_deref().unwrap_or(""));
                for extra in r.extra_snippets.iter().take(3) {
                    text.push('\n');
                    text.push_str(&strip_html(extra));
                }
                SearchHit {
                    title: clip(&strip_html(&r.title), 300),
                    url: r.url,
                    text: clip(text.trim(), MAX_TEXT),
                }
            })
            .collect();
        Ok(ExaSearchOutcome {
            hits,
            cost_usd: COST_PER_REQUEST_USD,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::testing::{open_net, Fake};

    fn public(text: &str) -> PublicText {
        PublicText::new(text, pa_policy::egress::DataOrigin::UserPublic).unwrap()
    }

    #[test]
    fn a_search_sends_the_token_header_and_reads_results_with_extra_snippets() {
        let _guard = open_net();
        let fake = Fake::new(&[(
            200,
            r#"{"web":{"results":[{"title":"Rust <strong>Sprache</strong>","url":"https://rust-lang.org/","description":"Eine <b>sichere</b> Sprache","extra_snippets":["Zusatz eins","Zusatz zwei"]},{"title":"Unsicher","url":"javascript:x"}]}}"#,
        )]);
        let client = BraveClient::with_transport("  BSA-key  ", fake.clone());
        let outcome = client
            .search(&public("rust lang & mehr"), 99)
            .expect("Antwort");
        assert_eq!(outcome.hits.len(), 1, "unsichere Adresse wird verworfen");
        assert_eq!(outcome.hits[0].title, "Rust Sprache");
        assert_eq!(
            outcome.hits[0].text,
            "Eine sichere Sprache\nZusatz eins\nZusatz zwei"
        );
        assert!((outcome.cost_usd - COST_PER_REQUEST_USD).abs() < 1e-12);
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen[0].0, "api.search.brave.com");
        assert_eq!(
            seen[0].2,
            "/res/v1/web/search?q=rust%20lang%20%26%20mehr&count=10&extra_snippets=true"
        );
        assert!(seen[0]
            .3
            .contains(&("X-Subscription-Token".to_owned(), "BSA-key".to_owned())));
    }

    #[test]
    fn errors_explain_themselves_without_the_key_and_empty_answers_are_ok() {
        let _guard = open_net();
        let fake = Fake::new(&[
            (401, r#"{"error":"BSA-geheim"}"#),
            (402, "{}"),
            (200, "{}"),
            (200, "kaputt"),
        ]);
        let client = BraveClient::with_transport("BSA-geheim", fake);
        let unauthorized = client.search(&public("x"), 3).unwrap_err().to_string();
        assert!(
            unauthorized.contains("Schlüssel") && !unauthorized.contains("BSA-geheim"),
            "{unauthorized}"
        );
        assert!(client
            .search(&public("x"), 3)
            .unwrap_err()
            .to_string()
            .contains("Guthaben"));
        assert!(client.search(&public("x"), 3).unwrap().hits.is_empty());
        assert!(matches!(
            client.search(&public("x"), 3),
            Err(ConnectorError::BadResponse(_))
        ));
    }
}
