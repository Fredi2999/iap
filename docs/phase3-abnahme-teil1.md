# Phase 3 (Teil 1) – Eskalationsleiter, Rubrik-Engine, Code-Backend

Stand: 18. September 2026. Diese Abnahme dokumentiert die drei
Backend-Meilensteine, die in dieser Session gebaut wurden. Die
UI-Erweiterung (Agenten-Laufbaum, CodeMirror-6-Editor, Projektbaum
mit Diff-Ansicht/Hunk-Annahme, Git-Status über gix, WASM-Runtime für
Stufe B) folgt in einer separaten Session — der Auftrag ist zu groß
für ein einziges Fenster.

## Meilenstein 1 — pa-agents (Konzept 7)

Neuer Crate [crates/pa-agents](crates/pa-agents) mit:

- **Rollen als Prompt-Blöcke** hinter der Cache-Grenze: Proposer,
  SelfCheck, Critic, Verifier, Synthesizer ([crates/pa-agents/src/roles.rs](crates/pa-agents/src/roles.rs)). Deterministisches
  Prompt-Rendering, damit der KV-Cache stabil bleibt (Konzept 5.3).
- **Vier Stufen L0-L3** mit Budgets nach Konzept 7.1 ([crates/pa-agents/src/runner.rs](crates/pa-agents/src/runner.rs)): L0=300 Tokens
  ~50 s, L1=500 Tokens ~90 s, L2=1 100 Tokens ~3 min, L3=1 700 Tokens ~5-7 min.
- **Router mit reiner Heuristik** ([crates/pa-agents/src/router.rs](crates/pa-agents/src/router.rs)): Schlüsselwörter (bewerte,
  plane, prüfe, berechne, …), Text-Länge, Tier. Kein Modellaufruf →
  0 Tokens. Auf **T0 nie automatisch L3** (Konzept-Regel). Zeitschätzung
  aus zuletzt gemessener tok/s vor jedem Klick verfügbar.
- **Frühabbruch** deterministisch nach Critic ([crates/pa-agents/src/severity.rs](crates/pa-agents/src/severity.rs) + [runner.rs](crates/pa-agents/src/runner.rs)): mindestens
  ein Befund mit `severity ≥ medium` — sonst wird Proposer-Text direkt
  zurückgegeben und Verifier + Synth übersprungen.
- **Zwischenstand-Persistenz** ([crates/pa-agents/src/persist.rs](crates/pa-agents/src/persist.rs)): `AgentRunStore`-Trait mit
  `upsert`/`get`/`list`. In-Memory-Implementierung im Prototyp; die
  Vault-Anbindung folgt analog zu `VaultAuditSink`. Nach jeder Rolle
  wird `upsert` aufgerufen — ein Absturz zwischen zwei Rollen führt zu
  einem sauber fortsetzbaren Zustand.
- **GBNF-Grammatiken** für Critic (`{"findings":[…]}`) und Verifier
  (`{"claims":[…]}`) in [crates/pa-agents/src/grammar.rs](crates/pa-agents/src/grammar.rs). Beide erlauben ein leeres
  Array; das ist der Frühabbruch-Anker bzw. das „nichts prüfbar"-Ergebnis.
- **Abbruch mit bestem Zwischenergebnis** (Konzept 7.3): `RunCallbacks::should_continue` wird vor jeder
  Rolle geprüft, bei `false` wird `run.aborted=true` gesetzt und der
  zuletzt sinnvolle Text zurückgeliefert.

**Test-Ergebnis: 31 Unit-Tests grün** ([crates/pa-agents](crates/pa-agents)).

## Meilenstein 2 — Rubrik-Engine (Konzept 4.2)

Neues Modul [crates/pa-tools/src/rubric.rs](crates/pa-tools/src/rubric.rs) mit den drei Konzeptstufen:

- **Stufe 1 (deterministisch): Fragebogen** — `QuestionnaireResponses`
  mit fünf Pflichtfeldern (Problem, Zielgruppe, Lösung, Erlösmodell,
  Kostentreiber) plus fünf optionalen. `missing_fields()` liefert die
  Liste, die die UI aktiv nachfragen muss.
- **Stufe 2 (LLM, eingeschränkt): Rubrik** — `Rubric::business_idea_default()` mit 9 Kriterien und geprüfter
  Gewichtssumme = 1. `RubricScoreSheet::validate` erzwingt Score 1..5,
  Pflichtbegründung, Herkunftsangabe (User / Document / Assumption).
  **GBNF-Grammatik `RUBRIC_GBNF`** erzwingt exakt diese Struktur formal
  gegenüber dem Server.
- **Stufe 3 (deterministisch): Gewichtete Auswertung + Monte-Carlo** —
  `WeightedResult::from` rechnet in Rust (das Modell rechnet nichts).
  `run_monte_carlo` benutzt splitmix64 als PRNG → reproduzierbar für
  denselben Seed. 10 000 Läufe in wenigen Millisekunden; liefert
  P10/P50/P90-Perzentile, Wahrscheinlichkeit-Positive-Month,
  Break-Even- und Runway-Erwartung.

**Test-Ergebnis: 8 neue Unit-Tests grün** (pa-tools jetzt 16 unit +
9 integration + 3 prompt_injection).

## Meilenstein 3 — pa-code (Konzept 9.3 + 9.4)

Neuer Crate [crates/pa-code](crates/pa-code) mit drei Modulen:

