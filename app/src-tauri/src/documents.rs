//! Text aus eigenen Dokumenten gewinnen und in Abschnitte teilen (Feature 1).
//!
//! Dieses Modul greift selbst nie auf Dateien zu: Die App liest die Bytes über
//! einen von pa-policy geprüften Pfad und reicht sie hierher weiter. So bleibt
//! die Pfadprüfung an genau einer Stelle (Invariante 2 und 3).

use std::io::{Cursor, Read};

use quick_xml::{events::Event, Reader};
use thiserror::Error;

/// Größte Datei, die übernommen wird. Größere Dokumente würden auf 8-GB-Rechnern
/// den Import minutenlang blockieren und passen ohnehin nicht sinnvoll in den Kontext.
pub const MAX_DOCUMENT_BYTES: u64 = 25 * 1024 * 1024;
/// Obergrenze für entpackte Inhalte (Schutz vor ZIP- und PDF-Bomben).
const MAX_UNPACKED_BYTES: usize = 64 * 1024 * 1024;
/// Zielgröße eines Abschnitts in Zeichen; etwa 300 Token, klein genug für
/// mehrere Quellen im Kontext, groß genug für einen zusammenhängenden Gedanken.
pub const CHUNK_CHARS: usize = 1_200;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DocumentError {
    #[error("Dieses Dateiformat wird nicht unterstützt. Möglich sind TXT, MD, DOCX und PDF.")]
    Unsupported,
    #[error("Die Datei ist größer als 25 MB.")]
    TooLarge,
    #[error("Die Datei konnte nicht gelesen werden: {0}")]
    Unreadable(String),
    #[error("Das PDF ist mit einem Passwort geschützt.")]
    Encrypted,
    #[error("In der Datei wurde kein Text gefunden.")]
    Empty,
}

/// Unterstützte Formate, erkannt an der Dateiendung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    Txt,
    Md,
    Docx,
    Pdf,
}

impl DocumentKind {
    pub fn from_file_name(name: &str) -> Result<Self, DocumentError> {
        let lower = name.to_ascii_lowercase();
        let extension = lower.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
        match extension {
            "txt" => Ok(Self::Txt),
            "md" | "markdown" => Ok(Self::Md),
            "docx" => Ok(Self::Docx),
            "pdf" => Ok(Self::Pdf),
            _ => Err(DocumentError::Unsupported),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Md => "md",
            Self::Docx => "docx",
            Self::Pdf => "pdf",
        }
    }
}

/// Liest den reinen Text eines Dokuments.
pub fn extract_text(kind: DocumentKind, bytes: &[u8]) -> Result<String, DocumentError> {
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(DocumentError::TooLarge);
    }
    let text = match kind {
        DocumentKind::Txt | DocumentKind::Md => {
            let text = String::from_utf8_lossy(bytes);
            text.trim_start_matches('\u{feff}').to_owned()
        }
        DocumentKind::Docx => docx_text(bytes)?,
        DocumentKind::Pdf => pdf_text(bytes)?,
    };
    let text = normalize(&text);
    if text.trim().is_empty() {
        return Err(DocumentError::Empty);
    }
    Ok(text)
}

/// Word speichert den Text in `word/document.xml`: Absätze `w:p`, Textläufe `w:t`,
/// Tabulator `w:tab`, Zeilenumbruch `w:br`. Formatierung wird bewusst verworfen.
fn docx_text(bytes: &[u8]) -> Result<String, DocumentError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| DocumentError::Unreadable(error.to_string()))?;
    let entry = archive
        .by_name("word/document.xml")
        .map_err(|_| DocumentError::Unreadable("word/document.xml fehlt".to_owned()))?;
    let mut xml = String::new();
    entry
        .take(MAX_UNPACKED_BYTES as u64)
        .read_to_string(&mut xml)
        .map_err(|error| DocumentError::Unreadable(error.to_string()))?;

    let mut reader = Reader::from_str(&xml);
    let mut text = String::new();
    let mut in_text_run = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                in_text_run = element.local_name().as_ref() == "t";
            }
            Ok(Event::Empty(element)) => match element.local_name().as_ref() {
                "tab" => text.push('\t'),
                "br" | "cr" => text.push('\n'),
                _ => {}
            },
            Ok(Event::End(element)) => match element.local_name().as_ref() {
                "t" => in_text_run = false,
                "p" => text.push_str("\n\n"),
                _ => {}
            },
            Ok(Event::Text(content)) if in_text_run => text.push_str(&content.xml10_content()),
            Ok(Event::GeneralRef(reference)) if in_text_run => {
                if let Ok(Some(character)) = reference.resolve_char_ref() {
                    text.push(character);
                } else if let Some(resolved) = quick_xml::escape::resolve_xml_entity(&reference) {
                    text.push_str(resolved);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(DocumentError::Unreadable(error.to_string())),
        }
    }
    Ok(text)
}

