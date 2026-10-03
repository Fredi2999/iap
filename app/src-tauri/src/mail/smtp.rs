//! Ein kleiner SMTP-Client für den Versand einer einzigen Antwort an einen Empfänger.
//!
//! Verbindung mit sofortiger Verschlüsselung (Port 465), Anmeldung per `AUTH PLAIN`. Es gibt
//! nur eine Sendefunktion mit genau einem Empfänger; mehrere Empfänger, Kopien oder Anhänge
//! kann dieser Client nicht.

use std::io::{Read, Write};

use super::codec::base64_encode;
use super::message::single_address;
use super::MailError;

const MAX_LINE: usize = 16 * 1024;

/// Ein SMTP-Client über einen Datenstrom.
pub struct SmtpClient<S: Read + Write> {
    stream: S,
    buffer: Vec<u8>,
}

impl<S: Read + Write> SmtpClient<S> {
    /// Liest die Begrüßung (Code 220).
    pub fn new(stream: S) -> Result<Self, MailError> {
        let mut client = Self {
            stream,
            buffer: Vec::new(),
        };
        client.expect(220)?;
        Ok(client)
    }

    fn read_line(&mut self) -> Result<String, MailError> {
        loop {
            if let Some(pos) = self.buffer.iter().position(|b| *b == b'\n') {
                let mut line: Vec<u8> = self.buffer.drain(..=pos).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(String::from_utf8_lossy(&line).into_owned());
            }
            if self.buffer.len() > MAX_LINE {
                return Err(MailError::TooLarge);
            }
            let mut chunk = [0_u8; 2048];
            let n = self.stream.read(&mut chunk)?;
            if n == 0 {
                return Err(MailError::Io(
                    "Der Server hat die Verbindung beendet".to_owned(),
                ));
            }
            self.buffer.extend_from_slice(&chunk[..n]);
        }
    }

    /// Liest eine (ggf. mehrzeilige) Antwort und liefert Code und Text der letzten Zeile.
    fn reply(&mut self) -> Result<(u16, String), MailError> {
        loop {
            let line = self.read_line()?;
            let code: u16 = line
                .get(..3)
                .and_then(|c| c.parse().ok())
                .ok_or_else(|| MailError::Protocol(line.chars().take(80).collect()))?;
            if line.as_bytes().get(3) == Some(&b'-') {
                continue;
            }
            return Ok((code, line.get(4..).unwrap_or("").to_owned()));
        }
    }

    fn expect(&mut self, wanted: u16) -> Result<String, MailError> {
        let (code, text) = self.reply()?;
        if code == wanted {
            Ok(text)
        } else if code == 535 || code == 534 {
            Err(MailError::Auth)
        } else {
            Err(MailError::Rejected(format!(
                "{code} {}",
                text.chars().take(100).collect::<String>()
            )))
        }
    }

    fn send(&mut self, text: &str) -> Result<(), MailError> {
        self.stream.write_all(text.as_bytes())?;
        self.stream.flush()?;
        Ok(())
    }

    /// Begrüßt den Server und meldet sich an.
    pub fn login(&mut self, user: &str, password: &str) -> Result<(), MailError> {
        if user.chars().any(char::is_control) || password.chars().any(char::is_control) {
            return Err(MailError::Protocol(
                "Ungültiges Zeichen in den Zugangsdaten".to_owned(),
            ));
        }
        let password: String = password.chars().filter(|c| !c.is_whitespace()).collect();
        self.send("EHLO iap.invalid\r\n")?;
        self.expect(250)?;
        let token = base64_encode(format!("\0{user}\0{password}").as_bytes());
        self.send(&format!("AUTH PLAIN {token}\r\n"))?;
        self.expect(235).map(|_| ())
    }

    /// Sendet eine Mail an genau einen Empfänger. Absender und Empfänger müssen einfache,
    /// einzelne Adressen sein; die Mail selbst wird mit Punkt-Verdopplung übertragen.
    pub fn send_mail(&mut self, from: &str, to: &str, message: &str) -> Result<(), MailError> {
        let (Some(from), Some(to)) = (single_address(from), single_address(to)) else {
            return Err(MailError::Protocol("Ungültige Adresse".to_owned()));
        };
        self.send(&format!("MAIL FROM:<{from}>\r\n"))?;
        self.expect(250)?;
        self.send(&format!("RCPT TO:<{to}>\r\n"))?;
        self.expect(250)?;
        self.send("DATA\r\n")?;
        self.expect(354)?;
        let mut data = String::with_capacity(message.len() + 16);
        for line in message.split("\r\n") {
            if line.starts_with('.') {
                data.push('.');
            }
            data.push_str(line);
            data.push_str("\r\n");
        }
        // `split` liefert nach einem abschließenden CRLF eine leere letzte Zeile; sie wird
        // bereits als eigene Zeile angehängt, daher genügt der Schlusspunkt.
        data.push_str(".\r\n");
        self.send(&data)?;
        self.expect(250).map(|_| ())
    }

