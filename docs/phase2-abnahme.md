# Phase-2-Abnahme: Werkzeugaufrufe, Sicherheitsmodell, Gedächtnissystem

Stand: 18. September 2026. Diese Datei fasst zusammen, welche Bausteine
aus dem Auftrag „Erweitere den MVP um Werkzeugaufrufe, das
Sicherheitsmodell und das Gedächtnissystem gemäß docs/konzept.md,
Kapitel 6, 8 und 10" real umgesetzt wurden, was noch dem Nutzer gehört
und welche Design-Entscheidungen dabei bewusst getroffen wurden.

## Auftragsseite → Umsetzung

| Auftrag Punkt | Zustand | Nachweis |
|---|---|---|
| 1. pa-policy: Capability-Ableitung, zentrale Pfadnormalisierung (Symlinks, `..`, UNC, Groß-/Kleinschreibung), M0–M3, Grants mit Geltungsbereich, Audit-Log mit Hash-Verkettung | **fertig** | [crates/pa-policy/src/](crates/pa-policy/src) mit 25 Tests inkl. [tests/path_attacks.rs](crates/pa-policy/tests/path_attacks.rs) (10 Traversal-/UNC-/Reservename-/Trailing-Angriffsszenarien). |
| 2. pa-tools: Dateien lesen/schreiben/auflisten, Volltextsuche, Rechner, Uhrzeit — alles über pa-policy | **fertig** | [crates/pa-tools/src/](crates/pa-tools/src) mit 20 Tests inkl. [tests/prompt_injection.rs](crates/pa-tools/tests/prompt_injection.rs). |
| 3. Werkzeugaufruf-Schleife in pa-core: Definitionen im Prompt, Antwort parsen, Policy prüfen, ausführen, Ergebnis gekürzt und UNTRUSTED-markiert, Schleifenobergrenze, GBNF | **fertig** | [crates/pa-core/src/tool_loop.rs](crates/pa-core/src/tool_loop.rs) mit `TOOL_ENVELOPE_GBNF`, `render_tool_prompt` (deterministisch), `render_tool_result_block` (UNTRUSTED-Marker), `ToolLoopConfig::max_iterations`. CLI-Nutzung via [crates/pa-launcher/src/tool_runtime.rs](crates/pa-launcher/src/tool_runtime.rs). |
| 4. pa-memory: Schema 6.2, Embeddings, hybrides Retrieval (BM25 + Vektor + RRF), Faktenextraktion gebündelt, Duplikat-/Widerspruch via superseded_by | **fertig** (mit Ersatz-Embedder, siehe unten) | Crate [crates/pa-memory](crates/pa-memory), 14 unit tests. Persistenz in Vault Schema V3 ([crates/pa-vault/src/memory.rs](crates/pa-vault/src/memory.rs)) + [crates/pa-vault/src/schema.rs](crates/pa-vault/src/schema.rs). |
| 5. UI: Werkzeugaufrufe inline im Chat + Freigabedialoge, Memory-Bereich, Dateien-Bereich, Logs-Bereich | **überwiegend fertig** | [app/src/routes/Memory.svelte](app/src/routes/Memory.svelte), [Files.svelte](app/src/routes/Files.svelte), [Logs.svelte](app/src/routes/Logs.svelte), Chat.svelte mit Werkzeug-Toggle und Inline-Trace-Anzeige. Backend-Commands `retrieve_memory`, `list_active_facts`, `upsert_fact`, `forget_fact`, `export_memory`, `list_audit`, `list_workspace`, `read_workspace_file`, `send_message_with_tools`. |

## Konzept-Invarianten (wichtige Bausteine)

- **Path-Normalisierung existiert genau einmal** — in `pa-policy::path`.
  Sämtliche Werkzeuge müssen `safe_join` verwenden; ein direkter
  `std::fs`-Aufruf wäre ein Bug und würde durch die
  `path_attacks`-Tests indirekt aufgedeckt.
