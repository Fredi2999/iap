# Phase 2 – Schritt 3: Werkzeug-Chat im CLI

Stand: 17. September 2026. Diese Datei dokumentiert die minimale Integration
der Werkzeugschleife aus `pa-core::tool_loop` in den Konsolen-Chat aus
Schritt 3 des MVP. Sie ist bewusst der einzige heute umgesetzte Teil von
Phase 2 und stoppt exakt an der im Auftrag geforderten Marke.

## Was neu ist

- `crates/pa-launcher/src/tool_runtime.rs` (neu): kapselt einen
  `PathScope`, den aktuellen `Mode`, den `GrantStore`, den `AuditStore`
  und die `ToolRegistry::with_defaults()`. Konstruktoren:
  `ToolRuntime::in_memory` (CLI-Standard, konform zur AGENTS-Invariante 6:
  keine Zustandsdaten außerhalb des Vaults) und `ToolRuntime::open` mit
  `AuditStorage::File` (nur für Tests, bis die Vault-Integration steht).
- `crates/pa-core/src/tool_loop.rs`: neue Variante
  `ToolLoopError::Engine(String)` für Sammel-Prompt-Fehler; existierende
  drei Tests unverändert grün.
- `crates/pa-launcher/src/cli.rs`: `--tools` aktiviert die Werkzeugschleife
  ab Start; `--workspace <pfad>` überschreibt den Standardpfad
  `AI/data/workspace`. Neuer Test `tools_and_workspace_are_accepted`
  sichert den Vertrag; bereits vorhandener Test „Passphrase ist kein
  Argument" bleibt unverändert.
- `crates/pa-launcher/src/main.rs`: neuer Turn-Zweig `run_tool_turn`, der
  bei aktivem Toggle eine synchrone Sammel-Prompt-Runde durch die
  Werkzeugschleife führt, `ToolEvent::*`-Ausgaben an das Terminal
  weitergibt und Nutzernachricht + finale Antwort in der Konversation
  persistiert (Streaming der Zwischenrunden ist bewusst *nicht*
  aktiviert; die Toolschleife braucht die vollständige Envelope, um
  gültiges JSON zu parsen). Slash-Kommandos `/werkzeuge on|off` und
  `/modus m0|m1|m2|m3` schalten Werkzeuge und Berechtigungsmodus zur
  Laufzeit um.

## Wie man den Werkzeug-Chat startet

Im ASCII-Junction (`%TEMP%\portableai-usb-ascii`) mit dem üblichen
Strawberry-Perl-Setup:

```powershell
.\target-ascii\release\pa-launcher.exe --cli --tools --root . --context 2048
```

- Ohne `--tools` bleibt der reine Streaming-Chat aus Schritt 3
  unverändert erhalten.
- `--workspace <pfad>` legt einen eigenen Werkzeug-Workspace fest; sonst
  wird `AI\data\workspace` unter `--root` benutzt.
- Slash-Kommandos:
  - `/werkzeuge on|off` – Werkzeugschleife an/aus.
  - `/modus m1` – zurück zum Standard-Modus (Workspace-Schreibrechte mit
    Diff-Bestätigung); höhere Modi verlangen laut Konzept 10.1 im UI
    eine Reauth, im CLI-Prototyp wird der Wechsel nur gemeldet.
  - `/model gemma-4-e2b-q4-k-m`, `/reload`, `/hilfe`, `/quit` – wie
    bisher.

## Ablauf pro Nutzer-Turn (Werkzeuge aktiv)

1. Nutzernachricht wird sofort persistiert; Assistenten-Platzhalter im
   Status `Streaming` (überlebt harten Abbruch als Aborted).
2. `ToolRuntime::run_turn` startet `run_tool_loop` mit dem
   Werkzeugkatalog + Envelope-Anweisung als Systemblock.
