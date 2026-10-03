//! Ein winziger XML-Baum für CalDAV-Antworten.
//!
//! Warum nicht direkt mit dem Leser von `quick-xml` arbeiten: Die Antworten von CalDAV-Servern sind
//! tief verschachtelt (`multistatus/response/propstat/prop/…`), und wir brauchen nur wenige Stellen
//! daraus. Ein Baum mit lokalen Namen (ohne Namensraum-Präfix) macht die Auswertung kurz und
//! testbar. Die Größe wird durch das Antwortlimit des Netz-Clients begrenzt; die Tiefe hier.

use quick_xml::{events::Event, Reader};

/// So tief darf das Dokument verschachtelt sein (Schutz vor Stapelüberlauf bei böswilligen Antworten).
const MAX_DEPTH: usize = 64;

/// Ein Element mit lokalem Namen, Text und Kindern.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Node {
    /// Name ohne Namensraum-Präfix, Kleinbuchstaben (`d:href` → `href`).
    pub name: String,
    pub attrs: Vec<(String, String)>,
    /// Alle direkten Textstücke zusammen, ohne Randleerzeichen.
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    /// Erstes direktes Kind mit diesem Namen.
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    /// Alle direkten Kinder mit diesem Namen.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Erstes Element mit diesem Namen irgendwo darunter (Tiefensuche).
    pub fn find(&self, name: &str) -> Option<&Node> {
        for child in &self.children {
            if child.name == name {
                return Some(child);
            }
            if let Some(found) = child.find(name) {
                return Some(found);
            }
        }
        None
    }

    /// Alle Elemente mit diesem Namen irgendwo darunter.
    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for child in &self.children {
            if child.name == name {
                out.push(child);
            }
            child.find_all(name, out);
        }
    }

    /// Wert eines Attributs (Name ohne Präfix, Kleinbuchstaben).
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or("").to_ascii_lowercase()
}

fn element(start: &quick_xml::events::BytesStart<'_>) -> Node {
    let attrs = start
        .attributes()
        .flatten()
        .filter_map(|a| {
            let key = local(a.key.as_ref());
            let value = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()?
                .into_owned();
            // Namensraum-Deklarationen (`xmlns`, `xmlns:d`) sind für die Auswertung unwichtig.
            (!a.key.as_ref().starts_with("xmlns")).then_some((key, value))
        })
        .collect();
    Node {
        name: local(start.name().as_ref()),
        attrs,
        ..Node::default()
    }
}

/// Liest ein XML-Dokument in einen Baum. Das Wurzelelement wird zurückgegeben.
///
/// # Errors
/// Text bei ungültigem XML, zu großer Tiefe oder leerem Dokument.
pub fn parse(xml: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                if stack.len() >= MAX_DEPTH {
                    return Err("XML zu tief verschachtelt".to_owned());
                }
                stack.push(element(&start));
            }
            Ok(Event::Empty(start)) => {
                let node = element(&start);
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Ok(Event::End(_)) => {
                let node = stack.pop().ok_or("Unerwartetes Ende im XML")?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Ok(Event::Text(text)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&text.xml10_content());
                }
            }
            Ok(Event::CData(data)) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&data.into_inner());
                }
            }
            Ok(Event::GeneralRef(reference)) => {
                if let Some(top) = stack.last_mut() {
                    if let Ok(Some(character)) = reference.resolve_char_ref() {
                        top.text.push(character);
                    } else if let Some(resolved) = quick_xml::escape::resolve_xml_entity(&reference)
                    {
                        top.text.push_str(resolved);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("XML nicht lesbar: {error}")),
        }
    }
    if !stack.is_empty() {
        return Err("XML ist unvollständig".to_owned());
    }
    let mut root = root.ok_or("Leere Antwort")?;
    trim_texts(&mut root);
    Ok(root)
}

fn trim_texts(node: &mut Node) {
    let trimmed = node.text.trim();
    if trimmed.len() != node.text.len() {
        node.text = trimmed.to_owned();
    }
    for child in &mut node.children {
        trim_texts(child);
    }
}

/// Maskiert Text für XML-Inhalt (Testdaten; die Anfragen der App enthalten keinen freien Text).
#[cfg(test)]
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_lose_their_prefix_and_text_is_trimmed() {
        let root = parse(
            r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:response>
    <d:href>
      /123/calendars/home/
    </d:href>
    <d:propstat><d:prop><c:comp name="VEVENT"/><D:Displayname>Privat &amp; Arbeit</D:Displayname></d:prop></d:propstat>
  </d:response>
</d:multistatus>"#,
        )
        .unwrap();
        assert_eq!(root.name, "multistatus");
        assert!(root.attrs.is_empty(), "xmlns zählt nicht");
        let response = root.child("response").unwrap();
        assert_eq!(response.child("href").unwrap().text, "/123/calendars/home/");
        let comp = root.find("comp").unwrap();
        assert_eq!(comp.attr("name"), Some("VEVENT"));
        assert_eq!(root.find("displayname").unwrap().text, "Privat & Arbeit");
    }

    #[test]
    fn character_references_and_cdata_are_resolved() {
        let root =
            parse("<a><b>Zeile 1&#13;&#10;Zeile 2 &lt;x&gt;</b><c><![CDATA[a & b]]></c></a>")
                .unwrap();
        assert_eq!(root.find("b").unwrap().text, "Zeile 1\r\nZeile 2 <x>");
        assert_eq!(root.find("c").unwrap().text, "a & b");
    }

    #[test]
    fn find_all_collects_nested_matches_in_order() {
        let root = parse("<a><r><h>1</h></r><x><r><h>2</h></r></x></a>").unwrap();
        let mut found = Vec::new();
        root.find_all("h", &mut found);
        assert_eq!(
            found.iter().map(|n| n.text.as_str()).collect::<Vec<_>>(),
            ["1", "2"]
        );
    }

    #[test]
    fn broken_documents_are_errors_not_panics() {
        for bad in [
            "",
            "   ",
            "<a>",
            "<a></b>",
            "</a>",
            "<a><b></a>",
            "nur text",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep = format!(
            "{}{}",
            "<a>".repeat(MAX_DEPTH + 5),
            "</a>".repeat(MAX_DEPTH + 5)
        );
        assert!(parse(&deep).is_err());
    }

    #[test]
    fn escape_covers_the_xml_specials() {
        assert_eq!(
            escape(r#"a&b<c>"d"'e'"#),
            "a&amp;b&lt;c&gt;&quot;d&quot;&apos;e&apos;"
        );
    }
}