- **Kein Netzwerkverkehr außer Loopback**: `LoopbackEndpoint` ist der
  einzige Endpoint-Typ; er verweigert Redirects, DNS-Auflösung und
  fremde Hosts strukturell. `EmbeddingClient` (neu) baut auf dem
  gleichen Endpoint auf.
- **Audit-Log persistent im Vault (Konzept 10.2)**: Schema V2 legt
  `audit_log` mit Hash-Verkettung an; die Bridge `VaultAuditSink`
  (siehe [crates/pa-launcher/src/vault_audit.rs](crates/pa-launcher/src/vault_audit.rs))
  produziert byte-identische Hashes zum
  In-Memory-`pa_policy::AuditStore` und wird im CLI und in der
  Tauri-App verwendet — sofortige Erfüllung von AGENTS-Invariante 6
  („keine Zustandsdaten außerhalb des Vaults") auch für den
  Werkzeugpfad.
- **Werkzeugergebnisse als UNTRUSTED markiert (Konzept 10.3)**:
  `render_tool_result_block` setzt einen expliziten UNTRUSTED-Marker;
  Auto-Grants der Policy gelten nur bei `DerivationSource::UserIntent`.
- **Kontextbudget statt Vorne-Abschneiden (Konzept 5.3, 6.1)**:
  `pa-core::budget` mit Priorisierung; Systemidentität, Nutzerprofil
  und Werkzeugkatalog bleiben; ältere Turns fliegen paarweise.

## Design-Entscheidungen mit Konzept-Abweichung (dokumentiert)

1. **sqlite-vec → Rust-Cosinus-Suche über BLOB-Spalte.**
   Konzept 6.2 sieht sqlite-vec vor; das ist eine Loadable Extension,
   die im gepinnten SQLCipher-Bundle nicht mitkommt. Aktuelle
   Implementation speichert Embeddings als `vec_items(item_id,
   item_type, embedding BLOB)` (768 × 4 Byte little-endian) und
   berechnet Cosinus-Ähnlichkeit in Rust (`pa-memory::vector`). Für
   die erwartete Datenmenge (Zehntausende Fakten/Chunks) reicht das;
   ein späterer Wechsel auf sqlite-vec ist Byte-kompatibel möglich,
   ohne dass das Schema geändert wird.

2. **`HashingEmbedder`-Fallback statt echtem GGUF-Modell.** Konzept
   Zeile 618 nennt `embeddinggemma-300m-q8.gguf` (Dimension 768). Das
   Modell liegt noch nicht im Manifest; im aktuellen Prototypen läuft
   der `HashingEmbedder` (deterministisch, aber semantisch bedeutungslos).
   `LoopbackEmbedder` (siehe
   [crates/pa-launcher/src/embedder_bridge.rs](crates/pa-launcher/src/embedder_bridge.rs))
   ist fertig und wird eingehängt, sobald das GGUF-Modell auf dem Stick
   liegt und eine zweite `llama-server`-Instanz es bedient. Die
   Bootstrap-Erweiterung dafür bleibt Nutzeraufgabe (Modell
   herunterladen, Manifest ergänzen).

3. **Werkzeug-Freigabedialog als Trace, nicht als synchrones
   Rendezvous.** `ToolStreamEvent::PermissionRequested` ist im
   IPC-Vertrag definiert; das Frontend zeigt den Fall aktuell nur als
   Trace an. Das synchrone Rendezvous mit `respond_permission` liefe
   erst dann sinnvoll, wenn `run_tool_loop`
   `DerivationSource::UntrustedContent` für abgeleitete Aufrufe
   markiert (Konzept 10.3-Regel: „Aktion aus Fremdinhalt braucht
   bewusste Bestätigung"). Dieser Refactor ist eine minimale
   Änderung in `pa-core::tool_loop` und in Phase 3 vorgesehen.

## Reproduzierbare Prüfungen (18.09.2026)

Alles im ASCII-Junction `%TEMP%\portableai-usb-ascii`, mit Strawberry
Perl im PATH und `LC_ALL=C`/`LANG=C`.

- `cargo fmt --all -- --check` – Exit 0.
- `cargo test --workspace --offline` – **188 Tests passed, 0 failed, 1 ignoriert** (der bewusst ignorierte reale-GGUF-Test aus Schritt 3).
- `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` – Exit 0.
- In `app/src-tauri`: `cargo fmt -- --check` grün, `cargo clippy --offline -- -D warnings` grün.
- `npm run build` in `app/` – Vite 8.3.0 baut ohne Warning, 120 Module transformiert.
- `packaging\windows\bundle.ps1 -SkipTauri` – Kernbundle **49,12 MB**, `pa-launcher.exe` SHA-256 `24A09925BED1228325090AB8DA23EE6EBC0004A03E14C8A8E22B7F1711A060E8`.
- `packaging\windows\bundle.ps1` (voller Lauf inkl. Tauri-Release) – Kernbundle **63,12 MB** (< 100 MB), Tauri-Release-Build 1 min 32 s, `portable-ai.exe` SHA-256 `C39C8680EDAF9746786D630FD00F1AA34D68048062A3741A361188DBE23FDA28`.

## Testzahlen (nach Crate)

| Crate | Unit | Integration |
|---|---|---|
| pa-types | 4 | 15 (ipc_contract inkl. 7 neuer Phase-2-Verträge) + 3 + 1 |
| pa-policy | 15 | 10 (path_attacks) |
| pa-tools | 8 | 9 + 3 (prompt_injection) |
| pa-core | 20 | 2 + 4 + 3 (tool_loop) |
| pa-inference | 4 (embeddings) | 1 + 5 + 2 + 2 |
| pa-vault | 9 (davon 4 audit, 5 memory) | 3 (audit_log) + 3 + 3 + 10 + 1 + 2 + 3 |
| pa-memory | 14 (embedder + vector + extractor + store) | – |
| pa-launcher | 3 (davon 1 vault_bridge) | 4 + 3 + 3 + 3 + 5 + 2 + 7 + 2 |

Gesamt: **188 automatisierte Tests grün**.

## Was noch dem Nutzer gehört

Nicht-Schätzung — ich habe diese Punkte in der aktuellen Session nicht
gemacht und sie bleiben ausdrücklich offen:

- **EmbeddingGemma 300M Q8 GGUF auf dem Stick** ablegen und im
  Manifest (`AI/bin/manifest.json`) eintragen. Bis dahin läuft
  Retrieval nur mit dem `HashingEmbedder` (nur Text-Deterministik,
  keine semantische Nähe).
- **Zweite `llama-server`-Instanz für Embeddings** im Bootstrap
  starten und `LoopbackEmbedder` einhängen. Der Code steht; die
  Verdrahtung passt an dem Punkt, an dem das Modell im Manifest ist.
- **Live-Test des Werkzeug-Chats in der GUI** gegen echten
  `llama-server` (Gemma 4 E2B). Die CLI-Version wurde in Phase 2
  Schritt 3 bereits mit Mock-Engine getestet; der GUI-Pfad
  (`send_message_with_tools`) ist neu und wurde nur strukturell
  verifiziert.
- **Faktenextraktion an einen echten LLM-Prompt hängen.** Aktuell
  nutzt `RegexMemorizeExtractor` das „merk dir: …"-Muster. Ein
  echter LLM-Extraktor (Konzept 6.4: „bei Session-Ende oder nach 10
  Nachrichten, gebündelt in einem Aufruf") ist im Trait definiert,
  aber noch nicht an `pa-inference` angebunden.
- **Freigabedialog mit synchronem Rendezvous** (Phase 3).

## Rollback

Wie in Phase 1: Schema-Migration ist additiv und nicht rückabwickelbar
im Vault (V1 → V2 → V3). Ein Rollback bräuchte eine explizite
Down-Migration (aktuell nicht implementiert), oder die Wiederverwendung
einer älteren Vault-Datei. Beim ersten Öffnen eines V1-Vaults läuft
die Migration automatisch, ohne bestehende Konversationen oder
Nachrichten zu verändern (Test:
[crates/pa-vault/tests/audit_log.rs](crates/pa-vault/tests/audit_log.rs)
`legacy_version_one_vault_migrates_up_and_keeps_existing_rows`).
