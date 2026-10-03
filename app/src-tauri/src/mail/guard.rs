//! Die Sperren der Auto-Antwort. Reine Entscheidungslogik ohne Netz und ohne Modell.
//!
//! Alles, was verhindert, dass IAP Unsinn an falsche Empfänger schickt, steht hier und wird
//! im Backend erzwungen, nie in der Oberfläche: nur der eine eingetragene Absender, eine
//! bestandene Google-Prüfung (DMARC, DKIM oder SPF), keine Auto-Mails, ein Limit pro Stunde und
//! Tag und genau eine Antwort je Message-ID.

use serde::{Deserialize, Serialize};

use super::message::{domain_of, ParsedMail};

/// So viele Message-IDs merkt sich der Tresor. Älteres fällt hinten heraus; die UNSEEN-Suche
/// liefert ohnehin nur aktuelle Mails.
const MAX_PROCESSED: usize = 500;
/// So viele Sendezeitpunkte werden für die Limits gehalten.
const MAX_SENT: usize = 300;
/// So viele Protokollzeilen bleiben sichtbar.
const MAX_LOG: usize = 100;
const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;

/// Warum eine Mail nicht beantwortet wird. Der Code ist stabil (Oberfläche übersetzt ihn).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    NoMessageId,
    AlreadyHandled,
    WrongSender,
    AutoMail,
    NotAuthenticated,
    NoText,
    RateLimited,
}

impl SkipReason {
    /// Stabiler Code für Protokoll und Oberfläche.
    pub fn code(self) -> &'static str {
        match self {
            Self::NoMessageId => "no_message_id",
            Self::AlreadyHandled => "already_handled",
            Self::WrongSender => "wrong_sender",
            Self::AutoMail => "auto_mail",
            Self::NotAuthenticated => "not_authenticated",
            Self::NoText => "no_text",
            Self::RateLimited => "rate_limited",
        }
    }

    /// Ob die Mail später noch einmal geprüft werden darf. Nur das Limit ist vorübergehend;
    /// alles andere ändert sich bei derselben Mail nie.
    pub fn is_temporary(self) -> bool {
        matches!(self, Self::RateLimited)
    }
}

/// Ergebnis der Prüfung einer Mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Reply,
    Skip(SkipReason),
}

/// Einstellungen, die die Prüfung braucht.
#[derive(Debug, Clone)]
pub struct Rules {
    /// Der eine erlaubte Absender (Kleinbuchstaben).
    pub target: String,
    pub max_per_hour: u32,
    pub max_per_day: u32,
}

/// Art eines Protokolleintrags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    Drafted,
    Sent,
    Skipped,
    Error,
    Info,
}

/// Eine Zeile im sichtbaren Protokoll.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub at_unix_ms: i64,
    pub kind: LogKind,
    /// Betreff, auf 60 Zeichen gekürzt. Nur im verschlüsselten Tresor, nie im Audit-Log.
    pub subject: String,
    /// Stabiler Code (z. B. `wrong_sender`) oder freier Text bei Fehlern.
    pub detail: String,
}

/// Merkzustand im Tresor: übersteht Neustarts, damit nie doppelt geantwortet wird.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MailState {
    pub processed: Vec<String>,
    pub sent_unix_ms: Vec<i64>,
    pub log: Vec<LogEntry>,
}

impl MailState {
    pub fn is_processed(&self, message_id: &str) -> bool {
        self.processed.iter().any(|id| id == message_id)
    }

    /// Merkt sich die Message-ID. Ältere fallen nach hinten heraus.
    pub fn mark_processed(&mut self, message_id: &str) {
        if !self.is_processed(message_id) {
            self.processed.push(message_id.to_owned());
            if self.processed.len() > MAX_PROCESSED {
                self.processed.remove(0);
            }
        }
    }

    /// Zählt einen Versand (Entwurf oder Mail) für die Limits.
    pub fn record_sent(&mut self, now_ms: i64) {
        self.sent_unix_ms.push(now_ms);
        self.sent_unix_ms.retain(|at| now_ms - at < DAY_MS);
        if self.sent_unix_ms.len() > MAX_SENT {
            let excess = self.sent_unix_ms.len() - MAX_SENT;
            self.sent_unix_ms.drain(..excess);
        }
    }

