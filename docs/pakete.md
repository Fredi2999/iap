# Zusatzpakete von IAP (Flow Version, Sprache, Bildschirm)

Stand 2026-09-30. Alle Pakete liegen getrennt vom Kernsystem unter `AI\packs\<id>\`,
haben eine `PACK.toml` (Kennung, Version, Lizenz, Dateiliste mit Größe und SHA-256)
und sind in den Einstellungen einzeln abschaltbar. IAP prüft Größen bei jedem Start
der Funktion und die Hashes einmal je Sitzung. Fehlt ein Paket oder stimmt ein Hash
nicht, ist die Funktion sichtbar „nicht eingerichtet“; nichts wird stillschweigend
ersetzt. Gebaut werden die Pakete mit `packaging/windows/packs.ps1` aus den
heruntergeladenen Originaldateien (`%USERPROFILE%\IAPDev\packs`); das Skript lädt nichts.

| Paket | Zweck | Version | Lizenz | Größe (gemessen) |
|---|---|---|---|---|
| `whisper` | Spracherkennung (whisper.cpp `whisper-server`, Modell `ggml-base`, mehrsprachig) | b5130 | MIT | 151 MB |
| `piper` | Sprachausgabe (Piper, Stimmen de/en/es/fr; für Japanisch gibt es keine Stimme) | 2023.11.14-2 | MIT, Stimmen CC0 / CC-BY 4.0 | 278 MB |
| `vision` | Bildprojektor `mmproj` zu Gemma 4 E2B (Bildschirm ansehen) | mmproj-gemma-4-E2B-it-Q8_0 | Gemma-Nutzungsbedingungen | 531 MB |
| `git` | MinGit für Agent-Flow-Worktrees | 2.56.0 | GPL-2.0 | 91 MB |

Kernsystem ohne Pakete und Modelle: **81,9 MB** (Grenze 100 MB), vorher 77,9 MB.

## Zuwachs des Kernsystems (+4,0 MB, unter der 5-MB-Schwelle)

- `gix` 0.87.1 (nur `sha1`, keine Standardfeatures; lesender Zugriff auf Repositories) – größter Anteil.
- Tauri-Features `tray-icon` und `image-png` (Symbol im Infobereich, Pet-Fenster).
- `png` für Bildschirmaufnahmen im Arbeitsspeicher (schon im Baum, jetzt direkt genutzt).
- Neuer Code: Job-Queue, Policy-Erweiterungen, Workflow-Runner, Audio, Bildschirm, Agent Flow.
- **Kein** zusätzlicher HTTP-/TLS-Client: Der einzige ausgehende Aufruf (Exa) läuft über
  WinHTTP des Betriebssystems mit dem bereits vorhandenen `windows`-Crate. Alternative wäre
  `ureq` oder `reqwest` mit rustls/ring gewesen (deutlich größer, plus eingebettete Zertifikate).
  Nachteil: derzeit nur Windows; Linux/macOS (Phase 4) brauchen dort einen eigenen Client.

## Netzwerk

Die einzige Ausnahme zum Offline-Grundsatz ist `POST https://api.exa.ai/search` und
`/contents` pro ausdrücklich freigegebenem Workflow-Lauf (Konzept 10.5). Pakete werden nie
aus dem Netz nachgeladen.
