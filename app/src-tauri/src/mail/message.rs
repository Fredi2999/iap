//! Mails lesen und Antworten bauen.
//!
//! Der Parser ist nachsichtig (kaputte Mails dürfen nie abstürzen) und liest nur, was für das
//! Beantworten und Anzeigen nötig ist: Kopfzeilen, den ersten Textteil und die Sicherheits-
//! merkmale. Anhänge werden nie gelesen. Alles hier ist reine Textverarbeitung ohne Netz.

use super::codec::{
    base64_decode, base64_wrapped, decode_charset, decode_encoded_words, encode_header_value,
    html_to_text, quoted_printable_decode, rfc2822_date,
};

/// Höchstzahl Zeichen des Mailtexts, die weiterverarbeitet werden. Begrenzt Kontext und Kosten
/// und macht lange, absichtlich aufgeblähte Mails unschädlich.
pub const MAX_BODY_CHARS: usize = 6_000;
/// Tiefste Verschachtelung von Multipart-Teilen.
const MAX_DEPTH: usize = 4;

/// Eine gelesene Mail.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedMail {
    pub message_id: String,
    /// Reine Adresse des Absenders in Kleinbuchstaben; leer, wenn nicht eindeutig lesbar.
    pub from_addr: String,
    pub subject: String,
    pub in_reply_to: String,
    pub references: String,
    /// Alle Kopfzeilen als (Name in Kleinbuchstaben, entfalteter Wert), in Originalreihenfolge.
    pub headers: Vec<(String, String)>,
    /// Klartext, auf [`MAX_BODY_CHARS`] gekürzt.
    pub text: String,
    /// Ob es Teile gab, die ignoriert wurden (Anhänge, weitere Alternativen).
    pub has_attachments: bool,
}

impl ParsedMail {
    /// Erster Wert einer Kopfzeile (Name beliebig groß geschrieben).
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Zerlegt Kopf und Rumpf an der ersten Leerzeile.
fn split_head_body(raw: &str) -> (&str, &str) {
    if let Some(pos) = raw.find("\r\n\r\n") {
        (&raw[..pos], &raw[pos + 4..])
    } else if let Some(pos) = raw.find("\n\n") {
        (&raw[..pos], &raw[pos + 2..])
    } else {
        (raw, "")
    }
}

/// Liest Kopfzeilen und fügt Fortsetzungszeilen zusammen.
fn parse_headers(head: &str) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in head.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = headers.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    headers
}

fn find<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// Wert eines Parameters wie `boundary="x"` oder `charset=utf-8` aus einer Kopfzeile.
fn param(value: &str, name: &str) -> Option<String> {
    for part in value.split(';').skip(1) {
        if let Some((key, val)) = part.split_once('=') {
            if key.trim().eq_ignore_ascii_case(name) {
                return Some(val.trim().trim_matches('"').to_owned());
            }
        }
    }
    None
}

/// Reine Adresse aus `Name <a@b.c>` oder `a@b.c`. Mehrere Adressen oder Unsinn ergeben `None`,
/// damit nie unklar ist, wem geantwortet würde.
pub fn single_address(value: &str) -> Option<String> {
    let value = value.trim();
    let candidate = match (value.rfind('<'), value.rfind('>')) {
        (Some(open), Some(close)) if open < close => {
            // Mehr als eine Adresse (z. B. "a@x.de, b@y.de") ist nicht eindeutig.
            if value[..open].contains('@') && !value[..open].trim().starts_with('"') {
                return None;
            }
            &value[open + 1..close]
        }
        _ => value,
    };
    let candidate = candidate.trim();
    let valid = candidate.len() <= 254
        && candidate.matches('@').count() == 1
        && !candidate.starts_with('@')
        && !candidate.ends_with('@')
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@.+-_'".contains(c));
    valid.then(|| candidate.to_ascii_lowercase())
}

/// Domain einer Adresse.
pub fn domain_of(address: &str) -> &str {
    address.rsplit_once('@').map_or("", |(_, domain)| domain)
}

