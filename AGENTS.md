# Projekt: Portabler lokaler KI-Agent (Arbeitstitel "PortableAI")

## Was gebaut wird
Eine portable Desktop-Anwendung, die von einem USB-Stick startet und ein lokales
LLM auf der Hardware des Host-PCs ausführt. Vollständig offline. Keine Cloud,
keine Telemetrie, keine Netzwerkverbindung außer 127.0.0.1 für den eigenen
Inferenzprozess.

Das vollständige Architekturkonzept liegt in `docs/konzept.md`. Es ist die
verbindliche Referenz. Bei Widersprüchen zwischen einem Auftrag und dem Konzept:
nachfragen, nicht selbst entscheiden.

## Zielhardware
Untergrenze: 8 GB RAM, CPU-only, kein dedizierter Grafikprozessor.
Jede Designentscheidung wird gegen diese Untergrenze geprüft, nicht gegen die
Entwicklungsmaschine. Eine Funktion, die nur mit 32 GB RAM läuft, ist eine
optionale Funktion und muss als solche gekennzeichnet und abschaltbar sein.

Plattformen: Windows (primär), Linux, macOS (Phase 4).

## Festgelegter Technologie-Stack
Diese Entscheidungen sind getroffen und werden nicht neu diskutiert:

| Bereich | Technologie |
|---|---|
| Core, Launcher, Skill-Host | Rust (Edition 2021+), Cargo-Workspace |
| Desktop-Shell | Tauri v2 |
| Frontend | Svelte 5 + TypeScript + Vite + TailwindCSS |
| Editor-Komponente | CodeMirror 6 (nicht Monaco) |
| LLM-Inferenz | llama.cpp als Subprozess (`llama-server`), HTTP auf 127.0.0.1 |
| Datenbank | SQLite mit SQLCipher, sqlite-vec, FTS5 |
| Schlüsselableitung | Argon2id |
| Skill-Sandbox | Wasmtime (WASM/WASI) |
| Git-Zugriff | gix (reine Rust-Implementierung); für Worktrees ergänzend das optionale MinGit-Paket (Konzept 10.5) |

Wenn dir eine dieser Entscheidungen für eine konkrete Aufgabe ungeeignet
erscheint: sag es, begründe es, und warte auf meine Antwort. Tausche sie nicht
eigenmächtig aus.

## Crate-Struktur
crates/pa-launcher   Hardware-Profiling, Modell-Cache, Start, Selbsttest
crates/pa-core       Orchestrator, Konversationen, Kontextbudget
crates/pa-inference  llama.cpp-Prozess, ModelAdapter-Trait, GBNF-Grammatiken
crates/pa-memory     SQLite, Embeddings, Hybrid-Retrieval, Faktenextraktion
crates/pa-agents     Rollen, Eskalationsleiter L0-L3, Budgets, Router
crates/pa-skills     Wasmtime-Host, Manifest-Parser, Skill-Registry
crates/pa-policy     Modi M0-M3, Capabilities, Pfadnormalisierung, Audit-Log
crates/pa-scheduler  Kalender, Aufgaben, Constraint-Solver, ICS
crates/pa-vault      SQLCipher, Argon2id, Hot-Copy-Sync, Backup
crates/pa-tools      Kern-Skills in Rust
crates/pa-types      gemeinsame Typen, IPC-Verträge
app/src-tauri        Tauri-Backend, IPC-Commands
app/src              Svelte-Frontend

Crate-Grenzen sind Austauschpunkte. Keine Abkürzungen quer durch die Schichten:
pa-core kennt pa-inference nur über Traits, das Frontend kennt den Core nur über
typisierte IPC-Commands in pa-types.

## Nicht verhandelbare Invarianten
1. Kein ausgehender Netzwerkverkehr. Keine HTTP-Clients außer zum eigenen
   Inferenzprozess auf 127.0.0.1. Keine Crates, die im Hintergrund "nach Hause
   telefonieren". Keine Telemetrie, kein Crash-Reporting nach außen.
   Einzige dokumentierte Ausnahmen: die feste Host-Tabelle der Konnektoren
   (Exa, Wikipedia, Open-Meteo, Brave pro freigegebenem Workflow-Lauf, nur für
   öffentliche Suchbegriffe; Gmail per IMAP/SMTP nur nach ausdrücklicher
   Aktivierung durch den Nutzer in der Sitzung; Kalender – Apple iCloud per
   CalDAV und Google per geheimer iCal-Adresse – nur auf einen Klick des
   Nutzers, nie im Hintergrund), alle hinter pa-policy und bei Air Gap
   gesperrt (Konzept 10.5, 10.8). Kein weiterer Host ohne Änderung dieser
   Regel.
