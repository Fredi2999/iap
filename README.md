# IAP: Intellectus Artificialis Portabilis

**IAP** (*Intellectus Artificialis Portabilis*, lateinisch für „tragbarer künstlicher Verstand“; Arbeitsname, ein endgültiger Produktname steht noch aus) ist ein persönlicher KI-Assistent, der von einem USB-Stick startet und ein
Sprachmodell **vollständig lokal** auf dem Rechner ausführt, an dem der Stick steckt. Es gibt keine Cloud und keine
Telemetrie. Alles, was du eingibst, wird in einem verschlüsselten Tresor auf dem Stick gespeichert.

> **Stand:** Entwicklungsstand, kein fertiges Produkt. Gebaut und getestet wird unter **Windows 10/11**. Linux und macOS
> sind geplant (Phase 4), laufen aber noch nicht. Was geprüft ist und was nicht, steht unter
> [Stand und Grenzen](#stand-und-grenzen).

## Inhalt

- [Was IAP kann](#was-iap-kann)
- [Sicherheit und Datenschutz](#sicherheit-und-datenschutz)
- [Voraussetzungen](#voraussetzungen)
- [Modelle und Laufzeit herunterladen](#modelle-und-laufzeit-herunterladen) (die liegen **nicht** in diesem Repository)
- [Zusatzpakete (Sprache, Bildschirm, Git)](#zusatzpakete-sprache-bildschirm-git)
- [Bauen und auf den Stick bringen](#bauen-und-auf-den-stick-bringen)
- [Prüfen](#prüfen)
- [Aufbau des Repositorys](#aufbau-des-repositorys)
- [Stand und Grenzen](#stand-und-grenzen)
- [Lizenzen](#lizenzen)

## Was IAP kann

| Bereich | Inhalt |
|---|---|
| **Chat** | Unterhaltungen mit lokalem Modell, Denkstufen, Werkzeuge, eigene Dokumente (TXT, MD, DOCX, PDF) mit Quellenangabe, Projekte, Vorlagen, „/“-Befehle und Skills |
| **Gedächtnis** | Fakten, Gedächtnis als Graph, Dokumentbibliothek, PCI (siehe unten) |
| **Kalender** | Termine, Aufgaben, Planer, ICS-Import; **optional** Apple-Kalender (iCloud: lesen und Termine anlegen) und Google-Kalender (lesen) verbinden, Fremdtermine auch auf der Startseite |
| **Code** | Dateibaum, Editor (CodeMirror), Suche, Git, „IAP fragen“; wahlweise im Arbeitsordner auf dem Stick oder in einem **freigegebenen Ordner auf dem PC**; **Code-Agent**, der den Ordner liest und Änderungen *vorschlägt*; Befehle (Tests, Build) nur mit Einzelbestätigung |
| **Flow Version** | Agent Flow (Kandidaten in Git-Worktrees, Diff, Übernahme nach Bestätigung) und ein Workflow-Builder |
| **Sprache und Bild** | Spracheingabe und Vorlesen, „Bildschirm ansehen“ (optionale Pakete) |
| **Pet und Tray** | kleines Begleitfenster, läuft nach dem Schließen weiter |
| **Konnektoren** (optional) | Exa, Wikipedia, Open-Meteo, Brave, Gmail (Auto-Antwort an eine Adresse, Postfach lesen), jeweils nur nach ausdrücklicher Freigabe und nie bei „Air Gap“. Der Kalender fragt nur auf Klick („Aktualisieren“, „Termin speichern“) und nie im Hintergrund |
| **PCI-Begleiter** (optional) | sichtbares Zusatzprogramm, das auf einem PC aufzeichnet, welche App im Vordergrund war |
| **Einstellungen** | Darstellung, Modell, Profil, Leistungs-Check, **Tresor-Passwort ändern** |

Die Oberfläche gibt es auf Deutsch, Englisch, Spanisch, Französisch und Japanisch.

## Sicherheit und Datenschutz

- **Offline.** Die einzige Netzwerkverbindung ist `127.0.0.1` zum eigenen Inferenzprozess. Ausnahmen sind die
  optionalen Konnektoren mit fester Host-Liste, jeweils nach Freigabe und bei „Air Gap“ gesperrt.
- **Kalender nur auf Klick.** Verbindest du einen Apple- oder Google-Kalender, verbindet sich IAP nur, wenn du
  aktualisierst oder einen Termin dorthin speicherst, und nie bei eingeschaltetem „Air Gap“. Passwort und geheime Adresse
  liegen verschlüsselt im Tresor. Beim Abruf gehen deine Zugangsdaten an Apple bzw. Google, beim Anlegen Titel, Zeit, Ort
  und Notiz des Termins; sonst verlässt nichts den Rechner. Für Apple brauchst du ein **app-spezifisches Passwort**
  (appleid.apple.com), für Google die **geheime Adresse im iCal-Format** (nur Lesen).
- **Verschlüsselter Tresor.** SQLCipher mit Schlüssel aus dem Passwort per Argon2id. Es gibt **keine Möglichkeit, ein
  vergessenes Passwort zurückzusetzen.** Mehrere Tresore (z. B. „Arbeit“ und „Privat“) auf einem Stick sind möglich;
  ein Tresor lässt sich im Startbildschirm nach Passwort und Namenseingabe löschen.
- **Standardversion.** Das Repository enthält nur den Quellcode und die Beschreibungen der Pflicht- und Wahlmodelle.
  Es enthält **keinen Tresor, keine gespeicherten Einstellungen, keine Zugangsdaten und keine persönliche Konfiguration**
  (auch nicht im Verlauf). Alles Persönliche entsteht erst beim ersten Start auf deinem Stick: das Master-Passwort
  legst du selbst fest, Darstellung, Profil und Konnektoren stellst du selbst ein. Mit
  `node app/scripts/check-clean-repo.mjs` lässt sich das jederzeit nachprüfen.
- **Jeder Zugriff geht durch eine Policy-Schicht** (`pa-policy`): Pfadprüfung, Modi, Audit-Protokoll im Tresor.
- **Das Modell schreibt nie selbst.** Änderungen erscheinen als Diff und werden erst nach deiner Bestätigung gespeichert.
- **Befehle laufen ohne Sandbox.** Ein vom Agenten vorgeschlagener Befehl wird dir mit Programm, Argumenten und
  Ordner gezeigt und läuft erst nach deiner Bestätigung. Das gestartete Programm kann trotzdem das Netz nutzen und
  außerhalb des Projekts schreiben. Der Dialog sagt das.
- **PCI-Begleiter nur auf eigenen Geräten.** Er zeichnet App-Name, Fenstertitel und Dauer auf (keine Inhalte, keine
  Tastatureingaben, kein Bildschirm, kein Netz), ist sichtbar und mit einem Klick entfernbar. Auf Geräten anderer
  Personen, des Arbeitgebers oder der Schule brauchst du deren Zustimmung.

Das verbindliche Konzept steht in [`portabler-ki-agent-konzept.md`](portabler-ki-agent-konzept.md), die Regeln für
die Entwicklung in [`AGENTS.md`](AGENTS.md).

## Voraussetzungen

- **Windows 10 oder 11** (64 Bit) mit WebView2 (ist bei Windows 11 dabei).
- **Mindestens 8 GB RAM**, es genügt die CPU, eine Grafikkarte wird nicht gebraucht. Mit dem Standardmodell belegt
  IAP rund 3 GB Arbeitsspeicher. Das größere Coder-Modell braucht rund 5,6 GB und ist deutlich langsamer.
- **USB-Stick** (oder ein anderer Datenträger) mit mindestens **8 GB** frei für Programm und Pflichtdateien
  (rund 6 GB), **rund 17 GB** mit allen optionalen Modellen und Paketen. Schnelle Sticks lohnen sich: Beim ersten Wählen eines
  Modells kopiert IAP es in einen Cache auf dem PC (`%LOCALAPPDATA%\IAP\model-cache`).

## Modelle und Laufzeit herunterladen

**Modelle und die `llama.cpp`-Laufzeit gehören nicht ins Git-Repository** (zu groß, eigene Lizenzen). Du lädst sie
selbst herunter und legst sie auf den Stick. Das Manifest [`AI/bin/manifest.json`](AI/bin/manifest.json) hält Größe
und SHA-256 jeder Datei fest. **Beim Start prüft IAP jede dort aufgeführte Datei**; fehlt eine oder stimmt ihr
Hash nicht, startet IAP nicht und nennt die Datei. Lade deshalb genau die unten genannten Dateien.

Alle Quellen unten sind öffentlich und ohne Anmeldung erreichbar. Dateinamen und SHA-256 habe ich am 02.10.2026
gegen die Angaben von Hugging Face abgeglichen. Die **Lizenz** nennt jeweils die Modellkarte; lies sie, bevor du ein Modell
einsetzt oder weitergibst.

### Empfehlung auf einen Blick

| Wofür | Modell | Arbeitsspeicher (Spitze, 8k Kontext) | Download |
|---|---|---:|---|
| **Fürs Erste, auch mit 8 GB RAM** | Gemma 4 E2B Q4_K_M (Standard) | rund 2,9 GB | [Hugging Face: unsloth/gemma-4-E2B-it-GGUF](https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF) |
| Ohne Ablehnungen, strenger JSON-Modus | Llama 3.2 3B abliterated Q4_K_M | rund 4,4 GB | [Hugging Face: mradermacher/Llama-3.2-3B-Instruct-abliterated-GGUF](https://huggingface.co/mradermacher/Llama-3.2-3B-Instruct-abliterated-GGUF) |
| Mehr Qualität bei Text, ab etwa 12 GB RAM | Qwen3 4B Instruct 2507 Q4_K_M | rund 5,5 GB | [Hugging Face: bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF](https://huggingface.co/bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF) |
| Programmcode, ab etwa 16 GB RAM | Qwen2.5-Coder 7B abliterated Q5_K_M | rund 5,6 GB | [Hugging Face: bartowski/Qwen2.5-Coder-7B-Instruct-abliterated-GGUF](https://huggingface.co/bartowski/Qwen2.5-Coder-7B-Instruct-abliterated-GGUF) |

Alle Werte sind Messungen auf dem Entwicklungs-PC (nur CPU, 7 Threads), **nicht** auf einem 8-GB-Rechner. Die
Empfehlung "ab etwa 12 beziehungsweise 16 GB" ist eine Faustregel für genug Reserve neben Windows, keine Messung.
Dazu braucht jeder Stick das Embedding-Modell und die Laufzeit (siehe unten). Details, Dateinamen und Hashes stehen in
den folgenden Tabellen.

### Pflichtdateien (stehen im mitgelieferten Manifest)

Lege sie in `AI/models/` neben die gleichnamigen `*.model.toml` aus dem Repository.

| Modell | Datei auf dem Stick | Größe | Quelle | Lizenz |
|---|---|---:|---|---|
| **Gemma 4 E2B** (Standardmodell) | `gemma-4-E2B-it-Q4_K_M.gguf` | 3,11 GB | [unsloth/gemma-4-E2B-it-GGUF](https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF) | Apache-2.0 |
| **Llama 3.2 3B Instruct, „abliterated“** | `llama32-3b-abl-q4.gguf` (siehe Hinweis) | 2,24 GB | [mradermacher/Llama-3.2-3B-Instruct-abliterated-GGUF](https://huggingface.co/mradermacher/Llama-3.2-3B-Instruct-abliterated-GGUF), Datei `Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf` | Llama 3.2 Community License |
| **EmbeddingGemma 300M** (Embedding-Modell, im Manifest eingetragen) | `embeddinggemma-300m-q8.gguf` (siehe Hinweis) | 329 MB | [mradermacher/embeddinggemma-300m-GGUF](https://huggingface.co/mradermacher/embeddinggemma-300m-GGUF), Datei `embeddinggemma-300m.Q8_0.gguf` | Gemma-Nutzungsbedingungen |

> **Umbenennen:** Zwei Dateien heißen bei Hugging Face anders, als IAP sie erwartet. Benenne
> `Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf` in **`llama32-3b-abl-q4.gguf`** und
> `embeddinggemma-300m.Q8_0.gguf` in **`embeddinggemma-300m-q8.gguf`** um. Der Inhalt bleibt unverändert, der Hash
> muss danach zum Manifest passen.

SHA-256 zum Nachprüfen (`Get-FileHash -Algorithm SHA256 <Datei>` in PowerShell):

```
gemma-4-E2B-it-Q4_K_M.gguf      740185b21d22ceb83a11c3aa62ad5842ef32c70f6096d756bbee85a1e4ec34b8
llama32-3b-abl-q4.gguf          3a8ad1732ca90048bc5ecf44dcb3b789469e8a68d389ab79516853bf0d27ad34
embeddinggemma-300m-q8.gguf     6e46cac1c44a7b17a7ea35befb1ce0fef81414b53000ea302344eaf0783a9146
```

Beispiel für den Download in PowerShell (im Ordner `AI\models` auf dem Stick):

```powershell
Invoke-WebRequest -Uri "https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF/resolve/main/gemma-4-E2B-it-Q4_K_M.gguf" -OutFile "gemma-4-E2B-it-Q4_K_M.gguf"
Get-FileHash -Algorithm SHA256 "gemma-4-E2B-it-Q4_K_M.gguf"
```

### Optionale Modelle (nicht im Manifest)

Sie erscheinen im Modellmenü, sobald Datei und `*.model.toml` in `AI/models/` liegen. IAP prüft ihren Hash beim
Kopieren in den Cache gegen den Wert in der `*.model.toml`.

| Modell | Datei | Größe | Quelle | Lizenz | Hinweis |
|---|---|---:|---|---|---|
| Qwen3 4B Instruct 2507 | `Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf` | 2,50 GB | [bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF](https://huggingface.co/bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF) | Apache-2.0 (Basismodell, nicht erneut geprüft) | |
| Ministral 3 3B Instruct 2512 | `Ministral-3-3B-Instruct-2512-Q4_K_M.gguf` | 2,15 GB | [mistralai/Ministral-3-3B-Instruct-2512-GGUF](https://huggingface.co/mistralai/Ministral-3-3B-Instruct-2512-GGUF) | Apache-2.0 (nicht erneut geprüft) | |
| Qwen2.5-Coder 7B, „abliterated“ | `Qwen2.5-Coder-7B-Instruct-abliterated-Q5_K_M.gguf` | 5,44 GB | [bartowski/Qwen2.5-Coder-7B-Instruct-abliterated-GGUF](https://huggingface.co/bartowski/Qwen2.5-Coder-7B-Instruct-abliterated-GGUF) | Apache-2.0 | für Programmcode; **deutlich langsamer** (gemessen 5,9 bis 6,3 gegen 16,5 Token/s bei Gemma) |

„Abliterated“ heißt: Das Modell wurde so verändert, dass es kaum noch Anfragen ablehnt. Das ist eine bewusste Wahl;
beachte die Nutzungsrichtlinien der Basismodelle und sei dir bewusst, dass solche Modelle weniger Schutzmechanismen haben.

SHA-256: Qwen3 `2fde00ce…464e`, Ministral `9ed150d4…5f8`, Coder `a0e62cdf…281b` (vollständige Werte in den
Dateien unter [`AI/models/`](AI/models/)).

### llama.cpp-Laufzeit

IAP startet `llama-server` aus **llama.cpp, Build `b10930`** (Commit `56381e407`) als eigenen Prozess.

1. Öffne das [Release b10930](https://github.com/ggml-org/llama.cpp/releases/tag/b10930) und lade den
   Windows-CPU-Build `llama-b10930-bin-win-cpu-x64.zip`.
2. Entpacke den Inhalt nach **`AI/bin/win-x64/`**.

Es muss **genau dieser Build** sein, weil das Manifest die Hashes der Dateien festhält. Ich habe das Archiv für diese
README nicht heruntergeladen; die Zuordnung zum CPU-Archiv beruht auf dem Namen des Releases und den
`ggml-cpu-*.dll`-Dateien im Manifest. Stimmt ein Hash nicht, nennt IAP die Datei beim Start.

## Zusatzpakete (Sprache, Bildschirm, Git)

Optional und einzeln abschaltbar. Sie liegen getrennt unter `AI/packs/<name>/` und werden mit
[`packaging/windows/packs.ps1`](packaging/windows/packs.ps1) aus den Originaldateien gebaut (das Skript lädt nichts
herunter). Einzelheiten und Größen: [`docs/pakete.md`](docs/pakete.md).

| Paket | Zweck | Originaldateien | Lizenz |
|---|---|---|---|
| `whisper` | Spracherkennung | [whisper.cpp Release b5130](https://github.com/ggml-org/whisper.cpp/releases/tag/b5130), `whisper-bin-x64.zip`; Modell `ggml-base.bin` von [ggerganov/whisper.cpp](https://huggingface.co/ggerganov/whisper.cpp) | MIT |
| `piper` | Vorlesen | [Piper 2023.11.14-2](https://github.com/rhasspy/piper/releases/tag/2023.11.14-2), `piper_windows_amd64.zip`; Stimmen aus [rhasspy/piper-voices](https://huggingface.co/rhasspy/piper-voices) (`de_DE-thorsten-medium`, `en_GB-alba-medium`, `es_ES-davefx-medium`, `fr_FR-siwis-medium`) | MIT; Stimmen CC0 oder CC-BY 4.0 (Namensnennung nötig) |
| `vision` | „Bildschirm ansehen“ | `mmproj-gemma-4-E2B-it-Q8_0.gguf` (557 MB) aus [ggml-org/gemma-4-E2B-it-GGUF](https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF) | Apache-2.0 laut Modellkarte |
| `git` | Git-Worktrees für Agent Flow | [MinGit 2.56.0](https://github.com/git-for-windows/git/releases/tag/v2.56.0.windows.1), `MinGit-2.56.0-64-bit.zip` | GPL-2.0 (liegt als eigener Prozess bei; Quelltext dort verfügbar) |

Der Quellordner erwartet unter anderem `_zips\whisper-bin-x64.zip`, `_zips\piper_windows_amd64.zip`,
`_zips\MinGit-2.56.0-64-bit.zip`, `whisper\ggml-base.bin`, `piper\voices\*` und `vision\mmproj-*.gguf`
(Standard: `%USERPROFILE%\IAPDev\packs`).

## Bauen und auf den Stick bringen

Du brauchst: **Rust** (stable, MSVC-Toolchain), **Node.js**, **PowerShell** und **Strawberry Perl** (portabel, für das
mitgebaute OpenSSL von SQLCipher). Entpacke Strawberry Perl nach `tools\build\strawberry`, sodass
`tools\build\strawberry\perl\bin\perl.exe` existiert. Das Perl von Git-Bash funktioniert dafür nicht.

> **Pfad ohne Umlaute:** Die Build-Skripte scheitern an Sonderzeichen im Projektpfad. Klone in einen reinen
> ASCII-Pfad (z. B. `C:\src\iap`) oder lege eine Junction darauf an. Bisher getestet ist der Weg über eine
> Junction (`%TEMP%\portableai-usb-ascii`); ein direkter ASCII-Klonpfad als `-Junction`-Wert ist nicht gesondert geprüft.

```powershell
git clone https://github.com/Fredi2999/iap.git C:\src\iap
cd C:\src\iap

# 1. Strawberry Perl nach tools\build\strawberry entpacken (siehe oben)
# 2. llama.cpp-Laufzeit nach AI\bin\win-x64 entpacken (siehe oben)

# 3. Abhängigkeiten laden (einmalig, braucht Netz; der Build läuft danach offline)
cargo fetch --locked
cargo fetch --locked --manifest-path app\src-tauri\Cargo.toml
npm --prefix app ci
npm --prefix app run build

# 4. Bundle bauen (ohne Modelle; die liegen später auf dem Stick)
powershell -ExecutionPolicy Bypass -File packaging\windows\bundle.ps1 `
  -Junction C:\src\iap -OutDir C:\src\iap\dist\iap-windows -SkipModel
```

Das Skript meldet am Ende die Größe des Kernbundles (Grenze 100 MB, derzeit rund 84 MB) und die SHA-256 der
Programme. Danach:

1. Kopiere den **Inhalt** von `dist\iap-windows\` auf den Stick.
2. Lege die Modelle (und ggf. die Zusatzpakete unter `AI\packs\`) wie oben beschrieben auf den Stick.
3. Starte `Start.cmd` oder `iap.exe` vom Stick. Beim ersten Start legst du ein Master-Passwort fest.

Aufbau auf dem Stick:

```
Start.cmd, iap.exe, pa-launcher.exe, IAP-Beenden.exe, LIESMICH.txt
AI\
├── bin\
│   ├── manifest.json             Größe und SHA-256 aller Pflichtdateien
│   └── win-x64\                  llama.cpp-Laufzeit und iap-pci.exe
├── models\                       *.gguf und *.model.toml
├── packs\                        optionale Zusatzpakete
└── data\                         Tresore (.db und .meta), Arbeitsordner, WebView-Daten
```

Außerhalb des Sticks legt IAP nur an: den Modell-Cache (`%LOCALAPPDATA%\IAP\model-cache`), eine kurzlebige
Arbeitskopie des Tresors unter `%TEMP%\PortableAI\vault-hot` und, falls du ihn startest, den PCI-Begleiter unter
`%LOCALAPPDATA%\IAPPCI`. Beim Beenden fragt IAP, ob diese Spuren entfernt werden sollen.

## Prüfen

```powershell
# Rust: Formatierung, Lint ohne Warnungen, Tests (Workspace und Tauri-App sind getrennt)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app\src-tauri
cargo fmt --all --check; cargo clippy --all-targets -- -D warnings; cargo test
cd ..

# Frontend
npm run check                       # Typen und Svelte-Prüfung
node scripts\i18n-keys.mjs --missing   # alle Texte in en, es, fr, ja übersetzt?
node --experimental-strip-types --test scripts\calendar.test.ts scripts\contrast.test.ts scripts\graphLayout.test.ts

# Standardversion: kein Tresor, keine Zugangsdaten, keine persönlichen Pfade im Repository (auch nicht im Verlauf)
cd ..
node app\scripts\check-clean-repo.mjs
```

Das Skript `packaging\windows\bundle.ps1` ruft Cargo mit `--offline` auf; `cargo fetch --locked` (siehe oben) füllt
dafür den lokalen Cache.

## Aufbau des Repositorys

```
crates/
  pa-launcher    Hardware-Profil, Modell-Cache, Start, Selbsttest
  pa-core        Orchestrator, Unterhaltungen, Kontextbudget, Workflows
  pa-inference   llama.cpp-Prozess, Modell-Adapter, GBNF-Grammatiken
  pa-memory      Embeddings, Hybrid-Suche, Fakten, Gedächtnis-Graph
  pa-agents      Rollen, Eskalationsleiter, Budgets
  pa-skills      Wasmtime-Host für WASM-Skills, SKILL.md-Skills
  pa-policy      Modi, Pfadprüfung, Freigaben, Befehle, Audit-Protokoll
  pa-scheduler   Kalender, Aufgaben, Planer, ICS
  pa-vault       SQLCipher-Tresor, Argon2id, Hot-Copy-Sync
  pa-tools       Werkzeuge in Rust
  pa-code        Diff, Git (gix), Snapshots, Code-Agent
  pa-pci         PCI-Begleiter (Aktivitätsjournal)
  pa-types       gemeinsame Typen und IPC-Verträge
app/src-tauri    Tauri-Backend (eigener Cargo-Workspace)
app/src          Svelte-5-Frontend
AI/              Manifest und Modellbeschreibungen (keine Modelle, keine Laufzeit)
packaging/       Bundle-Skripte (Windows; Linux und macOS noch Entwürfe)
docs/            Konzept, Handover, Messungen, Prüfberichte
```

Wichtige Dokumente: [`docs/konzept.md`](docs/konzept.md) (Verweis auf das Architekturkonzept),
[`docs/pakete.md`](docs/pakete.md), [`docs/messung-phase0.md`](docs/messung-phase0.md),
[`docs/handover-2026-10-04.md`](docs/handover-2026-10-04.md) (aktueller Stand und offene Punkte).

## Stand und Grenzen

- **Plattform:** nur Windows. Linux und macOS sind noch nicht lauffähig (Netzwerk-Client, Audio, Bildschirmaufnahme,
  Datenträgerabfrage sind Windows-spezifisch).
- **Zielhardware 8 GB RAM, nur CPU:** Das Standardmodell ist darauf ausgelegt. Das 8-GB-Verhalten ist **gerechnet und
  teilweise gemessen, aber nicht auf einem echten 8-GB-Rechner geprüft.** Der Ressourcenplan warnt dort nur und verhindert die Wahl größerer Modelle nicht.
- **Automatisierte Tests** laufen grün (Rust, Frontend, Übersetzungen). **Nicht live geprüft** sind unter anderem:
  echte Aufrufe der Konnektoren, die Gmail-TLS-Verbindung, der PCI-Begleiter bei echter Nutzung, der Code-Agent mit
  echtem Modell, Befehle in der echten App sowie Passwortwechsel und Löschen an einem echten Tresor.
- **Befehle sind keine Sandbox** (siehe oben).
- **CI:** Der Workflow `.github/workflows/ci.yml` prüft jede Änderung (Windows: Format, Lint, Tests, Frontend; Linux: Darstellung und
  Lizenzen). Ob er auf GitHub bereits grün durchlief, ist noch nicht bestätigt, denn er wurde in `iap` noch nicht ausgeführt.

## Lizenzen

- **Quellcode von IAP:** [Apache-2.0](LICENSE).
- **Abhängigkeiten:** Die Rust-Crates sind permissiv lizenziert (MIT, Apache-2.0, BSD, Zlib, Unicode); einzelne
  Tauri-Abhängigkeiten stehen unter MPL-2.0.
- **Bloub** (Animationskern des Avatars, `app/src/lib/vendor/bloub`): MIT, Copyright Jérémy Perret, siehe dortige
  `LICENSE` und `NOTICE.md`.
- **Oberflächeneffekte** (Hintergrund, Wortmarke, Laufband, Meldungen und weitere) sind eigene Umsetzungen in Svelte und CSS,
  ohne fremden Komponentencode. Wer eigene Ersatzdateien mit anderer Lizenz einsetzen will, legt sie unter `app/src/private/` ab (gleiche
  relative Pfade wie in `app/src/`); dieser Ordner wird beim Bauen bevorzugt und ist nicht Teil des Repositorys.
- **Modelle, Laufzeit und Pakete** haben eigene Lizenzen (siehe Tabellen oben) und sind nicht Teil dieses Repositorys.
  Die Lizenzangaben sind Hinweise nach der jeweiligen Modellkarte, keine Rechtsberatung.
