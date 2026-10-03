# MVP-Abnahme Schritt 6: portables Windows-Paket

Stand: 16. September 2026. Dieses Dokument beschreibt die Verzeichnisstruktur,
den Bauablauf und die noch offenen manuellen Prüfungen für das Windows-Bundle
`dist/portable-ai-windows/`.

## Zielstruktur

Nach `packaging/windows/bundle.ps1` liegt unter `dist/portable-ai-windows/`:

```
Start.cmd                      # startet portable-ai.exe oder verweist auf pa-launcher.exe
portable-ai.exe                # Tauri-App (fehlt, wenn --SkipTauri)
pa-launcher.exe                # CLI aus Schritt 3, für Diagnose
LIESMICH.txt                   # Anleitung, Grenzen, Datenschutz
AI\
├── bin\
│   ├── win-x64\               # llama-server.exe + Runtime-DLLs
│   └── manifest.json          # SHA-256 aller Runtime-/Modelldateien
├── models\
│   ├── gemma-4-E2B-it-Q4_K_M.gguf
│   └── gemma-4-e2b-q4-k-m.model.toml
└── data\                      # leer; Vault entsteht beim ersten Start
```

Alles, was das System zur Laufzeit außerhalb des Sticks anlegt, beschränkt
sich auf den klar benannten `%LOCALAPPDATA%\PortableAI\model-cache\<sha256>`
(bereits aus Schritt 1 dokumentiert) und die kurzlebige Hot-Copy unter
`%TEMP%\PortableAI\vault-hot\` (aus Schritt 2).

## Bauablauf

Wegen des `ü` im realen Projektpfad läuft alles über den bereits in Schritt 3
etablierten ASCII-Junction-Pfad `%TEMP%\portableai-usb-ascii`.

```powershell
$junction = Join-Path $env:TEMP 'portableai-usb-ascii'
Set-Location $junction
$env:CARGO_TARGET_DIR = Join-Path $pwd.Path 'target-ascii'
$env:PATH = (Join-Path $pwd.Path 'tools\build\strawberry\perl\bin') + ';' + $env:PATH
$env:LC_ALL = 'C'
$env:LANG = 'C'
Remove-Item Env:PERL5LIB -ErrorAction SilentlyContinue

# Frontend einmalig (benötigt Netzwerk):
npm --prefix app install
npm --prefix app run build

