//! Wikipedia: Suche und Kurzfassung über die REST-Schnittstelle (Wikimedia Core REST API).
//!
//! Formate geprüft am 2026-10-01:
//! - Suche: `GET /w/rest.php/v1/search/page?q=…&limit=…` → `{"pages":[{key,title,excerpt,description}]}`
//! - Kurzfassung: `GET /api/rest_v1/page/summary/<Titel>` → `{title, extract, content_urls.desktop.page}`
//!
//! Ohne Schlüssel und kostenlos. Der Nutzer-Agent nennt die Anwendung (Wikimedia verlangt das).

use std::sync::Arc;

use pa_core::workflow::{ExaSearchOutcome, SearchHit};
use pa_policy::egress::{Connector, PublicText};
use serde::Deserialize;

use super::{clip, get_json, safe_url, strip_html, ConnectorError};
use crate::net::{self, SystemTransport, Transport};

const SERVICE: &str = "Wikipedia";
const MAX_TEXT: usize = 4_000;

/// Sprachausgabe der Wikipedia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    De,
    En,
}

impl Lang {
    /// `de` oder `en`; alles andere fällt auf Deutsch zurück.
    pub fn parse(code: &str) -> Self {
        if code.trim().eq_ignore_ascii_case("en") {
            Lang::En
        } else {
            Lang::De
        }
    }

    pub fn host(self) -> &'static str {
        match self {
            Lang::De => "de.wikipedia.org",
            Lang::En => "en.wikipedia.org",
        }
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    pages: Vec<Page>,
}

#[derive(Debug, Deserialize)]
struct Page {
    #[serde(default)]
    key: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    excerpt: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

pub struct WikipediaClient {
    lang: Lang,
    transport: Arc<dyn Transport>,
}

impl WikipediaClient {
    pub fn new(lang: Lang) -> Self {
        Self::with_transport(lang, Arc::new(SystemTransport))
    }

    pub fn with_transport(lang: Lang, transport: Arc<dyn Transport>) -> Self {
        Self { lang, transport }
    }

    fn page_url(&self, key: &str) -> String {
        format!(
            "https://{}/wiki/{}",
            self.lang.host(),
            net::encode_segment(key)
        )
    }

    /// Sucht Artikel. Jeder Treffer trägt Titel, Adresse und Auszug samt Kurzbeschreibung.
    ///
    /// # Errors
    /// Netz-, Status- und Formatfehler.
    pub fn search(
        &self,
        query: &PublicText,
        limit: u32,
    ) -> Result<ExaSearchOutcome, ConnectorError> {
        let limit = limit.clamp(1, 10).to_string();
        let response: SearchResponse = get_json(
            self.transport.as_ref(),
            Connector::Wikipedia,
            SERVICE,
            self.lang.host(),
            "/w/rest.php/v1/search/page",
            &[("q", query.as_str()), ("limit", &limit)],
            &[("Accept", "application/json")],
        )?;
        let hits = response
            .pages
            .into_iter()
            .filter(|page| !page.key.is_empty())
            .map(|page| {
                let mut text = String::new();
                if let Some(description) = page.description.as_deref().filter(|d| !d.is_empty()) {
                    text.push_str(description);
                    text.push_str(". ");
                }
                text.push_str(&strip_html(page.excerpt.as_deref().unwrap_or("")));
                SearchHit {
                    title: clip(&page.title, 300),
                    url: self.page_url(&page.key),
                    text: clip(text.trim(), MAX_TEXT),
                }
            })
            .filter(|hit| safe_url(&hit.url))
            .collect();
        Ok(ExaSearchOutcome {
            hits,
            cost_usd: 0.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::testing::{open_net, Fake};
    use crate::net::Method;

    fn public(text: &str) -> PublicText {
        PublicText::new(text, pa_policy::egress::DataOrigin::UserPublic).unwrap()
    }

    #[test]
    fn a_search_sends_a_get_with_an_encoded_query_and_reads_the_documented_answer() {
        let _guard = open_net();
        let fake = Fake::new(&[(
            200,
            r#"{"pages":[{"id":9228,"key":"Earth","title":"Earth","excerpt":" <span class=\"searchmatch\">Earth</span> is the third planet","matched_title":null,"description":"Third planet from the Sun","thumbnail":null}]}"#,
        )]);
        let client = WikipediaClient::with_transport(Lang::En, fake.clone());
        let outcome = client.search(&public("Earth & Moon"), 50).expect("Antwort");
        assert_eq!(outcome.hits.len(), 1);
        assert_eq!(outcome.hits[0].url, "https://en.wikipedia.org/wiki/Earth");
        assert_eq!(
            outcome.hits[0].text,
            "Third planet from the Sun. Earth is the third planet"
        );
        assert_eq!(outcome.cost_usd, 0.0);
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen[0].0, "en.wikipedia.org");
        assert_eq!(seen[0].1, Method::Get);
        assert_eq!(
            seen[0].2,
            "/w/rest.php/v1/search/page?q=Earth%20%26%20Moon&limit=10"
        );
    }

    #[test]
    fn errors_and_garbage_are_clear_and_unknown_languages_fall_back_to_german() {
        let _guard = open_net();
        let fake = Fake::new(&[(429, "{}"), (200, "kein json")]);
        let client = WikipediaClient::with_transport(Lang::De, fake);
        assert!(client
            .search(&public("x"), 3)
            .unwrap_err()
            .to_string()
            .contains("Zu viele"));
        assert!(matches!(
            client.search(&public("x"), 3),
            Err(ConnectorError::BadResponse(_))
        ));
        assert_eq!(Lang::parse("EN"), Lang::En);
        assert_eq!(Lang::parse("fr"), Lang::De);
    }
}
