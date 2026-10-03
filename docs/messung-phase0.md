# Messung Phase 0

## Zweitmodell Qwen2.5-Coder-7B-Instruct-abliterated Q5_K_M (2026-10-02)

**Größer und langsamer als das Standardmodell.** Die Datei liegt nur auf dem Stick
(`E:\AI\models`, wie Qwen3 und Ministral); im Projekt liegt der Deskriptor
`AI/models/qwen25-coder-7b-abl-q5-k-m.model.toml`, byteidentisch zur Stick-Kopie.

| Datei | Bytes | SHA-256 | Quelle |
|---|---:|---|---|
| `Qwen2.5-Coder-7B-Instruct-abliterated-Q5_K_M.gguf` | 5.444.832.096 | `a0e62cdf3348a9f680adae4abd2785508b2a06f009e815271cd512c11d43281b` | huggingface.co/bartowski/Qwen2.5-Coder-7B-Instruct-abliterated-GGUF (Quantisierung von huihui-ai/Qwen2.5-Coder-7B-Instruct-abliterated), Apache-2.0 laut Modellkarte und GGUF-Metadaten |

Der Hash wurde nach dem Download gegen den von Hugging Face gemeldeten Wert geprüft.
Wie die anderen optionalen Modelle steht die Datei nicht im Manifest; der Hash wird beim
Kopieren in den SSD-Cache geprüft. Beim ersten Wählen braucht der Host dafür 5,07 GiB freien
Platz in `%LOCALAPPDATA%\PortableAI\model-cache`.

Messung mit dem gebündelten `llama-server` (b10930) auf dem Entwicklungs-PC (AMD Ryzen 7 PRO
250, CPU-only), 7 Threads, 8192 Kontext, ein Slot, f16-KV. Beide Modelle liefen mit demselben
Skript und denselben Flags (`--reasoning off`, `--cache-ram 64`, sonst wie `ServerConfig`),
Greedy-Sampling, Prompt-Cache aus.

| Messgröße | Qwen2.5-Coder 7B Q5_K_M | Gemma 4 E2B Q4_K_M |
|---|---:|---:|
| Antworten, Token/s (88 bis 89 Token) | 5,9 bis 6,3 (3 Läufe) | 16,5 bis 16,8 (2 Läufe) |
| Prompt-Verarbeitung, rund 2000 Token | 11,3 Token/s (2287 Token in 203 s) | 94,2 Token/s (2042 Token in 22 s) |
| Spitzen-RSS nach Ein-Token-Anfrage, f16 | 5.620.867.072 bis 5.622.251.520 Byte (4 Läufe) | 2.795.773.952 bis 2.796.060.672 Byte (2 Läufe) |
| Spitzen-RSS, q8_0-KV | 5.401.575.424 bis 5.401.636.864 Byte (2 Läufe) | nicht gemessen |
| KV bei 8192 Token (Serverprotokoll) | f16 448 MiB = 57.344 Byte/Token; q8_0 238 MiB = 30.464 Byte/Token; kein fester Anteil | siehe Deskriptor |
| Ladezeit bis `/health` | 3 bis 19 s (6 Läufe) | 5 bis 12 s (2 Läufe) |

Daraus folgt auf diesem PC: etwa 2,7-mal langsamere Antworten, etwa 8-mal langsamere
Prompt-Verarbeitung und doppelter Speicherbedarf. Die Prompt-Zahlen mit 30 bis 57 Token
(Qwen 12 bis 15, Gemma 75 bis 83 Token/s) sind für diesen Vergleich zu kurz und nicht belastbar.

Der Plan aus `resources.rs` (unverändert eingebunden, **berechnet, nicht gemessen**) liefert
für simulierte CPU-Rechner:

| Rechner (gesamt / frei) | Tier | Modellbudget | Kontext | Warnung |
|---|---|---:|---:|---|
| 8 GiB / 4 GiB | T0 | 1.610.612.736 Byte | 8192 | Spitzen-RSS übersteigt das Budget |
| 8 GiB / 6 GiB | T0 | 2.899.102.924 Byte | 8192 | Spitzen-RSS übersteigt das Budget |
| 16 GiB / 10 GiB | T1 | 5.476.083.302 Byte | 16384 | Spitzen-RSS übersteigt das Budget |
| 32 GiB / 20 GiB | T1 | 11.918.534.246 Byte | 16384 | keine |

