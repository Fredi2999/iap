//! Ausführungsstufen A–D nach Konzept 9.3.
//!
//! Der Auftrag: **Stufe A und B aktivieren, C nur vorbereiten, nicht
//! aktivieren.**
//!
//! - **Stufe A (Analyse)** ist reines Parsen ohne Ausführung — hier
//!   reicht ein Formatprüfer (Klammern, UTF-8, minimaler Syntaxcheck
//!   je Sprache). Ein echter Linter/Typprüfer kommt später als eigener
//!   Skill; die Grundstruktur `run_stage_a` erfüllt aber die
//!   Anforderung „vollständig sicher, überall verfügbar".
//! - **Stufe B (WASM)** ist als Handshake vorbereitet — die eigentliche
//!   Pyodide/QuickJS-WASM-Runtime wird in einem eigenen Meilenstein
//!   angebunden (großer Modell-/Runtime-Download). Aktuell meldet die
//!   API `StageAvailability::NotBundled`, damit die UI ehrlich zeigt,
//!   dass B strukturell da, aber noch nicht ausrollt ist.
//! - **Stufe C (Prozess-Sandbox)** ist mit fester Availability
//!   `Disabled` verankert. Der Aufruf wirft einen Fehler — Konzept 9.3
//!   „nur mit Dev-Pack, einmalige Bestätigung pro Session mit klarem
//!   Hinweistext" gehört bewusst nicht in diesen Prototypen.

use serde::{Deserialize, Serialize};

use crate::CodeError;

/// Die drei aktiven Stufen aus Konzept 9.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStage {
    A,
    B,
    C,
}

impl ExecutionStage {
    pub fn as_str(self) -> &'static str {
        match self {
            ExecutionStage::A => "a_analysis",
            ExecutionStage::B => "b_wasm",
            ExecutionStage::C => "c_process",
        }
    }
}

/// Was der Aufrufer über die Verfügbarkeit einer Stufe wissen darf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageAvailability {
    /// Wird sofort ausgeführt.
    Available,
    /// Strukturell vorhanden, aber der Runtime-Blob fehlt (z. B. Pyodide).
    NotBundled,
    /// Bewusst deaktiviert — nur mit Dev-Pack, hier nie automatisch.
    Disabled,
}

/// Anfrage an die Ausführungs-API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionRequest {
    pub language: String,
    pub code: String,
}

/// Rückgabe einer Ausführung. `stdout` ist auf 16 KiB gedeckelt; alles
/// darüber wird gekürzt (die UI zeigt das an).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub stage: ExecutionStage,
    pub status: ExecutionStatus,
    pub stdout: String,
    pub stderr: String,
    /// Wanduhr-Dauer der Ausführung in Millisekunden (Stufe A: reine Analyse).
    pub elapsed_ms: u64,
    /// Warnhinweise (Konzept 9.3 „Ehrlichkeit ist wichtiger als beruhigende
    /// Sandbox-Label"). Wird von der UI unter dem Ergebnis angezeigt.
    pub warnings: Vec<String>,
}

/// Status der Ausführung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Ok,
    ParseError,
    RuntimeError,
    /// Stufe war nicht verfügbar.
    Unavailable,
}

/// Meldet für jede Stufe, ob sie in der aktuellen Build-Konfiguration
/// verfügbar ist.
pub fn availability(stage: ExecutionStage) -> StageAvailability {
    match stage {
        ExecutionStage::A => StageAvailability::Available,
        ExecutionStage::B => StageAvailability::NotBundled,
        ExecutionStage::C => StageAvailability::Disabled,
    }
}

