# MVP-Abnahme Schritt 5: Tauri-Oberfläche

Stand: 16. September 2026. Der Konsolen-Chat aus Schritt 3 bleibt parallel
lauffähig. Diese Datei dokumentiert, was die neue Tauri-App leistet, wie sie
gebaut wird und welche Prüfungen noch offen sind.

## Umfang genau nach Auftrag

Zwei Bereiche, keine weiteren:

- **Chat.** Streaming Token für Token via `chat-stream`-Event, Abbruch mit
  Erhalt des Teiltextes, Statuszeile mit Phase und Tokens/s, Konversationsliste
  links, Umbenennen und Löschen.
- **Einstellungen.** Tier-Override, Modellwahl (aktuell genau ein installiertes
  Modell), Kontextlänge, Theme (System/Dark/Light), Vault-Wechsel mit
  Passphrase.

Nicht enthalten: Skills, Werkzeugaufrufe, Memory, Agent Mode, Kalender,
Workspace, Code-Bereich, Modelle & Logs-Reiter, Linux-/macOS-Builds.

## Verzeichnisstruktur

```
app/
├── package.json              # Svelte 5, Vite 8, Tailwind 4, Tauri v2
├── tsconfig.json
├── svelte.config.js
├── vite.config.ts
├── index.html
├── src/
│   ├── main.ts               # mountet App.svelte
│   ├── style.css             # Tailwind + PortableAI-Theme
│   ├── App.svelte            # Bootstrap-Status → Unlock → Chat/Settings
│   ├── lib/
│   │   ├── types.ts          # TypeScript-Spiegel von pa-types::ipc
│   │   └── ipc.ts            # typisierte invoke-/emit-Wrapper
│   └── routes/
│       ├── Unlock.svelte     # Passphraseneingabe, Vault anlegen/entsperren
│       ├── Chat.svelte       # Konversationen, Streaming, Statuszeile
│       └── Settings.svelte   # Tier, Modell, Kontext, Theme, Vault-Wechsel
└── src-tauri/
    ├── Cargo.toml            # eigenes [workspace]; Tauri 2.11.5
    ├── build.rs
    ├── tauri.conf.json
    ├── capabilities/default.json
    ├── icons/icon.ico
    └── src/
        ├── main.rs           # ruft lib::run()
        ├── lib.rs            # Tauri-Builder, alle Commands, Streaming
        └── session.rs        # StoredBootstrap + Session-Struktur
```

## IPC-Verträge (pa-types::ipc)

Frontend und Rust-Backend sehen dieselben, mit Round-Trip-Tests abgesicherten
Typen:

- `BootstrapStatus`, `ManifestSummary`, `AvailableModel`
- `ConversationDetail`
- `SendMessageRequest`
- `StreamEvent` (`started`, `delta`, `finished`, `failed`)
- `SettingsSnapshot`, `SettingsUpdate`, `ThemePreference`, `TierOverrideChange`
- `VaultSwitchRequest`

Tests: `crates/pa-types/tests/ipc_contract.rs` (Serde-Round-Trip pro Variante).

## Tauri-Commands

| Command | Zweck |
|---|---|
| `bootstrap_status` | Manifest, Hardware, Ressourcenplan, Standardmodell |
| `unlock_vault` | Passphrase → SQLCipher entsperren + Inferenzserver starten |
| `list_conversations` | Liste inkl. Titel und letzter Änderung |
| `open_conversation` | Konversation samt Nachrichten laden |
| `create_conversation`, `rename_conversation`, `delete_conversation` | Verwaltung |
| `installed_models` | tatsächlich installierte Modelle (im MVP eines) |
| `settings_snapshot`, `apply_settings` | Tier, Modell, Kontext, Theme |
| `switch_vault` | anderer Vault, mit Rücksync des alten Standes |
| `send_message` | Streaming-Turn im Hintergrund-Thread; Deltas via Event |
| `cancel_stream` | Abbruchsignal für den laufenden Turn |

Streaming: Der Rust-Handler startet in einem eigenen Thread einen
`ChatOrchestrator::run_turn`-Lauf und emittiert `chat-stream` mit
`StreamEvent`-Payloads. Der UI-Thread bleibt jederzeit reaktionsfähig.

## Wiederverwendbare Startpfade

Um doppelten Code zu vermeiden, ist der Startpfad aus Schritt 3 jetzt in
`crates/pa-launcher/src/bootstrap.rs` (`prepare`) und
`crates/pa-launcher/src/runtime.rs` (`VaultRuntime`, `SharedConversations`)
gebündelt. CLI-Launcher und Tauri-App nutzen dieselben Funktionen; die
dokumentierten Schritt-3-Messwerte bleiben deshalb reproduzierbar mit
`pa-launcher.exe --cli --root .`.

## Bauen (Windows)

Voraussetzungen: Node 20+, npm 10+, Rust-Toolchain (1.85+), Strawberry Perl
für die vendored OpenSSL-Bibliothek, ASCII-Junction-Pfad wegen des `ü` im
realen Projektpfad.

```powershell
$junction = Join-Path $env:TEMP 'portableai-usb-ascii'
Set-Location $junction
$env:CARGO_TARGET_DIR = Join-Path $pwd.Path 'target-ascii-app'
$env:PATH = (Join-Path $pwd.Path 'tools\build\strawberry\perl\bin') + ';' + $env:PATH
$env:LC_ALL = 'C'
$env:LANG = 'C'
Remove-Item Env:PERL5LIB -ErrorAction SilentlyContinue

# Frontend-Abhängigkeiten (einmalig, benötigt Netzwerk):
npm --prefix app install

# Rust-Check (offline möglich, sobald der Cache Tauri-Deps enthält):
Set-Location (Join-Path $junction 'app\src-tauri')
& "$env:USERPROFILE\.cargo\bin\cargo.exe" check --offline

# Entwicklungsstart (öffnet WebView2):
Set-Location (Join-Path $junction 'app')
npx tauri dev
```

Für Schritt 6 folgt ein reproduzierbarer Release-Build in
`packaging/windows/`, der aus dem Stick-Verzeichnis startet.

## Weiterhin offen (Nutzer prüft manuell)

- Realer 8-GB-Windows-PC ohne Adminrechte
- Mehrere unterschiedliche Windows-Rechner (Portabilität)
- WebView2-Verfügbarkeit auf einem PC ohne WebView2
- SmartScreen-Verhalten
- Physisches Ziehen des USB-Sticks während der GUI-Sitzung
- Externer Netzwerkmonitor
- End-to-End-Live-Chat gegen den echten `llama-server` durch die GUI (der
  Ablauf wird beim ersten Doppelklick auf `portable-ai.exe` automatisch
  ausgeführt; Ergebnisse gehören in Schritt 6)