    pub fn sent_within(&self, now_ms: i64, window_ms: i64) -> u32 {
        u32::try_from(
            self.sent_unix_ms
                .iter()
                .filter(|at| now_ms - **at < window_ms && **at <= now_ms)
                .count(),
        )
        .unwrap_or(u32::MAX)
    }

    pub fn push_log(&mut self, entry: LogEntry) {
        self.log.push(entry);
        if self.log.len() > MAX_LOG {
            let excess = self.log.len() - MAX_LOG;
            self.log.drain(..excess);
        }
    }
}

/// Domänen gelten als zusammengehörig, wenn eine die andere oder gleich ist (entspricht der
/// entspannten DMARC-Ausrichtung für typische Fälle wie `mail.example.com` und `example.com`).
fn aligned(a: &str, b: &str) -> bool {
    let (a, b) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
    !a.is_empty() && (a == b || a.ends_with(&format!(".{b}")) || b.ends_with(&format!(".{a}")))
}

/// Wert einer Eigenschaft wie `header.d=example.com` in einem Ergebnisteil.
fn property<'a>(part: &'a str, name: &str) -> Option<&'a str> {
    part.split_whitespace()
        .find_map(|token| token.strip_prefix(name)?.strip_prefix('='))
        .map(|value| value.trim_matches(['"', ';', '(', ')']))
}

/// Prüft, ob Google selbst die Herkunft bestätigt hat.
///
/// Es zählt nur die oberste `Authentication-Results`-Zeile mit der Kennung `mx.google.com`,
/// weil Google sie beim Empfang vorn anfügt. Eine vom Absender mitgeschickte, gefälschte Zeile
/// steht weiter hinten und wird nie gelesen. Ohne bestandene Prüfung antwortet IAP nicht,
/// denn die Absenderadresse allein ist trivial fälschbar.
pub fn authenticated(mail: &ParsedMail) -> bool {
    let Some(value) = mail
        .headers
        .iter()
        .find(|(name, _)| name == "authentication-results")
        .map(|(_, value)| value.as_str())
    else {
        return false;
    };
    let mut parts = value.split(';');
    if parts.next().map(str::trim) != Some("mx.google.com") {
        return false;
    }
    let from_domain = domain_of(&mail.from_addr);
    if from_domain.is_empty() {
        return false;
    }
    for part in parts {
        let part = part.trim();
        if part.starts_with("dmarc=pass") {
            return true;
        }
        if part.starts_with("dkim=pass")
            && property(part, "header.d").is_some_and(|domain| aligned(domain, from_domain))
        {
            return true;
        }
        if part.starts_with("spf=pass") {
            let mailfrom = property(part, "smtp.mailfrom").unwrap_or("");
            let domain = mailfrom.rsplit_once('@').map_or(mailfrom, |(_, d)| d);
            if aligned(domain, from_domain) {
                return true;
            }
        }
    }
    false
}

/// Ob die Mail eine automatisch erzeugte ist (Newsletter, Abwesenheitsnotiz, Systemmeldung).
/// Auf solche wird nie geantwortet, sonst könnten sich zwei Automaten endlos antworten.
pub fn is_automatic(mail: &ParsedMail) -> bool {
    if let Some(value) = mail.header("auto-submitted") {
        if !value.trim().eq_ignore_ascii_case("no") {
            return true;
        }
    }
    if let Some(value) = mail.header("precedence") {
        let value = value.trim().to_ascii_lowercase();
        if ["bulk", "junk", "list", "auto_reply", "auto-reply"].contains(&value.as_str()) {
            return true;
        }
    }
    [
        "list-id",
        "list-unsubscribe",
        "x-autoreply",
        "x-autorespond",
        "x-auto-response-suppress",
    ]
    .iter()
    .any(|name| mail.header(name).is_some())
}