/// Ein Baustein, den Stufe B braucht, um `Available` zu werden.
///
/// Wird von der UI angezeigt, damit klar ist, was fehlt („Wasmtime-Crate,
/// Pyodide-Blob, QuickJS-WASM"). Der Meilenstein-7-Aufbau folgt in einem
/// späteren, isolierten Schritt zusammen mit der Manifest-Erweiterung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageBPrerequisite {
    /// Kurzname für UI und Log.
    pub id: String,
    /// Menschlich lesbarer Text.
    pub label: String,
    /// Ist die Voraussetzung heute erfüllt?
    pub satisfied: bool,
    /// Kurzer Begründungssatz, warum nicht.
    pub reason: String,
}

/// Liste der Voraussetzungen von Stufe B. Wird von der UI/Doku genutzt, um
/// nichts zu verstecken (Konzept 9.3: „Ehrlichkeit vor beruhigenden Labels").
pub fn stage_b_prerequisites() -> Vec<StageBPrerequisite> {
    vec![
        StageBPrerequisite {
            id: "wasmtime_crate".to_owned(),
            label: "Rust-Wasmtime-Runtime".to_owned(),
            satisfied: false,
            reason: "Crate `wasmtime` ist im aktuellen Offline-Vendor-Cache nicht vorhanden."
                .to_owned(),
        },
        StageBPrerequisite {
            id: "pyodide_blob".to_owned(),
            label: "Pyodide-WASM-Blob (Python)".to_owned(),
            satisfied: false,
            reason: "Pyodide-Blob ist noch nicht Bestandteil des Portable-Bundles (~40 MB, Nutzeraufgabe)."
                .to_owned(),
        },
        StageBPrerequisite {
            id: "quickjs_blob".to_owned(),
            label: "QuickJS-WASM (JavaScript)".to_owned(),
            satisfied: false,
            reason: "QuickJS-WASM ist noch nicht Bestandteil des Portable-Bundles (~1 MB)."
                .to_owned(),
        },
    ]
}

/// Führt eine Anfrage aus. Delegiert an die Stufen-spezifischen
/// Funktionen; die UI ruft weiterhin diese eine Funktion und muss die
/// Availability-Logik nicht selbst kennen.
pub fn execute(
    stage: ExecutionStage,
    request: &ExecutionRequest,
) -> Result<ExecutionResult, CodeError> {
    match stage {
        ExecutionStage::A => run_stage_a(request),
        ExecutionStage::B => run_stage_b(request),
        ExecutionStage::C => Err(CodeError::Execution(
            "Stufe C ist bewusst deaktiviert; nur mit Dev-Pack und expliziter Nutzerbestätigung \
             pro Session zulässig (Konzept 9.3)."
                .to_owned(),
        )),
    }
}

/// Skeleton für Stufe B (Konzept 9.3, Meilenstein 7).
///
/// Solange nicht **alle** Voraussetzungen aus [`stage_b_prerequisites`]
/// erfüllt sind, liefert diese Funktion `Unavailable` mit einer präzisen
/// Warnung, die auflistet, was fehlt. Fake-Ausführungen sind bewusst
/// ausgeschlossen — der Nutzer soll ehrlich sehen, was der Grund ist.
pub fn run_stage_b(_request: &ExecutionRequest) -> Result<ExecutionResult, CodeError> {
    let prerequisites = stage_b_prerequisites();
    let missing: Vec<String> = prerequisites
        .iter()
        .filter(|p| !p.satisfied)
        .map(|p| format!("- {}: {}", p.label, p.reason))
        .collect();
    let warnings = if missing.is_empty() {
        vec!["Stufe B ist bereit; die konkrete Wasmtime-Ausführung wird in einem späteren Schritt aktiviert.".to_owned()]
    } else {
        let mut w = vec![
            "Stufe B (WASM) ist strukturell vorbereitet, aber noch nicht ausrollbar.".to_owned(),
        ];
        w.extend(missing);
        w
    };
    Ok(ExecutionResult {
        stage: ExecutionStage::B,
        status: ExecutionStatus::Unavailable,
        stdout: String::new(),
        stderr: String::new(),
        elapsed_ms: 0,
        warnings,
    })
}

