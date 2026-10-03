//! Ein kleiner IMAP-Client für genau die Aufgaben dieser App.
//!
//! Umfang: Anmelden, Postfach nur lesend öffnen (EXAMINE, es werden nie Flags verändert),
//! suchen, Mails holen (BODY.PEEK, markiert nichts als gelesen), den Entwürfe-Ordner finden
//! und einen Entwurf ablegen. Mehr gibt es absichtlich nicht: kein Löschen, kein Verschieben,
//! kein STORE. Das Protokoll läuft über einen beliebigen Datenstrom und lässt sich deshalb mit
//! einem Fake-Server testen.

use std::io::{Read, Write};

use super::MailError;

/// Längste erlaubte Antwortzeile.
const MAX_LINE: usize = 64 * 1024;
/// Zahl der Antwortzeilen je Befehl, ab der abgebrochen wird (Schutz vor Endlosantworten).
const MAX_RESPONSES: usize = 5_000;

/// Eine Antwortzeile des Servers, die mit `*` beginnt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untagged {
    /// Text vor dem ersten Literal (oder die ganze Zeile).
    pub head: String,
    /// Erstes Literal, falls vorhanden (der Mailinhalt bei FETCH).
    pub literal: Option<Vec<u8>>,
}

/// Abschluss eines Befehls.
#[derive(Debug)]
pub struct Response {
    pub untagged: Vec<Untagged>,
}

/// Ein IMAP-Client über einen Datenstrom.
pub struct ImapClient<S: Read + Write> {
    stream: S,
    buffer: Vec<u8>,
    counter: u32,
    max_literal: usize,
}

