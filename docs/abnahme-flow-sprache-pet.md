# Abnahme: Flow Version, Sprache, Bloub-Avatar, Pet

Stand 2026-09-30. Auftrag: `docs/agent-flow-workflows-sprache-gesicht.md`,
Plan: `docs/superpowers/plans/2026-09-28-flow-sprache-pet.md`.
Der Stick `D:\` wurde **nicht** verändert; ein Ersatz braucht deine ausdrückliche Freigabe.
Testbundle (ohne Modell, ohne Pakete): `dist/iap-windows-flow-2026-09-30`
(iap.exe SHA-256 `C679B65B81A58E1D0F732B65FEA59CF49D93E9F832D7CA305F287023AFC5E9AF`).

## Automatisch geprüft (grün)

| Prüfung | Ergebnis |
|---|---|
| `cargo fmt --all` (Workspace und App) | sauber |
| `cargo clippy --workspace --all-targets -- -D warnings` (Workspace und App) | sauber |
| `cargo test --workspace` | 400 bestanden, 0 fehlgeschlagen, 7 bewusst ignoriert |
| App-Tests (`app/src-tauri`) | 79 bestanden; mit `--include-ignored` und Paketen 80 (echter Rundlauf Piper → whisper-server) |
| Git-Integration gegen echtes MinGit (`IAP_GIT`, `--include-ignored`) | 34 Unit + 6 Integration bestanden |
| WinHTTP-Client real (`net::live_tests`, ungültiger Schlüssel) | TLS-Handshake, Kopfzeilen, Status 4xx bestätigt |
| `npm run check` | 0 Fehler, 0 Warnungen |
| Übersetzungen | 1102/1102 in en, es, fr, ja |
| Release-Bundle `bundle.ps1 -SkipModel` | 81,9 MB (< 100 MB) |

Inhaltlich abgedeckt durch Tests: Pfadangriffe und Junctions (pa-policy), Air Gap und Datenherkunft
für Exa, einmalige Aufnahmetickets, serielle Queue mit Abbruch, Vault-Migration v5 → v6,
Worktree-Abbruch ohne Änderung am Original, Konflikterkennung bei der Übernahme, Patch-Parser,
Workflow-Prüfung und -Runner (Budgets, Abbruch, Wiederholungen, Prompt-Injection im Webtext ändert
weder Knotenfolge noch Grenzen, neue Suchanfragen enthalten keinen Webtext).

## Am Bildschirm geprüft (Browser-Vorschau mit Mock, Tauri-Fenster für das Pet)

Startseite mit Bloub, Navigation Start ↔ Chat ↔ Flow (Entwurf bleibt), Zurück/Startseite,
Agent-Flow-Formular, Vergleich, Diff, Übernahme-Dialog mit zweiter Bestätigung, Workflow-Zeichenfläche
(Knoten ziehen, Verbindung anlegen, Prüfung, Startdialog, Live-Ansicht). Pet real geprüft
(1920×1080, sichtbar über der Taskleiste).

## Offene Tore (nicht erfüllt oder nicht geprüft) – bitte nicht als erledigt lesen

1. **8-GB-Hardware:** Nicht auf einem 8-GB-Rechner gemessen. Sprache bleibt auf T0 gesperrt, bis die
   gespeicherte Messung (Echtzeitfaktor, RAM-Reserve, UI-Reaktion) besteht; Bildverständnis bleibt auf T0 aus.
2. **Live-Test mit Mikrofon, Lautsprecher, Bildschirmaufnahme und Gemma-Projektor:** nicht gemacht
   (auf diesem PC liegen keine Modelle; die Geräte-Pfade sind kompiliert und die Logik getestet).
3. **Agent Flow mit echtem Modell:** Kandidatenlauf (Modell → Patch → Worktree → Diff → Übernahme) nur in
   Einzelteilen getestet, nicht als Ganzes mit Gemma auf dem Stick.
4. **Exa:** Der Weg bis TLS ist real belegt; ein Lauf mit **gültigem Exa-Schlüssel** wurde nicht gemacht
   (Schlüssel und Kosten liegen bei dir). Antwortformate folgen der Exa-Referenz vom 2026-09-29.
5. **Pet und Tray interaktiv:** Rechtsklick-Menü, Tray-Menü, „IAP vollständig beenden“ ohne verwaiste
   Prozesse, Mehrmonitor und andere Skalierungen sind nur gerechnet/getestet, nicht von Hand durchgespielt.
6. **Linux und macOS:** nicht Teil dieser Version. Der HTTPS-Client (WinHTTP) und die Datenträgerabfrage
   sind Windows-only und melden sonst „nicht eingerichtet“.
7. **Native Projekt-Tests im Agent Flow:** bleiben ohne nachgewiesene Sandbox deaktiviert („nicht ausgeführt“).
8. **Fehlermeldungen des Backends** (z. B. aus `agent_flow`, `workflow_cmds`) sind deutschsprachig und werden
   nicht übersetzt; die Oberfläche selbst ist vollständig übersetzt.
9. **Stick:** noch der Stand vom 2026-09-28. Ersetzen nur mit deiner Freigabe (Sicherung vorher nach
   `dist/usb-before-flow-2026-09-30`).

## Nachtrag 2026-09-30: Air Gap und Konnektoren

- WhatsApp und Gmail (Attrappen mit erfundenen Nachrichten) sind entfernt; IAP hat keine Vorschau-Funktionen mehr. Die einzige externe Verbindung ist Exa.
- Air Gap: Start immer AN; wirksam in drei Schichten: Policy (`authorize_exa`, je Aufruf), HTTPS-Client (harter Schalter vor jedem Netz-Aufruf) und Webview-CSP (nur `ipc:`).

## Sicherheitsverhalten in Kürze

- Air Gap EIN blockiert Exa pro Aufruf im Backend (`authorize_exa`); AUS erlaubt es nur mit Freigabe für genau
  einen Lauf (nicht gespeichert, neuer Lauf = neue Freigabe).
- An Exa gehen nur die im Ablauf sichtbaren öffentlichen Suchbegriffe und Modell-Neuformulierungen aus deinen
  Kriterien; Chat, Dateien, Gedächtnis, Bild und Ton haben keinen Weg dorthin.
- Workflows und Agent-Flow-Läufe enden beim Wechsel zum Pet und beim Beenden und setzen sich nie selbst fort.
- Bildschirm: genau eine Aufnahme je Auftrag, nur im Arbeitsspeicher; Bildtext ist unvertrauenswürdig.

## Nachtrag 2026-09-30 (Abend): SKILL.md, Code-Bereich, Pet, Agent-Bereich

Bundle `dist/iap-windows-flow3-2026-09-30` (Kern 82,04 MB, iap.exe SHA-256 `2A529F9253B9A3ABA4F58C7483B4AA156EB71B2CB5A092E705A7C4294BED7364`). **Nicht auf dem Stick.**

- **SKILL.md:** Anleitungs-Skills werden importiert (Ordner ziehen, wählen oder Pfad), sind ein-/ausschaltbar und werden dem Modell als Text mitgegeben. Sie haben keine Rechte und führen nichts aus; Skripte im Ordner werden übersprungen und gemeldet. 13 Importtests + 7 Parser-Tests.
- **Code-Bereich:** alle Dateizugriffe jetzt durch pa-policy mit Audit (vorher eigene Pfadprüfung). Tabs, Strg+S, Syntaxfarben, Suche, Dateibaum mit Anlegen/Umbenennen/Papierkorb, „IAP fragen“ (Vorschlag nur im Editor, Speichern erst nach Diff und Bestätigung). CRLF-Dateien behalten ihre Zeilenenden. 11 Tests mit echtem Tresor.
- **Pet:** erscheint schon beim Minimieren (real geprüft), Diktat wird automatisch gesendet (Logik im Browser-Mock geprüft, **nicht mit echtem Mikrofon**).
- **Agent(en) unter Werkzeuge** ist entfernt (Agent Flow und Workflows bleiben).
- Prüfungen: fmt, clippy `-D warnings`, Workspace-Tests, 104 App-Tests, `npm run check` 0/0, Übersetzungen 1088/1088.
- **Offen:** Code-Bereich nicht von Hand in der echten App durchgespielt und „IAP fragen“ nicht mit echtem Modell; Mikrofon-Auto-Senden; Ersetzen des Sticks nur mit Freigabe.
