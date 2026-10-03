//! Änderungsvorschläge des Modells lesen und prüfen (Agent Flow).
//!
//! Das Modell schreibt nie selbst: Es liefert Text, aus dem der Runner
//! **Änderungen** ableitet. Zwei Formen werden gelesen, weil kleine lokale
//! Modelle Diffs oft ungenau zählen, ganze Dateien aber zuverlässig liefern:
//!
//! 1. `### Datei: pfad` gefolgt von einem Codeblock mit dem **vollständigen
//!    neuen Inhalt** (auch für neue Dateien);
//! 2. ein Unified-Diff (```` ```diff ````) mit `--- a/pfad` / `+++ b/pfad`.
//!
//! Beides ergibt dieselben [`ResolvedChange`]. Erst danach prüft der Runner jeden
//! Pfad über `pa-policy` und wendet die Änderung ausschließlich im Worktree des
//! Kandidaten an. Diffs werden **nicht** nach Zeilennummern, sondern nach ihrem
//! Kontext angewendet (Zeilennummern von Sprachmodellen sind unzuverlässig);
//! ein nicht eindeutig auffindbarer Hunk ist ein harter Fehler, kein Raten.

use std::collections::BTreeMap;

use thiserror::Error;

/// Größte akzeptierte Dateigröße einer einzelnen Änderung.
pub const MAX_FILE_BYTES: usize = 1_000_000;
/// Höchstzahl Dateien pro Kandidat.
pub const MAX_FILES: usize = 40;

/// Fehler beim Lesen oder Anwenden eines Vorschlags.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PatchError {
    /// Im Text steht kein verwertbarer Änderungsvorschlag.
    #[error("Das Modell hat keine anwendbare Änderung geliefert")]
    Empty,
    /// Ein Pfad ist unzulässig (absolut, `..`, `.git`, Steuerzeichen).
    #[error("Unzulässiger Pfad `{0}`")]
    BadPath(String),
    /// Ein Hunk passt nicht eindeutig auf die Datei.
    #[error("Änderung an `{path}` passt nicht auf den aktuellen Dateiinhalt (Hunk {hunk})")]
    HunkMismatch { path: String, hunk: usize },
    /// Ein Diff-Block ist nicht lesbar.
    #[error("Diff nicht lesbar: {0}")]
    Malformed(String),
    /// Zu viele oder zu große Änderungen.
    #[error("Änderung zu groß: {0}")]
    TooLarge(String),
    /// Eine geänderte Datei existiert nicht, und der Diff legt sie nicht an.
    #[error("Datei `{0}` existiert nicht")]
    MissingFile(String),
}

/// Was mit einer Datei geschehen soll, nachdem alles aufgelöst ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// Datei mit diesem vollständigen Inhalt anlegen oder ersetzen.
    Write(String),
    /// Datei löschen.
    Delete,
}

/// Eine aufgelöste Änderung an einem relativen Pfad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedChange {
    pub path: String,
    pub kind: ChangeKind,
}

/// Roher, noch nicht angewendeter Vorschlag.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Proposal {
    FullFile {
        path: String,
        content: String,
    },
    Diff {
        path: String,
        hunks: Vec<Hunk>,
        creates: bool,
        deletes: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
    /// Erwartete Startzeile (1-basiert) laut Kopf; nur ein Hinweis für die Suche.
    hint: usize,
    old: Vec<String>,
    new: Vec<String>,
}

/// Der gelesene Vorschlag eines Modells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeProposal {
    proposals: Vec<Proposal>,
}

/// Prüft einen relativen Pfad rein textlich. Die eigentliche Auflösung im Worktree
/// (Symlinks, Junctions) macht danach `pa_policy::safe_join`.
pub fn validate_relative_path(raw: &str) -> Result<String, PatchError> {
    let path = raw.trim().trim_start_matches("./").replace('\\', "/");
    let bad = path.is_empty()
        || path.starts_with('/')
        || path.contains(':')
        || path.chars().any(|c| c.is_control())
        || path
            .split('/')
            .any(|part| part.is_empty() || part == ".." || part == ".")
        || path
            .split('/')
            .any(|part| part.eq_ignore_ascii_case(".git"));
    if bad {
        return Err(PatchError::BadPath(raw.trim().to_owned()));
    }
    Ok(path)
}