3. `emit_prompt` ruft `LlamaServerEngine::stream_chat` auf, sammelt alle
   Deltas zu einem einzigen String und liefert ihn an die Schleife.
   Abbruch via Ctrl+C setzt `should_continue = false` und wird als
   `ToolLoopError::Engine` an den Aufrufer gemeldet.
4. `ToolEvent`s werden zeilenweise auf der Konsole angezeigt:
   `[Modelltext]`, `[Werkzeug ⇒ name]`, `[Ergebnis name (UNTRUSTED)]`,
   `[Werkzeugfehler …]`.
5. Sobald das Modell mit `{"action":"answer",…}` antwortet, wird der
   Text als finale Antwort persistiert (`MessageStatus::Complete`).
   Fehlerfälle persistieren `[abgebrochen] …` als `Aborted`.

## Was ausdrücklich NICHT in dieser Session gemacht wurde

- Kein `pa-memory` – Crate existiert bewusst noch nicht.
- Keine UI-Erweiterungen (Werkzeug-Inline, Freigabedialog, Memory-,
  Dateien-, Logs-Bereich). Der Auftrag verlangt zuerst „einen
  lauffähigen Kommandozeilen-Chat".
- Kein persistenter Audit-Log im laufenden Betrieb – die AGENTS-
  Invariante 6 („keine Zustandsdaten außerhalb des Vaults") verbietet
  das, bis die Vault-Integration in Phase 2 Schritt 4/5 steht.
- Keine Neuimplementierung der bereits vorhandenen 1180 Zeilen
  `pa-tools` und 1085 Zeilen `pa-policy`. Der bestehende, unabhängig
  reviewte Code (25 pa-policy-Tests inkl. `path_attacks.rs`, 20
  pa-tools-Tests inkl. `prompt_injection.rs`, 3 tool_loop-Tests) bleibt
  unverändert.

## Reproduzierbare Tests

`%TEMP%\portableai-usb-ascii` mit Strawberry-Perl im PATH,
`CARGO_TARGET_DIR=…\target-ascii`, `LC_ALL=C`, `LANG=C`, `PERL5LIB` leer:

- `cargo fmt --all -- --check` – Exit 0.
- `cargo test --workspace --offline` – 149 Tests grün, 1 ignoriert
  (der bewusst ignorierte reale-GGUF-Test aus Schritt 3).
- `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` – Exit 0.
- Zusätzlich für `app/src-tauri` mit `CARGO_TARGET_DIR=…\target-ascii-app`:
  `cargo clippy --offline -- -D warnings` – Exit 0.

## Weiterhin offene manuelle Prüfung

Der Werkzeug-Chat wurde in dieser Session **nicht** live gegen den
echten `llama-server` mit einem GGUF-Modell durchgespielt. Der
Integrationstest `read_file_call_flows_through_policy_and_audit` deckt
den Toolschleifen-Pfad ohne echten Server ab; die Ende-zu-Ende-Runde
gegen Gemma 4 E2B über Loopback ist Nutzeraufgabe und gehört, sobald
sie durchgeführt ist, unter „Reproduzierbare Tests" ergänzt.

## Fortsetzung

- **Schritt 4 (pa-memory)**: Crate anlegen, Schema aus Konzept 6.2,
  Embeddings über `pa-inference`, hybrides Retrieval (BM25 + Vektor mit
  Reciprocal Rank Fusion), Faktenextraktion mit `superseded_by`. Braucht
  ein Embedding-Modell in `AI/models/` und eine Design-Entscheidung, ob
  BM25 über SQLite-FTS5 (bereits im SQLCipher-Bundle) läuft.
- **Schritt 5 (UI)**: Chat-Inline-Werkzeuganzeige, Freigabedialog,
  Memory-Bereich mit Nutzerkontrolle, Dateien-Bereich, Logs-Bereich mit
  Filter über den `AuditStore` (der bis dahin in den Vault gehört).
- **Tauri-Integration** des Werkzeug-Pfads (analog zur CLI, aber mit
  UI-Freigabedialog statt Auto-Ablehnung bei
  `DerivationSource::UntrustedContent`).
