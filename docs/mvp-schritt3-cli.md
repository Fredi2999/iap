# MVP-Abnahme bis Schritt 3: lokaler CLI-Chat

Stand: 15. September 2026. Dieses Dokument protokolliert den vereinbarten
Haltepunkt nach `pa-launcher`, `pa-vault` und `pa-inference`. `pa-core`, die
Tauri-Oberfläche und das endgültige USB-Paket sind noch nicht gebaut.

## Verwendete Artefakte

| Artefakt | Tatsächlicher Wert |
|---|---:|
| Standardmodell | Gemma 4 E2B Instruct Q4_K_M |
| GGUF-Größe | 3.106.738.272 Byte |
| GGUF SHA-256 | `740185b21d22ceb83a11c3aa62ad5842ef32c70f6096d756bbee85a1e4ec34b8` |
| llama.cpp | Build b10930, Commit `56381e407` |
| Manifest | 33 Dateien, 3.152.509.382 verifizierte Byte |
| Release-Launcher | 5.550.592 Byte |
| Release-Launcher SHA-256 | `aa930513cdd420aae017706044d66c57d2b1832684c3e3c2c86c85c79d2231bc` |
| Kernbundle ohne Modell | 51.326.052 Byte |

Das Kernbundle umfasst Release-Launcher, manifestierten llama.cpp-Runtime und
Manifest. Es bleibt unter dem Konzeptbudget von 100 MB. Das Modell liegt sowohl
im Projekt unter `AI/models/` als auch als inhaltsadressierte, bei jedem Start
erneut geprüfte SSD-Kopie unter
`%LOCALAPPDATA%/PortableAI/model-cache/<sha256>`.

## Konkrete Testmaschine

| Feld | Laufzeitmessung |
|---|---|
| CPU | AMD Ryzen 7 PRO 250 w/ Radeon 780M Graphics |
| Physische Inferenzthreads | 7 (`physical_cores - 1`) |
| Gesamter RAM | 27,64 GiB |
| Freier RAM bei den Läufen | 12,03 bis 12,30 GiB |
| Erkannter Tier | T2, ausdrücklich nicht hardwarevalidiert |
| Kontext der Abnahme | 2.048 Token |
| KV-Cache | f16 |
| Berechnete GPU-Layer | 35 |
| Tatsächliche GPU-Layer | 0, weil der gepinnte Phase-0-Runtime nur CPU-Backends enthält |

Der T2-Wert ist das Ergebnis der Formel, keine neue T2-Hardwarevalidierung. Die
aus Phase 0 übernommenen Grenzen bleiben: T0 bei 8 bis unter 16 GiB mit maximal
8K Kontext; T1 ab 16 GiB mit 16K Standard und optional höchstens 32K. T2/T3
bleiben bis zu Messungen auf passenden Rechnern vorläufig.

## Reale Chatmessungen

Alle Werte stammen aus der Ausgabe des echten `llama-server`, nicht aus einem
Mock. `nach Prompt` ist `Zeit bis erstes Delta - timings.prompt_ms`.

| Lauf | Modell laden | Erstes Delta | Promptzeit | Nach Prompt | Prompt Token/s | Generierung Token/s | Ergebnis |
|---|---:|---:|---:|---:|---:|---:|---|
| erster kurzer Chat | 3,296 s | 0,578 s | 572,624 ms | 0,005 s | 129,230 | 15,208 | `2 plus 2 ist 4.` |
| nach `/reload` | 3,157 s vor Reload; Reload bereit innerhalb des 30-s-Messfensters, Einzelzeit nicht erfasst | 5,463 s | 5.453,689 ms | 0,010 s | 129,454 | 16,941 | `Reload erfolgreich.` |
| nach hartem Serverende | automatischer Neustart innerhalb des 30-s-Messfensters, Einzelzeit nicht erfasst | 5,872 s | 5.854,916 ms | 0,017 s | 126,390 | 15,163 | `Neustart erfolgreich.` |
| final, Reasoning explizit aus | 3,055 s | 0,464 s | 460,334 ms | 0,004 s | 147,719 | 16,869 | `Schritt 3 funktioniert schnell.` |
| final, Server im Leerlauf hart beendet | automatischer Neustart vor dem Request | 0,776 s | 770,968 ms | 0,005 s | 129,707 | 18,245 | `Neustart ohne verlorenen Auftrag.` |