    /// Beendet die Sitzung; Fehler sind hier unwichtig.
    pub fn quit(&mut self) {
        let _ = self.send("QUIT\r\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::imap::fake::FakeStream;

    fn sent_text(sent: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>) -> String {
        String::from_utf8_lossy(&sent.lock().unwrap()).into_owned()
    }

    const GREETING: &str = "220 smtp.gmail.com ESMTP ready\r\n";

    #[test]
    fn login_uses_auth_plain_with_the_cleaned_password() {
        let script = format!("{GREETING}250-smtp.gmail.com at your service\r\n250-AUTH LOGIN PLAIN\r\n250 8BITMIME\r\n235 2.7.0 Accepted\r\n");
        let (stream, sent) = FakeStream::new(script.as_bytes());
        let mut client = SmtpClient::new(stream).unwrap();
        client.login("anna@gmail.com", "abcd efgh").unwrap();
        let text = sent_text(&sent);
        assert!(text.starts_with("EHLO iap.invalid\r\n"));
        let token = base64_encode(b"\0anna@gmail.com\0abcdefgh");
        assert!(text.contains(&format!("AUTH PLAIN {token}\r\n")));
    }

    #[test]
    fn rejected_login_is_an_auth_error() {
        let script = format!("{GREETING}250 ok\r\n535-5.7.8 Username and Password not accepted\r\n535 5.7.8 more\r\n");
        let (stream, _) = FakeStream::new(script.as_bytes());
        let mut client = SmtpClient::new(stream).unwrap();
        assert!(matches!(client.login("a@b.c", "x"), Err(MailError::Auth)));
    }

    #[test]
    fn a_mail_goes_to_exactly_one_recipient_with_dot_stuffing() {
        let script = format!("{GREETING}250 ok\r\n250 ok\r\n354 go ahead\r\n250 queued\r\n");
        let (stream, sent) = FakeStream::new(script.as_bytes());
        let mut client = SmtpClient::new(stream).unwrap();
        client
            .send_mail(
                "bot@gmail.com",
                "Anna <anna@gmail.com>",
                "Subject: x\r\n\r\n.heimlich\r\nEnde\r\n",
            )
            .unwrap();
        let text = sent_text(&sent);
        assert!(text.contains("MAIL FROM:<bot@gmail.com>\r\n"));
        assert!(text.contains("RCPT TO:<anna@gmail.com>\r\n"));
        assert_eq!(text.matches("RCPT TO").count(), 1);
        assert!(text.contains("\r\n..heimlich\r\n"), "{text}");
        assert!(
            text.ends_with("Ende\r\n\r\n.\r\n") || text.ends_with("Ende\r\n.\r\n"),
            "{text:?}"
        );
    }

    #[test]
    fn addresses_with_injected_commands_are_refused() {
        let (stream, sent) = FakeStream::new(GREETING.as_bytes());
        let mut client = SmtpClient::new(stream).unwrap();
        assert!(client
            .send_mail("bot@gmail.com", "a@b.c>\r\nRCPT TO:<evil@x.y", "x")
            .is_err());
        assert!(client
            .send_mail("bot@gmail.com", "a@b.c, d@e.f", "x")
            .is_err());
        assert!(
            sent_text(&sent).is_empty(),
            "es darf nichts gesendet werden"
        );
    }

    #[test]
    fn a_refused_recipient_stops_the_transaction() {
        let script = format!("{GREETING}250 ok\r\n550 5.1.1 no such user\r\n");
        let (stream, sent) = FakeStream::new(script.as_bytes());
        let mut client = SmtpClient::new(stream).unwrap();
        let result = client.send_mail("bot@gmail.com", "anna@gmail.com", "x");
        assert!(matches!(result, Err(MailError::Rejected(_))));
        assert!(!sent_text(&sent).contains("DATA"));
    }

    #[test]
    fn a_bad_greeting_is_rejected() {
        let (stream, _) = FakeStream::new(b"554 no service\r\n");
        assert!(SmtpClient::new(stream).is_err());
    }
}
