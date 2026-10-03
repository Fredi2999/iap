# IAP-Update: Flow Version, Sprachgespräch und Bloub-Avatar mit Desktop-Pet

Stand: 28. September 2026 · Status: Ergänzungskonzept, keine Implementierung

**Gegenstand ist ein Update der vorhandenen IAP-App.** Es entsteht keine zweite Anwendung, kein Fork und kein neuer Grundaufbau. Nach dem Entsperren öffnet IAP die neue **Avatar-Hauptseite mit Bloub**. Der Chat bleibt als eigener Bereich über die vorhandene Navigation erreichbar. Ein sichtbarer Button **„Flow Version“** öffnet im selben Fenster einen Modus mit den zwei Bereichen **Agent Flow** und **Workflow-Builder**. Der Avatar kann sich lokal per Sprache mit dem Nutzer unterhalten und nach einem ausdrücklichen Rechtsklick-Befehl eine Bildschirmaufnahme ansehen. Beim Schließen des Hauptfensters erscheint er unten rechts als Desktop-Pet. Alles verwendet dieselben Sitzungen, Modelle, Berechtigungen, den Vault und den lokalen Inferenzprozess von IAP.

Dieses Dokument beschreibt nur die dafür nötigen Ergänzungen zum [verbindlichen Architekturkonzept](../portabler-ki-agent-konzept.md). Es ändert dessen Regeln nicht stillschweigend. Vor einer Implementierung müssen die Ausnahmen für Exa und Kandidaten-Worktrees, die unten beschriebene optionale Spracheingabe auf 8-GB-Rechnern sowie Bildschirmrechte und der sichtbare Pet-Betrieb in die verbindliche Referenz übernommen werden. Die Modellauswahl ist bereits erfolgt und gehört nicht mehr hierher.

## 1. Anknüpfungspunkte im bestehenden IAP

- IAP ist für 8 GB RAM und reine CPU als Untergrenze ausgelegt. `llama-server` läuft lokal auf `127.0.0.1`; die App verwendet Rust, Tauri v2, Svelte 5, SQLCipher und `pa-policy`.
- Der bestehende Agentenbereich führt Rollen wie Vorschlag, Kritik und Prüfung mit einem lokalen Modell aus. Er verwaltet noch keine isolierten Git-Worktrees oder konkurrierenden Coding-Kandidaten.
- Der Code-Bereich besitzt Editor, Diff-Anzeige und Git-Funktionen, aber noch keinen Worktree-Lebenszyklus.
- Die Exa-Ansicht liefert zurzeit Beispieltreffer, keinen echten Exa-Aufruf. Die Air-Gap-Oberfläche muss bei einer Umsetzung die tatsächliche Netzsperre widerspiegeln.
- `pa-policy` verbietet ausgehendes Netzwerk derzeit vollständig. Vor Exa ist daher eine eng begrenzte, prüfbare Ausnahme nötig.
- Die Chat-Eingabe hat noch keine Mikrofonsteuerung oder lokale Spracherkennung. Der Bloub-Avatar wird zum Mittelpunkt einer neuen Hauptseite innerhalb der bestehenden Svelte-App; diese ersetzt den Chat als Startansicht.
- Die vorhandenen Leistungswerte wurden überwiegend auf einem Rechner mit rund 29 GB RAM ermittelt. Ein physischer 8-GB-CPU-Rechner ist als Freigabetest weiterhin erforderlich. Siehe [Phase-0-Messungen](messung-phase0.md).

## 2. Gemeinsame Regeln für diese Erweiterungen