/// Setzt einen Wert in Anführungszeichen. Steuerzeichen werden abgelehnt, damit sich nie ein
/// weiterer Befehl einschmuggeln lässt.
pub fn quote(value: &str) -> Result<String, MailError> {
    if value.chars().any(|c| c.is_control()) {
        return Err(MailError::Protocol(
            "Ungültiges Zeichen in einem Mailbefehl".to_owned(),
        ));
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

impl<S: Read + Write> ImapClient<S> {
    /// Liest die Begrüßung des Servers. `max_literal` begrenzt, wie große Mails angenommen werden.
    pub fn new(stream: S, max_literal: usize) -> Result<Self, MailError> {
        let mut client = Self {
            stream,
            buffer: Vec::new(),
            counter: 0,
            max_literal,
        };
        let greeting = client.read_line()?;
        if !greeting.starts_with("* OK") && !greeting.starts_with("* PREAUTH") {
            return Err(MailError::Protocol(format!(
                "Begrüßung: {}",
                greeting.chars().take(80).collect::<String>()
            )));
        }
        Ok(client)
    }

    fn fill(&mut self) -> Result<(), MailError> {
        let mut chunk = [0_u8; 4096];
        let n = self.stream.read(&mut chunk)?;
        if n == 0 {
            return Err(MailError::Io(
                "Der Server hat die Verbindung beendet".to_owned(),
            ));
        }
        self.buffer.extend_from_slice(&chunk[..n]);
        Ok(())
    }

    /// Liest eine Zeile ohne CRLF.
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
            self.fill()?;
        }
    }

    fn read_exact_bytes(&mut self, count: usize) -> Result<Vec<u8>, MailError> {
        while self.buffer.len() < count {
            self.fill()?;
        }
        Ok(self.buffer.drain(..count).collect())
    }

    /// Liest eine Antwortzeile samt Literalen. Das erste Literal wird behalten, weitere werden
    /// gelesen und verworfen.
    fn read_untagged(&mut self, first: String) -> Result<Untagged, MailError> {
        let mut line = first;
        let mut head: Option<String> = None;
        let mut literal: Option<Vec<u8>> = None;
        loop {
            let size = line
                .strip_suffix('}')
                .and_then(|rest| rest.rsplit_once('{'))
                .and_then(|(prefix, number)| {
                    number
                        .trim_end_matches('+')
                        .parse::<usize>()
                        .ok()
                        .map(|n| (prefix.to_owned(), n))
                });
            let Some((prefix, size)) = size else {
                if head.is_none() {
                    head = Some(line);
                }
                break;
            };
            if size > self.max_literal {
                return Err(MailError::TooLarge);
            }
            if head.is_none() {
                head = Some(prefix);
            }
            let data = self.read_exact_bytes(size)?;
            if literal.is_none() {
                literal = Some(data);
            }
            line = self.read_line()?;
        }
        Ok(Untagged {
            head: head.unwrap_or_default(),
            literal,
        })
    }

    fn send(&mut self, bytes: &[u8]) -> Result<(), MailError> {
        self.stream.write_all(bytes)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Sendet einen Befehl und sammelt alle Antworten bis zur Abschlusszeile.
    fn command(&mut self, command: &str) -> Result<Response, MailError> {
        self.counter += 1;
        let tag = format!("A{}", self.counter);
        self.send(format!("{tag} {command}\r\n").as_bytes())?;
        self.finish(&tag)
    }

    fn finish(&mut self, tag: &str) -> Result<Response, MailError> {
        let mut untagged = Vec::new();
        for _ in 0..MAX_RESPONSES {
            let line = self.read_line()?;
            if let Some(rest) = line.strip_prefix(&format!("{tag} ")) {
                return match rest.split_whitespace().next() {
                    Some("OK") => Ok(Response { untagged }),
                    Some("NO") => {
                        if rest.contains("AUTHENTICATIONFAILED")
                            || rest.to_ascii_lowercase().contains("invalid credentials")
                        {
                            Err(MailError::Auth)
                        } else {
                            Err(MailError::Rejected(rest.chars().take(120).collect()))
                        }
                    }
                    _ => Err(MailError::Protocol(rest.chars().take(120).collect())),
                };
            }
            if line.starts_with('*') {
                untagged.push(self.read_untagged(line)?);
            }
            // Andere Zeilen (z. B. Fortsetzungen) sind hier ohne Bedeutung.
        }
        Err(MailError::TooLarge)
    }

    /// Meldet sich an. Das App-Passwort darf Leerzeichen enthalten, sie werden entfernt.
    pub fn login(&mut self, user: &str, password: &str) -> Result<(), MailError> {
        let password: String = password.chars().filter(|c| !c.is_whitespace()).collect();
        self.command(&format!("LOGIN {} {}", quote(user)?, quote(&password)?))
            .map(|_| ())
    }

    /// Öffnet das Postfach nur lesend. `EXAMINE` verändert nie etwas.
    pub fn examine_inbox(&mut self) -> Result<(), MailError> {
        self.command("EXAMINE INBOX").map(|_| ())
    }

    /// UIDs der Mails, die den Kriterien entsprechen, älteste zuerst.
    pub fn search(&mut self, criteria: &Search) -> Result<Vec<u32>, MailError> {
        let response = self.command(&format!("UID SEARCH {}", criteria.render()?))?;
        let mut uids: Vec<u32> = response
            .untagged
            .iter()
            .filter_map(|line| line.head.strip_prefix("* SEARCH"))
            .flat_map(|rest| {
                rest.split_whitespace()
                    .filter_map(|n| n.parse().ok())
                    .collect::<Vec<u32>>()
            })
            .collect();
        uids.sort_unstable();
        Ok(uids)
    }

    /// Holt höchstens `limit` Bytes einer Mail, ohne sie als gelesen zu markieren.
    pub fn fetch_message(&mut self, uid: u32, limit: usize) -> Result<Vec<u8>, MailError> {
        let response = self.command(&format!("UID FETCH {uid} (BODY.PEEK[]<0.{limit}>)"))?;
        response
            .untagged
            .into_iter()
            .find_map(|line| line.literal)
            .ok_or_else(|| MailError::Protocol("Die Mail fehlt in der Antwort".to_owned()))
    }

    /// Findet den Entwürfe-Ordner über das Merkmal `\Drafts`. Der Name ist je nach Sprache des
    /// Kontos verschieden (z. B. `[Gmail]/Entwürfe`), deshalb wird er nie geraten.
    pub fn find_drafts(&mut self) -> Result<String, MailError> {
        let response = self.command("LIST \"\" \"*\"")?;
        for line in response.untagged {
            let Some(rest) = line.head.strip_prefix("* LIST (") else {
                continue;
            };
            let Some((attributes, tail)) = rest.split_once(')') else {
                continue;
            };
            if !attributes
                .split_whitespace()
                .any(|a| a.eq_ignore_ascii_case("\\Drafts"))
            {
                continue;
            }
            // Rest: ` "/" "Name"` oder ` "/" Name`.
            let tail = tail.trim();
            let name = tail.split_once(' ').map_or("", |(_, rest)| rest).trim();
            let name = name
                .strip_prefix('"')
                .and_then(|n| n.strip_suffix('"'))
                .unwrap_or(name);
            if !name.is_empty() {
                return Ok(name.replace("\\\"", "\"").replace("\\\\", "\\"));
            }
        }
        Err(MailError::NoDrafts)
    }

    /// Legt eine Mail als Entwurf ab (Flag `\Draft`). Es wird nichts gesendet.
    pub fn append_draft(&mut self, mailbox: &str, message: &[u8]) -> Result<(), MailError> {
        self.counter += 1;
        let tag = format!("A{}", self.counter);
        self.send(
            format!(
                "{tag} APPEND {} (\\Draft) {{{}}}\r\n",
                quote(mailbox)?,
                message.len()
            )
            .as_bytes(),
        )?;
        let continuation = self.read_line()?;
        if !continuation.starts_with('+') {
            return Err(MailError::Rejected(
                continuation.chars().take(120).collect(),
            ));
        }
        self.send(message)?;
        self.send(b"\r\n")?;
        self.finish(&tag).map(|_| ())
    }

    /// Beendet die Sitzung höflich; Fehler sind hier unwichtig.
    pub fn logout(&mut self) {
        let _ = self.command("LOGOUT");
    }
}

/// Suchkriterien. Es gibt nur feste Bausteine und zitierte Werte, nie rohen Text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search {
    pub unseen: bool,
    pub from: Option<String>,
    pub subject: Option<String>,
    /// Freitext im ganzen Inhalt.
    pub text: Option<String>,
}