2. Jeder Werkzeugaufruf geht durch pa-policy. Kein Dateizugriff, keine
   Prozessausführung an der Policy-Engine vorbei — auch nicht "nur zum Testen".
3. Pfadnormalisierung existiert genau einmal, in pa-policy, und wird von allem
   benutzt: Symlinks auflösen, "..", UNC-Pfade, Groß-/Kleinschreibung.
4. Das Modell schreibt nie direkt in Dateien. Es erzeugt Patches, die der
   Nutzer als Diff sieht und bestätigt.
5. Kein Blockieren des UI-Threads. Alles Langlaufende geht in eine
   Hintergrund-Queue mit sichtbarem Status.
6. Keine Zustandsdaten außerhalb des Vaults. Keine versteckten Dateien in
   Nutzerverzeichnissen außer dem klar benannten Modell-Cache und den
   sichtbaren Kandidaten-Worktrees von Agent Flow (Konzept 10.5).
   Einzige weitere Ausnahme: der PCI-Begleiter (Konzept 10.6). Er läuft
   **sichtbar** auf einem Host-PC, während der Stick draußen ist, und legt sein
   Aktivitätsjournal in einem **klar benannten** Ordner (`%LOCALAPPDATA%\IAPPCI`)
   ab. Er zeichnet nur auf, welche Anwendung im Vordergrund ist, deren Fenstertitel
   und die Dauer – **nie** Nachrichteninhalte, Tastatureingaben oder Bildschirm – und
   geht **nie** ins Netz. Der Nutzer startet ihn bewusst, kann ihn jederzeit stoppen
   und mit einem Klick samt Journal vom PC entfernen. Beim nächsten Einstecken wird
   das Journal in den Vault importiert (dort als PCI abrufbar). Ein verdecktes
   Mitlesen oder ein Abfangen fremder Kommunikation ist ausdrücklich nicht erlaubt.

Ergänzung zu Invariante 2 und 6 (Code-Bereich auf dem PC, Konzept 10.7): Der Nutzer kann einen
Projektordner auf dem PC freigeben (`pa-policy::host_root`, widerrufbar, im Vault gemerkt). Dort legt
IAP keine versteckten Ordner an und löscht nichts. Befehle im Projektordner gibt es nur über
`pa-policy::command::authorize_command` und nur nach Einzelbestätigung des Nutzers; sie sind keine
Sandbox (kein Netz-/Pfadschutz des gestarteten Programms) und werden im Dialog so benannt.
Das Modell schlägt Änderungen und Befehle nur vor.

Ergänzung zu Invariante 1 (Kalender, Konzept 10.8): Der Nutzer kann einen Apple-Kalender (iCloud,
`caldav.icloud.com` und die Gruppen `pNN-caldav.icloud.com`) und einen Google-Kalender (geheime iCal-Adresse auf
`calendar.google.com`) verbinden. Abrufen und Anlegen eines Termins sind die einzigen Auslöser; es gibt keinen Takt.
`pa-policy::egress::authorize_calendar` (Air Gap, feste Host-Tabelle, Audit) sitzt unter jeder Anfrage. Zugangsdaten
liegen nur im Tresor, nie in der Oberfläche. Die WebDAV-Verben `PUT`, `PROPFIND` und `REPORT` sind ausschließlich für
den Kalender-Konnektor freigeschaltet.

## Codestil
- Rust: `thiserror` für Bibliotheks-Fehler, `anyhow` nur in Binaries.
  Kein `unwrap()` außerhalb von Tests. `clippy -D warnings` muss durchlaufen.
- Alle öffentlichen Funktionen mit Doc-Kommentar, der erklärt *warum*, nicht was.
- Deutsche Kommentare sind in Ordnung, englische Bezeichner.
- Tests neben dem Code. Integrationstests in tests/.
- Keine Abhängigkeit ohne Begründung im Commit. Jede Crate kostet Binary-Größe.

## Größenbudget
Kernsystem ohne Modelle: unter 100 MB. Wenn eine Änderung das Bundle um mehr als
5 MB wachsen lässt, nenne den Grund und die Alternative.

## Was du nicht tun sollst
- Keine Platzhalter-Implementierungen, die so aussehen, als würden sie
  funktionieren. Wenn etwas noch nicht geht: `todo!()` mit Kommentar.
- Keine erfundenen API-Signaturen für llama.cpp, Tauri v2, sqlite-vec oder
  Wasmtime. Diese APIs ändern sich. Nachschlagen oder nachfragen.
- Keine Beispieldaten, die im Produktivpfad landen.
- Keine "hilfreichen" Zusatzfeatures, die nicht beauftragt waren.