impl ChangeProposal {
    /// Liest den Antworttext des Modells.
    ///
    /// # Errors
    /// [`PatchError::Empty`], wenn nichts Verwertbares darin steht.
    pub fn parse(text: &str) -> Result<Self, PatchError> {
        let mut proposals = Vec::new();
        let lines: Vec<&str> = text.lines().collect();
        let mut index = 0;
        while index < lines.len() {
            let line = lines[index].trim_end();
            if let Some(path) = full_file_header(line) {
                if let Some((content, next)) = fenced_block(&lines, index + 1) {
                    proposals.push(Proposal::FullFile {
                        path: validate_relative_path(&path)?,
                        content,
                    });
                    index = next;
                    continue;
                }
            }
            if is_diff_fence(line) {
                // Die Zeile selbst ist der öffnende Zaun; der Diff beginnt in der nächsten Zeile.
                if let Some((body, next)) = body_until_close(&lines, index + 1) {
                    proposals.extend(parse_diff(&body)?);
                    index = next;
                    continue;
                }
            }
            index += 1;
        }
        // Ein Diff ohne Zaun (die Antwort besteht nur aus Diff-Zeilen).
        if proposals.is_empty() && text.contains("\n+++ ") && text.contains("--- ") {
            let body: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
            proposals.extend(parse_diff(&body)?);
        }
        if proposals.is_empty() {
            return Err(PatchError::Empty);
        }
        Ok(Self { proposals })
    }

    /// Anzahl Dateien.
    pub fn len(&self) -> usize {
        self.proposals.len()
    }

    /// Ob nichts vorgeschlagen wurde (kommt nach `parse` nie vor).
    pub fn is_empty(&self) -> bool {
        self.proposals.is_empty()
    }

    /// Löst alles gegen den aktuellen Inhalt auf. `read` liefert den Text einer
    /// vorhandenen Datei aus dem Worktree oder `None`.
    ///
    /// # Errors
    /// Jeder nicht anwendbare Hunk, zu große Änderungen oder fehlende Dateien.
    pub fn resolve(
        &self,
        read: impl Fn(&str) -> Option<String>,
    ) -> Result<Vec<ResolvedChange>, PatchError> {
        if self.proposals.len() > MAX_FILES {
            return Err(PatchError::TooLarge(format!(
                "mehr als {MAX_FILES} Dateien"
            )));
        }
        // Mehrere Vorschläge für dieselbe Datei: der letzte gewinnt für Vollinhalt,
        // Diffs werden nacheinander angewendet.
        let mut current: BTreeMap<String, Option<String>> = BTreeMap::new();
        let mut order: Vec<String> = Vec::new();
        for proposal in &self.proposals {
            match proposal {
                Proposal::FullFile { path, content } => {
                    if content.len() > MAX_FILE_BYTES {
                        return Err(PatchError::TooLarge(format!(
                            "`{path}` ist größer als {MAX_FILE_BYTES} Byte"
                        )));
                    }
                    if !current.contains_key(path) {
                        order.push(path.clone());
                    }
                    current.insert(path.clone(), Some(normalize_end(content)));
                }
                Proposal::Diff {
                    path,
                    hunks,
                    creates,
                    deletes,
                } => {
                    if !current.contains_key(path) {
                        order.push(path.clone());
                    }
                    if *deletes {
                        current.insert(path.clone(), None);
                        continue;
                    }
                    let base = match current.get(path) {
                        Some(Some(text)) => text.clone(),
                        Some(None) => return Err(PatchError::MissingFile(path.clone())),
                        None if *creates => String::new(),
                        None => read(path).ok_or_else(|| PatchError::MissingFile(path.clone()))?,
                    };
                    let updated = apply_hunks(path, &base, hunks)?;
                    if updated.len() > MAX_FILE_BYTES {
                        return Err(PatchError::TooLarge(format!(
                            "`{path}` ist größer als {MAX_FILE_BYTES} Byte"
                        )));
                    }
                    current.insert(path.clone(), Some(updated));
                }
            }
        }
        let mut out = Vec::new();
        for path in order {
            let kind = match current.remove(&path) {
                Some(Some(content)) => ChangeKind::Write(content),
                Some(None) => ChangeKind::Delete,
                None => continue,
            };
            out.push(ResolvedChange { path, kind });
        }
        if out.is_empty() {
            return Err(PatchError::Empty);
        }
        Ok(out)
    }
}

