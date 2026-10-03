//! Unified-Diff-Erzeugung und Hunk-Anwendung (Konzept 9.4).
//!
//! „Jede Schreiboperation erzeugt einen Patch, der in der UI als Diff
//! erscheint (grün/rot, pro Hunk annehmbar)." — Diese Datei liefert
//! die Rust-Basis: `UnifiedDiff::compute` baut aus zwei Textständen
//! einen Hunk-Baum; `apply_hunks_to_string` wendet nur die vom Nutzer
//! akzeptierten Hunks an, damit die Feinabstimmung im Frontend liegt.
//!
//! Der eingebaute LCS-Diff ist bewusst simpel (O(n·m) Speicher, für
//! Editor-übliche Dateigrößen von bis zu einigen tausend Zeilen völlig
//! ausreichend) und ohne externe Abhängigkeiten — damit bleiben wir
//! frei von zusätzlichen Kompilations- und Binary-Kosten.

use serde::{Deserialize, Serialize};

use crate::CodeError;

/// Ein einzelner Diff-Hunk: eine kontextverbundene Änderungsgruppe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    /// 1-basierte Zeilennummer im alten Text (erste Zeile des Hunks).
    pub old_start: usize,
    pub old_lines: usize,
    /// 1-basierte Zeilennummer im neuen Text.
    pub new_start: usize,
    pub new_lines: usize,
    pub lines: Vec<HunkLine>,
}

/// Zeile innerhalb eines Hunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HunkLine {
    pub op: LineOp,
    pub text: String,
}

/// Operation einer einzelnen Zeile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineOp {
    Context,
    Insert,
    Delete,
}

/// Vollständiger Diff für eine Datei.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnifiedDiff {
    pub old_path: String,
    pub new_path: String,
    pub hunks: Vec<DiffHunk>,
}

impl UnifiedDiff {
    /// Berechnet einen Unified-Diff mit drei Kontextzeilen.
    pub fn compute(old_path: &str, old: &str, new_path: &str, new: &str) -> Self {
        Self::compute_with_context(old_path, old, new_path, new, 3)
    }

    /// Variante mit konfigurierbarer Kontextbreite.
    pub fn compute_with_context(
        old_path: &str,
        old: &str,
        new_path: &str,
        new: &str,
        context: usize,
    ) -> Self {
        let old_lines: Vec<&str> = split_lines(old);
        let new_lines: Vec<&str> = split_lines(new);
        let ops = lcs_diff(&old_lines, &new_lines);
        let hunks = group_hunks(&ops, context);
        Self {
            old_path: old_path.to_owned(),
            new_path: new_path.to_owned(),
            hunks,
        }
    }

    /// Serialisiert den Diff als klassisches Unified-Format
    /// (`--- a\n+++ b\n@@ -a,b +c,d @@\n …`). Für die UI oder das
    /// Speichern als `.patch`-Datei.
    pub fn to_unified_text(&self) -> String {
        let mut buffer = String::new();
        buffer.push_str(&format!("--- {}\n", self.old_path));
        buffer.push_str(&format!("+++ {}\n", self.new_path));
        for hunk in &self.hunks {
            buffer.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            ));
            for line in &hunk.lines {
                let prefix = match line.op {
                    LineOp::Context => ' ',
                    LineOp::Insert => '+',
                    LineOp::Delete => '-',
                };
                buffer.push(prefix);
                buffer.push_str(&line.text);
                buffer.push('\n');
            }
        }
        buffer
    }
}

