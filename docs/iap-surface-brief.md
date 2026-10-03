# IAP surface brief

Scope: vollständige Desktop-Oberfläche mit Chat als Referenzansicht. Nutzer arbeiten lokal mit einem verschlüsselten Vault und einem lokalen Modell. Die Hauptaktion ist eine Nachricht oder ein konkreter Arbeitsauftrag. Bestehende Funktionen und Policy-Grenzen bleiben erhalten. Die gewählte Komposition ist `.impeccable/mocks/signal-c.png`; Text im Bild ist nur ein Layoutbeispiel und wird nicht als Beispieldatum in den Produktpfad übernommen.

## Direction contract

**THESIS:** Eine ruhige, zentrierte Arbeitskonsole bündelt Gespräch und Eingabe. Häufige Ziele sind direkt erreichbar; weitere Bereiche öffnen sich über eine klare Bereichsauswahl. Die dauerhaften doppelten Seitenleisten verschwinden.

**OWN-WORLD:** Fast schwarzer Grund, abgestufte neutrale Grautöne, Systemschrift, dünne Kanten. Liquid Glass markiert nur bedienbare Schichten: Eingabe, Dock, einzelne Schalter. Keine umlaufenden Wellen, Randmottos, starken Glows oder gestapelten hellen Glasflächen. Monochrome SVG-Piktogramme.

**STORY:** Der Nutzer erkennt IAP, den lokalen/offline Zustand und den Vault-Status, findet das aktuelle Gespräch, liest eine Antwort ungestört und kann sofort weiterschreiben. Verlauf und seltene Bereiche sind einen klar beschrifteten Schritt entfernt.

**FIRST VIEWPORT:** Bei etwa 1586 × 992 px liegt oben ein schmaler Statusstreifen. Die Konsole nutzt rund 80 % Breite, beginnt unter dem Streifen und hält Titel/Verlauf in einer leisen Kopfzeile. Nachrichten stehen als lesbare Textblöcke ohne laute Blasen. Die breite Glaseingabe sitzt am unteren Konsolenrand, darunter ein kompaktes Dock. Die Senden-Taste ist die stärkste Aktion.

**FORM:** Gewählt wurde die zentrierte Konsole der Signalraum-Richtung, Form sechs der sieben geprüften Strukturen, Seed `820d948b`. Der Nutzer wählte Komposition C und verlangte anschließend ausdrücklich die ruhigere Apple-Design-Fassung. Der Markenmoment aus Wordmark und ScanlineOverlay bleibt auf den Start beschränkt; LatticeLoader, GlowFrame, FrostedPanel, Dock und PromptBar werden als Svelte-Bausteine funktional eingebunden.

**FINISH:** unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
