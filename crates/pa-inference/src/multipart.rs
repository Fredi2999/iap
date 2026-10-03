//! Aufbau eines `multipart/form-data`-Körpers im Arbeitsspeicher.
//!
//! Die lokale Spracherkennung (`whisper-server`) erwartet das Audio als
//! Formularfeld. Der Körper wird direkt aus den PCM-Bytes gebaut; Audio wird
//! dabei nie in eine Datei geschrieben.

use rand_core::{OsRng, RngCore};

/// Fertiger Körper samt passendem `Content-Type`-Wert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartBody {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// Eine Datei im Formular.
pub struct FilePart<'a> {
    pub field: &'a str,
    pub file_name: &'a str,
    pub content_type: &'a str,
    pub data: &'a [u8],
}

/// Baut den Körper aus einfachen Textfeldern und genau einer Datei.
///
/// Feld- und Dateinamen dürfen keine Anführungszeichen oder Zeilenumbrüche
/// enthalten (sonst wäre Header-Einschleusung möglich); sie stammen hier
/// ausschließlich aus fest einprogrammierten Werten.
pub fn build(fields: &[(&str, &str)], file: &FilePart<'_>) -> MultipartBody {
    let mut random = [0_u8; 12];
    OsRng.fill_bytes(&mut random);
    let boundary = format!("----iap{}", hex::encode(random));
    let mut bytes = Vec::with_capacity(file.data.len() + 512);
    for (name, value) in fields {
        bytes.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    bytes.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
            file.field, file.file_name, file.content_type
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(file.data);
    bytes.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    MultipartBody {
        content_type: format!("multipart/form-data; boundary={boundary}"),
        bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_contains_fields_file_and_closing_boundary() {
        let body = build(
            &[("language", "de"), ("response_format", "json")],
            &FilePart {
                field: "file",
                file_name: "audio.wav",
                content_type: "audio/wav",
                data: b"RIFFDATA",
            },
        );
        let text = String::from_utf8_lossy(&body.bytes).into_owned();
        let boundary = body
            .content_type
            .split("boundary=")
            .nth(1)
            .expect("Boundary");
        assert!(text.contains("name=\"language\"\r\n\r\nde\r\n"));
        assert!(text.contains("name=\"file\"; filename=\"audio.wav\""));
        assert!(text.contains("RIFFDATA"));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    }

    #[test]
    fn every_body_gets_its_own_boundary() {
        let part = FilePart {
            field: "f",
            file_name: "a",
            content_type: "x",
            data: b"",
        };
        assert_ne!(
            build(&[], &part).content_type,
            build(&[], &part).content_type
        );
    }
}