1. **Ein gemeinsamer Flow-Modus innerhalb von IAP:** Der Button „Flow Version“ führt zu Agent Flow für Coding-Aufgaben und zum kleinen visuellen Workflow-Builder für lokale Abläufe. Beide liegen im selben Modus und nutzen die vorhandene Modell-Warteschlange, Policy und Vault-Speicherung.
2. **Nur eigene lokale Modelle:** Planung, Patches, Zusammenfassung und Prüfung erfolgen über lokal installierte GGUF-Modelle. Exa dient ausschließlich als freigegebene Such- und Quellen-Schnittstelle, nicht als fremder Generierungsagent.
3. **Air Gap als Hauptschalter:** Ist er eingeschaltet, ist externer Verkehr gesperrt. Ist er ausgeschaltet, braucht Exa trotzdem eine eigene Freigabe für **jeden Workflow-Lauf**.
4. **Strikte Exa-Datengrenze:** Text aus lokalen Dateien, Chats, Memory und Code darf niemals an Exa gesendet werden, auch nicht nach einer zusätzlichen Bestätigung. Exa sieht nur öffentliche, sichtbare Suchbegriffe und gegebenenfalls öffentliche Exa-Ergebnisse für die nächste Suchrunde.
5. **Fenster und App-Lebensdauer:** Workflows laufen nur bei geöffnetem IAP-Hauptfenster. Dessen Schließen stoppt die Workflow-Ausführung und wechselt in den sichtbaren Pet-Modus. „IAP vollständig beenden“ beendet auch Pet, Sprachfunktionen und Inferenzprozesse. Der Pet-Modus ist Teil desselben laufenden App-Prozesses, kein versteckter Dienst.
6. **8-GB-Verhalten:** Kandidaten-Worktrees können gleichzeitig vorhanden sein, Modellaufrufe werden auf schwacher Hardware nacheinander abgearbeitet. Gleichzeitige Inferenz ist nur eine später gemessene Option auf stärkeren Rechnern.
7. **Sprachgespräch bleibt lokal und bewusst gestartet:** Mikrofonzugriff beginnt nur nach einer sichtbaren Aktion. Spracherkennung und Sprachausgabe sind beide Teil der geplanten Gesprächsfunktion und brauchen keine Cloud; Audio wird nicht gespeichert. Auf 8-GB-CPU-Rechnern ist die Funktion erst nach erfolgreicher Ressourcen- und Laufzeitmessung optional freizuschalten.
8. **Avatar als Hauptseite und Status:** Bloub ist der zentrale Einstieg nach dem Entsperren. Er visualisiert Zustände, startet aber durch bloßes Anklicken weder Mikrofon noch Agenten, Werkzeuge oder Exa. Der Chat ist weiterhin direkt erreichbar.
9. **Bildschirm nur auf Auftrag:** Rechtsklick auf den Avatar → „Bildschirm ansehen“ erlaubt genau eine Aufnahme des ausgewählten Bildschirms, Fensters oder Bereichs. Es gibt keine laufende Beobachtung; Bildschirmdaten und daraus extrahierter Text werden niemals an Exa gesendet.

### Einstieg in die Flow Version

Der Button **„Flow Version“** sitzt in der bestehenden Hauptnavigation und bleibt auf der Avatar-Hauptseite sowie aus Chat, Code und Agents erreichbar. Er wechselt die aktuelle IAP-Ansicht zum Flow-Modus; dort wählt der Nutzer zwischen **Agent Flow** und **Workflows**. Ein sichtbarer Zurück-Button führt zur vorherigen normalen IAP-Ansicht; „Startseite“ führt zum Avatar. Unterhaltungen, Entwürfe und laufende, erlaubte Aufgaben bleiben beim Ansichtswechsel erhalten. Der Modus ist Teil desselben App-Builds und benötigt weder eine zweite Installation noch einen zweiten Vault oder Hintergrunddienst.

## 3. Agent Flow innerhalb der Flow Version

### Bedienablauf

1. Im Flow-Modus öffnet der Bereich **Agent Flow** die Coding-Arbeitsfläche. Er verwendet die vorhandenen Code- und Agents-Funktionen von IAP. Der Nutzer wählt ein Git-Projekt auf dem USB-Stick oder ein ausdrücklich freigegebenes Host-Projekt. Das Worktree-Verzeichnis wird angezeigt und liegt beim jeweiligen Repository.
2. Bei uncommitteten Änderungen wählt der Nutzer sichtbar zwischen dem letzten Commit und einem geprüften Snapshot der aktuellen Änderungen als Startbasis aller Kandidaten. Der ursprüngliche Arbeitsbaum wird weder gestasht noch verändert. Ignorierte Dateien und sensible Inhalte dürfen nicht unbemerkt in den Snapshot gelangen.
3. Eine Aufgabe startet getrennte Kandidaten, höchstens fünf und auf T0 zunächst zwei. Jeder Kandidat bekommt einen Worktree, Branch, eigenen Aufgabenkontext, Berechtigungen, Budget und ein wählbares lokal installiertes Modell. Das aktive Modell ist die Vorgabe; Modellwechsel und Ladezeit erscheinen in der Queue.
4. Das Modell erzeugt Patches. Ein Runner validiert Zielpfade und Patch-Inhalt durch `pa-policy` und übernimmt die Änderungen unter der einmaligen Aufgabenfreigabe **nur im Wegwerf-Worktree dieses Kandidaten**. Auch Git-Operationen und Testprozesse gehen durch die Policy. Keine direkte Modell-Schreibberechtigung.
5. Die Vergleichsansicht zeigt Diffs, Tests, Fehler, Ressourcenverbrauch und Basisversion je Kandidat. Der Nutzer wählt einen Gewinner und sieht dessen vollständigen Diff. Erst eine weitere ausdrückliche Bestätigung integriert den Kandidaten in das Zielprojekt. Konflikte werden angezeigt; ein Push erfolgt nicht automatisch.