**Grenzen und offene Punkte (nichts davon wurde gemessen oder geprüft):**

- Ein physischer 8-GB-Rechner wurde mit diesem Modell nicht getestet. Der Planer **warnt** dort
  nur; er lehnt das Modell nicht ab und verkleinert den Kontext nicht.
- `select_model` (`app/src-tauri/src/lib.rs`) verwirft die Plan-Warnungen beim Modellwechsel; sie
  erreichen die Oberfläche nur beim Start des Standardmodells. Der Hinweis „Langsamer:“ im
  Anzeigenamen ist deshalb derzeit die einzige Warnung in der App.
- Das Modell wurde nicht in der grafischen App ausprobiert (Entsperren nötig). Die gebaute
  `iap.exe` auf dem Stick kennt es über die Familie `qwen3` (ChatML-Adapter), ohne Neubau.
- Plausibilitätsprobe mit einer einzigen Code-Aufgabe: kohärente deutsche Antwort,
  `finish_reason` `stop`, aber der verlangte Code fehlte in der Antwort. Qualität von
  Werkzeugaufrufen und strukturiertem JSON wurde nicht geprüft (Konzept 3.2 warnt bei
  abliterierten Modellen davor).
- Der neue Test `loads_qwen25_coder_install_descriptor_with_its_slowness_hint` in
  `crates/pa-launcher/tests/model_descriptor.rs` konnte im Workspace nicht laufen: dem lokalen
  Cargo-Cache dieses PCs fehlen die `gix`-Pakete (offline nicht auflösbar). Dieselben Aussagen
  wurden in einem isolierten Prüfprojekt gegen die unveränderten Quelldateien `model_config.rs`
  und `resources.rs` sowie den echten `AdapterKind` bestanden (4 von 4).
- Die GPU-Layer-Bytes stammen aus den GGUF-Metadaten (Python-Nachbau des Parsers
  `inspect_gguf_layers`, an den Gemma-Deskriptor angelegt: identisch).

## Nachmessung der aktuell gebündelten Llama-GGUF (2026-09-26)

Die derzeit gebündelte `AI/models/llama32-3b-abl-q4.gguf` hat 2.241.004.512 Byte
und SHA-256 `3a8ad1732ca90048bc5ecf44dcb3b789469e8a68d389ab79516853bf0d27ad34`.
Sie ist **nicht** die weiter unten im Risiko-1-Versuch gemessene Llama-Datei.
Mit dem gebündelten `llama-server.exe` auf Windows 11, CPU-only, 7 Threads,
`ctx=8192`, `np=1` und je einer Ein-Token-Anfrage ergaben sich:

| KV-Typ | Servermeldung für KV | Byte pro Token | Fester KV-Anteil | Peak Working Set |
|---|---:|---:|---:|---:|
| f16 | 896 MiB | 114.688 | 0 | 4.401.922.048 Byte |
| q8_0 | 476 MiB | 60.928 | 0 | 3.961.581.568 Byte |

Die KV-Größen stammen direkt aus den `llama_kv_cache`-Protokollzeilen; der Peak
ist `PeakWorkingSet64` nach `/health` und der Anfrage. Die Layer-Offload-Werte
im Deskriptor sind weiterhin grobe Richtwerte. Ein physischer 8-GB-Rechner
wurde für diese GGUF noch nicht gemessen.

> Stand: Risiken 1 und 2 abgeschlossen; Risiko 3 teilweise durchgeführt (1/3 Rechner).

## Testmaschine

| Merkmal | Gemessener Wert |
|---|---|
| CPU | AMD Ryzen 7 PRO 250 w/ Radeon 780M Graphics |
| Physische Kerne | 8 |
| Logische Prozessoren | 16 |
| RAM gesamt | 29673291776 Bytes (28298.66 MiB) |
| RAM frei bei Start | 14559371264 Bytes (13884.90 MiB) |
| Betriebssystem | Windows 11 Pro |
| llama.cpp | version: 0.4.0-dev (build 10930, commit 56381e407)  built with Clang 20.1.8 for Windows x86_64 |
| CPU-Threads | 7 |
| RAM-Sicherheitsreserve | 1610612736 Bytes (1536.00 MiB) |

## Risiko 1 – Modellverhalten auf Minimalhardware

### Eingabedateien