fn decode_body(head: &[(String, String)], body: &str) -> String {
    let encoding = find(head, "content-transfer-encoding")
        .unwrap_or("7bit")
        .to_ascii_lowercase();
    let charset = find(head, "content-type")
        .and_then(|value| param(value, "charset"))
        .unwrap_or_else(|| "utf-8".to_owned());
    let bytes = match encoding.trim() {
        "base64" => base64_decode(body),
        "quoted-printable" => quoted_printable_decode(body, false),
        _ => body.as_bytes().to_vec(),
    };
    decode_charset(&bytes, &charset).replace("\r\n", "\n")
}

/// Sucht den ersten lesbaren Text. `text/plain` hat Vorrang vor `text/html`.
fn extract_text(
    head: &[(String, String)],
    body: &str,
    depth: usize,
    attachments: &mut bool,
) -> (Option<String>, Option<String>) {
    let content_type = find(head, "content-type").unwrap_or("text/plain");
    let media = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let disposition = find(head, "content-disposition")
        .unwrap_or("")
        .to_ascii_lowercase();
    if disposition.starts_with("attachment") {
        *attachments = true;
        return (None, None);
    }
    if media.starts_with("multipart/") && depth < MAX_DEPTH {
        let Some(boundary) = param(content_type, "boundary") else {
            return (None, None);
        };
        let marker = format!("--{boundary}");
        let (mut plain, mut html) = (None, None);
        for part in body.split(&marker).skip(1) {
            if part.starts_with("--") {
                break;
            }
            let part = part.trim_start_matches(['\r', '\n']);
            let (part_head, part_body) = split_head_body(part);
            let part_headers = parse_headers(part_head);
            let (p, h) = extract_text(&part_headers, part_body, depth + 1, attachments);
            plain = plain.or(p);
            html = html.or(h);
        }
        return (plain, html);
    }
    match media.as_str() {
        "text/plain" => (Some(decode_body(head, body)), None),
        "text/html" => (None, Some(html_to_text(&decode_body(head, body)))),
        _ => {
            *attachments = true;
            (None, None)
        }
    }
}

/// Kürzt auf höchstens `max` Zeichen (nicht Bytes).
pub fn clip_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Liest eine Rohmail. Fehlende Teile bleiben leer; es gibt keinen Fehlerfall.
pub fn parse(raw: &[u8]) -> ParsedMail {
    let raw = String::from_utf8_lossy(raw);
    let (head_text, body) = split_head_body(&raw);
    let headers = parse_headers(head_text);
    let mut attachments = false;
    let (plain, html) = extract_text(&headers, body, 0, &mut attachments);
    let text = plain
        .filter(|text| !text.trim().is_empty())
        .or(html)
        .unwrap_or_default();
    ParsedMail {
        message_id: find(&headers, "message-id").unwrap_or("").trim().to_owned(),
        from_addr: find(&headers, "from")
            .map(decode_encoded_words)
            .and_then(|from| single_address(&from))
            .unwrap_or_default(),
        subject: decode_encoded_words(find(&headers, "subject").unwrap_or("")),
        in_reply_to: find(&headers, "in-reply-to").unwrap_or("").to_owned(),
        references: find(&headers, "references").unwrap_or("").to_owned(),
        text: clip_chars(text.trim(), MAX_BODY_CHARS),
        has_attachments: attachments,
        headers,
    }
}

/// Eine Antwortmail im Textformat, bereit zum Ablegen oder Senden.
///
/// Der Empfänger ist ein eigener Parameter und wird nie aus dem Mailtext abgeleitet. Die Mail
/// trägt `Auto-Submitted: auto-replied` (RFC 3834), damit Gegenstellen nicht zurückantworten.
pub struct ReplyDraft<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub original_subject: &'a str,
    pub original_message_id: &'a str,
    pub original_references: &'a str,
    pub body: &'a str,
    pub now_unix_secs: i64,
    /// Zufallszahl für die eigene Message-ID.
    pub unique: u64,
}