### Technische Grenzen

Ein Worktree trennt Arbeitsdateien, ist aber keine Prozess-Sandbox und teilt Git-Metadaten mit dem Haupt-Repository. Git-Hooks, externe Filter, Symlinks, Pfadwechsel und volle Datenträger sind vor jedem Lauf zu prüfen. Das Zielprojekt darf seit dem Start nicht unbemerkt verändert worden sein; andernfalls wird die Übernahme gestoppt und eine Konfliktansicht geöffnet. Kandidaten-Commits und Audit-Daten bleiben bis zur bewussten Bereinigung nachvollziehbar.

`gix` bleibt der festgelegte Git-Baustein. Für Worktree-Anlegen, -Reparieren und die spätere Integration ist ein **optionales portables Git-Paket** vorgesehen, weil `gix` die benötigte Worktree-Manipulation noch nicht vollständig anbietet. Dieses Paket wird separat vermessen und nicht still in das unter 100 MB große Kernsystem eingebaut. [Git-Worktree-Dokumentation](https://git-scm.com/docs/git-worktree) · [Gitoxide-Status](https://github.com/GitoxideLabs/gitoxide/discussions/2840)

Native Projekt-Tests sind nur nach einer **Freigabe pro Lauf** erlaubt: feste Befehlsliste, Zeit- und Speichergrenzen, Protokoll, keine Netzverbindung. Windows Job Objects allein beschränken keine Netzwerkzugriffe. Kann eine geprüfte OS-Sandbox die Netzsperre und Pfadgrenzen nicht sicher durchsetzen, bleiben native Tests deaktiviert und der Status lautet ehrlich „nicht ausgeführt“. [Microsoft: Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)

## 4. Workflow-Builder innerhalb der Flow Version

Der Bereich **Workflows** im selben Flow-Modus bietet in der ersten Version ein kleines Drag-and-drop-Canvas mit festen, typisierten Knoten: **manueller Start, Eingabe, Exa-Suche, Exa-Inhalte, lokales Modell, Prüfung, Bedingung/begrenzte Wiederholung und Ausgabe**. Frei programmierbare JavaScript- oder Python-Knoten gehören nicht zu Version 1. Die Oberfläche zeigt die Datenübergänge und weist ungültige Verbindungen vor dem Start zurück.

Beispiel: `öffentliche Suchfrage → Exa-Suche → Quelleninhalt → lokales Modell prüft anhand sichtbarer Kriterien → bei Lücken präzisierte öffentliche Suche → Ergebnis mit Quellen und offenen Punkten`.

Vor jedem Start sind Erfolgskriterien sowie Höchstwerte für Suchaufrufe, Wiederholungen, Zeit, Tokens und mögliche Exa-Kosten sichtbar. „Wiederholen bis richtig“ bedeutet **keine Endlosschleife und keine Wahrheitsgarantie**. Werden die Kriterien nicht erreicht, endet der Lauf mit „nicht ausreichend belegt“. Webinhalte sind unvertrauenswürdige Daten; Anweisungen darin dürfen keine Werkzeuge oder Berechtigungen steuern.

Der Exa-Knoten akzeptiert nur eine vom Nutzer sichtbar als öffentlich eingegebene Suchfrage und öffentliche Suchergebnisse. Ein Modell zur Suchverfeinerung läuft in einem getrennten Kontext, der nie lokale Dateien, Chats, Memory, Code oder Bildschirmaufnahmen gesehen hat. Eine Graph-Verbindung von privaten Quellen zu Exa wird abgewiesen. Air Gap EIN stoppt weitere Exa-Aufrufe sofort; bereits gesendete Anfragen können nicht zurückgeholt werden. Beim Schließen des Hauptfensters endet der Lauf auch dann, wenn das Pet sichtbar bleibt. Sein Protokoll bleibt im Vault; eine Fortsetzung mit Exa verlangt eine neue Lauf-Freigabe. [Exa Search](https://exa.ai/docs/reference/search) · [Exa Contents](https://exa.ai/docs/reference/get-contents) · [Exa-Preise](https://exa.ai/pricing?tab=api)

## 5. Anbindung an vorhandene IAP-Module

| Vorhandener Baustein | Ergänzung für diese Funktionen |
|---|---|
| `pa-types` | Versionierte Typen für Kandidaten, Graphen, Berechtigungen, Status, Sprachsitzungen, einzelne Bildschirmaufnahmen und Pet-/Hauptfenster-Zustand. |
| `pa-core` | Orchestrierung, gemeinsame Modell-Queue, begrenzte Workflow-Ausführung. |
| `pa-agents` und Code-Bereich | Kandidatenzustand, Worktree-Lebenszyklus, Patch-Prüfung, Vergleich und Übernahme. |
| `pa-policy` | Zentrale Pfadnormalisierung, Werkzeugrechte, Mikrofon- und Bildschirmfreigaben sowie eng begrenzte Exa-Ausnahme. Kein allgemeiner HTTP-Client. |
| `pa-inference` | Lokale Modellaufrufe und gemeinsame Inferenz-Queue; Ressourcenlimits für Spracherkennung, Sprachausgabe und unterstützte Bildanalyse, damit keine großen Modellprozesse unbemerkt parallel laden. |
| `pa-vault` | Workflow-Definitionen, Laufereignisse, Kandidaten-Metadaten, Audit, Sprach- und Pet-Einstellungen und verschlüsselter Exa-Schlüssel. Worktrees selbst sind sichtbare Projektarbeitsräume; rohe Mikrofonaufnahmen und Bildschirmbilder werden nicht abgelegt. |
| Bestehende Tauri/Svelte-App | Bloub-Hauptseite, Chat, Button „Flow Version“ und Rückweg; kleines Pet-Fenster unten rechts mit Kontextmenü sowie native Aufnahme- und Sprachsteuerung über typisierte IPC. |

Vor einer Implementierung sind diese Risiken praktisch zu prüfen: Worktrees auf USB/Host samt Laufwerksbuchstabenwechsel; native Test-Sandbox ohne Netzwerk; echte Air-Gap-Sperre und Exa-Request-Audit; CPU-, RAM-, Ladezeit- und Qualitätsmessung auf einem echten 8-GB-Rechner; lokale Sprachfunktionen und Bildanalyse mit dem gleichzeitig benötigten App-Speicher; Bildschirmrechte und Pet-Fenster bei mehreren Monitoren, unterschiedlichen Skalierungen und Taskleisten. exFAT unterstützt nicht alle Dateisystemfunktionen von NTFS; ein Preflight muss inkompatible Projekte erkennen. [Microsoft: Dateisystemvergleich](https://learn.microsoft.com/en-us/windows/win32/fileio/filesystem-functionality-comparison)

**Abnahmebedingung:** „Flow Version“ öffnet im bestehenden IAP-Fenster beide Bereiche und der Rückweg erhält die normale Sitzung. Kein Agent schreibt außerhalb seines freigegebenen Worktrees; kein Workflow sendet private lokale Texte an Exa; Air Gap EIN unterbindet externe Aufrufe; jeder Exa-Lauf besitzt eine neue Freigabe; Abbruch und Schließen des Hauptfensters stoppen Workflow-Schritte. Vollständiges Beenden stoppt alle App-Funktionen; Gewinnerübernahme ist erst nach vollständiger Diff-Vorschau möglich. Sprachfunktionen bleiben ohne Netz erreichbar, Mikrofonaktivität ist sichtbar und sofort stoppbar, und der Avatar bleibt auch ohne Animation bedienbar.

## 6. Lokales Sprachgespräch auf Hauptseite, im Chat und mit dem Pet

**Ziel:** Der Nutzer spricht mit IAP und erhält lokal erzeugte gesprochene Antworten. Hauptseite, Chat und Desktop-Pet verwenden dieselbe Unterhaltung. Diktat und eine kleine Menge klar benannter Navigationsbefehle ergänzen das Gespräch. Die gewählte Sprache der Oberfläche ist der Startwert für die Erkennung; sie kann für die Spracheingabe separat geändert werden. Textbedienung bleibt vollständig erhalten.

### Bedienung und Berechtigungen

1. Ein beschrifteter Mikrofonknopf auf der Avatar-Hauptseite beziehungsweise an der Chat-Eingabe und ein Tastenkürzel starten eine einzelne Aufnahme. Währenddessen zeigen Mikrofonknopf, Statuszeile und Avatar eindeutig „Hört zu“. Dieselben Bedienelemente stoppen oder verwerfen die Aufnahme sofort. Es gibt kein dauerhaft aktives Mikrofon und zunächst kein Aktivierungswort.
2. Diktierter Chat-Text erscheint zuerst editierbar in der Eingabe und wird erst nach „Senden“ zur Nachricht. Ein klar erkannter Befehl aus einer festen Liste darf unmittelbar eine harmlose Navigation auslösen; bei Unsicherheit bleibt der erkannte Text stehen und löst nichts aus.
3. Erlaubte Sprachbefehle der ersten Version betreffen Navigation, etwa „Chat öffnen“, „Flow Version öffnen“ und „Workflows anzeigen“. Aktionen mit Folgen für Dateien, Prozesse, Agent Flow, Workflows oder Exa behalten ihre vorhandenen sichtbaren Freigaben. Sprache kann weder den Policy-Modus erhöhen noch eine Freigabe ersetzen.
4. „Mit mir reden“ startet eine sichtbare Sprachsitzung, auch über das Rechtsklick-Menü des Pets. In ihr werden bestätigte Äußerungen lokal beantwortet und Antworten lokal vorgelesen. Die Ausgabe lässt sich unterbrechen oder stummschalten. Während das Pet spricht, ist die Aufnahme pausiert, damit es sich nicht selbst als Nutzer hört. Webinhalte dürfen weder Aufnahme noch Sprachausgabe starten.

Die Betriebssystem-Mikrofonfreigabe und eine sichtbare IAP-Sitzungsfreigabe sind beide erforderlich. Audiopuffer bleiben flüchtig im Arbeitsspeicher und werden bei Abbruch, vollständigem Beenden oder Fehler verworfen. Beim Wechsel vom Hauptfenster zum Pet endet die aktuelle Aufnahme; eine neue beginnt erst durch eine sichtbare Pet-Aktion. IAP speichert keine Rohaufnahme. Ein Transkript wird erst als normale Chat-Nachricht gespeichert, wenn der Nutzer es sendet. Audio und nicht gesendete Transkripte gelangen nie zu Exa. Für eine ausdrücklich freigegebene öffentliche Exa-Suchfrage gelten weiterhin die Regeln aus Abschnitt 2 und 4.

### Laufzeit und 8-GB-Grenze

Mikrofonaufnahme, lokale Erkennung und Sprachausgabe laufen als begrenzte Hintergrundarbeit über typisierte Tauri-IPC; die Oberfläche darf dabei nicht blockieren. Sie nutzen keine Cloud, keinen Browser-Sprachdienst und keinen zusätzlichen ausgehenden Netzwerkzugriff. Sprachlaufzeiten und lokale Stimmen müssen als portable, separat abschaltbare Pakete samt Größenbudget geprüft werden. Auf schwacher Hardware werden Erkennung, große LLM-Inferenz und Sprachsynthese seriell geplant; nicht benötigte Laufzeiten geben ihren Speicher frei. Das kann Pausen zwischen Aufnahme, Antwort und Vorlesen verursachen und wird vor Freischaltung gemessen.

Das [verbindliche Architekturkonzept](../portabler-ki-agent-konzept.md) setzt STT auf T0 derzeit auf „aus“ und nennt es erst ab T1. **Hier ist die vom Nutzer gewählte Erweiterung:** Auf 8-GB-CPU-Rechnern darf Sprachsteuerung nur optional freigeschaltet werden, wenn ein physischer Test genug RAM-Reserve, brauchbare Erkennungsgeschwindigkeit und eine weiterhin bedienbare Oberfläche nachweist. Scheitert der Test, bleibt Spracheingabe auf T0 aus und Textbedienung funktioniert weiter. Diese Abweichung muss vor der Umsetzung in die verbindliche Referenz übernommen werden.

**Abnahme:** Ohne Mikrofonfreigabe wird nichts aufgenommen; Start und Stopp sind sichtbar; Erkennung, Antwort und Sprachausgabe bleiben lokal; Vorlesen lässt sich sofort unterbrechen; ein fehlerhaftes Transkript löst keine Werkzeugaktion aus; bestehende Freigaben werden durch Sprachbefehle nicht umgangen. Vollständiges Beenden stoppt Aufnahme, Erkennung und Ausgabe. Auf T0 wird die Funktion erst nach Messung aktiviert.

## 7. Bloub-Avatar als neue Hauptseite und Desktop-Pet

[Bloub](https://github.com/jeremy-prt/bloub) dient als technische Grundlage für den zentralen, animierten SVG-Avatar auf der Hauptseite. Das Projekt bietet eine zeitabhängig abtastbare Animations-Engine in `src/bot/`; seine sichtbare `BloubBot.vue`-Komponente und Demo verwenden Vue 3. IAP verwendet Svelte 5. Die Umsetzung übernimmt deshalb nur den benötigten, auf einen festen Upstream-Stand gepinnten Engine-Code und bindet ihn über eine Svelte-Komponente ein. Die Vue-App, Exportfunktionen, `mediabunny` und ein zusätzlicher UI-Framework-Laufzeitprozess gehören nicht in IAP. Alle benötigten Dateien werden lokal gebündelt; zur Laufzeit wird nichts nachgeladen. [Bloub-Architektur](https://github.com/jeremy-prt/bloub/blob/main/docs/architecture.md) · [Bloub-Paket](https://github.com/jeremy-prt/bloub/blob/main/package.json)

Die Hauptseite stellt den Avatar in den Mittelpunkt und zeigt daneben die Sprachbedienung und einen lesbaren Status. Sie ersetzt den Chat als Startansicht nach dem Entsperren. Der vollständige Gesprächsverlauf bleibt im Bereich **Chat**; Hauptseite und Chat greifen auf dieselbe Unterhaltung zu. Über **Chat** und **„Flow Version“** sind die anderen Arbeitsflächen direkt erreichbar. Die Hauptseite bleibt auch bei deaktivierter Animation als statische Avatar-Ansicht nutzbar.

Der Avatar zeigt nur tatsächliche App-Zustände: **bereit, hört zu, transkribiert, verarbeitet, spricht** (nur bei aktivierter lokaler Sprachausgabe), **wartet auf Freigabe, gestoppt und Fehler**. Die Animation darf keine Sicherheit oder Korrektheit eines KI-Ergebnisses behaupten. Ein Klick auf den Avatar selbst startet weder Mikrofon noch Werkzeuge; dafür bleiben beschriftete Bedienelemente zuständig.

Die [MIT-Lizenz](https://github.com/jeremy-prt/bloub/blob/main/LICENSE) deckt den Bloub-Code. Laut Projekt-README deckt sie **nicht** das von x.ai nachgebildete visuelle Design. IAP übernimmt deshalb die Animationstechnik, gestaltet Silhouette, Augen, Farben und Ausdruck aber als eigenständige IAP-Figur. Der Lizenzhinweis und die Herkunft des übernommenen Codes bleiben im Paket erhalten. Vor Auslieferung wird die konkrete Gestaltung auf ausreichende Eigenständigkeit geprüft. [Bloub-README](https://github.com/jeremy-prt/bloub#license)

Bei deaktivierten Animationen oder `prefers-reduced-motion` erscheint ein ruhiges SVG-Standbild mit lesbarem Textstatus. Unsichtbare oder minimierte Fenster halten den Animations-Takt an. Auf der 8-GB-CPU-Untergrenze wird gemessen, ob die Animation die Antwortgeschwindigkeit merklich senkt; falls ja, startet sie dort statisch. Die Bloub-Animationskomponente erhält nur Zustandsereignisse und hat selbst keine Mikrofon-, Bildschirm- oder Werkzeugrechte. Aufnahme und Verarbeitung übernimmt das IAP-Backend über `pa-policy`; die Gesprächsoberfläche zeigt die freigegebenen Inhalte an.

### Desktop-Pet beim Schließen des Hauptfensters

Das Schließen des IAP-Hauptfensters wechselt in ein kleines, transparentes Pet-Fenster **unten rechts auf dem Bildschirm, innerhalb der Arbeitsfläche oberhalb der Taskleiste**. Es verwendet denselben Bloub-Avatar und dieselbe Unterhaltung. Monitorwechsel und Bildschirm-Skalierung dürfen das Pet nicht außerhalb der sichtbaren Fläche platzieren. Transparenz, Platzierung und Verhalten über anderen Fenstern werden zuerst auf Windows geprüft; für Linux und macOS sind eigene Plattformtests nötig.

Das Rechtsklick-Menü des Avatars auf der Hauptseite und im Pet-Modus enthält:

- **IAP öffnen** – zeigt das Hauptfenster mit der bestehenden Sitzung.
- **Mit mir reden** – startet die sichtbare lokale Sprachsitzung; Aufnahme und Ausgabe sind stoppbar.
- **Bildschirm ansehen** – startet die unten beschriebene einzelne Bildschirmaufnahme.
- **Sprachsitzung stoppen / Sprachausgabe stummschalten** – beendet die Aufnahme beziehungsweise unterbricht das Vorlesen.
- **IAP vollständig beenden** – schließt auch das Pet und beendet alle zugehörigen Prozesse.

Beim Wechsel in den Pet-Modus stoppen Workflows und Agent-Flow-Ausführungen einschließlich ihrer Testprozesse kontrolliert. Kandidaten, Entwürfe und bisherige Ergebnisse bleiben erhalten; beim Wiederöffnen wird nichts automatisch fortgesetzt. Eine laufende Mikrofonaufnahme wird beendet. Das Pet verarbeitet danach nur ausdrücklich gestartete Gespräche oder einzelne Bildschirmaufträge. Es startet keine Hintergrund-Automatisierung.

**Hauptfenster schließen und vollständig beenden sind unterschiedliche Aktionen:** Ein sichtbares Pet setzt einen laufenden IAP-Prozess voraus. Ein Hinweis beim ersten Wechsel erklärt dieses Verhalten. Zusätzlich bietet ein sichtbares Tray-Symbol „IAP öffnen“ und „IAP vollständig beenden“. Tauri v2 stellt dafür Tray-Menüs bereit. Bei vollständigem Beenden werden Aufnahmen gestoppt, Puffer verworfen, Aufgaben beendet und der Vault sauber geschlossen; das Pet verschwindet ebenfalls. Es gibt keinen neuen Autostart oder separaten Dienst. [Tauri: System Tray](https://v2.tauri.app/learn/system-tray/)

Im untätigen Pet-Modus werden nicht benötigte Sprach-, Bild- und Inferenzlaufzeiten entladen. Das kleine Fenster benötigt weiterhin Speicher; beim nächsten Auftrag kann deshalb Ladezeit entstehen. Auf der 8-GB-Untergrenze wird auch dieser Leerlaufverbrauch gemessen. Ein gesperrter Vault erlaubt keine neue Sprach- oder Bildschirmverarbeitung.

### Bildschirm ansehen per Rechtsklick

1. Der Nutzer wählt **„Bildschirm ansehen“** und anschließend den gewünschten Bildschirm, ein Fenster oder einen Bereich. Diese Aktion erlaubt genau **eine Aufnahme** dieser Auswahl. Es gibt keine automatische Folgeaufnahme oder dauernde Bildschirmbeobachtung.
2. IAP zeigt sichtbar, dass die Aufnahme lokal ausgewertet wird. Der Nutzer kann dazu eine Frage eingeben oder sprechen. Das Ergebnis erscheint in derselben Unterhaltung und kann lokal vorgelesen werden.
3. Aufnahme und Bildverarbeitung gehen durch `pa-policy`. Das Bild wird nur vorübergehend im Arbeitsspeicher gehalten und nach Abschluss oder Abbruch freigegeben. Es wird weder als Bilddatei noch in Protokollen gespeichert. Übernommene Fragen und Antworten unterliegen der normalen Chat-Speicherung im Vault.
4. Bildschirmbilder und daraus erkannter Text gelten als private Daten. Sie gelangen **niemals an Exa**. Anweisungen in einem Screenshot sind unvertrauenswürdiger Inhalt und können weder Berechtigungen erteilen noch Werkzeuge starten.
5. „Ansehen“ erlaubt keine Maus- oder Tastatursteuerung und keine Änderungen an anderen Anwendungen. Die Antwort bezieht sich ausdrücklich auf die Aufnahme zum angezeigten Zeitpunkt; spätere Bildschirmänderungen kennt IAP erst nach einem neuen Auftrag.

**Technisches Tor:** Bloub ist die Animation, nicht die Bildanalyse. Die vorhandenen Textmodell-Dateien allein belegen keine funktionierende Bildverarbeitung. IAP braucht einen zum installierten Modell passenden lokalen Bildpfad und gegebenenfalls einen kompatiblen Bildprojektor (`mmproj`); auch der gebündelte `llama-server` muss diesen Pfad unterstützen. Modell, Projektor, Laufzeit und Ressourcenverbrauch werden gemeinsam geprüft. Eine geeignete lokale Kombination wird für das Feature genutzt, ohne im Konzept erneut eine Modellauswahl festzulegen. Fehlt sie, lautet der Status **„Bildschirmverständnis nicht eingerichtet“**; IAP darf keine erfundene Bildbeschreibung liefern. [llama.cpp: Multimodal-Unterstützung](https://github.com/ggml-org/llama.cpp/blob/master/docs/multimodal.md)

Das verbindliche Architekturkonzept setzt Vision auf T0 derzeit auf „aus“. Diese Grenze bleibt bis zu einem nachgewiesenen lokalen Betrieb und einer ausdrücklichen Anpassung der Referenz bestehen. Die vollständige Bildanalyse darf auf 8-GB-CPU-Rechnern daher nicht als garantiert verfügbar beworben werden. Aufnahmefehler, verweigerte Betriebssystemrechte und geschützte Inhalte werden verständlich angezeigt. Insbesondere können nicht alle Fenster oder Inhalte zuverlässig aufgenommen werden. [Microsoft: Bildschirmaufnahme](https://learn.microsoft.com/en-us/windows/apps/develop/media-authoring-processing/screen-capture)

**Abnahme:** Nach dem Entsperren erscheint die Avatar-Hauptseite. Chat und „Flow Version“ sind direkt erreichbar; ein Ansichtswechsel erhält die bestehende Unterhaltung. Hauptfenster schließen zeigt das Pet unten rechts und stoppt Flow-Ausführungen; vollständiges Beenden entfernt es und beendet die Prozesse. Bildschirmaufnahme erfolgt ausschließlich nach dem ausdrücklichen Menüauftrag, genau einmal und lokal; keine Bildschirmdaten gehen an Exa. Fehlende Bildunterstützung wird angezeigt. Die Zustände bleiben per Tastatur und Screenreader verständlich, funktionieren ohne Animation und Netzwerk und laden kein Vue-Runtime-Paket. Der zusätzliche Ressourcenverbrauch besteht die Messung auf der vorgesehenen Zielhardware.

## 8. Einbaufolge und offene Tore

1. Die Produktregeln für Exa, Worktrees, optionale T0-Sprachfunktionen, einzelne Bildschirmaufnahmen und den sichtbaren Pet-Betrieb werden vor Implementierung mit dem verbindlichen Architekturkonzept abgeglichen. Vision auf T0 bleibt ein gesondertes Freigabetor.
2. Sprachaufnahme, lokale Erkennung, Sprachausgabe, Unterbrechen und Speicherverhalten werden zuerst ohne Gesicht auf Windows gemessen. Danach folgen einzelne Bildschirmaufnahme und kompatible lokale Bildanalyse. Linux und macOS benötigen ihre jeweiligen Rechte- und Plattformtests.
3. Die Statusereignisse werden an das Bloub-basierte Svelte-Gesicht gebunden; dieses wird die Startseite nach dem Entsperren. Eine statische Variante ist von Anfang an Teil derselben Oberfläche.
4. Das kleine Pet-Fenster und sein Rechtsklick-Menü werden ergänzt. Geprüft werden mehrere Monitore, Taskleiste, Skalierung, Ressourcen im Leerlauf sowie der Unterschied zwischen Hauptfenster schließen und vollständig beenden, einschließlich sauberem Prozessende und Vault-Abschluss.
5. Agent Flow und Workflow-Builder werden als zwei Bereiche hinter dem einen Button „Flow Version“ in IAP eingebaut. Sprache und Gesicht ergänzen die normale App und bleiben auch außerhalb des Flow-Modus verfügbar. Der Wechsel zum Pet stoppt die Flow-Ausführung verlässlich.

Dieses Ergänzungskonzept beschreibt das Ziel und die Abnahmekriterien innerhalb von IAP. Es installiert keine Sprach- oder Bildlaufzeit und integriert noch keinen fremden Code.