impl Search {
    fn render(&self) -> Result<String, MailError> {
        let mut parts = Vec::new();
        if self.unseen {
            parts.push("UNSEEN".to_owned());
        }
        for (key, value) in [
            ("FROM", &self.from),
            ("SUBJECT", &self.subject),
            ("TEXT", &self.text),
        ] {
            if let Some(value) = value {
                parts.push(format!("{key} {}", quote(value)?));
            }
        }
        Ok(if parts.is_empty() {
            "ALL".to_owned()
        } else {
            parts.join(" ")
        })
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! Fake-Server für Tests: spielt vorbereitete Antworten ab und protokolliert Gesendetes.
    use std::io::{Cursor, Read, Write};
    use std::sync::{Arc, Mutex};

    pub struct FakeStream {
        input: Cursor<Vec<u8>>,
        pub sent: Arc<Mutex<Vec<u8>>>,
    }

    impl FakeStream {
        pub fn new(script: &[u8]) -> (Self, Arc<Mutex<Vec<u8>>>) {
            let sent = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    input: Cursor::new(script.to_vec()),
                    sent: Arc::clone(&sent),
                },
                sent,
            )
        }
    }

    impl Read for FakeStream {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(buf)
        }
    }

    impl Write for FakeStream {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.sent.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeStream;
    use super::*;

    fn sent_text(sent: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>) -> String {
        String::from_utf8_lossy(&sent.lock().unwrap()).into_owned()
    }

    #[test]
    fn login_sends_a_quoted_command_and_strips_spaces_from_the_app_password() {
        let (stream, sent) = FakeStream::new(b"* OK Gimap ready\r\nA1 OK user authenticated\r\n");
        let mut client = ImapClient::new(stream, 1 << 20).unwrap();
        client
            .login("anna@gmail.com", "abcd efgh ijkl mnop")
            .unwrap();
        assert_eq!(
            sent_text(&sent),
            "A1 LOGIN \"anna@gmail.com\" \"abcdefghijklmnop\"\r\n"
        );
    }

    #[test]
    fn rejected_credentials_become_an_auth_error() {
        let (stream, _) = FakeStream::new(
            b"* OK ready\r\nA1 NO [AUTHENTICATIONFAILED] Invalid credentials (Failure)\r\n",
        );
        let mut client = ImapClient::new(stream, 1 << 20).unwrap();
        assert!(matches!(client.login("a@b.c", "x"), Err(MailError::Auth)));
    }

    #[test]
    fn quoting_blocks_injection() {
        assert!(quote("a\r\nA2 DELETE INBOX").is_err());
        assert_eq!(quote("a\"b\\c").unwrap(), "\"a\\\"b\\\\c\"");
        let (stream, _) = FakeStream::new(b"* OK ready\r\n");
        let mut client = ImapClient::new(stream, 1 << 20).unwrap();
        assert!(client.login("a@b.c", "pw\nA9 LOGOUT").is_err());
    }

    #[test]
    fn a_bad_greeting_is_a_protocol_error() {
        let (stream, _) = FakeStream::new(b"* BYE go away\r\n");
        assert!(matches!(
            ImapClient::new(stream, 10),
            Err(MailError::Protocol(_))
        ));
    }

    #[test]
    fn inbox_is_opened_read_only() {
        let (stream, sent) =
            FakeStream::new(b"* OK ready\r\n* 3 EXISTS\r\nA1 OK [READ-ONLY] done\r\n");
        let mut client = ImapClient::new(stream, 1 << 20).unwrap();
        client.examine_inbox().unwrap();
        assert_eq!(sent_text(&sent), "A1 EXAMINE INBOX\r\n");
    }

    #[test]
    fn search_returns_sorted_uids_and_quotes_values() {
        let (stream, sent) = FakeStream::new(b"* OK ready\r\n* SEARCH 9 3 5\r\nA1 OK done\r\n");
        let mut client = ImapClient::new(stream, 1 << 20).unwrap();
        let uids = client
            .search(&Search {
                unseen: true,
                from: Some("anna@gmail.com".into()),
                ..Search::default()
            })
            .unwrap();
        assert_eq!(uids, vec![3, 5, 9]);
        assert_eq!(
            sent_text(&sent),
            "A1 UID SEARCH UNSEEN FROM \"anna@gmail.com\"\r\n"
        );
    }

    #[test]
    fn an_empty_search_is_all() {
        assert_eq!(Search::default().render().unwrap(), "ALL");
        assert!(Search {
            text: Some("x\ny".into()),
            ..Search::default()
        }
        .render()
        .is_err());
    }

    #[test]
    fn fetch_reads_the_literal_and_peeks_with_a_limit() {
        let mail = b"From: a@b.c\r\n\r\nHallo";
        let mut script = b"* OK ready\r\n".to_vec();
        script.extend_from_slice(
            format!("* 1 FETCH (UID 7 BODY[]<0> {{{}}}\r\n", mail.len()).as_bytes(),
        );
        script.extend_from_slice(mail);
        script.extend_from_slice(b")\r\nA1 OK Fetch completed\r\n");
        let (stream, sent) = FakeStream::new(&script);
        let mut client = ImapClient::new(stream, 1 << 20).unwrap();
        assert_eq!(client.fetch_message(7, 262_144).unwrap(), mail);
        assert_eq!(
            sent_text(&sent),
            "A1 UID FETCH 7 (BODY.PEEK[]<0.262144>)\r\n"
        );
    }

    #[test]
    fn oversized_literals_are_refused() {
        let mut script = b"* OK ready\r\n* 1 FETCH (BODY[] {5000}\r\n".to_vec();
        script.extend_from_slice(&[b'x'; 5000]);
        let (stream, _) = FakeStream::new(&script);
        let mut client = ImapClient::new(stream, 100).unwrap();
        assert!(matches!(
            client.fetch_message(1, 100),
            Err(MailError::TooLarge)
        ));
    }

    #[test]
    fn the_drafts_folder_is_found_by_its_attribute_not_by_name() {
        let script = b"* OK ready\r\n* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasNoChildren \\Drafts) \"/\" \"[Gmail]/Entw&APw-rfe\"\r\n* LIST (\\HasNoChildren \\Sent) \"/\" \"[Gmail]/Gesendet\"\r\nA1 OK done\r\n";
        let (stream, _) = FakeStream::new(script);
        let mut client = ImapClient::new(stream, 1 << 20).unwrap();
        assert_eq!(client.find_drafts().unwrap(), "[Gmail]/Entw&APw-rfe");
    }

    #[test]
    fn no_drafts_folder_is_an_error_not_a_guess() {
        let (stream, _) = FakeStream::new(
            b"* OK ready\r\n* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\nA1 OK done\r\n",
        );
        let mut client = ImapClient::new(stream, 1 << 20).unwrap();
        assert!(matches!(client.find_drafts(), Err(MailError::NoDrafts)));
    }

    #[test]
    fn append_waits_for_the_continuation_and_marks_the_message_as_draft() {
        let (stream, sent) = FakeStream::new(
            b"* OK ready\r\n+ Ready for literal data\r\nA1 OK [APPENDUID 1 2] done\r\n",
        );
        let mut client = ImapClient::new(stream, 1 << 20).unwrap();
        client
            .append_draft("[Gmail]/Drafts", b"Subject: Re: x\r\n\r\nHi")
            .unwrap();
        let text = sent_text(&sent);
        assert!(text.starts_with("A1 APPEND \"[Gmail]/Drafts\" (\\Draft) {20}\r\n"));
        assert!(text.ends_with("Subject: Re: x\r\n\r\nHi\r\n"));
    }

    #[test]
    fn a_refused_append_is_reported() {
        let (stream, _) = FakeStream::new(b"* OK ready\r\nA1 NO [TRYCREATE] no such mailbox\r\n");
        let mut client = ImapClient::new(stream, 1 << 20).unwrap();
        assert!(client.append_draft("x", b"m").is_err());
    }

    #[test]
    fn a_dropped_connection_is_an_io_error() {
        let (stream, _) = FakeStream::new(b"* OK ready\r\n");
        let mut client = ImapClient::new(stream, 1 << 20).unwrap();
        assert!(matches!(client.examine_inbox(), Err(MailError::Io(_))));
    }

    #[test]
    fn an_endless_line_is_cut_off() {
        let mut script = b"* OK ready\r\n".to_vec();
        script.extend(std::iter::repeat_n(b'x', MAX_LINE * 2));
        let (stream, _) = FakeStream::new(&script);
        let mut client = ImapClient::new(stream, 1 << 20).unwrap();
        assert!(matches!(client.examine_inbox(), Err(MailError::TooLarge)));
    }
}