| Modell | Bytes | SHA-256 | SSD-Pfad | USB-Pfad |
|---|---:|---|---|---|
| gemma-4-E2B-it-Q4_K_S.gguf | 3043934304 | `96ee1ee1d562be52dbbc4c12694ed67de8c6ae09451d420fed7d067e552c25cb` | `\\?\<Projektordner>\tools\pa-probe\.runtime\models\gemma-4-E2B-it-Q4_K_S.gguf` | `\\?\D:\USB\tools\pa-probe\.runtime\models\gemma-4-E2B-it-Q4_K_S.gguf` |
| gemma-4-E2B-it-Q4_K_M.gguf | 3106738272 | `740185b21d22ceb83a11c3aa62ad5842ef32c70f6096d756bbee85a1e4ec34b8` | `\\?\<Projektordner>\tools\pa-probe\.runtime\models\gemma-4-E2B-it-Q4_K_M.gguf` | `\\?\D:\USB\tools\pa-probe\.runtime\models\gemma-4-E2B-it-Q4_K_M.gguf` |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | 2019377472 | `e1f2b428e316d8bd1cb5ca13e9649329e299d951fbcee6b3cd0e9f1d646c9fa7` | `\\?\<Projektordner>\tools\pa-probe\.runtime\models\Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf` | `\\?\D:\USB\tools\pa-probe\.runtime\models\Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf` |

### Ladezeit und Resident-Speicher

RSS ist der Windows Working Set. Gemessen wurde nach `/health` und nach einer Ein-Token-Anfrage bei 8192 Kontext-Token.

| Modell | Speicherort | Modus | Lauf | Ladezeit s | RSS bereit MiB | RSS nach Anfrage MiB | Peak-RSS MiB | Status |
|---|---|---|---:|---:|---:|---:|---:|---|
| gemma-4-E2B-it-Q4_K_S.gguf | usb | mmap | 1 | 20.307 | 2725.14 | 2728.62 | 2728.62 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | ssd | mmap | 1 | 4.258 | 2724.98 | 2728.46 | 2728.46 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | ssd | mmap | 2 | 3.649 | 2724.89 | 2728.38 | 2728.38 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | usb | mmap | 2 | 3.748 | 2725.67 | 2729.15 | 2729.15 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | usb | mmap | 3 | 3.646 | 2725.16 | 2728.65 | 2728.65 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | ssd | mmap | 3 | 3.250 | 2725.00 | 2728.48 | 2728.48 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | usb | dio | 1 | 3.651 | 3291.84 | 3295.31 | 3295.31 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | ssd | dio | 1 | 3.644 | 3291.17 | 3294.64 | 3294.64 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | ssd | dio | 2 | 4.054 | 3291.97 | 3295.43 | 3295.43 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | usb | dio | 2 | 4.248 | 3291.73 | 3295.20 | 3295.20 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | usb | dio | 3 | 4.060 | 3291.78 | 3295.25 | 3295.25 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | ssd | dio | 3 | 3.960 | 3291.61 | 3295.08 | 3295.08 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | ssd | mmap | 1 | 4.252 | 2661.52 | 2664.99 | 2664.99 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | usb | mmap | 1 | 18.168 | 2661.59 | 2665.07 | 2665.07 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | usb | mmap | 2 | 3.132 | 2661.45 | 2664.93 | 2664.93 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | ssd | mmap | 2 | 2.940 | 2661.73 | 2665.20 | 2665.20 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | ssd | mmap | 3 | 3.037 | 2661.77 | 2665.26 | 2665.26 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | usb | mmap | 3 | 3.042 | 2661.30 | 2664.78 | 2664.78 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | ssd | dio | 1 | 3.643 | 3351.50 | 3354.97 | 3354.97 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | usb | dio | 1 | 3.454 | 3351.71 | 3355.18 | 3355.18 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | usb | dio | 2 | 3.445 | 3352.05 | 3355.51 | 3355.51 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | ssd | dio | 2 | 3.352 | 3351.55 | 3355.01 | 3355.01 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | ssd | dio | 3 | 3.350 | 3351.45 | 3354.91 | 3354.91 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | usb | dio | 3 | 3.340 | 3351.71 | 3355.18 | 3355.18 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | usb | mmap | 1 | 13.041 | 4197.52 | 4199.42 | 4199.42 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | ssd | mmap | 1 | 2.843 | 4197.47 | 4199.38 | 4199.38 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | ssd | mmap | 2 | 2.427 | 4197.38 | 4199.28 | 4199.28 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | usb | mmap | 2 | 2.325 | 4197.39 | 4199.29 | 4199.29 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | usb | mmap | 3 | 2.229 | 4197.38 | 4199.29 | 4199.29 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | ssd | mmap | 3 | 2.737 | 4197.11 | 4199.01 | 4199.01 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | usb | dio | 1 | 2.334 | 2897.63 | 2899.54 | 2899.54 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | ssd | dio | 1 | 2.226 | 2898.04 | 2899.95 | 2899.95 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | ssd | dio | 2 | 2.440 | 2897.29 | 2899.20 | 2899.20 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | usb | dio | 2 | 2.226 | 2897.94 | 2899.84 | 2899.84 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | usb | dio | 3 | 2.232 | 2897.92 | 2899.83 | 2899.83 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | ssd | dio | 3 | 2.232 | 2898.50 | 2900.40 | 2900.40 | gemessen |