/// Wendet die angegebenen Hunks in Reihenfolge auf `old` an und liefert
/// den resultierenden Text zurück.
///
/// Der Aufrufer entscheidet, welche Hunks aus einem Diff er annimmt —
/// die UI zeigt sie als Checkboxen (Konzept 9.4). Diese Funktion tut
/// bewusst nicht mehr, als die ausgewählten Hunks der Reihe nach zu
/// applizieren.
pub fn apply_hunks_to_string(old: &str, hunks: &[&DiffHunk]) -> Result<String, CodeError> {
    let mut lines: Vec<String> = split_lines(old).into_iter().map(String::from).collect();
    // Hunks von hinten nach vorne applizieren, damit die 1-basierten
    // Zeilennummern der noch nicht angefassten Hunks stabil bleiben.
    let mut sorted: Vec<&DiffHunk> = hunks.to_vec();
    sorted.sort_by_key(|h| std::cmp::Reverse(h.old_start));
    for hunk in sorted {
        let start = hunk.old_start.saturating_sub(1);
        if start + hunk.old_lines > lines.len() {
            return Err(CodeError::Diff(format!(
                "Hunk bei Zeile {} passt nicht (alte Länge {}, Text hat nur {} Zeilen)",
                hunk.old_start,
                hunk.old_lines,
                lines.len()
            )));
        }
        // Verify context/delete matches original lines.
        let mut old_cursor = start;
        for line in &hunk.lines {
            match line.op {
                LineOp::Context | LineOp::Delete => {
                    if lines.get(old_cursor).map(String::as_str) != Some(line.text.as_str()) {
                        return Err(CodeError::Diff(format!(
                            "Hunk bei Zeile {} passt inhaltlich nicht mehr",
                            hunk.old_start
                        )));
                    }
                    old_cursor += 1;
                }
                LineOp::Insert => {}
            }
        }
        // Baue neuen Slice und tausche.
        let mut replacement: Vec<String> = Vec::new();
        for line in &hunk.lines {
            match line.op {
                LineOp::Context => replacement.push(line.text.clone()),
                LineOp::Insert => replacement.push(line.text.clone()),
                LineOp::Delete => {}
            }
        }
        lines.splice(start..start + hunk.old_lines, replacement);
    }
    Ok(lines.join("\n"))
}

fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n').collect()
}