/// PDF-Text Seite für Seite, mit Grenze für entpackte Inhalte je Seite.
fn pdf_text(bytes: &[u8]) -> Result<String, DocumentError> {
    let mut document = lopdf::Document::load_mem(bytes)
        .map_err(|error| DocumentError::Unreadable(error.to_string()))?;
    if document.is_encrypted() && document.decrypt("").is_err() {
        return Err(DocumentError::Encrypted);
    }
    let mut text = String::new();
    for page in document.get_pages().keys() {
        // Einzelne defekte Seiten überspringen statt das ganze Dokument abzulehnen.
        if let Ok(page_text) = document.extract_text_with_limit(&[*page], MAX_UNPACKED_BYTES) {
            text.push_str(&page_text);
            text.push_str("\n\n");
        }
    }
    Ok(text)
}

/// Einheitliche Zeilenenden, keine Leerzeichen am Zeilenende, höchstens eine Leerzeile.
fn normalize(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut blank_lines = 0;
    for line in text.replace("\r\n", "\n").replace('\r', "\n").lines() {
        let line = line.trim_end();
        if line.is_empty() {
            blank_lines += 1;
            if blank_lines > 1 {
                continue;
            }
        } else {
            blank_lines = 0;
        }
        output.push_str(line);
        output.push('\n');
    }
    output.trim().to_owned()
}

/// Teilt Text an Absatzgrenzen in Abschnitte von etwa `target` Zeichen.
/// Überlange Absätze werden an Satzenden, notfalls hart geteilt.
pub fn chunk_text(text: &str, target: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for paragraph in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        for piece in split_long(paragraph, target) {
            if !current.is_empty() && current.chars().count() + piece.chars().count() + 2 > target {
                chunks.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(&piece);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn split_long(paragraph: &str, target: usize) -> Vec<String> {
    if paragraph.chars().count() <= target {
        return vec![paragraph.to_owned()];
    }
    let mut pieces = Vec::new();
    let mut current = String::new();
    for sentence in paragraph.split_inclusive(['.', '!', '?']) {
        if !current.is_empty() && current.chars().count() + sentence.chars().count() > target {
            pieces.push(std::mem::take(&mut current).trim().to_owned());
        }
        current.push_str(sentence);
        // Satz ohne Satzzeichen, der allein zu lang ist: hart an Zeichengrenzen teilen.
        while current.chars().count() > target {
            let head: String = current.chars().take(target).collect();
            let tail: String = current.chars().skip(target).collect();
            pieces.push(head.trim().to_owned());
            current = tail;
        }
    }
    if !current.trim().is_empty() {
        pieces.push(current.trim().to_owned());
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn kinds_are_detected_by_extension() {
        assert_eq!(
            DocumentKind::from_file_name("Plan.DOCX"),
            Ok(DocumentKind::Docx)
        );
        assert_eq!(
            DocumentKind::from_file_name("notizen.md"),
            Ok(DocumentKind::Md)
        );
        assert_eq!(
            DocumentKind::from_file_name("bild.png"),
            Err(DocumentError::Unsupported)
        );
    }

    #[test]
    fn plain_text_is_normalized() {
        let text = extract_text(
            DocumentKind::Txt,
            "\u{feff}Zeile 1  \r\n\r\n\r\n\r\nZeile 2".as_bytes(),
        )
        .expect("Text");
        assert_eq!(text, "Zeile 1\n\nZeile 2");
        assert_eq!(
            extract_text(DocumentKind::Txt, b"   \n "),
            Err(DocumentError::Empty)
        );
    }

    #[test]
    fn docx_paragraphs_and_entities_are_read() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:t>Erster Absatz &amp; mehr</w:t></w:r></w:p>
<w:p><w:r><w:t>Zweiter</w:t><w:tab/><w:t>Teil</w:t></w:r></w:p>
</w:body></w:document>"#;
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            writer
                .start_file("word/document.xml", options)
                .expect("Eintrag");
            writer.write_all(xml.as_bytes()).expect("Schreiben");
            writer.finish().expect("Abschluss");
        }
        let text = extract_text(DocumentKind::Docx, buffer.get_ref()).expect("DOCX");
        assert_eq!(text, "Erster Absatz & mehr\n\nZweiter\tTeil");
    }

    #[test]
    fn pdf_text_is_read() {
        use lopdf::{
            content::{Content, Operation},
            dictionary, Object, Stream,
        };
        let mut document = lopdf::Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier" },
        );
        let resources_id =
            document.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 24.into()]),
                Operation::new("Td", vec![72.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal("Hallo IAP")]),
                Operation::new("ET", vec![]),
            ],
        };
        let content_id = document.add_object(Stream::new(
            dictionary! {},
            content.encode().expect("Inhalt"),
        ));
        let page_id = document.add_object(
            dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content_id },
        );
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1,
                "Resources" => resources_id, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id =
            document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("PDF schreiben");

        let text = extract_text(DocumentKind::Pdf, &bytes).expect("PDF");
        assert!(text.contains("Hallo IAP"), "gelesen: {text:?}");
    }

    #[test]
    fn chunks_respect_target_and_keep_all_text() {
        let paragraph = "Ein Satz über IAP. ".repeat(80);
        let text = format!("Kurz.\n\n{paragraph}\n\nEnde.");
        let chunks = chunk_text(&text, 300);
        assert!(chunks.len() > 3);
        assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 300));
        assert!(chunks
            .first()
            .is_some_and(|chunk| chunk.starts_with("Kurz.")));
        assert!(chunks.last().is_some_and(|chunk| chunk.ends_with("Ende.")));
    }
}
