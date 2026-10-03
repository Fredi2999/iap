//! Gmail-Anbindung: Postfach lesen und eine Auto-Antwort an genau eine Adresse.
//!
//! Aufbau von unten nach oben: `codec` und `message` (reine Textverarbeitung), `imap` und
//! `smtp` (Protokolle über einen austauschbaren Datenstrom), `guard` (alle Sperren),
//! `engine` (ein Prüflauf) und `poller` (Takt, Schalter, Not-Aus). Nur `tls` greift auf das
//! Betriebssystem zu. Kein Teil spricht mit einem anderen Host als der Tabelle in
//! `pa-policy::egress` (imap.gmail.com:993, smtp.gmail.com:465).

pub mod codec;
pub mod engine;
pub mod guard;
pub mod imap;
pub mod message;
pub mod poller;
pub mod reader;
pub mod smtp;
pub mod tls;

use std::io::{Read, Write};
use std::time::Duration;

use thiserror::Error;

/// Fehler der Mail-Anbindung. Die Texte sind für die Nutzerin oder den Nutzer lesbar.
#[derive(Debug, Error)]
pub enum MailError {
    /// Auf dieser Plattform gibt es noch keine geprüfte Verschlüsselung.
    #[error("Mail ist auf dieser Plattform noch nicht eingerichtet (Phase 4)")]
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
    /// Die Policy erlaubt die Verbindung nicht (Air Gap, Host).
    #[error("{0}")]
    Blocked(String),
    /// Verbindung oder Verschlüsselung fehlgeschlagen.
    #[error("Keine Verbindung zu Google: {0}")]
    #[cfg_attr(not(windows), allow(dead_code))]
    Connect(String),
    /// Google hat die Anmeldung abgelehnt.
    #[error("Anmeldung abgelehnt. Prüfe Adresse und App-Passwort.")]
    Auth,
    /// Unerwartete Antwort des Servers.
    #[error("Unerwartete Antwort des Mail-Servers: {0}")]
    Protocol(String),
    /// Der Server hat eine Aktion abgelehnt.
    #[error("Der Mail-Server hat abgelehnt: {0}")]
    Rejected(String),
    /// Eine Antwort oder Mail war größer als erlaubt.
    #[error("Die Mail ist größer als erlaubt")]
    TooLarge,
    /// Es gibt keinen Entwürfe-Ordner (Gmail meldet ihn mit dem Merkmal \Drafts).
    #[error("Im Konto wurde kein Entwürfe-Ordner gefunden")]
    NoDrafts,
    /// Lesen oder Schreiben auf dem Datenstrom ist fehlgeschlagen.
    #[error("Verbindung unterbrochen: {0}")]
    Io(String),
}

impl From<std::io::Error> for MailError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Ein verschlüsselter Datenstrom zum Mail-Server. Tests ersetzen ihn durch einen Fake.
pub trait MailStream: Read + Write {}
impl<T: Read + Write> MailStream for T {}

/// Erzeugt Verbindungen. Die echte Fassung prüft Host und Port gegen die Tabelle und baut TLS auf.
pub trait MailTransport: Send + Sync {
    /// Öffnet eine verschlüsselte Verbindung zu `host:port`.
    fn connect(
        &self,
        host: &str,
        port: u16,
        timeout: Duration,
    ) -> Result<Box<dyn MailStream>, MailError>;
}

/// Die zwei einzigen erlaubten Ziele (Host, Port). Eine zweite Absicherung neben der Policy:
/// selbst wenn ein Aufrufer die Policy vergäße, öffnet der Transport nichts anderes.
pub const IMAP_ENDPOINT: (&str, u16) = ("imap.gmail.com", 993);
pub const SMTP_ENDPOINT: (&str, u16) = ("smtp.gmail.com", 465);

/// Ob `host:port` eines der zwei erlaubten Ziele ist.
pub fn endpoint_allowed(host: &str, port: u16) -> bool {
    [IMAP_ENDPOINT, SMTP_ENDPOINT]
        .iter()
        .any(|(allowed_host, allowed_port)| {
            host.eq_ignore_ascii_case(allowed_host) && port == *allowed_port
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_two_gmail_endpoints_are_allowed() {
        assert!(endpoint_allowed("imap.gmail.com", 993));
        assert!(endpoint_allowed("SMTP.GMAIL.COM", 465));
        assert!(!endpoint_allowed("imap.gmail.com", 143));
        assert!(!endpoint_allowed("imap.gmail.com", 465));
        assert!(!endpoint_allowed("mail.example.com", 993));
        assert!(!endpoint_allowed("gmail.com", 993));
        assert!(!endpoint_allowed("imap.gmail.com.evil.example", 993));
    }
}