### Durchsatz

llama-bench, fünf Wiederholungen je Zeile, CPU-only.

| Modell | Art | Token | Mittel Token/s | Standardabweichung | Einzelwerte Token/s | Status |
|---|---|---:|---:|---:|---|---|
| gemma-4-E2B-it-Q4_K_S.gguf | prompt | 512 | 137.771 | 2.162 | 140.098, 135.779, 135.188, 138.664, 139.128 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | prompt | 2048 | 126.622 | 0.515 | 126.121, 127.363, 126.736, 126.749, 126.140 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | prompt | 8192 | 101.437 | 5.697 | 104.140, 98.465, 96.622, 97.751, 110.208 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | generation | 128 | 25.243 | 0.865 | 23.796, 25.125, 25.972, 25.638, 25.685 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | prompt | 512 | 129.500 | 4.910 | 123.861, 124.854, 133.696, 130.688, 134.400 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | prompt | 2048 | 121.862 | 0.612 | 122.505, 120.941, 121.742, 121.793, 122.327 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | prompt | 8192 | 99.703 | 0.720 | 99.593, 99.327, 100.603, 100.212, 98.780 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | generation | 128 | 24.486 | 0.352 | 23.941, 24.890, 24.585, 24.616, 24.398 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | prompt | 512 | 101.069 | 0.857 | 101.926, 100.301, 100.446, 100.602, 102.071 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | prompt | 2048 | 88.254 | 0.436 | 87.865, 88.635, 88.444, 88.615, 87.710 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | prompt | 8192 | 66.883 | 0.390 | 67.371, 66.568, 66.831, 67.186, 66.462 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | generation | 128 | 22.619 | 0.413 | 21.881, 22.826, 22.823, 22.788, 22.776 | gemessen |

### Kontextgrenze

Jeder Wert stammt aus einem frischen Prozess und einer tatsächlichen Eingabe von Kontext minus 128 Token. Größere Kandidaten werden nach dem ersten Fehlschlag oder Sicherheitsstopp nicht ausgeführt.