- **Snapshots** ([crates/pa-code/src/snapshot.rs](crates/pa-code/src/snapshot.rs)): Copy-basiert, weil Hardlinks unter Windows
  (NTFS ohne COW) sich den Inode teilen und in-place-Änderungen den
  Snapshot mitkorrumpieren würden — der Test
  `restore_recovers_deleted_and_modified_files` deckt genau dieses
  Szenario auf. Bewusste Abweichung von Konzept 9.4 („wo möglich"): auf
  Zielplattformen ohne echtes Reflink ist die sichere Wahl eine Kopie.
  Ein späterer Ausbau kann per Filesystem `CopyFileExW` oder
  `ioctl_ficlone` versuchen (im Code dokumentiert).
- **Diff & Hunk-Anwendung** ([crates/pa-code/src/diff.rs](crates/pa-code/src/diff.rs)): eigener LCS-Diff ohne externe
  Abhängigkeit; `UnifiedDiff::compute` mit konfigurierbarem Kontext,
  `apply_hunks_to_string` wendet nur die vom Nutzer ausgewählten Hunks
  an und schlägt fehl, wenn sich der Kontext einer Zieldatei
  inzwischen geändert hat (kein stiller Merge). `to_unified_text`
  liefert klassisches `--- old\n+++ new\n@@ …`-Format für Export als
  `.patch`.
- **Execution-Stufen** ([crates/pa-code/src/execution.rs](crates/pa-code/src/execution.rs)):
  - **Stufe A (Analyse)** ist aktiv: reine Delimiter-Balance als
    Placeholder-Parser; echte Sprach-Parser (tree-sitter etc.) sind
    späterer Ausbau.
  - **Stufe B (WASM)** ist strukturell vorhanden, meldet aber
    `StageAvailability::NotBundled` und liefert bei Aufruf eine
    Warnung, dass Pyodide/QuickJS-WASM im aktuellen Bundle nicht
    enthalten ist. **Das war die im Auftrag vorgesehene Vorbereitung
    ohne Aktivierung.**
  - **Stufe C** ist mit `StageAvailability::Disabled` verankert und
    liefert bei Aufruf einen harten Fehler — im Einklang mit dem
    Auftrag „nur vorbereiten, nicht aktivieren".

**Test-Ergebnis: 16 Unit-Tests grün** (5 diff + 4 snapshot + 5 execution + 2 kind/status).

## Verifikation (18.09.2026)

Alles im ASCII-Junction unter `%TEMP%\portableai-usb-ascii`, mit
Strawberry Perl im PATH und `LC_ALL=C`/`LANG=C`.

| Kommando | Ergebnis |
|---|---|
| `cargo fmt --all -- --check` | Exit 0 |
| `cargo test --workspace --offline` | **243 Tests passed**, 0 failed, 1 bewusst ignoriert |
| `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` | Exit 0 |
| `cargo clippy --offline -- -D warnings` in `app/src-tauri` | Exit 0 |

Zuwachs gegenüber Ende Phase 2 (188 Tests): **+55 neue Tests**
(31 pa-agents, 8 pa-tools/rubric, 16 pa-code).

## Was in dieser Session bewusst NICHT gemacht wurde

Nicht-Schätzung — Aufgaben, die im Auftrag stehen und in einer weiteren
Session anzugehen sind:

1. **UI-Agentenlauf-Baum** (Konzept 7.3): jede Rolle einzeln
   aufklappbar, Tokenverbrauch/Dauer sichtbar, Router-Vorschlag +
   Zeitschätzung vor Klick auf Start, Abbruch-Button. Backend
   (`RunProgress`-Events) ist fertig; die Tauri-Command-Registrierung
   und die Svelte-Komponente fehlen noch.
2. **Anbindung von `EscalationRunner` an `LlamaServerEngine`.** Der
   `EngineCall`-Trait ist definiert; die konkrete Adapter-Funktion,
   die `stream_chat` sammelt und ggf. das GBNF-Grammatik-Feld an
   `pa-inference::chat` übergibt, fehlt noch (analog zum
   Werkzeugmodus-Adapter aus Phase 2).
3. **Vault-Persistenz für `AgentRun`**: bisher In-Memory. Analog zu
   `VaultAuditSink` folgt hier ein `VaultAgentRunStore` mit
   Schema-Migration V3 → V4.
4. **Rubrik-UI**: Fragebogen, Rubrik-Anzeige, Monte-Carlo-Verteilung.
5. **CodeMirror 6 im Frontend**, Projektbaum-Ansicht, Diff-Ansicht mit
   Hunk-Checkboxen, Git-Status/Commit über `gix`.
6. **Stufe A: echte Sprach-Parser** (tree-sitter oder syn/rustc-Parser),
   Analyzer-Skills.
7. **Stufe B: Pyodide/QuickJS-WASM-Runtime** in Wasmtime, mit
   WASI-Freigaben nach pa-policy.
8. **Vault-persistenter Snapshot-Katalog** — aktuell werden Snapshots
   als reine Dateisystem-Verzeichnisse geführt; die Metadaten liegen
   bei jedem Aufrufer, statt zentral im Vault.

## Abgrenzung zum Auftrag „NICHT TUN"

Der Auftrag verbietet ausdrücklich parallele Modellinstanzen. Der
Runner in [crates/pa-agents/src/runner.rs](crates/pa-agents/src/runner.rs) verwendet **eine** `EngineCall`-Referenz für
alle Rollen; es gibt keine Stelle im Code, die einen zweiten Server
oder ein zweites Modell startet. Der `HashingEmbedder` aus Phase 2 und
der Chat-Server aus Phase 1 laufen weiterhin als getrennte Instanzen
für getrennte Zwecke (Chat vs. Embedding), aber das ist konzeptkonform
und keine parallele Rollen-Instanz.