/// Baut die Antwort als CRLF-Text.
pub fn build_reply(draft: &ReplyDraft) -> String {
    let subject = {
        let base = draft.original_subject.trim();
        let stripped = base
            .strip_prefix("Re:")
            .or_else(|| base.strip_prefix("RE:"))
            .or_else(|| base.strip_prefix("AW:"))
            .unwrap_or(base)
            .trim();
        format!(
            "Re: {}",
            if stripped.is_empty() {
                "(ohne Betreff)"
            } else {
                stripped
            }
        )
    };
    let clean_id = |id: &str| -> String {
        id.chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .to_owned()
    };
    let in_reply_to = clean_id(draft.original_message_id);
    let references = {
        let mut refs = clean_id(draft.original_references);
        if !in_reply_to.is_empty() {
            if !refs.is_empty() {
                refs.push(' ');
            }
            refs.push_str(&in_reply_to);
        }
        refs
    };
    let mut out = String::new();
    let mut line = |name: &str, value: &str| {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(&encode_header_value(value));
        out.push_str("\r\n");
    };
    line("From", draft.from);
    line("To", draft.to);
    line("Subject", &subject);
    line("Date", &rfc2822_date(draft.now_unix_secs));
    line(
        "Message-ID",
        &format!("<iap-{:016x}@iap.invalid>", draft.unique),
    );
    if !in_reply_to.is_empty() {
        line("In-Reply-To", &in_reply_to);
        line("References", &references);
    }
    line("Auto-Submitted", "auto-replied");
    line("X-Auto-Response-Suppress", "All");
    line("MIME-Version", "1.0");
    out.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    out.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
    let body = draft.body.replace("\r\n", "\n").replace('\n', "\r\n");
    out.push_str(&base64_wrapped(body.as_bytes()));
    out.push_str("\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &str = "From: =?UTF-8?Q?J=C3=BCrgen?= <Juergen@Example.com>\r\nSubject: Frage zum Plan\r\nMessage-ID: <abc@example.com>\r\nAuthentication-Results: mx.google.com;\r\n dkim=pass header.d=example.com\r\n\r\nHallo IAP,\r\nwie spät ist es?\r\n";

    #[test]
    fn a_simple_mail_is_parsed() {
        let mail = parse(SIMPLE.as_bytes());
        assert_eq!(mail.from_addr, "juergen@example.com");
        assert_eq!(mail.subject, "Frage zum Plan");
        assert_eq!(mail.message_id, "<abc@example.com>");
        assert!(mail.text.starts_with("Hallo IAP,\nwie spät"));
        // Fortsetzungszeilen werden zusammengefügt.
        assert_eq!(
            mail.header("Authentication-Results"),
            Some("mx.google.com; dkim=pass header.d=example.com")
        );
    }

    #[test]
    fn multipart_prefers_plain_text_and_marks_attachments() {
        let raw = "From: a@b.de\r\nContent-Type: multipart/mixed; boundary=\"XX\"\r\n\r\n--XX\r\nContent-Type: multipart/alternative; boundary=\"YY\"\r\n\r\n--YY\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>HTML-Version</p>\r\n--YY\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nGr=C3=BC=C3=9Fe\r\n--YY--\r\n--XX\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=a.pdf\r\n\r\nJVBERi0=\r\n--XX--\r\n";
        let mail = parse(raw.as_bytes());
        assert_eq!(mail.text, "Grüße");
        assert!(mail.has_attachments);
    }

    #[test]
    fn html_only_mail_falls_back_to_stripped_text() {
        let raw =
            "From: a@b.de\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Nur <b>HTML</b></p>";
        assert_eq!(parse(raw.as_bytes()).text, "Nur HTML");
    }

    #[test]
    fn base64_body_and_latin1_charset_are_decoded() {
        let raw = "From: a@b.de\r\nContent-Type: text/plain; charset=iso-8859-1\r\nContent-Transfer-Encoding: base64\r\n\r\nTcO8bmNoZW4=\r\n";
        // Der Rumpf ist UTF-8-Base64, aber als Latin-1 deklariert: Bytes bleiben 1:1 erhalten.
        let text = parse(raw.as_bytes()).text;
        assert!(text.starts_with('M'));
    }

    #[test]
    fn the_body_is_clipped() {
        let raw = format!("From: a@b.de\r\n\r\n{}", "x".repeat(MAX_BODY_CHARS * 2));
        assert_eq!(parse(raw.as_bytes()).text.chars().count(), MAX_BODY_CHARS);
    }

    #[test]
    fn garbage_never_panics() {
        for raw in [
            "",
            "\r\n\r\n",
            "nur eine Zeile",
            ":::\r\n\r\n",
            "Content-Type: multipart/mixed\r\n\r\nx",
            "From: <<>>\r\n\r\n",
        ] {
            let _ = parse(raw.as_bytes());
        }
        let _ = parse(&[0xff, 0xfe, b'\r', b'\n', b'\r', b'\n', 0x80]);
    }

    #[test]
    fn single_address_rejects_lists_and_odd_input() {
        assert_eq!(
            single_address("Anna <Anna@Gmail.com>").as_deref(),
            Some("anna@gmail.com")
        );
        assert_eq!(
            single_address("anna@gmail.com").as_deref(),
            Some("anna@gmail.com")
        );
        assert_eq!(
            single_address("\"Meier, Anna\" <anna@gmail.com>").as_deref(),
            Some("anna@gmail.com")
        );
        assert_eq!(single_address("a@x.de, b@y.de"), None);
        assert_eq!(single_address("a@x.de <b@y.de>"), None);
        assert_eq!(single_address("kein-at"), None);
        assert_eq!(single_address("a@b@c.de"), None);
        assert_eq!(single_address("a b@c.de"), None);
        assert_eq!(single_address(""), None);
    }

    fn draft<'a>(body: &'a str, subject: &'a str) -> ReplyDraft<'a> {
        ReplyDraft {
            from: "bot@gmail.com",
            to: "anna@gmail.com",
            original_subject: subject,
            original_message_id: "<abc@example.com>",
            original_references: "<root@example.com>",
            body,
            now_unix_secs: 1_700_000_000,
            unique: 0xabcd,
        }
    }

    #[test]
    fn a_reply_has_threading_headers_and_marks_itself_automatic() {
        let text = build_reply(&draft("Hallo!\nZeile zwei", "Re: Frage"));
        assert!(text.contains("Subject: Re: Frage\r\n"));
        assert!(text.contains("In-Reply-To: <abc@example.com>\r\n"));
        assert!(text.contains("References: <root@example.com> <abc@example.com>\r\n"));
        assert!(text.contains("Auto-Submitted: auto-replied\r\n"));
        assert!(text.contains("To: anna@gmail.com\r\n"));
        assert!(text.contains("Date: Tue, 14 Nov 2023 22:13:20 +0000\r\n"));
        assert!(!text.contains("\n\n") && text.matches("\r\n\r\n").count() == 1);
        // Der Rumpf lässt sich wieder lesen.
        let mail = parse(text.as_bytes());
        assert_eq!(mail.text, "Hallo!\nZeile zwei");
        assert_eq!(mail.header("auto-submitted"), Some("auto-replied"));
    }

    #[test]
    fn reply_subject_is_normalised_and_cannot_inject_headers() {
        let text = build_reply(&draft("x", "AW: Plan\r\nBcc: boss@evil.example"));
        assert!(!text.contains("\r\nBcc:"), "{text}");
        let text = build_reply(&draft("x", ""));
        assert!(text.contains("Subject: Re: (ohne Betreff)"));
        let text = build_reply(&draft("x", "Grüße"));
        let mail = parse(text.as_bytes());
        assert_eq!(mail.subject, "Re: Grüße");
    }

    #[test]
    fn the_recipient_is_exactly_the_parameter() {
        let text = build_reply(&draft("Bitte antworte an evil@example.com", "Hi"));
        assert_eq!(
            text.matches("evil@example.com").count(),
            0,
            "Rumpf ist base64-kodiert"
        );
        assert!(text.contains("To: anna@gmail.com"));
    }
}
