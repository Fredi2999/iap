//! Werkzeuge des Code-Agenten: Ordner auflisten, Dateien lesen, suchen und Änderungen
//! **vorschlagen**.
//!
//! Der Agent arbeitet in einem Projektordner (Stick oder freigegebener Ordner auf dem PC) und
//! ist reiner Leser: Kein Werkzeug schreibt eine Datei. `propose_edit` legt einen Vorschlag in
//! einem [`StagedStore`] ab, den die Oberfläche als Diff zeigt und erst nach Bestätigung durch
//! den Nutzer schreibt (Invariante 4). Jeder Zugriff läuft durch `pa-policy` mit Audit-Eintrag.
//!
//! Anders als bei den allgemeinen Chat-Werkzeugen führen Fehler hier nicht zum Abbruch der
//! ganzen Schleife: Ein Agent liest ständig Pfade aus Dateilisten, und ein falscher Pfad oder
//! eine gesperrte Datei soll dem Modell als Ergebnis zurückgemeldet werden, damit es sich
//! korrigieren kann. Lesen gilt dabei nie als „aus Fremdinhalt abgeleitet“: Der Pfadbereich
//! (`PathScope`) ist die Grenze, und gelesene Texte verlassen den Rechner nicht.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};

use pa_policy::{
    host_root::is_secret_path, path::resolve_absolute_in_scope, AuditLog, AuditOutcome,
    CapabilityAction, CapabilityRequest, DerivationSource,
};
use pa_tools::{
    evaluate_and_audit, Tool, ToolContext, ToolError, ToolInvocation, ToolOutput, ToolRegistry,
    ToolSpec,
};
use serde_json::{json, Value};

use crate::{DiffHunk, LineOp, UnifiedDiff};

/// Ordner, die zu IAP gehören oder Git-Interna sind; der Agent sieht sie nie.
const HIDDEN: [&str; 6] = [
    ".git",
    ".snapshots",
    ".trash",
    ".iap-backup",
    ".iap-worktrees",
    ".svn",
];
/// Große, erzeugte Ordner, die die Suche überspringt.
const SEARCH_SKIP: [&str; 4] = ["node_modules", "target", "dist", "build"];
/// Größte Datei, die gelesen oder vorgeschlagen wird.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_SEARCH_FILES: usize = 5_000;
const MAX_SEARCH_BYTES: u64 = 512 * 1024;

/// Grenzen für Ergebnisgrößen; sie folgen der Kontextgröße des Modells, damit auch 8 GB reichen.
#[derive(Debug, Clone, Copy)]
pub struct AgentLimits {
    /// Zeichen, die `read_file` auf einmal liefert.
    pub read_chars: usize,
    /// Einträge, die `list_dir` liefert.
    pub list_entries: usize,
    /// Treffer, die `search` liefert.
    pub search_hits: usize,
}

impl AgentLimits {
    /// Grenzen passend zur Kontextgröße in Token.
    pub fn for_context(context_tokens: u32) -> Self {
        let tokens = usize::try_from(context_tokens).unwrap_or(4_096);
        Self {
            read_chars: (tokens.saturating_mul(3) / 2).clamp(1_500, 8_000),
            list_entries: 150,
            search_hits: (tokens / 100).clamp(15, 50),
        }
    }
}

/// Ein vorgeschlagener neuer Dateistand, noch nicht geschrieben.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedChange {
    /// Pfad relativ zum Projektordner, mit `/`.
    pub path: String,
    /// Inhalt, als der Vorschlag entstand (Zeilenenden als `\n`); leer bei neuen Dateien.
    pub original: String,
    /// Der vorgeschlagene Inhalt (Zeilenenden als `\n`).
    pub proposed: String,
    /// Die Datei gibt es noch nicht.
    pub is_new: bool,
}

impl StagedChange {
    /// Zeilen, die der Vorschlag hinzufügt und entfernt.
    pub fn line_counts(&self) -> (usize, usize) {
        counts(&UnifiedDiff::compute(
            &self.path,
            &self.original,
            &self.path,
            &self.proposed,
        ))
    }
}

fn counts(diff: &UnifiedDiff) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    for DiffHunk { lines, .. } in &diff.hunks {
        for line in lines {
            match line.op {
                LineOp::Insert => added += 1,
                LineOp::Delete => removed += 1,
                LineOp::Context => {}
            }
        }
    }
    (added, removed)
}

/// Gemeinsamer Speicher der Vorschläge einer Sitzung. Klonen teilt den Inhalt.
#[derive(Debug, Clone, Default)]
pub struct StagedStore(Arc<Mutex<Vec<StagedChange>>>);