/// Führt Stufe A aus: parsen, ohne auszuführen.
///
/// Sprachspezifisch:
/// - **Rust:** vollständiger Parse über `syn::parse_file` — echte
///   Syntaxprüfung inklusive Attribut-, Item- und Ausdrucksstruktur.
/// - **JSON:** über `serde_json::from_str::<Value>()` deserialisiert
///   und damit strikt geprüft.
/// - **Python:** Balance plus Prüfung der Einrückungskonsistenz
///   (kein Mix aus Tabs und Leerzeichen auf derselben Ebene).
/// - **JavaScript, TypeScript, Markdown, Rest:** aktuell noch Balance-
///   Check als Fallback. Tree-sitter-Grammatiken sind wegen ihrer
///   Größe (mehrere MB pro Sprache) im Offline-Vendor-Cache noch nicht
///   verfügbar und bleiben ein späterer, isolierter Ausbau.
pub fn run_stage_a(request: &ExecutionRequest) -> Result<ExecutionResult, CodeError> {
    let started = std::time::Instant::now();
    let language = normalize_language(&request.language);
    let outcome = match language {
        Language::Rust => parse_rust(&request.code),
        Language::Json => parse_json(&request.code),
        Language::Python => parse_python(&request.code),
        Language::JavaScript | Language::TypeScript | Language::Markdown | Language::Other => {
            balanced_delimiters(&request.code).into()
        }
    };
    let elapsed_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    match outcome {
        ParseOutcome::Ok { detail } => Ok(ExecutionResult {
            stage: ExecutionStage::A,
            status: ExecutionStatus::Ok,
            stdout: detail,
            stderr: String::new(),
            elapsed_ms,
            warnings: stage_a_warnings(language),
        }),
        ParseOutcome::Err { message } => Ok(ExecutionResult {
            stage: ExecutionStage::A,
            status: ExecutionStatus::ParseError,
            stdout: String::new(),
            stderr: message,
            elapsed_ms,
            warnings: Vec::new(),
        }),
    }
}

/// Sprach-Ansätze für Stufe A.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    Json,
    Python,
    JavaScript,
    TypeScript,
    Markdown,
    Other,
}

fn normalize_language(raw: &str) -> Language {
    match raw.to_ascii_lowercase().as_str() {
        "rust" | "rs" => Language::Rust,
        "json" => Language::Json,
        "python" | "py" => Language::Python,
        "javascript" | "js" => Language::JavaScript,
        "typescript" | "ts" | "tsx" => Language::TypeScript,
        "markdown" | "md" => Language::Markdown,
        _ => Language::Other,
    }
}

fn stage_a_warnings(language: Language) -> Vec<String> {
    match language {
        Language::Rust => vec!["Rust: syn 2 hat den Vollparse durchlaufen.".to_owned()],
        Language::Json => vec!["JSON: serde_json hat strict parsed.".to_owned()],
        Language::Python => vec![
            "Python: Balance und Einrückungskonsistenz geprüft (Tabs/Spaces gemischt = Warnung).".to_owned(),
            "Vollparser über tree-sitter folgt später (Grammatiken derzeit nicht im Vendor-Cache).".to_owned(),
        ],
        _ => vec![
            "Sprache: nur Delimiter-Balance geprüft; sprachspezifischer Parser folgt in einem späteren Meilenstein.".to_owned(),
        ],
    }
}

enum ParseOutcome {
    Ok { detail: String },
    Err { message: String },
}

impl From<(bool, String)> for ParseOutcome {
    fn from((ok, message): (bool, String)) -> Self {
        if ok {
            ParseOutcome::Ok {
                detail: "Delimiter-Balance: OK.".to_owned(),
            }
        } else {
            ParseOutcome::Err { message }
        }
    }
}