fn normalize_end(content: &str) -> String {
    let mut text = content.replace("\r\n", "\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// Erkennt `### Datei: pfad`, `**Datei: pfad**`, `File: pfad` u. Ä.
fn full_file_header(line: &str) -> Option<String> {
    let cleaned = line
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches('*')
        .trim();
    let lower = cleaned.to_lowercase();
    for prefix in [
        "datei:",
        "file:",
        "fichier :",
        "fichier:",
        "archivo:",
        "ファイル:",
    ] {
        if lower.starts_with(prefix) {
            let rest = cleaned[prefix.len()..]
                .trim()
                .trim_matches('`')
                .trim_matches('*')
                .trim();
            if !rest.is_empty() {
                return Some(rest.to_owned());
            }
        }
    }
    None
}

fn is_diff_fence(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("```diff") || trimmed.starts_with("```patch")
}

fn fence_start(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

/// Liest einen Codezaun ab `from` (Zeile mit ```` ``` ````) und gibt den Text zurück.
fn fenced_block(lines: &[&str], from: usize) -> Option<(String, usize)> {
    fenced_block_lines(lines, from).map(|(body, next)| (body.join("\n"), next))
}

fn fenced_block_lines(lines: &[&str], from: usize) -> Option<(Vec<String>, usize)> {
    let mut index = from;
    // Leerzeilen zwischen Kopf und Zaun überspringen.
    while index < lines.len() && lines[index].trim().is_empty() {
        index += 1;
    }
    if index >= lines.len() || !fence_start(lines[index]) {
        return None;
    }
    body_until_close(lines, index + 1)
}

/// Liest ab `start` (erste Zeile **im** Zaun) bis zum schließenden Zaun.
fn body_until_close(lines: &[&str], start: usize) -> Option<(Vec<String>, usize)> {
    let mut body = Vec::new();
    let mut index = start;
    while index < lines.len() {
        if lines[index].trim_end() == "```" {
            return Some((body, index + 1));
        }
        body.push(lines[index].trim_end_matches('\r').to_owned());
        index += 1;
    }
    None
}

fn strip_prefix_path(raw: &str) -> String {
    let token = raw.split('\t').next().unwrap_or(raw).trim();
    token
        .strip_prefix("a/")
        .or_else(|| token.strip_prefix("b/"))
        .unwrap_or(token)
        .to_owned()
}

fn parse_diff(body: &[String]) -> Result<Vec<Proposal>, PatchError> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < body.len() {
        if !body[index].starts_with("--- ") {
            index += 1;
            continue;
        }
        let old_raw = body[index][4..].trim().to_owned();
        let Some(next) = body.get(index + 1) else {
            return Err(PatchError::Malformed("`---` ohne `+++`".to_owned()));
        };
        if !next.starts_with("+++ ") {
            index += 1;
            continue;
        }
        let new_raw = next[4..].trim().to_owned();
        index += 2;
        let creates = old_raw.starts_with("/dev/null");
        let deletes = new_raw.starts_with("/dev/null");
        let path_raw = if deletes {
            strip_prefix_path(&old_raw)
        } else {
            strip_prefix_path(&new_raw)
        };
        let path = validate_relative_path(&path_raw)?;
        let mut hunks = Vec::new();
        while index < body.len() && !body[index].starts_with("--- ") {
            if let Some(header) = body[index].strip_prefix("@@") {
                let hint = header
                    .split_whitespace()
                    .find(|part| part.starts_with('-'))
                    .and_then(|part| part[1..].split(',').next())
                    .and_then(|n| n.parse::<usize>().ok())
                    .unwrap_or(1);
                index += 1;
                let mut old = Vec::new();
                let mut new = Vec::new();
                while index < body.len()
                    && !body[index].starts_with("@@")
                    && !body[index].starts_with("--- ")
                {
                    let line = &body[index];
                    match line.chars().next() {
                        Some('+') => new.push(line[1..].to_owned()),
                        Some('-') => old.push(line[1..].to_owned()),
                        Some(' ') => {
                            old.push(line[1..].to_owned());
                            new.push(line[1..].to_owned());
                        }
                        None => {
                            // Leerzeile = leere Kontextzeile (viele Modelle lassen das Leerzeichen weg).
                            old.push(String::new());
                            new.push(String::new());
                        }
                        Some('\\') => {}
                        Some(_) => {
                            return Err(PatchError::Malformed(format!(
                                "unerwartete Zeile `{line}`"
                            )))
                        }
                    }
                    index += 1;
                }
                hunks.push(Hunk { hint, old, new });
            } else {
                index += 1;
            }
        }
        if hunks.is_empty() && !deletes {
            return Err(PatchError::Malformed(format!(
                "Diff für `{path}` ohne Hunk"
            )));
        }
        out.push(Proposal::Diff {
            path,
            hunks,
            creates,
            deletes,
        });
    }
    Ok(out)
}

fn lines_of(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n")
        .split('\n')
        .map(str::to_owned)
        .collect::<Vec<_>>()
}

fn same(a: &str, b: &str) -> bool {
    a.trim_end() == b.trim_end()
}

fn apply_hunks(path: &str, base: &str, hunks: &[Hunk]) -> Result<String, PatchError> {
    let had_trailing_newline = base.ends_with('\n') || base.is_empty();
    let mut lines = lines_of(base);
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let mut offset: isize = 0;
    for (number, hunk) in hunks.iter().enumerate() {
        let position =
            locate(&lines, &hunk.old, hunk.hint, offset).ok_or(PatchError::HunkMismatch {
                path: path.to_owned(),
                hunk: number + 1,
            })?;
        lines.splice(
            position..position + hunk.old.len(),
            hunk.new.iter().cloned(),
        );
        offset += hunk.new.len() as isize - hunk.old.len() as isize;
    }
    let mut text = lines.join("\n");
    if had_trailing_newline || !text.is_empty() {
        text.push('\n');
    }
    Ok(text)
}

/// Sucht `old` in `lines`. Es muss genau eine Fundstelle geben; bei mehreren gewinnt
/// die zum Hinweis nächste nur, wenn sie eindeutig näher liegt.
fn locate(lines: &[String], old: &[String], hint: usize, offset: isize) -> Option<usize> {
    if old.is_empty() {
        // Reine Einfügung ohne Kontext: nur an der angegebenen Stelle, nie geraten.
        let at = (hint as isize - 1 + offset).clamp(0, lines.len() as isize) as usize;
        return Some(at);
    }
    let mut found = Vec::new();
    if old.len() <= lines.len() {
        for start in 0..=(lines.len() - old.len()) {
            if old
                .iter()
                .zip(&lines[start..start + old.len()])
                .all(|(a, b)| same(a, b))
            {
                found.push(start);
            }
        }
    }
    match found.len() {
        0 => None,
        1 => Some(found[0]),
        _ => {
            let expected = (hint as isize - 1 + offset).max(0) as usize;
            let mut ranked: Vec<(usize, usize)> = found
                .iter()
                .map(|&start| (start.abs_diff(expected), start))
                .collect();
            ranked.sort_unstable();
            if ranked[0].0 < ranked[1].0 {
                Some(ranked[0].1)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn full_file_blocks_become_writes_and_normalize_line_endings() {
        let text = "Hier die Lösung.\n\n### Datei: src/lib.rs\n```rust\nfn main() {}\r\n```\n\n**Datei: docs/neu.md**\n```\n# Neu\n```\n";
        let proposal = ChangeProposal::parse(text).expect("lesbar");
        let changes = proposal.resolve(read_none).expect("auflösbar");
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].path, "src/lib.rs");
        assert_eq!(
            changes[0].kind,
            ChangeKind::Write("fn main() {}\n".to_owned())
        );
        assert_eq!(changes[1].path, "docs/neu.md");
    }

    #[test]
    fn diff_is_applied_by_context_even_with_wrong_line_numbers() {
        let base = "a\nb\nc\nd\ne\n";
        let text = "```diff\n--- a/x.txt\n+++ b/x.txt\n@@ -99,3 +99,3 @@\n b\n-c\n+C\n d\n```";
        let changes = ChangeProposal::parse(text)
            .unwrap()
            .resolve(|p| (p == "x.txt").then(|| base.to_owned()))
            .unwrap();
        assert_eq!(
            changes[0].kind,
            ChangeKind::Write("a\nb\nC\nd\ne\n".to_owned())
        );
    }

    #[test]
    fn ambiguous_or_missing_context_is_a_hard_error() {
        let base = "x\ny\nx\ny\n";
        let ambiguous = "```diff\n--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-x\n+z\n```";
        // Zwei gleich gute Fundstellen, Hinweis genau dazwischen -> nicht raten.
        let result = ChangeProposal::parse(ambiguous)
            .unwrap()
            .resolve(|_| Some(base.to_owned()));
        assert!(result.is_ok(), "Hinweis 1 ist eindeutig näher an Zeile 1");
        let missing = "```diff\n--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-gibt es nicht\n+z\n```";
        assert!(matches!(
            ChangeProposal::parse(missing)
                .unwrap()
                .resolve(|_| Some(base.to_owned())),
            Err(PatchError::HunkMismatch { .. })
        ));
    }

    #[test]
    fn new_and_deleted_files_via_dev_null() {
        let text = "```diff\n--- /dev/null\n+++ b/neu.txt\n@@ -0,0 +1,2 @@\n+eins\n+zwei\n```\n```diff\n--- a/alt.txt\n+++ /dev/null\n```";
        let changes = ChangeProposal::parse(text)
            .unwrap()
            .resolve(read_none)
            .unwrap();
        assert_eq!(
            changes[0].kind,
            ChangeKind::Write("eins\nzwei\n".to_owned())
        );
        assert_eq!(changes[1].kind, ChangeKind::Delete);
    }

    #[test]
    fn dangerous_paths_are_rejected() {
        for path in [
            "../geheim.txt",
            "/etc/passwd",
            "C:/Windows/x",
            ".git/config",
            "a/../../b",
            "sub/.GIT/hooks/pre-commit",
            "a\u{0007}b",
        ] {
            let text = format!("### Datei: {path}\n```\nx\n```");
            assert!(
                matches!(ChangeProposal::parse(&text), Err(PatchError::BadPath(_))),
                "{path}"
            );
        }
    }

    #[test]
    fn text_without_a_change_is_empty_not_a_success() {
        assert_eq!(
            ChangeProposal::parse("Ich habe mir das überlegt, aber es gibt nichts zu ändern."),
            Err(PatchError::Empty)
        );
    }

    #[test]
    fn oversize_and_too_many_files_are_refused() {
        let big = "x".repeat(MAX_FILE_BYTES + 1);
        let text = format!("### Datei: a.txt\n```\n{big}\n```");
        assert!(matches!(
            ChangeProposal::parse(&text).unwrap().resolve(read_none),
            Err(PatchError::TooLarge(_))
        ));
        let many: String = (0..=MAX_FILES)
            .map(|i| format!("### Datei: f{i}.txt\n```\nx\n```\n"))
            .collect();
        assert!(matches!(
            ChangeProposal::parse(&many).unwrap().resolve(read_none),
            Err(PatchError::TooLarge(_))
        ));
    }

    #[test]
    fn missing_file_for_a_plain_diff_is_reported() {
        let text = "```diff\n--- a/fehlt.txt\n+++ b/fehlt.txt\n@@ -1 +1 @@\n-a\n+b\n```";
        assert_eq!(
            ChangeProposal::parse(text).unwrap().resolve(read_none),
            Err(PatchError::MissingFile("fehlt.txt".to_owned()))
        );
    }
}
