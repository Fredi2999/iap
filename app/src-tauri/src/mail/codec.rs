//! Kleine Kodierungs-Bausteine für Mails: Base64, Quoted-Printable, RFC-2047-Wörter,
//! Zeichensätze und das Datum im Mailformat.
//!
//! Handgeschrieben, weil nur ein winziger Ausschnitt gebraucht wird und jede Bibliothek
//! Binärgröße kostet (AGENTS.md). Die Funktionen sind bewusst nachsichtig beim Lesen
//! (kaputte Mails dürfen nie abstürzen) und streng beim Schreiben.

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64 mit `=`-Füllung, ohne Zeilenumbrüche.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        out.push(B64[usize::from(b[0] >> 2)] as char);
        out.push(B64[usize::from(((b[0] & 3) << 4) | (b[1] >> 4))] as char);
        out.push(if chunk.len() > 1 {
            B64[usize::from(((b[1] & 15) << 2) | (b[2] >> 6))] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[usize::from(b[2] & 63)] as char
        } else {
            '='
        });
    }
    out
}

/// Base64 in Zeilen zu höchstens 76 Zeichen, getrennt durch CRLF (für Mailrümpfe).
pub fn base64_wrapped(data: &[u8]) -> String {
    let encoded = base64_encode(data);
    encoded
        .as_bytes()
        .chunks(76)
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// Base64 lesen. Leerraum und unbekannte Zeichen werden übersprungen, damit Zeilenumbrüche und
/// abgeschnittene Mails keinen Fehler auslösen.
pub fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer = 0_u32;
    let mut bits = 0_u8;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => continue,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    out
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Quoted-Printable lesen. `header` stellt für RFC-2047 `_` als Leerzeichen dar.
pub fn quoted_printable_decode(text: &str, header: bool) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'=' => {
                // Weicher Zeilenumbruch: `=` am Zeilenende.
                if bytes.get(i + 1) == Some(&b'\r') && bytes.get(i + 2) == Some(&b'\n') {
                    i += 3;
                } else if bytes.get(i + 1) == Some(&b'\n') {
                    i += 2;
                } else if let (Some(h), Some(l)) = (
                    bytes.get(i + 1).and_then(|b| hex_value(*b)),
                    bytes.get(i + 2).and_then(|b| hex_value(*b)),
                ) {
                    out.push(h * 16 + l);
                    i += 3;
                } else {
                    out.push(b'=');
                    i += 1;
                }
            }
            b'_' if header => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

/// Bytes in Text wandeln. UTF-8 und Latin-1/Windows-1252 werden verstanden, alles andere
/// ersetzt unlesbare Zeichen. Weitere Zeichensätze sind für den Zweck (kurze Textmails
/// vom eigenen Konto) nicht nötig und würden eine Bibliothek kosten.
pub fn decode_charset(bytes: &[u8], charset: &str) -> String {
    let name = charset.trim().trim_matches('"').to_ascii_lowercase();
    match name.as_str() {
        "iso-8859-1" | "latin1" | "iso-8859-15" | "windows-1252" | "cp1252" | "us-ascii" => {
            bytes.iter().map(|b| char::from(*b)).collect()
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// RFC-2047-Wörter (`=?utf-8?B?...?=`) in einem Kopfzeilenwert auflösen.
pub fn decode_encoded_words(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let (before, tail) = rest.split_at(start);
        // Leerraum zwischen zwei kodierten Wörtern gehört nicht zum Text.
        if !(last_was_word && before.trim().is_empty()) {
            out.push_str(before);
        }
        let parts: Vec<&str> = tail[2..].splitn(4, '?').collect();
        if parts.len() == 4 && parts[3].starts_with('=') {
            let (charset, encoding, data) = (parts[0], parts[1], parts[2]);
            let decoded = match encoding {
                "B" | "b" => Some(base64_decode(data)),
                "Q" | "q" => Some(quoted_printable_decode(data, true)),
                _ => None,
            };
            if let Some(bytes) = decoded {
                out.push_str(&decode_charset(&bytes, charset));
                rest = &parts[3][1..];
                last_was_word = true;
                continue;
            }
        }
        out.push_str("=?");
        rest = &tail[2..];
        last_was_word = false;
    }
    out.push_str(rest);
    out
}

/// Kopfzeilenwert für die Ausgabe: reines ASCII bleibt stehen, sonst ein UTF-8-Base64-Wort.
/// Zeilenumbrüche werden entfernt, damit nichts in andere Kopfzeilen hineinragen kann.
pub fn encode_header_value(value: &str) -> String {
    let clean: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let clean = clean.trim();
    if clean.is_ascii() {
        clean.to_owned()
    } else {
        format!("=?utf-8?B?{}?=", base64_encode(clean.as_bytes()))
    }
}

/// Tage seit 1970-01-01 in (Jahr, Monat, Tag), bürgerlicher Kalender (Algorithmus von Hinnant).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (year + i64::from(month <= 2), month, day)
}

/// Datum für die `Date`-Kopfzeile (RFC 5322), immer in UTC.
pub fn rfc2822_date(unix_secs: i64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = unix_secs.div_euclid(86_400);
    let secs = unix_secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{}, {day:02} {} {year} {:02}:{:02}:{:02} +0000",
        DAYS[days.rem_euclid(7) as usize],
        MONTHS[(month - 1) as usize],
        secs / 3_600,
        secs % 3_600 / 60,
        secs % 60
    )
}

/// Grobe Textfassung einer HTML-Mail: Tags, Skripte und Stile entfernen, Absätze erhalten.
/// Absichtlich einfach; die Ausgabe geht als Text an das Modell, nie in ein Dokument.
pub fn html_to_text(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len() / 2);
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let skip_block = ["script", "style"]
                .iter()
                .find(|name| lower[i + 1..].starts_with(*name));
            if let Some(name) = skip_block {
                let close = format!("</{name}");
                i = lower[i..].find(&close).map_or(bytes.len(), |p| i + p);
            }
            let end = html[i..].find('>').map_or(bytes.len(), |p| i + p + 1);
            let tag = lower[i..end.min(lower.len())].trim_start_matches('<');
            if ["br", "/p", "/div", "/tr", "/li", "p", "li"]
                .iter()
                .any(|t| tag.starts_with(t))
            {
                out.push('\n');
            }
            i = end;
        } else {
            let next = html[i..].find('<').map_or(bytes.len(), |p| i + p);
            out.push_str(&html[i..next]);
            i = next;
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let mut cleaned = String::new();
    let mut blank = 0;
    for line in out.lines() {
        let line = line.trim();
        if line.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        cleaned.push_str(line);
        cleaned.push('\n');
    }
    cleaned.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_all_padding_cases() {
        for sample in ["", "f", "fo", "foo", "foob", "fooba", "foobar", "äöü €"] {
            assert_eq!(
                base64_decode(&base64_encode(sample.as_bytes())),
                sample.as_bytes()
            );
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
    }

    #[test]
    fn base64_decode_ignores_whitespace_and_garbage() {
        assert_eq!(base64_decode("Zm9v\r\nYmFy"), b"foobar");
        assert_eq!(base64_decode("Zm9v!!!YmFy"), b"foobar");
    }

    #[test]
    fn wrapped_base64_lines_are_at_most_76_characters() {
        let text = base64_wrapped(&vec![b'a'; 500]);
        assert!(text.split("\r\n").all(|line| line.len() <= 76));
        assert_eq!(base64_decode(&text), vec![b'a'; 500]);
    }

    #[test]
    fn quoted_printable_handles_soft_breaks_and_hex() {
        assert_eq!(
            quoted_printable_decode("Gr=C3=BC=C3=9Fe=\r\n dich", false),
            "Grüße dich".as_bytes()
        );
        assert_eq!(quoted_printable_decode("a=ZZb", false), b"a=ZZb");
        assert_eq!(quoted_printable_decode("a_b", true), b"a b");
        assert_eq!(quoted_printable_decode("a_b", false), b"a_b");
    }

    #[test]
    fn encoded_words_are_decoded_and_joined() {
        assert_eq!(decode_encoded_words("=?UTF-8?B?w6TDtsO8?="), "äöü");
        assert_eq!(
            decode_encoded_words("=?utf-8?Q?Gr=C3=BC=C3=9Fe?= Welt"),
            "Grüße Welt"
        );
        assert_eq!(
            decode_encoded_words("=?iso-8859-1?Q?M=FCnchen?="),
            "München"
        );
        // Leerraum zwischen zwei kodierten Wörtern verschwindet.
        assert_eq!(decode_encoded_words("=?utf-8?Q?a?= =?utf-8?Q?b?="), "ab");
        assert_eq!(decode_encoded_words("kein =? Wort"), "kein =? Wort");
    }

    #[test]
    fn header_values_are_encoded_only_when_needed_and_never_contain_line_breaks() {
        assert_eq!(encode_header_value("Re: Plan"), "Re: Plan");
        let encoded = encode_header_value("Re: Grüße");
        assert!(encoded.starts_with("=?utf-8?B?"));
        assert_eq!(decode_encoded_words(&encoded), "Re: Grüße");
        assert_eq!(encode_header_value("a\r\nBcc: x@y.z"), "a  Bcc: x@y.z");
    }

    #[test]
    fn dates_are_formatted_in_mail_style() {
        assert_eq!(rfc2822_date(0), "Thu, 01 Jan 1970 00:00:00 +0000");
        assert_eq!(
            rfc2822_date(1_700_000_000),
            "Tue, 14 Nov 2023 22:13:20 +0000"
        );
        assert_eq!(rfc2822_date(951_782_400), "Tue, 29 Feb 2000 00:00:00 +0000");
    }

    #[test]
    fn html_is_reduced_to_readable_text() {
        let text = html_to_text("<html><style>p{color:red}</style><body><p>Hallo&nbsp;Welt</p><script>alert(1)</script><p>a &amp; b</p></body></html>");
        assert!(text.contains("Hallo Welt") && text.contains("a & b"));
        assert!(!text.contains("alert") && !text.contains("color"));
    }
}