/// Prüft eine Mail gegen alle Sperren, in fester Reihenfolge.
pub fn evaluate(mail: &ParsedMail, rules: &Rules, state: &MailState, now_ms: i64) -> Verdict {
    use SkipReason::*;
    if mail.message_id.is_empty() {
        return Verdict::Skip(NoMessageId);
    }
    if state.is_processed(&mail.message_id) {
        return Verdict::Skip(AlreadyHandled);
    }
    if mail.from_addr.is_empty() || mail.from_addr != rules.target.to_ascii_lowercase() {
        return Verdict::Skip(WrongSender);
    }
    if is_automatic(mail) {
        return Verdict::Skip(AutoMail);
    }
    if !authenticated(mail) {
        return Verdict::Skip(NotAuthenticated);
    }
    if mail.text.trim().is_empty() {
        return Verdict::Skip(NoText);
    }
    if state.sent_within(now_ms, HOUR_MS) >= rules.max_per_hour
        || state.sent_within(now_ms, DAY_MS) >= rules.max_per_day
    {
        return Verdict::Skip(RateLimited);
    }
    Verdict::Reply
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    fn mail(extra: &[(&str, &str)]) -> ParsedMail {
        let mut headers = vec![
            ("authentication-results".to_owned(), "mx.google.com; dkim=pass header.i=@gmail.com header.d=gmail.com; spf=pass smtp.mailfrom=anna@gmail.com; dmarc=pass (p=NONE) header.from=gmail.com".to_owned()),
        ];
        headers.extend(
            extra
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        ParsedMail {
            message_id: "<m1@gmail.com>".into(),
            from_addr: "anna@gmail.com".into(),
            subject: "Hallo".into(),
            text: "Wie spät ist es?".into(),
            headers,
            ..ParsedMail::default()
        }
    }

    fn rules() -> Rules {
        Rules {
            target: "Anna@Gmail.com".into(),
            max_per_hour: 6,
            max_per_day: 30,
        }
    }

    #[test]
    fn a_normal_authenticated_mail_from_the_target_is_answered() {
        assert_eq!(
            evaluate(&mail(&[]), &rules(), &MailState::default(), NOW),
            Verdict::Reply
        );
    }

    #[test]
    fn mails_from_anyone_else_are_ignored() {
        let mut other = mail(&[]);
        other.from_addr = "chef@firma.de".into();
        assert_eq!(
            evaluate(&other, &rules(), &MailState::default(), NOW),
            Verdict::Skip(SkipReason::WrongSender)
        );
        other.from_addr = String::new();
        assert_eq!(
            evaluate(&other, &rules(), &MailState::default(), NOW),
            Verdict::Skip(SkipReason::WrongSender)
        );
        // Ähnliche Adresse zählt nicht.
        other.from_addr = "anna@gmail.com.evil.de".into();
        assert_eq!(
            evaluate(&other, &rules(), &MailState::default(), NOW),
            Verdict::Skip(SkipReason::WrongSender)
        );
    }

    #[test]
    fn a_forged_sender_without_a_google_verdict_is_ignored() {
        let mut forged = mail(&[]);
        forged.headers.clear();
        assert_eq!(
            evaluate(&forged, &rules(), &MailState::default(), NOW),
            Verdict::Skip(SkipReason::NotAuthenticated)
        );
        // Ergebnis eines fremden Servers zählt nicht.
        forged.headers = vec![(
            "authentication-results".into(),
            "evil.example; dmarc=pass".into(),
        )];
        assert!(!authenticated(&forged));
        // Gefälschte Zeile hinter der echten wird nie gelesen: die echte meldet fail.
        forged.headers = vec![
            (
                "authentication-results".into(),
                "mx.google.com; dkim=fail; spf=fail; dmarc=fail".into(),
            ),
            (
                "authentication-results".into(),
                "mx.google.com; dmarc=pass".into(),
            ),
        ];
        assert!(!authenticated(&forged));
    }

    #[test]
    fn dkim_or_spf_must_be_aligned_with_the_from_domain() {
        let mut m = mail(&[]);
        m.headers = vec![(
            "authentication-results".into(),
            "mx.google.com; dkim=pass header.d=other.example; spf=none".into(),
        )];
        assert!(!authenticated(&m), "DKIM einer fremden Domain reicht nicht");
        m.headers = vec![(
            "authentication-results".into(),
            "mx.google.com; dkim=pass header.d=mail.gmail.com".into(),
        )];
        assert!(authenticated(&m));
        m.headers = vec![(
            "authentication-results".into(),
            "mx.google.com; spf=pass smtp.mailfrom=bounce@gmail.com".into(),
        )];
        assert!(authenticated(&m));
        m.headers = vec![(
            "authentication-results".into(),
            "mx.google.com; spf=pass smtp.mailfrom=bounce@evil.example".into(),
        )];
        assert!(!authenticated(&m));
        m.headers = vec![(
            "authentication-results".into(),
            "mx.google.com; spf=softfail smtp.mailfrom=anna@gmail.com".into(),
        )];
        assert!(!authenticated(&m));
    }

    #[test]
    fn automatic_mails_are_never_answered() {
        for header in [
            ("auto-submitted", "auto-replied"),
            ("auto-submitted", "auto-generated"),
            ("precedence", "bulk"),
            ("precedence", "auto_reply"),
            ("list-id", "<news.example.com>"),
            ("list-unsubscribe", "<mailto:x@y.z>"),
            ("x-auto-response-suppress", "All"),
        ] {
            assert_eq!(
                evaluate(&mail(&[header]), &rules(), &MailState::default(), NOW),
                Verdict::Skip(SkipReason::AutoMail),
                "{header:?}"
            );
        }
        // `Auto-Submitted: no` ist ausdrücklich keine Automatik.
        assert_eq!(
            evaluate(
                &mail(&[("auto-submitted", "no")]),
                &rules(),
                &MailState::default(),
                NOW
            ),
            Verdict::Reply
        );
    }

    #[test]
    fn each_message_id_is_answered_once() {
        let m = mail(&[]);
        let mut state = MailState::default();
        state.mark_processed(&m.message_id);
        assert_eq!(
            evaluate(&m, &rules(), &state, NOW),
            Verdict::Skip(SkipReason::AlreadyHandled)
        );
        let mut no_id = mail(&[]);
        no_id.message_id.clear();
        assert_eq!(
            evaluate(&no_id, &rules(), &MailState::default(), NOW),
            Verdict::Skip(SkipReason::NoMessageId)
        );
    }

    #[test]
    fn empty_text_is_skipped() {
        let mut m = mail(&[]);
        m.text = "  \n ".into();
        assert_eq!(
            evaluate(&m, &rules(), &MailState::default(), NOW),
            Verdict::Skip(SkipReason::NoText)
        );
    }

    #[test]
    fn the_hourly_and_daily_limits_hold_and_recover() {
        let mut state = MailState::default();
        for i in 0..6 {
            state.record_sent(NOW - i * 60_000);
        }
        assert_eq!(
            evaluate(&mail(&[]), &rules(), &state, NOW),
            Verdict::Skip(SkipReason::RateLimited)
        );
        assert!(SkipReason::RateLimited.is_temporary());
        // Nach einer Stunde ist wieder Platz.
        assert_eq!(
            evaluate(&mail(&[]), &rules(), &state, NOW + HOUR_MS),
            Verdict::Reply
        );
        // Tageslimit.
        let mut day = MailState::default();
        for i in 0..30 {
            day.record_sent(NOW - 2 * HOUR_MS - i * 60_000);
        }
        assert_eq!(
            evaluate(&mail(&[]), &rules(), &day, NOW),
            Verdict::Skip(SkipReason::RateLimited)
        );
        assert_eq!(
            evaluate(&mail(&[]), &rules(), &day, NOW + DAY_MS),
            Verdict::Reply
        );
    }

    #[test]
    fn a_limit_of_zero_blocks_everything() {
        let mut zero = rules();
        zero.max_per_hour = 0;
        assert_eq!(
            evaluate(&mail(&[]), &zero, &MailState::default(), NOW),
            Verdict::Skip(SkipReason::RateLimited)
        );
    }

    #[test]
    fn the_state_is_bounded() {
        let mut state = MailState::default();
        for i in 0..(MAX_PROCESSED + 50) {
            state.mark_processed(&format!("<{i}@x>"));
        }
        assert_eq!(state.processed.len(), MAX_PROCESSED);
        assert!(
            !state.is_processed("<0@x>")
                && state.is_processed(&format!("<{}@x>", MAX_PROCESSED + 49))
        );
        for i in 0..(MAX_LOG + 20) {
            state.push_log(LogEntry {
                at_unix_ms: i as i64,
                kind: LogKind::Info,
                subject: String::new(),
                detail: String::new(),
            });
        }
        assert_eq!(state.log.len(), MAX_LOG);
        // Doppeltes Merken vergrößert nichts.
        let before = state.processed.len();
        state.mark_processed(&format!("<{}@x>", MAX_PROCESSED + 49));
        assert_eq!(state.processed.len(), before);
    }

    #[test]
    fn a_partial_stored_state_loads() {
        let state: MailState = serde_json::from_str(r#"{"processed":["<a@b>"]}"#).unwrap();
        assert!(state.is_processed("<a@b>") && state.log.is_empty());
    }
}
