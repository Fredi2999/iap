# Bloub (Animationskern)

Die TypeScript-Dateien in diesem Ordner stammen aus dem Projekt **Bloub** von
Jérémy Perret, Ordner `src/bot/`:

- Quelle: https://github.com/jeremy-prt/bloub
- Festgehaltener Stand: Commit `b4bb3c1b5f93c7b87a2e8d620f667c4093d97749` (17. August 2026)
- Lizenz: MIT, siehe `LICENSE` in diesem Ordner (Copyright (c) 2026 Jérémy Perret)

Übernommen wurden nur die Dateien, die der Animationskern braucht
(`decor`, `engine`, `expressions`, `eyefit`, `face`, `math`, `profiles`, `repere`,
`shape`, `skins`, `states`), unverändert. Nicht übernommen wurden die Vue-App,
die Exportfunktionen (`mediabunny`), die Zeitleiste und alle Tests. Der Kern ist
reines TypeScript ohne Vue oder andere Laufzeitpakete; zur Laufzeit wird nichts
nachgeladen.

## Hinweis zum Design

Laut README des Projekts deckt die MIT-Lizenz den **Code**, nicht das von x.ai
nachgebildete visuelle Design. IAP verwendet ausschließlich die
Animationstechnik und gestaltet die Figur eigenständig (siehe
`../../components/Bloub.svelte`): eigene Silhouette („Galet“ statt Kreis),
blaue IAP-Farbwelt statt Schwarz, eigene Augenwahl (Ausdruck „attentif“) und
eine eigene Zuordnung der Animationen zu den App-Zuständen. Vor einer
Auslieferung ist die Eigenständigkeit der Gestaltung zu prüfen.