Die drei früheren Läufe erfüllten das Kriterium "erstes Token höchstens drei
Sekunden nach der Prompt-Verarbeitung" mit 0,005 s, 0,010 s und 0,017 s. Die
beiden finalen Läufe mit explizit deaktiviertem Reasoning erfüllten es mit
0,004 s und 0,005 s. Die gesamte Zeit bis zum ersten Token steigt mit dem zu
verarbeitenden Gesprächsverlauf; sie darf nicht mit dem geforderten Abstand
*nach* Promptverarbeitung verwechselt werden.

Ein zusätzlicher frischer Realtest deckte zunächst eine Verletzung auf: Mit dem
llama.cpp-Default `reasoning=auto` betrug der Abstand nach Promptverarbeitung
10,455 s (erstes Delta 10,880 s, Prompt 425,103 ms). Der Server hatte vor dem
sichtbaren Text interne Reasoning-Tokens erzeugt. Der MVP startet den gepinnten
Runtime deshalb nun ausdrücklich mit `--reasoning off`. Die beiden anschließenden
Messungen ergaben 0,004 s und 0,005 s. Der fehlgeschlagene Lauf wird hier bewusst
nicht aus der Messhistorie entfernt.

## Abbruch, Neustart und Modellwechsel

- Ein Ctrl+C noch vor dem ersten Delta beendete den Request und speicherte den
  Zustand als `aborted`.
- Ein zweiter Test wurde mitten in einer langen, bereits sichtbaren Antwort
  nach Listenpunkt 32 abgebrochen. Der angezeigte Teiltext blieb erhalten und
  wurde als `aborted` in SQLCipher gespeichert. Abschluss-Timings sind bei einem
  bewusst geschlossenen Stream nicht vorhanden und werden daher als "nicht
  gemessen" ausgewiesen.
- Der erste reale Ctrl+C-Versuch deckte auf, dass der Server das Konsolensignal
  des Launchers erbte. `pa-inference` startet ihn nun in einer eigenen Windows-
  Prozessgruppe. Der wiederholte Realtest bestand.
- `/reload` beendete den alten Server, wartete auf dessen Exit, lud dieselbe
  installierte und geprüfte Modelldatei neu und meldete erst nach `/health`
  Bereitschaft. Der Launcherprozess wurde dabei nicht neu gestartet.
- Im ersten Realtest wurde `llama-server` mit PID 30528 hart beendet. Damals ging
  noch ein Request mit Windows-Fehler 10061 verloren; der direkte Folgerequest
  antwortete nach dem Restart erfolgreich. Nach der Review-Korrektur wurde PID
  7496 im Leerlauf hart beendet. Der Supervisor startete den Server nun *vor*
  dem nächsten Request neu; genau dieser erste Auftrag antwortete korrekt mit
  `Neustart ohne verlorenen Auftrag.`

## Vault-Befunde

| Prüfung | Tatsächlicher Befund |
|---|---|
| SQLCipher- und SQLite-Integrität beim Wiederöffnen | bestanden |
| Portabler Snapshot des finalen Tests | 24.576 Byte |
| Erste 16 Dateibytes | `E4B08941B7529AAE9881DA7CA12725ED` |
| Klarer SQLite-Header vorhanden | nein |
| Chattext `Neustart ohne verlorenen Auftrag` im Binärfile auffindbar | nein |
| Hot-Copy-Dateien nach sauberem Beenden | 0 |
| Verbliebene `llama-server`-Prozesse | 0 |

Ein Hintergrundworker synchronisiert einen dirty Vault nach exakt fünf Minuten
atomar zurück und beim Beenden sofort. Die automatisierten Vault-Tests decken
zusätzlich Fehler vor Flush/Rename, fehlendes Medium, die automatische
Dirty-Markierung sowie auseinanderlaufende portable und Host-Stände ab. Jeder
Write erhöht eine verschlüsselte monotone Generation und setzt eine neue
zufällige Revisions-ID. Dadurch werden auch getrennte Zweige mit gleich hohem
Zähler erkannt; frühere Vaults ohne Revisions-ID erhalten beim schreibgeschützten
Prüfen einen logischen Inhaltsfingerprint. Bei jeder Abweichung muss der Benutzer
Host oder Stick ausdrücklich wählen. Vor dem Umschalten werden *alle* nicht
gewählten eindeutigen Stände atomar und inhaltsadressiert unter `conflicts/`
gesichert. Der normale Rücksync verändert diese Konfliktkopien nicht.