impl StagedStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, Vec<StagedChange>> {
        // Ein vergifteter Speicher enthält nur Texte; er bleibt benutzbar.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn list(&self) -> Vec<StagedChange> {
        self.guard().clone()
    }

    pub fn get(&self, path: &str) -> Option<StagedChange> {
        self.guard().iter().find(|c| c.path == path).cloned()
    }

    /// Legt einen Vorschlag ab oder ersetzt den bisherigen für dieselbe Datei. Der Ausgangsstand
    /// bleibt dabei der der ersten Fassung, damit ein Diff immer zur echten Datei passt.
    pub fn put(&self, mut change: StagedChange) {
        let mut all = self.guard();
        if let Some(existing) = all.iter_mut().find(|c| c.path == change.path) {
            change.original = existing.original.clone();
            change.is_new = existing.is_new;
            *existing = change;
        } else {
            all.push(change);
        }
    }

    pub fn remove(&self, path: &str) {
        self.guard().retain(|c| c.path != path);
    }

    pub fn clear(&self) {
        self.guard().clear();
    }
}

fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn is_hidden_name(name: &str) -> bool {
    HIDDEN.iter().any(|h| h.eq_ignore_ascii_case(name))
}

/// Eine Meldung an das Modell. Fehler sind hier ein normales Ergebnis, kein Abbruch.
fn answer(invocation: &ToolInvocation, text: impl Into<String>, untrusted: bool) -> ToolOutput {
    ToolOutput {
        tool: invocation.name.clone(),
        content: text.into(),
        is_untrusted: untrusted,
        truncated_from_bytes: None,
    }
}

fn problem(invocation: &ToolInvocation, text: impl std::fmt::Display) -> ToolOutput {
    answer(invocation, format!("FEHLER: {text}"), false)
}

fn string_arg(invocation: &ToolInvocation, key: &str) -> Option<String> {
    invocation
        .arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn number_arg(invocation: &ToolInvocation, key: &str) -> Option<usize> {
    invocation
        .arguments
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
}

/// Prüft den Pfad mit der Policy und liefert den kanonischen Pfad. Existiert das Ziel noch
/// nicht (neue Datei), wird der tiefste vorhandene Vorfahre kanonisiert und der Rest geprüft.
fn resolve(
    invocation: &ToolInvocation,
    context: &mut ToolContext<'_>,
    action: CapabilityAction,
    relative: &str,
    why: &str,
) -> Result<PathBuf, String> {
    let cleaned = relative.trim().replace('\\', "/");
    let cleaned = cleaned.trim_start_matches("./").trim_matches('/');
    let cleaned = if cleaned.is_empty() { "." } else { cleaned };
    if cleaned.split('/').any(is_hidden_name) {
        return Err("Dieser Ordner gehört nicht zum Projekt und ist gesperrt.".to_owned());
    }
    if is_secret_path(cleaned) {
        return Err(
            "Dateien mit Zugangsdaten oder Schlüsseln liest und ändert IAP nicht.".to_owned(),
        );
    }
    let candidate = context.workspace.root().join(cleaned);
    if cleaned == "." || candidate.exists() {
        let request = CapabilityRequest {
            action,
            relative_path: Some(PathBuf::from(cleaned)),
            // Lesen ist durch den Pfadbereich begrenzt; die Herkunft des Pfads spielt dafür
            // keine Rolle (siehe Moduldoku).
            source: DerivationSource::UserIntent,
            reason: why.to_owned(),
        };
        return evaluate_and_audit(&invocation.name, &request, context)
            .map_err(|e| e.to_string())?
            .canonical_path
            .ok_or_else(|| "kein kanonischer Pfad".to_owned());
    }
    let resolved =
        resolve_absolute_in_scope(context.workspace, &candidate).map_err(|e| e.to_string())?;
    context
        .audit
        .append(
            AuditLog {
                mode: context.mode,
                action,
                target: Some(cleaned.to_owned()),
                outcome: AuditOutcome::Allow,
                reason: why.to_owned(),
            },
            context.now_unix_ms,
        )
        .map_err(|e| e.to_string())?;
    Ok(resolved)
}

fn read_text(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|_| "Die Datei gibt es nicht.".to_owned())?;
    if !meta.is_file() {
        return Err("Das ist keine Datei.".to_owned());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err("Die Datei ist größer als 1 MB.".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|_| "Die Datei lässt sich nicht lesen.".to_owned())?;
    String::from_utf8(bytes)
        .map(|text| lf(&text))
        .map_err(|_| "Das ist keine Textdatei (kein gültiges UTF-8).".to_owned())
}

fn relative_to_root(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).map_or_else(
        |_| path.to_string_lossy().into_owned(),
        |p| p.to_string_lossy().replace('\\', "/"),
    )
}

