//! Anleitungs-Skills (`SKILL.md`).
//!
//! Ein Anleitungs-Skill ist ein Ordner mit einer `SKILL.md`: Kopfzeilen
//! (`name`, `description`) und darunter eine Anleitung in Markdown, dazu
//! optional weitere Textdateien. Er enthält **keinen ausführbaren Code** und
//! bekommt keine Rechte: IAP liest den Text und gibt ihn dem Modell als
//! zusätzliche Anweisung, sobald der Nutzer den Skill aktiviert hat. Werkzeuge
//! bleiben weiterhin durch pa-policy begrenzt.
//!
//! Skript-Dateien (etwa `scripts/*.py`) werden beim Import bewusst nicht
//! übernommen und nie ausgeführt; der Import meldet, was übersprungen wurde.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Größte erlaubte `SKILL.md`.
pub const MAX_SKILL_MD_BYTES: u64 = 64 * 1024;
/// Gesamtgröße aller übernommenen Dateien eines Skills.
pub const MAX_TOTAL_BYTES: u64 = 512 * 1024;
/// Höchstzahl übernommener Dateien.
pub const MAX_FILES: usize = 40;
/// Dateiendungen, die als Text übernommen werden.
const TEXT_EXTENSIONS: [&str; 8] = [
    "md", "markdown", "txt", "json", "yaml", "yml", "toml", "csv",
];

/// Fehler beim Lesen eines Anleitungs-Skills.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum InstructionError {
    /// Die `SKILL.md` fehlt.
    #[error("Im Ordner fehlt die Datei SKILL.md")]
    Missing,
    /// Die `SKILL.md` ist zu groß oder kein lesbarer Text.
    #[error("SKILL.md ist nicht lesbar: {0}")]
    Unreadable(String),
    /// Der Name ergibt keine brauchbare Kennung.
    #[error("Der Skill braucht einen Namen aus Buchstaben oder Zahlen")]
    BadName,
}

/// Der gelesene Inhalt einer `SKILL.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedSkill {
    /// Kennung für Ordnername und Einstellungen (`a-z0-9-_`).
    pub id: String,
    /// Anzeigename.
    pub name: String,
    /// Kurzbeschreibung („wann benutzen“).
    pub description: String,
    /// Anleitung ohne Kopfzeilen.
    pub body: String,
}

/// Ein installierter Anleitungs-Skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub body: String,
    /// Mitgelieferte weitere Dateien (relative Pfade).
    pub files: Vec<String>,
}

/// Macht aus einem Namen eine sichere Kennung.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    let spelled: String = name
        .trim()
        .chars()
        .flat_map(|c| match c {
            'ä' | 'Ä' => vec!['a', 'e'],
            'ö' | 'Ö' => vec!['o', 'e'],
            'ü' | 'Ü' => vec!['u', 'e'],
            'ß' => vec!['s', 's'],
            other => vec![other],
        })
        .collect();
    for c in spelled.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').chars().take(64).collect()
}

fn unquote(value: &str) -> String {
    let v = value.trim();
    if v.len() >= 2
        && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')))
    {
        v[1..v.len() - 1].to_owned()
    } else {
        v.to_owned()
    }
}

/// Liest die einfachen Kopfzeilen (`key: value`, auch `key: >` / `key: |` mit
/// eingerückten Folgezeilen). Mehr Syntax braucht IAP nicht und führt nichts aus.
fn parse_front_matter(text: &str) -> (Vec<(String, String)>, &str) {
    let trimmed = text.trim_start_matches('\u{feff}');
    let Some(rest) = trimmed.strip_prefix("---") else {
        return (Vec::new(), trimmed);
    };
    let rest = rest.trim_start_matches(['\r', ' ']);
    let Some(rest) = rest.strip_prefix('\n') else {
        return (Vec::new(), trimmed);
    };
    let mut pairs = Vec::new();
    let mut consumed = 0;
    let mut closed = false;
    let mut current: Option<(String, String, bool)> = None; // (Schlüssel, Wert, Blockform)
    for line in rest.split_inclusive('\n') {
        consumed += line.len();
        let stripped = line.trim_end_matches(['\r', '\n']);
        if stripped.trim() == "---" {
            closed = true;
            break;
        }
        let indented = stripped.starts_with(' ') || stripped.starts_with('\t');
        if indented {
            if let Some((_, value, _)) = current.as_mut() {
                if !value.is_empty() {
                    value.push(' ');
                }
                value.push_str(stripped.trim());
            }
            continue;
        }
        if let Some(done) = current.take() {
            pairs.push((done.0, done.1));
        }
        if let Some((key, value)) = stripped.split_once(':') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            let block = matches!(value, ">" | "|" | ">-" | "|-" | ">+" | "|+");
            current = Some((
                key,
                if block { String::new() } else { unquote(value) },
                block,
            ));
        }
    }
    if let Some(done) = current.take() {
        pairs.push((done.0, done.1));
    }
    if !closed {
        return (Vec::new(), trimmed);
    }
    (pairs, &rest[consumed..])
}