| Modell | KV-Cache | Kontext-Token | Verarbeitung s | RSS MiB | Peak-RSS MiB | Status |
|---|---|---:|---:|---:|---:|---|
| gemma-4-E2B-it-Q4_K_S.gguf | f16 | 8192 | 82.154 | 2861.24 | 2861.24 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | f16 | 16384 | 201.099 | 2918.74 | 2918.74 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | f16 | 32768 | 514.172 | 3030.68 | 3030.68 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | f16 | 65536 | 1641.885 | 3262.45 | 3264.45 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | — | — | — | — | — | nicht durchgeführt: cannot read loopback response from 127.0.0.1:59767: Ein Verbindungsversuch ist fehlgeschlagen, da die Gegenstelle nach einer bestimmten Zeitspanne nicht richtig reagiert hat, oder die hergestellte Verbindung war fehlerhaft, da der verbundene Host nicht reagiert hat. (os error 10060) |
| gemma-4-E2B-it-Q4_K_S.gguf | q8_0 | 8192 | 125.710 | 2830.16 | 2830.16 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | q8_0 | 16384 | 372.982 | 2865.86 | 2865.86 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | q8_0 | 32768 | 1295.982 | 2933.50 | 2933.50 | gemessen |
| gemma-4-E2B-it-Q4_K_S.gguf | — | — | — | — | — | nicht durchgeführt: cannot read loopback response from 127.0.0.1:61554: Ein Verbindungsversuch ist fehlgeschlagen, da die Gegenstelle nach einer bestimmten Zeitspanne nicht richtig reagiert hat, oder die hergestellte Verbindung war fehlerhaft, da der verbundene Host nicht reagiert hat. (os error 10060) |
| gemma-4-E2B-it-Q4_K_S.gguf | — | — | — | — | — | übersprungen: larger contexts skipped after prior failure for q8_0 |
| gemma-4-E2B-it-Q4_K_M.gguf | f16 | 8192 | 84.780 | 2797.16 | 2797.16 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | f16 | 16384 | 209.610 | 2855.02 | 2855.02 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | f16 | 32768 | 520.904 | 2967.56 | 2967.56 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | f16 | 65536 | 1677.440 | 3202.98 | 3205.03 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | — | — | — | — | — | nicht durchgeführt: cannot read loopback response from 127.0.0.1:59989: Ein Verbindungsversuch ist fehlgeschlagen, da die Gegenstelle nach einer bestimmten Zeitspanne nicht richtig reagiert hat, oder die hergestellte Verbindung war fehlerhaft, da der verbundene Host nicht reagiert hat. (os error 10060) |
| gemma-4-E2B-it-Q4_K_M.gguf | q8_0 | 8192 | 135.624 | 2762.88 | 2762.88 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | q8_0 | 16384 | 372.251 | 2801.68 | 2801.68 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | q8_0 | 32768 | 1200.075 | 2870.35 | 2870.35 | gemessen |
| gemma-4-E2B-it-Q4_K_M.gguf | — | — | — | — | — | nicht durchgeführt: cannot read loopback response from 127.0.0.1:57379: Ein Verbindungsversuch ist fehlgeschlagen, da die Gegenstelle nach einer bestimmten Zeitspanne nicht richtig reagiert hat, oder die hergestellte Verbindung war fehlerhaft, da der verbundene Host nicht reagiert hat. (os error 10060) |
| gemma-4-E2B-it-Q4_K_M.gguf | — | — | — | — | — | übersprungen: larger contexts skipped after prior failure for q8_0 |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | f16 | 8192 | 122.550 | 4277.45 | 4277.45 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | f16 | 16384 | 359.340 | 5168.83 | 5170.92 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | f16 | 32768 | 1204.677 | 6981.14 | 6981.14 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | — | — | — | — | — | nicht durchgeführt: cannot read loopback response from 127.0.0.1:65186: Ein Verbindungsversuch ist fehlgeschlagen, da die Gegenstelle nach einer bestimmten Zeitspanne nicht richtig reagiert hat, oder die hergestellte Verbindung war fehlerhaft, da der verbundene Host nicht reagiert hat. (os error 10060) |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | — | — | — | — | — | übersprungen: larger contexts skipped after prior failure for f16 |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | q8_0 | 8192 | 357.060 | 3841.32 | 3850.03 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | q8_0 | 16384 | 1437.077 | 4329.29 | 4332.28 | gemessen |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | — | — | — | — | — | nicht durchgeführt: cannot read loopback response from 127.0.0.1:59066: Ein Verbindungsversuch ist fehlgeschlagen, da die Gegenstelle nach einer bestimmten Zeitspanne nicht richtig reagiert hat, oder die hergestellte Verbindung war fehlerhaft, da der verbundene Host nicht reagiert hat. (os error 10060) |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | — | — | — | — | — | übersprungen: larger contexts skipped after prior failure for q8_0 |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | — | — | — | — | — | übersprungen: larger contexts skipped after prior failure for q8_0 |

## Zwischenstand nach Risiko 1

**Nach Risiko 1 allein war Gemma 4 E2B Q4_K_S der vorläufige Kandidat.** Es war bei allen
Durchsatzwerten am schnellsten (25,243 Generierungs-Token/s; 101,437
Prompt-Token/s bei 8192 Token) und benötigte mit `mmap` 2728–2729 MiB RSS.
Gemma Q4_K_M war rund 3 % langsamer, aber etwa 64 MiB kleiner. Die endgültige
Tier-0-Auswahl nach der Werkzeugmessung steht im Abschnitt zu Risiko 2.

**Vorläufige Tier-Grenzen:** T0 bleibt bei 8 GiB RAM und 8K Kontext. T1 beginnt
bei 16 GiB und kann 16K, optional 32K verwenden. 64K wird nicht zum Standard:
Die tatsächlich gefüllte Gemma-Eingabe benötigte 1641,885–1677,440 Sekunden.
128K f16 und 64K Q8 lieferten innerhalb von 3600 Sekunden keine Antwort. Ein
echter 8-GiB-Rechner wurde in Risiko 1 **nicht** getestet; dies wird weder
simuliert noch geschätzt.