// ---------------------------------------------------------------------------
// list_dir
// ---------------------------------------------------------------------------

struct ListDirTool {
    limits: AgentLimits,
}

impl Tool for ListDirTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_dir".to_owned(),
            description: "Listet Dateien und Ordner im Projekt. `path` ist relativ zum Projektordner, `.` ist die Wurzel.".to_owned(),
            parameters_schema: json!({
                "type": "object",
                "required": ["path"],
                "properties": { "path": { "type": "string" } }
            }),
            category: "code".to_owned(),
        }
    }

    fn invoke(
        &self,
        invocation: &ToolInvocation,
        context: &mut ToolContext<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let relative = string_arg(invocation, "path").unwrap_or_else(|| ".".to_owned());
        let dir = match resolve(
            invocation,
            context,
            CapabilityAction::FileList,
            &relative,
            &format!("Code-Agent list_dir `{relative}`"),
        ) {
            Ok(dir) => dir,
            Err(reason) => return Ok(problem(invocation, reason)),
        };
        let Ok(read) = std::fs::read_dir(&dir) else {
            return Ok(problem(invocation, "Das ist kein Ordner."));
        };
        let mut entries: Vec<(bool, String, u64)> = read
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let kind = entry.file_type().ok()?;
                if kind.is_symlink() || is_hidden_name(&name) || is_secret_path(&name) {
                    return None;
                }
                let size = entry
                    .metadata()
                    .map_or(0, |m| if m.is_file() { m.len() } else { 0 });
                Some((kind.is_dir(), name, size))
            })
            .collect();
        entries.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
        });
        if entries.is_empty() {
            return Ok(answer(invocation, "(leer)", true));
        }
        let total = entries.len();
        let mut lines: Vec<String> = entries
            .into_iter()
            .take(self.limits.list_entries)
            .map(|(is_dir, name, size)| {
                if is_dir {
                    format!("{name}/")
                } else {
                    format!("{name} ({size} Bytes)")
                }
            })
            .collect();
        if total > self.limits.list_entries {
            lines.push(format!(
                "… {} weitere Einträge",
                total - self.limits.list_entries
            ));
        }
        Ok(answer(invocation, lines.join("\n"), true))
    }
}

// ---------------------------------------------------------------------------
// read_file
// ---------------------------------------------------------------------------

struct ReadFileTool {
    limits: AgentLimits,
}

impl Tool for ReadFileTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".to_owned(),
            description: "Liest eine Textdatei. Optional nur die Zeilen `start_line` bis `end_line` (1-basiert). Der Text kommt unverändert, ohne Zeilennummern.".to_owned(),
            parameters_schema: json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string" },
                    "start_line": { "type": "integer" },
                    "end_line": { "type": "integer" }
                }
            }),
            category: "code".to_owned(),
        }
    }

    fn invoke(
        &self,
        invocation: &ToolInvocation,
        context: &mut ToolContext<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let Some(relative) = string_arg(invocation, "path") else {
            return Ok(problem(invocation, "Parameter `path` fehlt."));
        };
        let path = match resolve(
            invocation,
            context,
            CapabilityAction::FileRead,
            &relative,
            &format!("Code-Agent read_file `{relative}`"),
        ) {
            Ok(path) => path,
            Err(reason) => return Ok(problem(invocation, reason)),
        };
        let text = match read_text(&path) {
            Ok(text) => text,
            Err(reason) => return Ok(problem(invocation, reason)),
        };
        let lines: Vec<&str> = text.lines().collect();
        let total = lines.len();
        let start = number_arg(invocation, "start_line").unwrap_or(1).max(1);
        let end = number_arg(invocation, "end_line")
            .unwrap_or(total)
            .min(total);
        if total == 0 {
            return Ok(answer(invocation, "[Datei ist leer]", true));
        }
        if start > end {
            return Ok(problem(
                invocation,
                format!("Die Datei hat nur {total} Zeilen."),
            ));
        }
        let mut body = lines[start - 1..end].join("\n");
        let mut shown_end = end;
        if body.chars().count() > self.limits.read_chars {
            body = body.chars().take(self.limits.read_chars).collect();
            shown_end = start - 1 + body.lines().count();
            body.push_str("\n[gekürzt, lies den Rest mit start_line und end_line]");
        }
        Ok(answer(
            invocation,
            format!("[{relative}: Zeilen {start}–{shown_end} von {total}]\n{body}"),
            true,
        ))
    }
}