/// Liest den Text einer `SKILL.md`.
///
/// # Errors
/// [`InstructionError::BadName`], wenn weder `name` noch eine Überschrift eine Kennung ergeben.
pub fn parse_skill_md(text: &str, fallback_name: &str) -> Result<ParsedSkill, InstructionError> {
    let (pairs, body) = parse_front_matter(text);
    let get = |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.trim().to_owned())
    };
    let body = body.trim().to_owned();
    let heading = body
        .lines()
        .find_map(|line| line.trim().strip_prefix("# ").map(|h| h.trim().to_owned()));
    let name = get("name")
        .filter(|n| !n.is_empty())
        .or(heading)
        .unwrap_or_else(|| fallback_name.trim().to_owned());
    let id = slug(&name);
    if id.is_empty() {
        return Err(InstructionError::BadName);
    }
    let description = get("description")
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| {
            body.split("\n\n")
                .map(str::trim)
                .find(|p| !p.is_empty() && !p.starts_with('#'))
                .unwrap_or("")
                .chars()
                .take(240)
                .collect()
        });
    Ok(ParsedSkill {
        id,
        name,
        description,
        body,
    })
}

/// Ob die Datei als Textbeigabe übernommen wird.
pub fn is_text_asset(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| TEXT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Ergebnis einer Durchsicht des Quellordners.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Survey {
    /// Zu übernehmende Dateien (relativ, mit `/`), `SKILL.md` zuerst.
    pub keep: Vec<String>,
    /// Bewusst übersprungene Dateien (Skripte, zu groß, zu viele, …).
    pub skipped: Vec<String>,
}

/// Geht den Quellordner durch und entscheidet, was übernommen wird. Symbolische
/// Verknüpfungen, versteckte Ordner und `node_modules` werden nie verfolgt.
pub fn survey(dir: &Path) -> std::io::Result<Survey> {
    let mut result = Survey::default();
    let mut total = 0_u64;
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((folder, prefix)) = stack.pop() {
        let mut entries: Vec<_> = fs::read_dir(&folder)?.filter_map(Result::ok).collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                result.skipped.push(format!("{relative} (Verknüpfung)"));
            } else if kind.is_dir() {
                if name.starts_with('.') || name == "node_modules" || name == "__pycache__" {
                    continue;
                }
                stack.push((entry.path(), relative));
            } else if kind.is_file() {
                let size = entry.metadata().map_or(0, |m| m.len());
                let is_skill_md = relative.eq_ignore_ascii_case("SKILL.md");
                if !is_skill_md && !is_text_asset(&entry.path()) {
                    result.skipped.push(format!(
                        "{relative} (kein Text; IAP führt keine Dateien aus)"
                    ));
                } else if size > MAX_SKILL_MD_BYTES {
                    result.skipped.push(format!("{relative} (zu groß)"));
                } else if !is_skill_md
                    && (result.keep.len() >= MAX_FILES || total + size > MAX_TOTAL_BYTES)
                {
                    result.skipped.push(format!("{relative} (Grenze erreicht)"));
                } else {
                    total += size;
                    if is_skill_md {
                        result.keep.insert(0, relative);
                    } else {
                        result.keep.push(relative);
                    }
                }
            }
        }
    }
    Ok(result)
}

/// Sammelt die installierten Anleitungs-Skills unter `root` (`<id>/SKILL.md`).
pub struct InstructionRegistry {
    root: PathBuf,
}

impl InstructionRegistry {
    /// Wurzel ist `AI/skills/instructions`.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Ordner eines Skills.
    pub fn folder(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// Liest alle lesbaren Skills; defekte werden übersprungen.
    pub fn scan(&self) -> Vec<InstructionSkill> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut skills: Vec<InstructionSkill> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|entry| self.read(&entry.file_name().to_string_lossy()))
            .collect();
        skills.sort_by_key(|a| a.name.to_lowercase());
        skills
    }

    /// Liest einen Skill.
    pub fn read(&self, id: &str) -> Option<InstructionSkill> {
        if id.is_empty() || id != slug(id) {
            return None;
        }
        let folder = self.folder(id);
        let text = fs::read_to_string(folder.join("SKILL.md")).ok()?;
        let parsed = parse_skill_md(&text, id).ok()?;
        let survey = survey(&folder).ok()?;
        Some(InstructionSkill {
            id: id.to_owned(),
            name: parsed.name,
            description: parsed.description,
            body: parsed.body,
            files: survey
                .keep
                .into_iter()
                .filter(|f| !f.eq_ignore_ascii_case("SKILL.md"))
                .collect(),
        })
    }
}