Der 32-Byte-SQLCipher-Schlüssel wird ohne hexadezimale Stringkopie an
`sqlite3_key` übergeben und beim Drop überschrieben. `cipher_memory_security`
bleibt in diesem MVP ausgeschaltet: Der reale nicht erhöhte Windows-Prozess
konnte die von SQLCipher verlangte `VirtualLock`-Operation nicht zuverlässig
ausführen. Die Datenbankseiten und alle Snapshots bleiben davon unabhängig
SQLCipher-verschlüsselt; der abgeleitete Schlüssel liegt während der Laufzeit
zwangsläufig im Prozessspeicher.

Die Abschlussreview deckte Datenverlustrisiken auf, die vor diesem Haltepunkt
behoben wurden: Recovery beruht nicht mehr auf Dateizeitstempeln, erkennt auch
gleich hohe divergente Revisionen, verlangt eine bewusste Wahl und erhält bei
Mehrfachkonflikten jeden nicht gewählten Zweig; mutable Repository-Zugriffe
markieren vor der Operation automatisch `dirty`; und Transportfehler geben
bereits gestreamten Teiltext an die Persistenzgrenze zurück. Zusätzlich ist
`[DONE]` jetzt ein verpflichtender SSE-Abschluss, Ctrl+C wird bereits während
Status-/Header-Wartezeiten geprüft und die Servergesundheit wird vor jedem
Auftrag kontrolliert. Die abschließende unabhängige Re-Review fand danach keine
kritischen oder wichtigen Fehler mehr im geprüften Schritt-3-Scope.

## Integrität und Offline-Grenze

`AI/bin/manifest.json` enthält Größe und SHA-256 aller 31 Runtime-Dateien,
des Modelldeskriptors und des GGUF. Der reale Start hat alle 33 Einträge und
3.152.509.382 Byte geprüft. Der Inferenzclient besitzt keinen URL- oder DNS-Typ,
sondern verbindet strukturell nur mit `127.0.0.1`. Der Child startet mit
`--offline --host 127.0.0.1`, zufälligem Port und 256-Bit-Zugriffstoken; Proxy-,
Hugging-Face-, Agent-, MCP- und Werkzeug-Umgebungsvariablen werden entfernt.

Eine externe Netzwerkmonitor-Messung wurde an diesem Haltepunkt **nicht
durchgeführt**. Deshalb wird "kein einziger ausgehender Netzwerkaufruf" noch
nicht als extern gemessen bestanden bezeichnet. Diese Abnahme gehört zum
fertigen Windows-Paket in Schritt 6.

## Build- und Testnachweis

Der Release-Build musste wegen des `ü` im realen Projektpfad über den bereits
geprüften Junction-Pfad `%TEMP%/portableai-usb-ascii` erfolgen. Ohne diesen
ASCII-Pfad meldete der OpenSSL-Generator fehlende Quelldateien. Der Build über
den Junction-Pfad endete mit Exitcode 0. Linkerhinweise zu nicht mitgelieferten
`ossl_static.pdb` betreffen nur Debugsymbole der statisch gelinkten
OpenSSL-Objekte.

```text
cargo build -p pa-launcher --release --offline
RELEASE_BUILD_EXIT=0

cargo test --workspace --offline
WORKSPACE_TEST_EXIT=0
60 bestanden, 0 fehlgeschlagen, 1 bewusst ignorierter Real-GGUF-Test

cargo clippy --workspace --all-targets --all-features --offline -- -D warnings
CLIPPY_EXIT=0
```

## An diesem Haltepunkt noch nicht durchgeführt

- Start auf einem realen 8-GB-Windows-Rechner ohne Administratorrechte
- hartes Beenden des gesamten Launchers in einem manuellen Realtest; die
  entsprechenden Recovery- und Fehlergrenzen sind automatisiert getestet
- physisches Ziehen und Wiederanstecken des USB-Sticks während des Betriebs;
  die Pending-Media-Strecke ist automatisiert getestet
- externer Netzwerkmonitor
- Tauri-/Svelte-Oberfläche und das endgültige Stick-Verzeichnis aus Schritt 6

Diese Punkte werden nicht als bestanden geschätzt. Der vereinbarte technische
Haltepunkt nach Schritt 3 ist erreicht; der nächste Auftragsschritt ist
`pa-core`, danach UI und Windows-Paket.