// ---------------------------------------------------------------------------
// search
// ---------------------------------------------------------------------------

struct SearchTool {
    limits: AgentLimits,
}

fn search_files(root: &Path, base: &Path, query: &str, max_hits: usize) -> Vec<String> {
    let needle = query.to_lowercase();
    let mut hits = Vec::new();
    let mut stack = vec![base.to_path_buf()];
    let mut visited = 0_usize;
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = read.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() || is_hidden_name(&name) || is_secret_path(&name) {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                if !SEARCH_SKIP.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                    stack.push(path);
                }
                continue;
            }
            visited += 1;
            if visited > MAX_SEARCH_FILES {
                return hits;
            }
            if entry
                .metadata()
                .map_or(true, |m| m.len() > MAX_SEARCH_BYTES)
            {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if bytes.contains(&0) {
                continue;
            }
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let relative = relative_to_root(root, &path);
            for (index, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&needle) {
                    let shown: String = line.trim().chars().take(160).collect();
                    hits.push(format!("{relative}:{}: {shown}", index + 1));
                    if hits.len() >= max_hits {
                        return hits;
                    }
                }
            }
        }
    }
    hits
}

impl Tool for SearchTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "search".to_owned(),
            description: "Sucht Text (ohne Groß-/Kleinschreibung) in den Dateien des Projekts. Optional nur unter `path`.".to_owned(),
            parameters_schema: json!({
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "path": { "type": "string" }
                }
            }),
            category: "code".to_owned(),
        }
    }

    fn invoke(
        &self,
        invocation: &ToolInvocation,
        context: &mut ToolContext<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let query = string_arg(invocation, "query").unwrap_or_default();
        if query.trim().chars().count() < 2 {
            return Ok(problem(
                invocation,
                "Die Suche braucht mindestens zwei Zeichen.",
            ));
        }
        let relative = string_arg(invocation, "path").unwrap_or_else(|| ".".to_owned());
        let base = match resolve(
            invocation,
            context,
            CapabilityAction::FileList,
            &relative,
            &format!("Code-Agent search `{}` in `{relative}`", query.trim()),
        ) {
            Ok(base) => base,
            Err(reason) => return Ok(problem(invocation, reason)),
        };
        let root = context.workspace.root().to_path_buf();
        let hits = search_files(&root, &base, query.trim(), self.limits.search_hits);
        Ok(answer(
            invocation,
            if hits.is_empty() {
                "(keine Treffer)".to_owned()
            } else {
                hits.join("\n")
            },
            true,
        ))
    }
}

// ---------------------------------------------------------------------------
// propose_edit
// ---------------------------------------------------------------------------

struct ProposeEditTool {
    store: StagedStore,
}