fn parse_rust(source: &str) -> ParseOutcome {
    match syn::parse_file(source) {
        Ok(file) => {
            let item_count = file.items.len();
            ParseOutcome::Ok {
                detail: format!("Rust: {item_count} Top-Level-Items geparst."),
            }
        }
        Err(error) => ParseOutcome::Err {
            message: format!("Rust-Parse-Fehler: {error}"),
        },
    }
}

fn parse_json(source: &str) -> ParseOutcome {
    match serde_json::from_str::<serde_json::Value>(source) {
        Ok(value) => {
            let kind = match &value {
                serde_json::Value::Null => "null",
                serde_json::Value::Bool(_) => "bool",
                serde_json::Value::Number(_) => "number",
                serde_json::Value::String(_) => "string",
                serde_json::Value::Array(_) => "array",
                serde_json::Value::Object(_) => "object",
            };
            ParseOutcome::Ok {
                detail: format!("JSON: Root-Typ = {kind}."),
            }
        }
        Err(error) => ParseOutcome::Err {
            message: format!(
                "JSON-Parse-Fehler bei Zeile {}, Spalte {}: {}",
                error.line(),
                error.column(),
                error
            ),
        },
    }
}

fn parse_python(source: &str) -> ParseOutcome {
    let (balanced, message) = balanced_delimiters(source);
    if !balanced {
        return ParseOutcome::Err { message };
    }
    // Einrückungs-Konsistenz: pro Zeile prüfen, ob eine Mischung aus Tab und
    // Space im führenden Whitespace vorkommt (PEP-8 explizit unerwünscht;
    // führt in Python 3 zu TabError).
    for (index, line) in source.lines().enumerate() {
        let leading: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        if leading.contains('\t') && leading.contains(' ') {
            return ParseOutcome::Err {
                message: format!(
                    "Zeile {}: gemischte Einrückung aus Tabs und Leerzeichen (Python: TabError)",
                    index + 1
                ),
            };
        }
    }
    ParseOutcome::Ok {
        detail: "Python: Balance und Einrückung konsistent.".to_owned(),
    }
}