# Bundle-Skript:
powershell -ExecutionPolicy Bypass -File packaging\windows\bundle.ps1
```

Optionen des Skripts:

| Flag | Zweck |
|---|---|
| `-Junction <Pfad>` | Anderer ASCII-Pfad, falls %TEMP% nicht passt |
| `-OutDir <Pfad>` | Zielverzeichnis überschreiben |
| `-SkipTauri` | Nur pa-launcher.exe bauen (wenn npm offline nicht verfügbar) |
| `-SkipModel` | AI\models nicht mitkopieren (für Größenmessung des Kernbundles) |

Am Ende zeigt das Skript die SHA-256-Werte von `pa-launcher.exe` und (falls
gebaut) `portable-ai.exe` sowie die Größe des Kernbundles ohne Modell.

## Größenbudget

Konzept 5.2 fordert das Kernsystem ohne Modell unter 100 MB.

- Schritt 3 hat für den reinen CLI-Launcher 51,3 MB gemessen; die neuen
  Bibliotheks-Splits (`pa-core`, `pa-launcher::bootstrap`, `runtime`,
  `inference_backend`) verlagern nur Code, keine neuen Abhängigkeiten.
- Die Tauri-App bringt zusätzliches Frontend-Bundle plus Tauri-Runtime (WebView2
  wird nicht mitgeliefert). Erwartet ~15–25 MB extra.
- Kein neuer nativer Abhängigkeitsgraph.

Die tatsächliche Zahl gibt das Skript aus; bei Überschreitung meldet es das
mit gelber Warnung und muss dann untersucht werden. Ich habe hier keinen
geschätzten Zielwert eingetragen.

## Automatisierbare Akzeptanzkriterien

Was ohne fremde Hardware messbar ist:

- `cargo fmt --all -- --check` grün.
- `cargo test --workspace --offline` grün.
- `cargo clippy --workspace --all-targets --all-features --offline -- -D warnings` grün.
- `cargo clippy --offline -- -D warnings` in `app/src-tauri/` grün.
- Der `Loopback-only`-Vertrag wird strukturell durch den Typ `LoopbackEndpoint`
  in `pa-inference` garantiert; ein anderer Endpoint-Typ existiert nicht.
- SHA-256 aller kopierten Runtime-Dateien stimmt vor dem Kopieren mit dem
  Manifest überein (im CLI beim Start durch `verify_manifest` erneut geprüft).
- Streaming-Startzeit von unter 3 s nach Prompt-Verarbeitung wurde bereits in
  Schritt 3 auf der Entwicklungsmaschine gemessen (0,004–0,017 s abhängig vom
  Lauf); das Bundle-Skript ändert daran nichts.
- Abbruch-Erhalt und Recovery nach hartem Prozessende sind durch die
  automatisierten `pa-vault`- und `pa-core`-Tests abgedeckt.

## Manuelle Prüfungen (Nutzer, nicht geschätzt)

Diese Punkte kann nur ein Mensch auf realer Hardware bestätigen; sie werden
hier ausdrücklich als offen geführt:

- [ ] Doppelklick-Start von `portable-ai.exe` auf einem echten 8-GB-Windows-PC
      ohne Administratorrechte.
- [ ] Start auf zwei weiteren physischen Windows-Rechnern (Portabilität aus
      Phase 0 Risiko 3).
- [ ] Verhalten von SmartScreen bei unsignierter EXE.
- [ ] WebView2-Verfügbarkeit auf einem Rechner ohne vorinstalliertes WebView2.
- [ ] Physisches Ziehen und Wiederanstecken des USB-Sticks während einer
      laufenden Chat-Sitzung.
- [ ] Externer Netzwerkmonitor (Wireshark oder Windows-Ressourcenmonitor) über
      eine komplette Sitzung: kein ausgehender Traffic außer 127.0.0.1.
- [ ] End-to-End-Chat in der GUI gegen den echten `llama-server` mit
      Streaming, Abbruch und Neustart nach hartem Prozessende.

Die Ergebnisse dieser sechs Prüfungen gehören ergänzend in dieses Dokument,
sobald du sie ausgeführt hast.

## Rollback

Das Skript legt bei jedem Lauf das Zielverzeichnis komplett neu an. Um zu
einem älteren Bundle zurückzukehren, reicht das erneute Bauen aus einem
Git-Stand mit dem gewünschten `AI\bin\manifest.json`.

## Bundle-Lauf 2026-09-17

Reproduktion des Skripts inklusive Tauri-Release (ohne `-SkipTauri`) auf
der Entwicklungsmaschine, im ASCII-Junction unter
`%TEMP%\portableai-usb-ascii` mit Strawberry-Perl im PATH und
`LC_ALL=C`/`LANG=C`.

- Frontend: `npm install` (60 Pakete, 0 Vulnerabilities), danach
  `npm run build` mit `vite v8.3.0`, 117 Module transformiert,
  Bundle 60,17 kB (JS gzip 22,15 kB) / 16,91 kB (CSS gzip 4,30 kB).
- Tauri-Release-Build in `target-ascii-app\release`: dauerte 13 min 18 s
  (vendored OpenSSL). LNK4099-Meldungen zu `ossl_static.pdb` bleiben wie
  in Schritt 3 dokumentiert; keine echten Buildfehler.
- Bundle-Ziel: `dist\portable-ai-windows\` mit `Start.cmd`,
  `LIESMICH.txt`, `pa-launcher.exe`, `portable-ai.exe`, `AI\bin\`,
  `AI\models\` und leerem `AI\data\`.
- Kernbundle (ohne `AI\models`): **62,49 MB** – unter dem 100-MB-Budget
  aus Konzept 5.2.
- SHA-256 `pa-launcher.exe`:
  `863D853FDEF323D3C655136A88AEB1B05AB98DDB4368A28D9B5A62A74B3E005D`
- SHA-256 `portable-ai.exe`:
  `C8A6124BF696F7E06CD6636AD26AF13E719E5E77ACB0730F7BBBF45AEB7EF361`

### Nebenfixes im Zug dieses Laufs

- `app/src-tauri/src/lib.rs`: `cargo fmt` hat vier rein mechanische
  Umbrüche (langer `format!`, `stop().map_err(...)?`, `*engine_guard =
  ...`, `dropped_older_turns:`) angewendet, keine Verhaltensänderung.
- `app/src/routes/Settings.svelte`: die fünf lokalen `$state`-Init aus
  `settings.*` sind jetzt in `untrack(() => ...)` gewickelt. Die
  Svelte-5-Warnung `state_referenced_locally` verschwindet, das
  Snapshot-Verhalten (lokale Auswahl überlebt einen Prop-Refresh)
  bleibt unverändert – `<Settings>` wird beim Routewechsel ohnehin neu
  gemountet.

### Automatisierbare Akzeptanzkriterien, die dieser Lauf tatsächlich bestätigt

- `cargo fmt --all -- --check` grün (Workspace, Exitcode 0).
- `cargo test --workspace --offline` grün, keine Regression an den
  Schritt-1/2/3-Tests (Recovery, Cleanup, Sync-Worker,
  Passphrasen-Wrong-Path, Repository, IPC-Contract, chat_contract,
  hardware_contract).
- `cargo clippy --workspace --all-targets --all-features --offline
  -- -D warnings` grün (Exitcode 0).
- In `app/src-tauri`: `cargo fmt -- --check` grün, `cargo clippy
  --offline -- -D warnings` grün.
- Bundle-Skript erzeugt reproduzierbar `dist\portable-ai-windows\` und
  meldet die Größen- und Hash-Werte oben.

Die in Konzept 5.3 dokumentierte Streaming-Startzeit unter 3 s ist
ausschließlich aus Schritt 3 (Konsolen-Chat) belegt. Für die GUI selbst
fehlt weiter ein realer End-to-End-Lauf gegen `llama-server` – siehe
Liste "Manuelle Prüfungen (Nutzer, nicht geschätzt)".