#[derive(Debug, Clone, Copy)]
enum Op<'a> {
    Equal(&'a str),
    Insert(&'a str),
    Delete(&'a str),
}

fn lcs_diff<'a>(old: &'a [&'a str], new: &'a [&'a str]) -> Vec<Op<'a>> {
    let m = old.len();
    let n = new.len();
    let mut table = vec![vec![0_usize; n + 1]; m + 1];
    for i in 0..m {
        for j in 0..n {
            table[i + 1][j + 1] = if old[i] == new[j] {
                table[i][j] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let mut ops = Vec::with_capacity(m + n);
    let mut i = m;
    let mut j = n;
    while i > 0 && j > 0 {
        if old[i - 1] == new[j - 1] {
            ops.push(Op::Equal(old[i - 1]));
            i -= 1;
            j -= 1;
        } else if table[i - 1][j] >= table[i][j - 1] {
            ops.push(Op::Delete(old[i - 1]));
            i -= 1;
        } else {
            ops.push(Op::Insert(new[j - 1]));
            j -= 1;
        }
    }
    while i > 0 {
        ops.push(Op::Delete(old[i - 1]));
        i -= 1;
    }
    while j > 0 {
        ops.push(Op::Insert(new[j - 1]));
        j -= 1;
    }
    ops.reverse();
    ops
}

fn group_hunks(ops: &[Op<'_>], context: usize) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut old_line = 1_usize;
    let mut new_line = 1_usize;

    let mut i = 0;
    while i < ops.len() {
        if !matches!(ops[i], Op::Equal(_)) {
            // Rückwärts Kontextzeilen einsammeln.
            let start_context = i.saturating_sub(context);
            let mut ctx_start = start_context;
            while ctx_start < i && matches!(ops[ctx_start], Op::Equal(_)) {
                ctx_start += 1;
            }
            let leading_context = i - ctx_start.max(start_context);
            let _ = leading_context;
            let hunk_start = i.saturating_sub(context);
            let head_context = i - hunk_start;
            let mut old_start = old_line - head_context;
            let mut new_start = new_line - head_context;
            if old_start == 0 {
                old_start = 1;
            }
            if new_start == 0 {
                new_start = 1;
            }

            let mut hunk_lines: Vec<HunkLine> = Vec::new();
            let mut old_advance = 0_usize;
            let mut new_advance = 0_usize;

            // Leading context einfügen.
            for op in ops.iter().take(i).skip(hunk_start) {
                if let Op::Equal(text) = op {
                    hunk_lines.push(HunkLine {
                        op: LineOp::Context,
                        text: (*text).to_owned(),
                    });
                    old_advance += 1;
                    new_advance += 1;
                }
            }

            // Änderungen und ihre unmittelbaren Kontext-Follow-ups aufnehmen.
            let mut j = i;
            while j < ops.len() {
                match ops[j] {
                    Op::Equal(text) => {
                        // Prüfen, ob innerhalb der nächsten `context * 2`
                        // Ops eine weitere Änderung folgt; wenn ja, weiter
                        // sammeln, sonst Hunk beenden.
                        let mut lookahead = j + 1;
                        let mut change_ahead = false;
                        let horizon = j + context * 2 + 1;
                        while lookahead < ops.len().min(horizon) {
                            if !matches!(ops[lookahead], Op::Equal(_)) {
                                change_ahead = true;
                                break;
                            }
                            lookahead += 1;
                        }
                        if !change_ahead {
                            // Trailing context anhängen (bis zu `context`).
                            let mut ctx_added = 0_usize;
                            let mut k = j;
                            while k < ops.len() && ctx_added < context {
                                if let Op::Equal(t) = ops[k] {
                                    hunk_lines.push(HunkLine {
                                        op: LineOp::Context,
                                        text: t.to_owned(),
                                    });
                                    old_advance += 1;
                                    new_advance += 1;
                                    ctx_added += 1;
                                    k += 1;
                                } else {
                                    break;
                                }
                            }
                            j = k;
                            break;
                        } else {
                            hunk_lines.push(HunkLine {
                                op: LineOp::Context,
                                text: text.to_owned(),
                            });
                            old_advance += 1;
                            new_advance += 1;
                            j += 1;
                        }
                    }
                    Op::Delete(text) => {
                        hunk_lines.push(HunkLine {
                            op: LineOp::Delete,
                            text: text.to_owned(),
                        });
                        old_advance += 1;
                        j += 1;
                    }
                    Op::Insert(text) => {
                        hunk_lines.push(HunkLine {
                            op: LineOp::Insert,
                            text: text.to_owned(),
                        });
                        new_advance += 1;
                        j += 1;
                    }
                }
            }

            hunks.push(DiffHunk {
                old_start,
                old_lines: old_advance,
                new_start,
                new_lines: new_advance,
                lines: hunk_lines,
            });
            // Cursor auf j (an das Ende dieses Hunks) setzen; wir müssen die
            // Zeilennummern nachtragen.
            for op in ops.iter().take(j).skip(hunk_start) {
                match op {
                    Op::Equal(_) => {
                        old_line += 1;
                        new_line += 1;
                    }
                    Op::Delete(_) => old_line += 1,
                    Op::Insert(_) => new_line += 1,
                }
            }
            i = j;
        } else {
            old_line += 1;
            new_line += 1;
            i += 1;
        }
    }

    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_texts_produce_no_hunks() {
        let diff = UnifiedDiff::compute("a", "eins\nzwei\n", "b", "eins\nzwei\n");
        assert!(diff.hunks.is_empty());
    }

    #[test]
    fn insertion_is_captured_as_single_hunk() {
        let diff = UnifiedDiff::compute("a", "eins\ndrei\n", "b", "eins\nzwei\ndrei\n");
        assert_eq!(diff.hunks.len(), 1);
        let hunk = &diff.hunks[0];
        assert!(hunk
            .lines
            .iter()
            .any(|l| matches!(l.op, LineOp::Insert) && l.text == "zwei"));
    }

    #[test]
    fn apply_single_hunk_reproduces_full_new_text() {
        let old = "a\nb\nc\n";
        let new = "a\nx\nc\n";
        let diff = UnifiedDiff::compute("a", old, "b", new);
        let applied = apply_hunks_to_string(old, &diff.hunks.iter().collect::<Vec<_>>()).unwrap();
        assert_eq!(applied, new);
    }

    #[test]
    fn skipping_a_hunk_leaves_original_content_untouched_in_that_region() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
        let new = "a\nB\nc\nd\ne\nf\ng\nh\nI\nj\n";
        let diff = UnifiedDiff::compute_with_context("a", old, "b", new, 1);
        assert!(diff.hunks.len() >= 2);
        // Nur den ersten Hunk annehmen.
        let picked: Vec<&DiffHunk> = diff.hunks.iter().take(1).collect();
        let applied = apply_hunks_to_string(old, &picked).unwrap();
        assert!(applied.contains("\nB\n"));
        assert!(applied.contains("\ni\n")); // zweiter Hunk wurde nicht angewendet
    }

    #[test]
    fn unified_text_starts_with_file_headers() {
        let diff = UnifiedDiff::compute("old.rs", "a\nb\n", "new.rs", "a\nB\n");
        let text = diff.to_unified_text();
        assert!(text.starts_with("--- old.rs\n+++ new.rs\n"));
        assert!(text.contains("@@"));
    }

    #[test]
    fn applying_a_hunk_whose_context_has_shifted_is_a_hard_error() {
        let old = "a\nb\nc\n";
        let new = "a\nB\nc\n";
        let diff = UnifiedDiff::compute("a", old, "b", new);
        let hunks: Vec<&DiffHunk> = diff.hunks.iter().collect();
        // Wende den Hunk auf einen anderen Text an, der die Kontextzeile geändert hat.
        let shifted = "x\nb\nc\n";
        let error = apply_hunks_to_string(shifted, &hunks).unwrap_err();
        assert!(matches!(error, CodeError::Diff(_)));
    }
}
