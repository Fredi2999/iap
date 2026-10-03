//! Aufbereitung von Antworttext zum Vorlesen.

/// Entfernt Markdown-Zeichen, Codeblöcke und Links, damit IAP weder „Stern
/// Stern“ noch Programmcode vorliest. Die Sätze bleiben zeilenweise getrennt,
/// damit die Sprachsynthese schon nach dem ersten Satz zu sprechen beginnt.
pub fn for_speech(markdown: &str) -> String {
    let mut lines = Vec::new();
    let mut in_code = false;
    for raw in markdown.lines() {
        let line = raw.trim();
        if line.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code || line.is_empty() {
            continue;
        }
        let without_marks = line.trim_start_matches(['#', '>', '-', '*', '+', ' ']);
        let plain = strip_links(without_marks)
            .replace(['*', '_', '`', '~'], "")
            .trim()
            .to_owned();
        if !plain.is_empty() {
            lines.push(plain);
        }
    }
    lines.join("\n")
}

fn strip_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            if let Some(close) = chars[i..].iter().position(|&c| c == ']') {
                let label_end = i + close;
                if chars.get(label_end + 1) == Some(&'(') {
                    if let Some(paren) = chars[label_end..].iter().position(|&c| c == ')') {
                        out.extend(&chars[i + 1..label_end]);
                        i = label_end + paren + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_marks_and_code_blocks_are_not_read_aloud() {
        let text = "# Titel\n\n**Wichtig:** siehe [Doku](https://x.example/a).\n\n```rust\nlet x = 1;\n```\n- Punkt eins\n";
        assert_eq!(for_speech(text), "Titel\nWichtig: siehe Doku.\nPunkt eins");
    }

    #[test]
    fn plain_text_stays_and_empty_input_gives_empty_output() {
        assert_eq!(for_speech("Hallo Welt."), "Hallo Welt.");
        assert_eq!(for_speech("   \n\n"), "");
    }
}