**KV-Cache:** Q8 ist nach Risiko 1 nicht zwingend. Bei Gemma Q4_K_M sparte Q8
bei 8K 34,28 MiB Gesamt-RSS, benötigte aber 135,624 statt 84,780 Sekunden. Bei
32K betrug die Ersparnis 97,21 MiB, die Laufzeit stieg von 520,904 auf 1200,075
Sekunden. f16 ist damit der gemessene Standardkandidat; Q8 bleibt optional. Die
Notwendigkeit von **GBNF** wird im Abschnitt zu Risiko 2 mit Messwerten beantwortet.

Beim ersten `mmap`-Lauf brauchte USB 13,041–20,307 Sekunden, die SSD
2,843–4,258 Sekunden. Spätere Wiederholungen waren durch den Windows-Dateicache
geprägt. Der Cache wurde nicht mit Administratorrechten geleert.

## Widerlegte Annahmen aus dem Konzept

Abgleich mit `portabler-ki-agent-konzept.md`:

- **„Llama-3.2-3B Q4_K_M (~2,0 GB) ist die RAM-schonende Alternative auf T0“
  ist für `mmap` widerlegt.** Gemessen wurden 4197–4199 MiB RSS, gegenüber
  2661–2729 MiB für Gemma. Nur mit direktem I/O lag Llama mit rund 2900 MiB
  unter Gemma (3295–3355 MiB). Ohne Angabe des Lademodus ist die Annahme falsch.
- **Ein auf llama.cpp/GGUF übertragbarer Gemma-INT4-Footprint unter 1,5 GB ist
  widerlegt.** Der kleinste gemessene Resident-Wert nach dem Laden betrug
  2661,30 MiB (Q4_K_M, `mmap`).
- **Q8 als notwendige Voraussetzung für 8K auf T0 wird von dieser Maschine
  nicht gestützt.** Gemma f16 verarbeitete 8K bei 2797–2861 MiB RSS. Für einen
  echten 8-GiB-Rechner bleibt diese Annahme ausdrücklich ungetestet.
- **Die im Konzept als zentraler Vorteil behandelte native
  Function-Calling-Fähigkeit von Gemma reicht im gemessenen einheitlichen
  JSON-Adapter allein nicht aus.** Ohne GBNF waren bei beiden Gemma-Dateien nur
  18,18 % der Antworten syntaktisch gültig; die korrekte Werkzeugwahl lag bei
  4,44–6,67 %. Häufig setzte Gemma den Werkzeugnamen in `action`, statt das
  vereinbarte Schema `action=call` plus `tool` zu verwenden.

## Risiko 2 – Zuverlässigkeit der Werkzeugaufrufe

50 Fälle × 3 Wiederholungen je Modell und Modus bei Temperatur 0.3. Die Suite enthält 20 eindeutige, 15 Auswahl-, 10 Negativ- und 5 Mehrschrittfälle. Ein Mehrschrittfall erzeugt zwei Antworten; deshalb sind es 165 Antworten, 135 erwartete Werkzeugaufrufe und 30 Negativentscheidungen je Modell und Modus.

Suite: `\\?\<Projektordner>\tools\pa-probe\risk2\cases.json` · SHA-256 `2a32ffe99000f2b3737cd0393ca3dafd0cae4f1dba514ef0c839f749fd51ea63` · Seeds 1001, 2002, 3003 · llama.cpp `version: 0.4.0-dev (build 10930, commit 56381e407)  built with Clang 20.1.8 for Windows x86_64` · CPU-Threads 7.

Syntaktisch gültig bedeutet genau ein parsebares JSON-Objekt des vereinbarten Aufrufschemas. Werkzeug und Parameter werden exakt gegen die vorab festgelegten Sollwerte verglichen. Die Streuung ist die Populations-Standardabweichung der drei vollständigen Wiederholungen. Fehlerantworten bleiben im Nenner.