impl Tool for ProposeEditTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "propose_edit".to_owned(),
            description: "Schlägt eine Änderung vor, ohne etwas zu schreiben; die Nutzerin oder der Nutzer prüft den Unterschied und übernimmt ihn. Entweder `old` und `new` (ersetzt genau eine Stelle; `old` muss exakt und eindeutig im Text stehen) oder `content` (ganze neue Datei oder vollständiger neuer Inhalt).".to_owned(),
            parameters_schema: json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string" },
                    "old": { "type": "string" },
                    "new": { "type": "string" },
                    "content": { "type": "string" }
                }
            }),
            category: "code".to_owned(),
        }
    }

    fn invoke(
        &self,
        invocation: &ToolInvocation,
        context: &mut ToolContext<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let Some(relative) = string_arg(invocation, "path") else {
            return Ok(problem(invocation, "Parameter `path` fehlt."));
        };
        let path = match resolve(
            invocation,
            context,
            CapabilityAction::FileRead,
            &relative,
            &format!("Code-Agent propose_edit `{relative}` (nur Vorschlag, schreibt nichts)"),
        ) {
            Ok(path) => path,
            Err(reason) => return Ok(problem(invocation, reason)),
        };
        let shown = relative_to_root(context.workspace.root(), &path);
        let on_disk = if path.exists() {
            match read_text(&path) {
                Ok(text) => Some(text),
                Err(reason) => return Ok(problem(invocation, reason)),
            }
        } else {
            None
        };
        // Der Ausgangspunkt für `old`/`new` ist der bisherige Vorschlag, falls es schon einen
        // gibt: So lassen sich mehrere Änderungen an derselben Datei nacheinander vorschlagen.
        let base = self
            .store
            .get(&shown)
            .map(|c| c.proposed)
            .or_else(|| on_disk.clone());
        let proposed = if let Some(content) = string_arg(invocation, "content") {
            lf(&content)
        } else if let (Some(old), Some(new)) =
            (string_arg(invocation, "old"), string_arg(invocation, "new"))
        {
            let Some(base) = base else {
                return Ok(problem(
                    invocation,
                    "Die Datei gibt es nicht. Für eine neue Datei nimm `content`.",
                ));
            };
            let old = lf(&old);
            if old.is_empty() {
                return Ok(problem(invocation, "`old` darf nicht leer sein."));
            }
            match base.matches(&old).count() {
                0 => {
                    return Ok(problem(
                        invocation,
                        "`old` steht nicht im Text. Lies die Datei mit read_file und kopiere die Stelle exakt, einschließlich Einrückung.",
                    ))
                }
                1 => base.replacen(&old, &lf(&new), 1),
                n => {
                    return Ok(problem(
                        invocation,
                        format!("`old` kommt {n}-mal vor. Nimm mehr Umgebung, damit die Stelle eindeutig ist."),
                    ))
                }
            }
        } else {
            return Ok(problem(
                invocation,
                "Gib entweder `old` und `new` oder `content` an.",
            ));
        };
        if proposed.len() as u64 > MAX_FILE_BYTES {
            return Ok(problem(invocation, "Der Vorschlag ist größer als 1 MB."));
        }
        let change = StagedChange {
            path: shown.clone(),
            original: on_disk.clone().unwrap_or_default(),
            proposed,
            is_new: on_disk.is_none(),
        };
        let (added, removed) = StagedChange {
            original: self
                .store
                .get(&shown)
                .map_or_else(|| change.original.clone(), |c| c.original),
            ..change.clone()
        }
        .line_counts();
        self.store.put(change);
        Ok(answer(
            invocation,
            format!(
                "Vorschlag für {shown} gemerkt (+{added} −{removed} Zeilen gegenüber der Datei). Geschrieben ist noch nichts: Die Nutzerin oder der Nutzer prüft ihn und übernimmt ihn selbst."
            ),
            false,
        ))
    }
}

// ---------------------------------------------------------------------------
// run_command
// ---------------------------------------------------------------------------

/// Eine Anfrage des Modells, einen Befehl im Projektordner auszuführen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRequest {
    pub program: String,
    pub args: Vec<String>,
    /// Arbeitsordner relativ zum Projekt; `.` ist die Wurzel.
    pub cwd: String,
    pub timeout_seconds: u64,
    /// Das Modell hat Programm oder Argumente aus gelesenem Dateiinhalt übernommen. Der
    /// Bestätigungsdialog weist darauf hin, denn so sähe ein Prompt-Injection-Versuch aus.
    pub derived_from_content: bool,
}

/// Die Stelle, die einen Befehl prüft, vom Nutzer bestätigen lässt und ausführt. Sie liegt
/// außerhalb dieser Crate (Dialog und Prozessstart gehören zur App); das Werkzeug kennt nur
/// diese Schnittstelle und bekommt einen Text für das Modell zurück: das Ergebnis, eine
/// Ablehnung oder einen Fehler.
pub trait CommandGate: Send + Sync {
    fn run(&self, request: CommandRequest) -> String;
}

const DEFAULT_COMMAND_SECONDS: u64 = 120;
const MAX_COMMAND_SECONDS: u64 = 600;

struct RunCommandTool {
    gate: Arc<dyn CommandGate>,
}

impl Tool for RunCommandTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "run_command".to_owned(),
            description: "Führt einen Befehl im Projekt aus, zum Beispiel Tests oder einen Build. Die Nutzerin oder der Nutzer bestätigt jeden Befehl einzeln und kann ihn ablehnen. Keine Shell: `program` ist ein Programmname wie `cargo` oder `npm`, `args` die Argumente als Liste. Du bekommst die Ausgabe zurück.".to_owned(),
            parameters_schema: json!({
                "type": "object",
                "required": ["program"],
                "properties": {
                    "program": { "type": "string" },
                    "args": { "type": "array", "items": { "type": "string" } },
                    "cwd": { "type": "string" },
                    "timeout_seconds": { "type": "integer" }
                }
            }),
            category: "code".to_owned(),
        }
    }

    fn invoke(
        &self,
        invocation: &ToolInvocation,
        _context: &mut ToolContext<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let Some(program) = string_arg(invocation, "program") else {
            return Ok(problem(invocation, "Parameter `program` fehlt."));
        };
        let args: Vec<String> = match invocation.arguments.get("args") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .collect(),
            Some(_) => {
                return Ok(problem(
                    invocation,
                    "`args` muss eine Liste von Texten sein.",
                ))
            }
        };
        let seconds = number_arg(invocation, "timeout_seconds")
            .map_or(DEFAULT_COMMAND_SECONDS, |n| n as u64)
            .clamp(1, MAX_COMMAND_SECONDS);
        let request = CommandRequest {
            program,
            args,
            cwd: string_arg(invocation, "cwd").unwrap_or_else(|| ".".to_owned()),
            timeout_seconds: seconds,
            derived_from_content: invocation.source == DerivationSource::UntrustedContent,
        };
        Ok(answer(invocation, self.gate.run(request), true))
    }
}