/// Simple Delimiter-Balance-Prüfung: (), [], {}, "" und ''.
///
/// Erkennt Backslash-Escapes in Strings. Kein Kommentar-Handling — das
/// wäre sprachabhängig und gehört in den späteren echten Parser.
fn balanced_delimiters(source: &str) -> (bool, String) {
    let mut stack: Vec<char> = Vec::new();
    let mut in_string: Option<char> = None;
    let mut escape = false;
    let mut line = 1_usize;
    let mut column = 0_usize;
    for character in source.chars() {
        column += 1;
        if character == '\n' {
            line += 1;
            column = 0;
        }
        if let Some(delimiter) = in_string {
            if escape {
                escape = false;
                continue;
            }
            if character == '\\' {
                escape = true;
                continue;
            }
            if character == delimiter {
                in_string = None;
            }
            continue;
        }
        match character {
            '"' | '\'' => in_string = Some(character),
            '(' | '[' | '{' => stack.push(character),
            ')' => match stack.pop() {
                Some('(') => {}
                _ => {
                    return (
                        false,
                        format!("unerwartetes ')' bei Zeile {line}, Spalte {column}"),
                    )
                }
            },
            ']' => match stack.pop() {
                Some('[') => {}
                _ => {
                    return (
                        false,
                        format!("unerwartetes ']' bei Zeile {line}, Spalte {column}"),
                    )
                }
            },
            '}' => match stack.pop() {
                Some('{') => {}
                _ => {
                    return (
                        false,
                        format!("unerwartetes '}}' bei Zeile {line}, Spalte {column}"),
                    )
                }
            },
            _ => {}
        }
    }
    if in_string.is_some() {
        return (false, "unabgeschlossenes Anführungszeichen".to_owned());
    }
    if !stack.is_empty() {
        return (
            false,
            format!("unerwartetes Ende, offene Klammern: {stack:?}"),
        );
    }
    (true, String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(lang: &str, code: &str) -> ExecutionRequest {
        ExecutionRequest {
            language: lang.to_owned(),
            code: code.to_owned(),
        }
    }

    #[test]
    fn availability_matches_conceptual_stages() {
        assert_eq!(
            availability(ExecutionStage::A),
            StageAvailability::Available
        );
        assert_eq!(
            availability(ExecutionStage::B),
            StageAvailability::NotBundled
        );
        assert_eq!(availability(ExecutionStage::C), StageAvailability::Disabled);
    }

    #[test]
    fn stage_a_reports_ok_for_valid_rust() {
        let result = execute(
            ExecutionStage::A,
            &req("rust", "fn main() { let x = 1 + 2; println!(\"{x}\"); }"),
        )
        .unwrap();
        assert_eq!(result.status, ExecutionStatus::Ok);
        assert!(result.stdout.contains("Top-Level-Items"));
    }

    #[test]
    fn stage_a_reports_parse_error_for_broken_rust() {
        let result = execute(ExecutionStage::A, &req("rust", "fn main() {")).unwrap();
        assert_eq!(result.status, ExecutionStatus::ParseError);
        assert!(result.stderr.contains("Rust-Parse-Fehler"));
    }

    #[test]
    fn stage_a_reports_ok_for_valid_json() {
        let result = execute(
            ExecutionStage::A,
            &req("json", r#"{"a": 1, "b": [true, null]}"#),
        )
        .unwrap();
        assert_eq!(result.status, ExecutionStatus::Ok);
        assert!(result.stdout.contains("object"));
    }

    #[test]
    fn stage_a_reports_parse_error_for_broken_json() {
        let result = execute(ExecutionStage::A, &req("json", r#"{"a":,}"#)).unwrap();
        assert_eq!(result.status, ExecutionStatus::ParseError);
        assert!(result.stderr.contains("JSON-Parse-Fehler"));
    }

    #[test]
    fn stage_a_flags_mixed_python_indentation() {
        let source = "def f():\n\t x = 1\n";
        let result = execute(ExecutionStage::A, &req("python", source)).unwrap();
        assert_eq!(result.status, ExecutionStatus::ParseError);
        assert!(result.stderr.contains("gemischte Einrückung"));
    }

    #[test]
    fn stage_a_reports_ok_for_clean_python() {
        let source = "def f():\n    return 1\n";
        let result = execute(ExecutionStage::A, &req("python", source)).unwrap();
        assert_eq!(result.status, ExecutionStatus::Ok);
    }

    #[test]
    fn stage_a_falls_back_to_balance_for_javascript() {
        let ok = execute(ExecutionStage::A, &req("js", "const a = () => (1 + 2);")).unwrap();
        assert_eq!(ok.status, ExecutionStatus::Ok);
        let bad = execute(ExecutionStage::A, &req("js", "const a = (")).unwrap();
        assert_eq!(bad.status, ExecutionStatus::ParseError);
    }

    #[test]
    fn stage_b_returns_unavailable_with_honest_warning() {
        let result = execute(ExecutionStage::B, &req("python", "print('hi')")).unwrap();
        assert_eq!(result.status, ExecutionStatus::Unavailable);
        assert!(result.warnings.iter().any(|w| w.contains("Pyodide")));
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("wasmtime") || w.contains("Wasmtime")));
    }

    #[test]
    fn stage_b_prerequisites_list_is_stable_and_all_unsatisfied_today() {
        let list = stage_b_prerequisites();
        assert_eq!(list.len(), 3);
        assert!(list.iter().all(|p| !p.satisfied));
    }

    #[test]
    fn stage_c_is_hard_denied() {
        let error = execute(ExecutionStage::C, &req("python", "print('hi')")).unwrap_err();
        match error {
            CodeError::Execution(message) => assert!(message.contains("Stufe C")),
            other => panic!("unerwarteter Fehler: {other:?}"),
        }
    }
}