| Modell | Modus | Antworten | erwartete Aufrufe | Negativfälle | Syntax % | Werkzeug % | Parameter % | Falsch-positiv % | Laufzeit s |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| gemma-4-E2B-it-Q4_K_S.gguf | frei | 165 | 135 | 30 | 18,18 ± 0,00 | 6,67 ± 0,00 | 5,93 ± 1,05 | 0,00 ± 0,00 | 251.892 |
| gemma-4-E2B-it-Q4_K_S.gguf | GBNF | 165 | 135 | 30 | 100,00 ± 0,00 | 84,44 ± 0,00 | 77,04 ± 1,05 | 10,00 ± 0,00 | 254.508 |
| gemma-4-E2B-it-Q4_K_M.gguf | frei | 165 | 135 | 30 | 18,18 ± 0,00 | 4,44 ± 0,00 | 4,44 ± 0,00 | 0,00 ± 0,00 | 255.543 |
| gemma-4-E2B-it-Q4_K_M.gguf | GBNF | 165 | 135 | 30 | 100,00 ± 0,00 | 85,93 ± 1,05 | 78,52 ± 2,10 | 10,00 ± 0,00 | 280.619 |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | frei | 165 | 135 | 30 | 94,55 ± 2,57 | 74,81 ± 2,77 | 58,52 ± 4,57 | 30,00 ± 0,00 | 384.984 |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | GBNF | 165 | 135 | 30 | 100,00 ± 0,00 | 78,52 ± 1,05 | 60,74 ± 4,57 | 30,00 ± 0,00 | 388.577 |

### Einzelwerte der drei Wiederholungen

| Modell | Modus | Syntax % | Werkzeug % | Parameter % | Falsch-positiv % |
|---|---|---|---|---|---|
| gemma-4-E2B-it-Q4_K_S.gguf | frei | 18,18 / 18,18 / 18,18 | 6,67 / 6,67 / 6,67 | 6,67 / 6,67 / 4,44 | 0,00 / 0,00 / 0,00 |
| gemma-4-E2B-it-Q4_K_S.gguf | GBNF | 100,00 / 100,00 / 100,00 | 84,44 / 84,44 / 84,44 | 77,78 / 77,78 / 75,56 | 10,00 / 10,00 / 10,00 |
| gemma-4-E2B-it-Q4_K_M.gguf | frei | 18,18 / 18,18 / 18,18 | 4,44 / 4,44 / 4,44 | 4,44 / 4,44 / 4,44 | 0,00 / 0,00 / 0,00 |
| gemma-4-E2B-it-Q4_K_M.gguf | GBNF | 100,00 / 100,00 / 100,00 | 86,67 / 86,67 / 84,44 | 80,00 / 80,00 / 75,56 | 10,00 / 10,00 / 10,00 |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | frei | 96,36 / 90,91 / 96,36 | 77,78 / 71,11 / 75,56 | 64,44 / 53,33 / 57,78 | 30,00 / 30,00 / 30,00 |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | GBNF | 100,00 / 100,00 / 100,00 | 80,00 / 77,78 / 77,78 | 66,67 / 55,56 / 60,00 | 30,00 / 30,00 / 30,00 |

### GBNF-Ergebnisse nach Fallart

| Modell | eindeutig: Werkzeug / Parameter % | Auswahl: Werkzeug / Parameter % | Mehrschritt: Werkzeug / Parameter % | Negativ: Falsch-positiv % |
|---|---:|---:|---:|---:|
| gemma-4-E2B-it-Q4_K_S.gguf | 95,00 / 80,00 | 73,33 / 73,33 | 80,00 / 76,67 | 10,00 |
| gemma-4-E2B-it-Q4_K_M.gguf | 95,00 / 80,00 | 77,78 / 77,78 | 80,00 / 76,67 | 10,00 |
| Llama-3.2-3B-Instruct-abliterated.Q4_K_M.gguf | 93,33 / 68,33 | 95,56 / 82,22 | 23,33 / 13,33 | 30,00 |

### Gemessene Empfehlung

**Tier-0-Standard: Gemma 4 E2B Q4_K_M mit verpflichtendem GBNF.** Unter GBNF
erzielte dieses Modell 100,00 % syntaktisch gültige Antworten, 85,93 % korrekte
Werkzeugwahlen, 78,52 % vollständig korrekte Parameter und 10,00 %
Falsch-Positive in Negativfällen. Es war damit bei Werkzeugwahl und Parametern
jeweils 1,49 Prozentpunkte besser als Q4_K_S und benötigte nach Risiko 1 rund
64 MiB weniger RSS; Q4_K_S blieb beim Durchsatz rund 3 % schneller.

