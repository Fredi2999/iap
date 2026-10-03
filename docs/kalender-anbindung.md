# Kalender-Anbindung (Apple, Google)

Stand: 2. Oktober 2026. Entwurf und Entscheidungen zum Auftrag „Kalenderzugriff auf Apple und Google“.

## Auftrag

Im Kalender-Tab lässt sich ein Apple- oder Google-Kalender verbinden oder auch nicht. IAP zieht die
Termine auf Knopfdruck, kann neue Termine eintragen, zeigt kommende Termine auf der Startseite und
nimmt im Air-Gap-Modus keine Verbindung auf.

## Entscheidungen

1. **Nur auf Klick, nie im Hintergrund.** Es gibt keinen Takt wie bei Gmail. „Aktualisieren“ und
   „Termin speichern“ sind die einzigen Auslöser. Bei eingeschaltetem Air Gap kommt kein Aufruf
   durch: die Policy lehnt ab, und `net.rs` sperrt als zweite Stufe im einzigen Code, der ins Netz sendet.
2. **Neuer Konnektor `Calendar` mit fester Host-Tabelle** (Änderung an Invariante 1, siehe unten):
   - Apple iCloud: `caldav.icloud.com` und die Server-Gruppen `pNN-caldav.icloud.com` (NN = Ziffern).
     Anmeldung mit Apple-ID und **app-spezifischem Passwort** (Basic über TLS). Kein OAuth nötig.
   - Google, nur Lesen, ohne Einrichtung bei Google Cloud: `calendar.google.com` (geheime iCal-Adresse).
   - Google, Lesen und Schreiben: `www.googleapis.com` und `oauth2.googleapis.com` (OAuth mit
     eigenem Client aus der eigenen Google-Cloud-Konsole; Anmeldung im System-Browser, Rückweg über
     `127.0.0.1`). Wird in Phase 2 gebaut.
3. **Schreiben = Termine anlegen.** Ändern und Löschen fremder Termine gibt es nicht (nicht beauftragt,
   und es wäre die riskantere Hälfte). Importierte Termine sind in IAP schreibgeschützt.
4. **Vault statt Klartext.** Zugangsdaten, Kalenderliste und der Terminspeicher liegen im verschlüsselten
   Tresor (`calendar.*`-Einstellungen). Die Oberfläche bekommt nie ein Passwort oder eine geheime Adresse zurück.
5. **Terminspeicher ersetzt sich bei jedem erfolgreichen Abruf.** Zeitfenster: 30 Tage zurück bis 180 Tage
   voraus. Schlägt der Abruf fehl, bleibt der alte Stand sichtbar und der Fehler steht an der Quelle.
6. **Serienttermine:** Apple liefert über CalDAV `expand` fertige Einzeltermine. Bei der geheimen
   Google-Adresse expandiert IAP selbst (Täglich, Wöchentlich, Monatlich, Jährlich mit Intervall,
   Anzahl, Ende, Wochentagen, Ausnahmen und Änderungen einzelner Termine). Nicht unterstützte Regeln
   werden nicht geraten: der erste Termin erscheint, die Quelle meldet „Regel nicht unterstützt“.
7. **Zeitzonen ohne Datenbank.** `VTIMEZONE` aus der Datei bestimmt den Versatz (Sommer-/Winterzeit
   über `RRULE`). Fehlt sie, gilt der Versatz des Rechners (vom Frontend übergeben).
8. **Anzeige.** Kalender-Tab: Verbindungen-Karte (verbinden, aktualisieren, trennen, Fehler),
   Fremdtermine mit Quellenname und Schloss, Auswahl des Zielkalenders im Termin-Dialog („Nur IAP“
   ist der Standard). Startseite: Karte „Anstehende Termine“ (nächste 7 Tage, IAP und verbundene
   Kalender). Der Wochenplaner behandelt Fremdtermine als belegt.

## Änderung an den Regeln

`AGENTS.md` Invariante 1 und Konzept 10.5 bekommen den Konnektor „Kalender“ in der Host-Tabelle,
ebenfalls nur nach ausdrücklicher Aktion des Nutzers und gesperrt bei Air Gap. Datenschutz: Beim
Abruf gehen Zugangsdaten an Apple bzw. Google (die der Nutzer selbst gewählt hat), beim Anlegen
gehen Titel, Zeit, Ort und Notiz des neuen Termins dorthin. Sonst verlässt nichts den Rechner.

## Nicht Teil dieser Änderung

Ändern oder Löschen von Fremdterminen, Einladungen und Teilnehmer, Erinnerungen, Aufgaben (VTODO)
aus der Cloud, Hintergrund-Abgleich, Microsoft/Outlook, beliebige CalDAV-Server.

## Ungeprüft

Gegen echte Apple- und Google-Konten wurde nichts getestet (es gibt hier keine Zugangsdaten und es
wird keine Anmeldung auf fremden Konten versucht). Die Protokollschicht ist mit nachgebauten
Serverantworten geprüft; der erste echte Abruf ist ein Live-Test des Nutzers (Anleitung im Handoff).