/// Baut den Zusatztext für den Systemprompt aus den aktiven Skills.
///
/// Warum Grenzen: Auf 8-GB-Rechnern ist der Kontext klein. Jeder Skill bekommt
/// höchstens `per_skill` Zeichen, alle zusammen `total`; was fehlt, wird als
/// gekürzt gekennzeichnet, nie stillschweigend weggelassen.
pub fn prompt_for(skills: &[InstructionSkill], per_skill: usize, total: usize) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut out = String::from(
        "Zusätzliche Anleitungen des Nutzers (Skills). Befolge sie, wenn sie zur Frage passen. \
         Sie ändern keine Rechte: Werkzeuge und Dateizugriffe bleiben durch die Sicherheitsregeln begrenzt.\n",
    );
    for skill in skills {
        let mut section = format!("\n## Skill: {}\n", skill.name);
        if !skill.description.is_empty() {
            section.push_str(&format!("Wann: {}\n\n", skill.description));
        }
        let body: String = skill.body.chars().take(per_skill).collect();
        section.push_str(&body);
        if skill.body.chars().count() > per_skill {
            section.push_str("\n[Anleitung gekürzt]");
        }
        if out.chars().count() + section.chars().count() > total {
            out.push_str(&format!(
                "\n## Skill: {} (nicht geladen: Platz im Kontext reicht nicht)\n",
                skill.name
            ));
            continue;
        }
        out.push_str(&section);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_with_quotes_and_folded_description_is_read() {
        let text = "---\nname: \"PDF Helfer\"\ndescription: >-\n  Hilft bei PDFs.\n  Nutze es bei Formularen.\nlicense: MIT\n---\n\n# PDF Helfer\nSchritt 1.\n";
        let parsed = parse_skill_md(text, "egal").unwrap();
        assert_eq!(parsed.id, "pdf-helfer");
        assert_eq!(parsed.name, "PDF Helfer");
        assert_eq!(
            parsed.description,
            "Hilft bei PDFs. Nutze es bei Formularen."
        );
        assert!(parsed.body.starts_with("# PDF Helfer"));
        assert!(!parsed.body.contains("license"));
    }

    #[test]
    fn a_file_without_front_matter_uses_heading_and_first_paragraph() {
        let parsed = parse_skill_md(
            "# Briefe schreiben\n\nFreundlich und kurz.\n\n- Punkt",
            "ordner",
        )
        .unwrap();
        assert_eq!(parsed.name, "Briefe schreiben");
        assert_eq!(parsed.description, "Freundlich und kurz.");
        assert_eq!(parsed.id, "briefe-schreiben");
    }

    #[test]
    fn crlf_and_bom_do_not_break_the_header() {
        let parsed = parse_skill_md(
            "\u{feff}---\r\nname: Test\r\ndescription: Eine Zeile\r\n---\r\nText",
            "x",
        )
        .unwrap();
        assert_eq!(parsed.name, "Test");
        assert_eq!(parsed.description, "Eine Zeile");
        assert_eq!(parsed.body, "Text");
    }

    #[test]
    fn an_unclosed_header_is_treated_as_plain_text() {
        let parsed = parse_skill_md("---\nname: X\nkein Ende", "fallback-name").unwrap();
        assert_eq!(parsed.name, "fallback-name");
    }

    #[test]
    fn names_become_safe_ids() {
        assert_eq!(slug("  Über Größe & Maß!  "), "ueber-groesse-mass");
        assert_eq!(slug("../../etc"), "etc");
        assert_eq!(slug("***"), "");
        assert!(matches!(
            parse_skill_md("---\nname: ***\n---\n", ""),
            Err(InstructionError::BadName)
        ));
    }

    #[test]
    fn survey_keeps_text_and_reports_scripts_and_links() {
        let dir = std::env::temp_dir().join(format!("iap-skill-survey-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::create_dir_all(dir.join("node_modules")).unwrap();
        fs::write(dir.join("SKILL.md"), "# A").unwrap();
        fs::write(dir.join("reference.md"), "mehr").unwrap();
        fs::write(dir.join("scripts").join("run.py"), "print(1)").unwrap();
        fs::write(dir.join("node_modules").join("x.md"), "nein").unwrap();
        let result = survey(&dir).unwrap();
        assert_eq!(result.keep.first().map(String::as_str), Some("SKILL.md"));
        assert!(result.keep.contains(&"reference.md".to_owned()));
        assert!(result
            .skipped
            .iter()
            .any(|s| s.starts_with("scripts/run.py")));
        assert!(!result.keep.iter().any(|f| f.contains("node_modules")));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn prompt_respects_both_limits_and_says_when_it_shortens() {
        let skill = |n: &str, len: usize| InstructionSkill {
            id: slug(n),
            name: n.to_owned(),
            description: "wenn nötig".to_owned(),
            body: "x".repeat(len),
            files: vec![],
        };
        let text = prompt_for(&[skill("Lang", 5000)], 1000, 10_000).unwrap();
        assert!(text.contains("[Anleitung gekürzt]"));
        assert!(text.contains("Sie ändern keine Rechte"));
        let crowded = prompt_for(&[skill("Eins", 900), skill("Zwei", 900)], 1000, 1300).unwrap();
        assert!(crowded.contains("nicht geladen"));
        assert!(prompt_for(&[], 1000, 1000).is_none());
    }
}