**GBNF ist für den Tier-0-Standard zwingend.** Ohne Grammatik fiel Q4_K_M auf
18,18 % gültige Syntax, 4,44 % korrekte Werkzeugwahl und 4,44 % korrekte
Parameter. Die scheinbar bessere Falsch-Positiv-Rate von 0 % im freien Modus ist
kein Sicherheitsgewinn: 81,82 % aller Antworten waren dort bereits syntaktisch
unbrauchbar. GBNF ersetzt die Policy-Prüfung nicht, da selbst mit Grammatik 10 %
der Negativfälle fälschlich ein Werkzeug auslösten.

**Llama-3.2-3B-Instruct-Abliterated schnitt als Agentenmodell deutlich
schlechter ab.** Mit GBNF wurden 78,52 % der Werkzeuge und 60,74 % der Parameter
korrekt gewählt; die Falsch-Positiv-Rate blieb bei 30,00 %. In den
Mehrschrittfällen lagen Werkzeugwahl und Parameter nur bei 23,33 % und 13,33 %.
Ohne GBNF war Llamas Syntax mit 94,55 % zwar wesentlich stabiler als die freie
Gemma-Ausgabe, semantisch blieb es unter dem GBNF-Gemma-Standard.

### Rohdaten

Rohdaten-ID: `risk2-1789326100`. Die Datei enthält alle 990 Einzelantworten,
Seeds, Laufzeiten, Fehlerfelder und Bewertungsmerkmale. Es traten keine HTTP-
oder Prozessfehler auf.

## Risiko 3 – Portabler Start

**Status: 1 von 3 geforderten physischen Windows-Rechnern gemessen.** Mehrfachläufe derselben Maschinen-ID zählen nur einmal.

Startzeit ist die monotone Zeit vom Prozessbeginn bis zum ersten erfolgreichen IPC-Aufruf aus der WebView. Der Schreibtest schreibt 268.435.456 Bytes neben die EXE, ruft `sync_all` auf und löscht nur seine eigene Testdatei.

| Rechner | USB-Pfad | Start ms | WebView2 | ohne Admin | SmartScreen | USB-Schreiben | Betriebssystem |
|---|---|---:|---|---|---|---:|---|
| FREDLS-PC | `D:\USB\tools\pa-probe\.runtime\risk3\pa-probe-risk3.exe` | 716 | verfügbar | ja (Prozess nicht erhöht) | nicht beobachtet | 122,55 MB/s | Windows 11 Pro; Version 11 (26200); Kernel/Build 26200 |
| noch offener Rechner 1 | — | — | nicht durchgeführt | nicht durchgeführt | nicht durchgeführt | — | — |
| noch offener Rechner 2 | — | — | nicht durchgeführt | nicht durchgeführt | nicht durchgeführt | — | — |

### Hardwareprofil FREDLS-PC

| Merkmal | Messwert |
|---|---|
| CPU | AMD Ryzen 7 PRO 250 w/ Radeon 780M Graphics |
| Physische / logische Kerne | 8 / 16 |
| AVX2 / AVX-512 / NEON | ja / ja / nein |
| RAM gesamt / frei beim Profil | 29673291776 Bytes (28298.66 MiB) / 10615902208 Bytes (10124.11 MiB) |
| GPU / dedizierter VRAM | AMD Radeon 780M Graphics / 4001.15 MiB |
| Grafik-APIs | Vulkan: Runtime vorhanden; CUDA: Runtime fehlt; DirectML: Runtime vorhanden; Metal: nicht unterstützt |
| WebView2 User-Agent | Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36 Edg/152.0.0.0 |
| Rohdaten-ID | 1789363377-267052 |

### Ergebnis und offene Evidenz

Risiko 3 ist noch nicht abgeschlossen: 2 weitere physische Windows-Rechner müssen direkt vom Stick gestartet werden. Werte dafür wurden nicht geschätzt.

Die bisherige Teilmessung widerlegt keine weitere Konzeptannahme: Die Portable-EXE liegt im angenommenen Größenbereich, WebView2 war auf dem gemessenen Windows-11-Rechner vorhanden und die gemessene USB-Schreibrate liegt innerhalb der Konzeptspanne. Das ist keine Aussage über die zwei noch offenen Rechner.

## Noch nicht durchgeführt

- Risiko 3: USB-Start auf 2 weitere physische Windows-Rechner; dort jeweils WebView2, Start ohne Administratorrechte und SmartScreen-Beobachtung erfassen.