/// Die Werkzeuge des Code-Agenten. Eine eigene Registry, nicht die allgemeine: Es gibt keinen
/// schreibenden Pfad, und `write_file` aus dem Chat gehört nicht hierher. `run_command` gibt es
/// nur, wenn eine Bestätigungsstelle (`gate`) angeschlossen ist.
pub fn agent_registry(
    store: &StagedStore,
    limits: AgentLimits,
    gate: Option<Arc<dyn CommandGate>>,
) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(ListDirTool { limits }));
    registry.register(Arc::new(ReadFileTool { limits }));
    registry.register(Arc::new(SearchTool { limits }));
    registry.register(Arc::new(ProposeEditTool {
        store: store.clone(),
    }));
    if let Some(gate) = gate {
        registry.register(Arc::new(RunCommandTool { gate }));
    }
    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use pa_policy::{AuditStore, GrantStore, Mode, PathScope};
    use std::collections::BTreeMap;

    struct Fixture {
        temp: tempfile::TempDir,
        scope: PathScope,
        audit: AuditStore,
        grants: GrantStore,
        store: StagedStore,
        registry: ToolRegistry,
    }

    fn fixture() -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("projekt");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        )
        .unwrap();
        std::fs::write(root.join("README.md"), "# Projekt\n").unwrap();
        std::fs::write(root.join(".env"), "TOKEN=geheim\n").unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/config"), "[core]\n").unwrap();
        let store = StagedStore::new();
        let registry = agent_registry(&store, AgentLimits::for_context(4096), None);
        Fixture {
            scope: PathScope::new(&root).unwrap(),
            temp,
            audit: AuditStore::open_in_memory().unwrap(),
            grants: GrantStore::default(),
            store,
            registry,
        }
    }

    impl Fixture {
        fn call(&mut self, tool: &str, args: Value, source: DerivationSource) -> String {
            let arguments: BTreeMap<String, Value> = match args {
                Value::Object(map) => map.into_iter().collect(),
                _ => BTreeMap::new(),
            };
            let invocation = ToolInvocation {
                name: tool.to_owned(),
                arguments,
                source,
            };
            let mut context = ToolContext {
                workspace: &self.scope,
                mode: Mode::M0Observe,
                grants: &self.grants,
                audit: &mut self.audit,
                now_unix_ms: 1,
                permission_hook: None,
            };
            self.registry
                .invoke(&invocation, &mut context)
                .expect("Werkzeug darf nicht abbrechen")
                .content
        }

        fn root(&self) -> PathBuf {
            self.temp.path().join("projekt")
        }
    }

    #[test]
    fn the_agent_lists_reads_and_searches_but_never_sees_secrets_or_git_internals() {
        let mut fx = fixture();
        let listing = fx.call(
            "list_dir",
            json!({"path": "."}),
            DerivationSource::UserIntent,
        );
        assert!(listing.contains("src/"), "{listing}");
        assert!(listing.contains("README.md"), "{listing}");
        assert!(!listing.contains(".env"), "{listing}");
        assert!(!listing.contains(".git"), "{listing}");

        let read = fx.call(
            "read_file",
            json!({"path": "src/main.rs"}),
            DerivationSource::UserIntent,
        );
        assert!(read.contains("println!"), "{read}");
        assert!(read.contains("Zeilen 1–3 von 3"), "{read}");

        let range = fx.call(
            "read_file",
            json!({"path": "src/main.rs", "start_line": 2, "end_line": 2}),
            DerivationSource::UserIntent,
        );
        assert!(
            range.contains("println!") && !range.contains("fn main"),
            "{range}"
        );

        let hits = fx.call(
            "search",
            json!({"query": "PRINTLN"}),
            DerivationSource::UserIntent,
        );
        assert!(hits.starts_with("src/main.rs:2:"), "{hits}");
        let none = fx.call(
            "search",
            json!({"query": "geheim"}),
            DerivationSource::UserIntent,
        );
        assert_eq!(none, "(keine Treffer)", "{none}");

        for blocked in [".env", ".git/config"] {
            let result = fx.call(
                "read_file",
                json!({"path": blocked}),
                DerivationSource::UserIntent,
            );
            assert!(result.starts_with("FEHLER:"), "{blocked}: {result}");
        }
    }

    #[test]
    fn paths_derived_from_earlier_results_do_not_abort_the_agent() {
        // Regression gegen das Verhalten der Chat-Werkzeuge: Ein Pfad, der in einer Liste stand,
        // gilt dort als Fremdinhalt, und die ganze Schleife bricht ab.
        let mut fx = fixture();
        let result = fx.call(
            "read_file",
            json!({"path": "src/main.rs"}),
            DerivationSource::UntrustedContent,
        );
        assert!(result.contains("println!"), "{result}");
    }

    #[test]
    fn paths_that_leave_the_project_are_refused_as_results_not_as_aborts() {
        let mut fx = fixture();
        for bad in [
            "../geheim.txt",
            "..\\x",
            "src/../../x",
            "C:/Windows/win.ini",
            "/etc/passwd",
        ] {
            let result = fx.call(
                "read_file",
                json!({"path": bad}),
                DerivationSource::UserIntent,
            );
            assert!(result.starts_with("FEHLER:"), "{bad}: {result}");
        }
    }

    #[test]
    fn a_proposal_changes_nothing_on_disk_and_chains_on_the_previous_proposal() {
        let mut fx = fixture();
        let before = std::fs::read(fx.root().join("src/main.rs")).unwrap();
        let first = fx.call(
            "propose_edit",
            json!({"path": "src/main.rs", "old": "\"hi\"", "new": "\"hallo\""}),
            DerivationSource::UserIntent,
        );
        assert!(first.contains("+1 −1"), "{first}");
        let second = fx.call(
            "propose_edit",
            json!({"path": "src/main.rs", "old": "fn main()", "new": "fn start()"}),
            DerivationSource::UserIntent,
        );
        assert!(second.contains("gemerkt"), "{second}");
        // Beide Änderungen stecken im einen Vorschlag; der Ausgangsstand ist die echte Datei.
        let staged = fx.store.get("src/main.rs").unwrap();
        assert!(staged.proposed.contains("fn start()") && staged.proposed.contains("\"hallo\""));
        assert_eq!(
            staged.original,
            lf(&String::from_utf8(before.clone()).unwrap())
        );
        assert_eq!(fx.store.list().len(), 1);
        // Auf der Platte ist nichts passiert.
        assert_eq!(
            std::fs::read(fx.root().join("src/main.rs")).unwrap(),
            before
        );
    }

    #[test]
    fn ambiguous_or_missing_old_text_is_reported_back_to_the_model() {
        let mut fx = fixture();
        std::fs::write(fx.root().join("a.txt"), "x = 1\nx = 1\n").unwrap();
        let twice = fx.call(
            "propose_edit",
            json!({"path": "a.txt", "old": "x = 1", "new": "x = 2"}),
            DerivationSource::UserIntent,
        );
        assert!(
            twice.starts_with("FEHLER:") && twice.contains("2-mal"),
            "{twice}"
        );
        let missing = fx.call(
            "propose_edit",
            json!({"path": "a.txt", "old": "y = 9", "new": "y = 0"}),
            DerivationSource::UserIntent,
        );
        assert!(missing.contains("steht nicht im Text"), "{missing}");
        assert!(fx.store.list().is_empty());
    }

    #[test]
    fn new_files_and_deep_new_folders_can_be_proposed_but_not_secrets_or_escapes() {
        let mut fx = fixture();
        let created = fx.call(
            "propose_edit",
            json!({"path": "neu/tief/lib.rs", "content": "pub fn a() {}\n"}),
            DerivationSource::UserIntent,
        );
        assert!(created.contains("gemerkt"), "{created}");
        let staged = fx.store.get("neu/tief/lib.rs").unwrap();
        assert!(staged.is_new && staged.original.is_empty());
        assert!(!fx.root().join("neu").exists(), "nichts angelegt");

        for bad in [
            ".env",
            "../draussen.txt",
            "deploy/server.pem",
            ".git/hooks/pre-commit",
        ] {
            let result = fx.call(
                "propose_edit",
                json!({"path": bad, "content": "x"}),
                DerivationSource::UserIntent,
            );
            assert!(result.starts_with("FEHLER:"), "{bad}: {result}");
        }
        assert_eq!(fx.store.list().len(), 1);
    }

    #[test]
    fn crlf_files_are_compared_in_normalized_form() {
        let mut fx = fixture();
        std::fs::write(fx.root().join("w.txt"), "a\r\nb\r\n").unwrap();
        let result = fx.call(
            "propose_edit",
            json!({"path": "w.txt", "old": "a\nb", "new": "a\nc"}),
            DerivationSource::UserIntent,
        );
        assert!(result.contains("gemerkt"), "{result}");
        assert_eq!(fx.store.get("w.txt").unwrap().proposed, "a\nc\n");
    }

    #[test]
    fn every_access_lands_in_the_audit_log() {
        let mut fx = fixture();
        fx.call(
            "read_file",
            json!({"path": "README.md"}),
            DerivationSource::UserIntent,
        );
        fx.call(
            "list_dir",
            json!({"path": "src"}),
            DerivationSource::UserIntent,
        );
        fx.call(
            "propose_edit",
            json!({"path": "neu.txt", "content": "x"}),
            DerivationSource::UserIntent,
        );
        let entries = fx.audit.all().unwrap();
        assert_eq!(
            fx.audit.verify().unwrap(),
            entries.len(),
            "Hash-Kette intakt"
        );
        assert!(entries.len() >= 3, "{}", entries.len());
    }

    #[test]
    fn long_files_are_cut_to_the_limit_with_a_hint() {
        let mut fx = fixture();
        let big: String = (0..2000).map(|i| format!("zeile {i}\n")).collect();
        std::fs::write(fx.root().join("big.txt"), big).unwrap();
        let result = fx.call(
            "read_file",
            json!({"path": "big.txt"}),
            DerivationSource::UserIntent,
        );
        assert!(
            result.contains("gekürzt"),
            "{}",
            &result[..80.min(result.len())]
        );
        assert!(result.chars().count() < AgentLimits::for_context(4096).read_chars + 200);
    }

    struct RecordingGate {
        seen: Mutex<Vec<CommandRequest>>,
    }

    impl CommandGate for RecordingGate {
        fn run(&self, request: CommandRequest) -> String {
            self.seen.lock().unwrap().push(request);
            "Exit-Code 0".to_owned()
        }
    }

    #[test]
    fn run_command_only_exists_with_a_gate_and_passes_a_checked_request_to_it() {
        let mut fx = fixture();
        assert!(fx
            .registry
            .all_specs()
            .iter()
            .all(|s| s.name != "run_command"));

        let gate = Arc::new(RecordingGate {
            seen: Mutex::new(Vec::new()),
        });
        fx.registry = agent_registry(
            &fx.store,
            AgentLimits::for_context(4096),
            Some(gate.clone()),
        );
        let result = fx.call(
            "run_command",
            json!({"program": "cargo", "args": ["test", "-p", 5], "cwd": "src", "timeout_seconds": 99999}),
            DerivationSource::UserIntent,
        );
        assert_eq!(result, "Exit-Code 0");
        let seen = gate.seen.lock().unwrap();
        assert_eq!(seen[0].program, "cargo");
        assert_eq!(seen[0].args, ["test", "-p", "5"]);
        assert_eq!(seen[0].cwd, "src");
        assert_eq!(seen[0].timeout_seconds, 600, "Obergrenze");
        assert!(!seen[0].derived_from_content);
    }

    #[test]
    fn a_command_derived_from_file_content_is_flagged_for_the_dialog() {
        let mut fx = fixture();
        let gate = Arc::new(RecordingGate {
            seen: Mutex::new(Vec::new()),
        });
        fx.registry = agent_registry(
            &fx.store,
            AgentLimits::for_context(4096),
            Some(gate.clone()),
        );
        fx.call(
            "run_command",
            json!({"program": "cargo"}),
            DerivationSource::UntrustedContent,
        );
        let missing = fx.call("run_command", json!({}), DerivationSource::UserIntent);
        assert!(missing.starts_with("FEHLER:"), "{missing}");
        let bad = fx.call(
            "run_command",
            json!({"program": "x", "args": "kein array"}),
            DerivationSource::UserIntent,
        );
        assert!(bad.starts_with("FEHLER:"), "{bad}");
        let seen = gate.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].derived_from_content);
        assert_eq!(seen[0].timeout_seconds, 120, "Standard");
    }
}
