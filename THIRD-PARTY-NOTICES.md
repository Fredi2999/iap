# Drittanbieter-Hinweise (Third-Party Notices)

IAP selbst steht unter **Apache-2.0** (siehe `LICENSE`). Diese Datei nennt, was IAP an fremden
Komponenten enthält oder ausliefert, unter welcher Lizenz sie stehen und was bei der Weitergabe zu
beachten ist.

> **Stand und Grenzen.** Die Listen in Abschnitt 4 und 5 sind aus `Cargo.lock` (Workspace und Tauri-App)
> und `package-lock.json` erzeugt, mit `node app/scripts/gen-third-party-notices.mjs` (Windows-Ziel, nur
> Laufzeitabhängigkeiten). Die CI prüft, dass die Datei aktuell ist. Die Lizenz ist das Lizenzfeld des
> Pakets, nicht der geprüfte Lizenztext. `cargo deny` prüft zusätzlich, dass keine unerwartete Lizenz
> und keine bekannte Sicherheitslücke in den Rust-Abhängigkeiten steckt.
> Abschnitt 2 (Modelle) enthält Einträge, die **nicht an der Quelle geprüft** sind; sie sind markiert.
> Die vollständigen Lizenztexte der Pakete liegen in deren Quellen und sind noch **nicht** in dieses
> Verzeichnis kopiert. Das ist Voraussetzung für eine Weitergabe (MIT, BSD und Apache-2.0 verlangen,
> dass Lizenz- und Urhebervermerk mitgeliefert werden). Keine Rechtsberatung.

## 1. Mitgelieferte Programme und Pakete

| Komponente | Version | Lizenz | Hinweis |
|---|---|---|---|
| llama.cpp (`llama-server`, ggml-DLLs in `AI/bin`) | b10930 | MIT | Lizenztext und Urhebervermerk der ggml-Autoren beilegen. |
| whisper.cpp (`whisper-server`, Paket `whisper`) | b5130 | MIT | Paket enthält `LICENSE.txt`. |
| Whisper-Modell `ggml-base` (OpenAI) | n/a | MIT | Copyright (c) 2022 OpenAI; Paket `whisper`. |
| Piper (Paket `piper`) | 2023.11.14-2 | MIT | Paket enthält `LICENSE.txt`. |
| Piper-Stimmen `de_DE-thorsten-medium`, `es_ES-davefx-medium` | n/a | CC0 | Keine Pflicht, Nennung höflich. |
| Piper-Stimmen `fr_FR-siwis-medium`, `en_GB-alba-medium` | n/a | **CC-BY 4.0** | **Namensnennung Pflicht:** Siwis (University of Edinburgh), Alba (University of Edinburgh). Quelle: https://huggingface.co/rhasspy/piper-voices |
| MinGit (Paket `git`) | 2.56.0 | **GPL-2.0** | Als eigenes, getrenntes Programm. Beim Weitergeben des Sticks Lizenztext und Quellangebot beilegen: https://github.com/git-for-windows/git/releases |
| SQLCipher (einkompiliert über `libsqlite3-sys`, Feature `bundled-sqlcipher-vendored-openssl`) | siehe `Cargo.lock` | BSD-artig (Zetetic LLC) | Lizenztext liegt im Quellbaum von `libsqlite3-sys`; nicht an der Quelle geprüft. Der Urhebervermerk von Zetetic ist beizulegen. |
| OpenSSL (einkompiliert über `openssl-src`) | 3.6.3 | Apache-2.0 | OpenSSL 3.x steht unter Apache-2.0; nicht an der Quelle geprüft. |

## 2. Sprachmodelle

Die Gewichte liegen **nicht** im Repository, aber auf dem Stick. Jede Datei `AI/models/*.model.toml`
trägt dieselben Angaben (`license`, `source_url`, `license_status`).

| Modell | Lizenz | Quelle | Stand der Prüfung |
|---|---|---|---|
| Gemma 4 E2B Instruct (Standard) | Apache-2.0 | https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF | Aus dem Prüfbericht vom 2026-10-02 übernommen, nicht erneut geprüft. |
| Qwen2.5-Coder 7B Instruct Abliterated | Apache-2.0 | https://huggingface.co/bartowski/Qwen2.5-Coder-7B-Instruct-abliterated-GGUF | Modellkarte und GGUF-Metadatum geprüft am 2026-10-02. |
| Llama 3.2 3B Instruct Abliterated | Llama 3.2 Community License und Acceptable Use Policy | https://huggingface.co/mlabonne/Llama-3.2-3B-Instruct-abliterated | **Nicht geprüft.** Aus dem Basismodell abgeleitet. Die Llama-Lizenz verlangt bei Weitergabe u. a. Lizenztext, Hinweis „Built with Llama“ und die Einhaltung der Nutzungsrichtlinie. Vor Weitergabe an der Modellkarte bestätigen. |
| Ministral 3 3B Instruct 2512 | Apache-2.0 (vermutet) | https://huggingface.co/mistralai/Ministral-3-3B-Instruct-2512-GGUF | **Nicht geprüft.** |
| Qwen3 4B Instruct 2507 | Apache-2.0 (vermutet) | https://huggingface.co/bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF | **Nicht geprüft.** |
| EmbeddingGemma 300M (Embeddings) | Gemma-Nutzungsbedingungen (Google) | https://huggingface.co/mradermacher/embeddinggemma-300m-GGUF | Aus README und Prüfbericht übernommen, nicht an der Quelle geprüft. Bei Weitergabe sind die Bedingungen samt Hinweistext und Nutzungsbeschränkungen weiterzugeben. |
| Bildprojektor `mmproj` zu Gemma 4 E2B (Paket `vision`) | **Widerspruch:** Paket nennt „Gemma-Nutzungsbedingungen“, die Modellkarte des Textmodells Apache-2.0 | https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF | **Offen.** An der Modellkarte klären und `packaging/windows/packs.ps1` sowie `docs/pakete.md` angleichen. |

Hinweis zu Modellen ohne Verweigerungsverhalten („abliterated“): Sie sind als Zweitmodelle wählbar,
nicht als Standard. Wer sie weitergibt, bleibt an die Nutzungsrichtlinie des Basismodells gebunden.

## 3. Eingebetteter Fremdcode

| Komponente | Lizenz | Herkunft |
|---|---|---|
| Bloub (Animationskern des Pets), `app/src/lib/vendor/bloub` | MIT, Copyright (c) 2026 Jérémy Perret | https://github.com/jeremy-prt/bloub, siehe `NOTICE.md` und `LICENSE` im Ordner |
| Tailwind CSS (zur Bauzeit, Ergebnis steckt im CSS) | MIT | https://github.com/tailwindlabs/tailwindcss |

## 4. Rust-Abhängigkeiten (Windows-Ziel, Laufzeit)

Pakete unter **MPL-2.0** (Dateien-Copyleft) werden unverändert benutzt; ihr Quelltext ist über crates.io öffentlich, bei Weitergabe ist darauf zu verweisen.

<!-- BEGIN:rust -->
437 Pakete (Workspace und Tauri-App). Lizenz laut `Cargo.toml` des Pakets; wo zwei Lizenzen mit `OR` stehen, darf man wählen.

| Paket | Version | Lizenz |
|---|---|---|
| addr2line | 0.26.1 | Apache-2.0 OR MIT |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 |
| aes | 0.9.3 | MIT OR Apache-2.0 |
| aho-corasick | 1.1.5 | Unlicense OR MIT |
| alloc-no-stdlib | 2.0.4 | BSD-3-Clause |
| alloc-stdlib | 0.2.4 | BSD-3-Clause |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 |
| anyhow | 1.0.104 | MIT OR Apache-2.0 |
| arbitrary | 1.4.2 | MIT OR Apache-2.0 |
| arc-swap | 1.9.2 | MIT OR Apache-2.0 |
| argon2 | 0.5.3 | MIT OR Apache-2.0 |
| async-trait | 0.1.92 | MIT OR Apache-2.0 |
| base64 | 0.22.1 | MIT OR Apache-2.0 |
| base64 | 0.23.1 | MIT OR Apache-2.0 |
| base64ct | 1.8.3 | Apache-2.0 OR MIT |
| bit-set | 0.8.0 | Apache-2.0 OR MIT |
| bit-vec | 0.8.0 | Apache-2.0 OR MIT |
| bitflags | 1.3.2 | MIT/Apache-2.0 |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |
| blake2 | 0.10.6 | MIT OR Apache-2.0 |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 |
| block-buffer | 0.12.1 | MIT OR Apache-2.0 |
| block-padding | 0.4.2 | MIT OR Apache-2.0 |
| brotli | 8.0.4 | BSD-3-Clause AND MIT |
| brotli-decompressor | 5.0.3 | BSD-3-Clause/MIT |
| bs58 | 0.5.1 | MIT/Apache-2.0 |
| bstr | 1.13.1 | MIT OR Apache-2.0 |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT |
| byteorder | 1.5.0 | Unlicense OR MIT |
| byteorder-lite | 0.1.0 | Unlicense OR MIT |
| bytes | 1.12.1 | MIT |
| camino | 1.2.5 | MIT OR Apache-2.0 |
| cargo_metadata | 0.19.2 | MIT |
| cargo-platform | 0.1.9 | MIT OR Apache-2.0 |
| cbc | 0.2.1 | MIT OR Apache-2.0 |
| cfb | 0.7.3 | MIT |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 |
| chrono | 0.4.45 | MIT OR Apache-2.0 |
| cipher | 0.5.2 | MIT OR Apache-2.0 |
| clru | 0.6.3 | MIT |
| cobs | 0.3.0 | MIT OR Apache-2.0 |
| const-oid | 0.10.2 | Apache-2.0 OR MIT |
| cookie | 0.18.2 | MIT OR Apache-2.0 |
| core_detect | 1.0.0 | MIT/Apache-2.0 |
| cpp_demangle | 0.5.1 | MIT OR Apache-2.0 |
| cpubits | 0.1.1 | MIT OR Apache-2.0 |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 |
| cranelift-assembler-x64 | 0.136.2 | Apache-2.0 WITH LLVM-exception |
| cranelift-bforest | 0.136.2 | Apache-2.0 WITH LLVM-exception |
| cranelift-bitset | 0.136.2 | Apache-2.0 WITH LLVM-exception |
| cranelift-codegen | 0.136.2 | Apache-2.0 WITH LLVM-exception |
| cranelift-codegen-shared | 0.136.2 | Apache-2.0 WITH LLVM-exception |
| cranelift-control | 0.136.2 | Apache-2.0 WITH LLVM-exception |
| cranelift-entity | 0.136.2 | Apache-2.0 WITH LLVM-exception |
| cranelift-frontend | 0.136.2 | Apache-2.0 WITH LLVM-exception |
| cranelift-native | 0.136.2 | Apache-2.0 WITH LLVM-exception |
| crc32fast | 1.5.2 | MIT OR Apache-2.0 |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 |
| cssparser | 0.36.0 | MPL-2.0 |
| cssparser-macros | 0.6.1 | MPL-2.0 |
| ctor | 0.8.0 | Apache-2.0 OR MIT |
| ctor-proc-macro | 0.0.7 | Apache-2.0 OR MIT |
| ctrlc | 3.5.2 | MIT/Apache-2.0 |
| darling | 0.24.1 | MIT |
| darling_core | 0.24.1 | MIT |
| darling_macro | 0.24.1 | MIT |
| defmt | 0.3.100 | MIT OR Apache-2.0 |
| defmt | 1.1.1 | MIT OR Apache-2.0 |
| defmt-macros | 1.1.1 | MIT OR Apache-2.0 |
| defmt-parser | 1.0.0 | MIT OR Apache-2.0 |
| deranged | 0.5.8 | MIT OR Apache-2.0 |
| derive_arbitrary | 1.4.2 | MIT OR Apache-2.0 |
| derive_more | 2.1.1 | MIT |
| derive_more-impl | 2.1.1 | MIT |
| digest | 0.10.7 | MIT OR Apache-2.0 |
| digest | 0.11.3 | MIT OR Apache-2.0 |
| dirs | 6.0.0 | MIT OR Apache-2.0 |
| dirs-sys | 0.5.0 | MIT OR Apache-2.0 |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 |
| dom_query | 0.27.0 | MIT |
| dpi | 0.1.2 | Apache-2.0 AND MIT |
| dtoa | 1.0.11 | MIT OR Apache-2.0 |
| dtoa-short | 0.3.5 | MPL-2.0 |
| dtor | 0.3.0 | Apache-2.0 OR MIT |
| dtor-proc-macro | 0.0.6 | Apache-2.0 OR MIT |
| dunce | 1.0.5 | CC0-1.0 OR MIT-0 OR Apache-2.0 |
| dyn-clone | 1.0.20 | MIT OR Apache-2.0 |
| ecb | 0.2.1 | MIT OR Apache-2.0 |
| either | 1.18.0 | MIT OR Apache-2.0 |
| embedded-io | 0.4.0 | MIT OR Apache-2.0 |
| embedded-io | 0.6.1 | MIT OR Apache-2.0 |
| encoding_rs | 0.8.42 | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| equivalent | 1.0.2 | Apache-2.0 OR MIT |
| erased-serde | 0.4.10 | MIT OR Apache-2.0 |
| fallible-iterator | 0.3.0 | MIT/Apache-2.0 |
| fallible-streaming-iterator | 0.1.9 | MIT/Apache-2.0 |
| faster-hex | 0.10.1 | MIT |
| fastrand | 2.5.0 | Apache-2.0 OR MIT |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 |
| flate2 | 1.1.10 | MIT OR Apache-2.0 |
| fnv | 1.0.7 | Apache-2.0 / MIT |
| foldhash | 0.2.0 | Zlib |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 |
| futures | 0.3.34 | MIT OR Apache-2.0 |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 |
| futures-core | 0.3.34 | MIT OR Apache-2.0 |
| futures-io | 0.3.34 | MIT OR Apache-2.0 |
| futures-macro | 0.3.34 | MIT OR Apache-2.0 |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 |
| futures-task | 0.3.34 | MIT OR Apache-2.0 |
| futures-util | 0.3.34 | MIT OR Apache-2.0 |
| generic-array | 0.14.7 | MIT |
| getrandom | 0.2.17 | MIT OR Apache-2.0 |
| getrandom | 0.3.4 | MIT OR Apache-2.0 |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |
| gimli | 0.33.0 | MIT OR Apache-2.0 |
| gix | 0.87.1 | MIT OR Apache-2.0 |
| gix-actor | 0.42.0 | MIT OR Apache-2.0 |
| gix-attributes | 0.35.0 | MIT OR Apache-2.0 |
| gix-chunk | 0.8.0 | MIT OR Apache-2.0 |
| gix-command | 0.10.1 | MIT OR Apache-2.0 |
| gix-commitgraph | 0.39.0 | MIT OR Apache-2.0 |
| gix-config | 0.60.0 | MIT OR Apache-2.0 |
| gix-config-value | 0.19.1 | MIT OR Apache-2.0 |
| gix-date | 0.16.0 | MIT OR Apache-2.0 |
| gix-diff | 0.67.1 | MIT OR Apache-2.0 |
| gix-discover | 0.55.0 | MIT OR Apache-2.0 |
| gix-error | 0.3.2 | MIT OR Apache-2.0 |
| gix-features | 0.49.1 | MIT OR Apache-2.0 |
| gix-filter | 0.34.0 | MIT OR Apache-2.0 |
| gix-fs | 0.22.1 | MIT OR Apache-2.0 |
| gix-glob | 0.27.1 | MIT OR Apache-2.0 |
| gix-hash | 0.26.2 | MIT OR Apache-2.0 |
| gix-hashtable | 0.16.0 | MIT OR Apache-2.0 |
| gix-lock | 24.0.0 | MIT OR Apache-2.0 |
| gix-macros | 0.1.6 | MIT OR Apache-2.0 |
| gix-note | 0.1.1 | MIT OR Apache-2.0 |
| gix-object | 0.64.1 | MIT OR Apache-2.0 |
| gix-odb | 0.84.0 | MIT OR Apache-2.0 |
| gix-pack | 0.74.2 | MIT OR Apache-2.0 |
| gix-packetline | 0.22.2 | MIT OR Apache-2.0 |
| gix-path | 0.12.6 | MIT OR Apache-2.0 |
| gix-protocol | 0.65.1 | MIT OR Apache-2.0 |
| gix-quote | 0.8.0 | MIT OR Apache-2.0 |
| gix-ref | 0.67.1 | MIT OR Apache-2.0 |
| gix-refspec | 0.45.1 | MIT OR Apache-2.0 |
| gix-revision | 0.49.1 | MIT OR Apache-2.0 |
| gix-revwalk | 0.35.0 | MIT OR Apache-2.0 |
| gix-sec | 0.14.2 | MIT OR Apache-2.0 |
| gix-shallow | 0.13.0 | MIT OR Apache-2.0 |
| gix-tempfile | 24.0.0 | MIT OR Apache-2.0 |
| gix-trace | 0.1.21 | MIT OR Apache-2.0 |
| gix-transport | 0.59.2 | MIT OR Apache-2.0 |
| gix-traverse | 0.61.0 | MIT OR Apache-2.0 |
| gix-url | 0.38.0 | MIT OR Apache-2.0 |
| gix-utils | 0.3.6 | MIT OR Apache-2.0 |
| gix-validate | 0.11.4 | MIT OR Apache-2.0 |
| gix-worktree-stream | 0.36.1 | MIT OR Apache-2.0 |
| gix-zlib | 0.1.0 | MIT OR Apache-2.0 |
| glob | 0.3.4 | MIT OR Apache-2.0 |
| global-hotkey | 0.8.0 | Apache-2.0 OR MIT |
| hash32 | 0.3.1 | MIT OR Apache-2.0 |
| hashbrown | 0.12.3 | MIT OR Apache-2.0 |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| hashlink | 0.12.2 | MIT OR Apache-2.0 |
| heapless | 0.8.0 | MIT OR Apache-2.0 |
| heck | 0.5.0 | MIT OR Apache-2.0 |
| hex | 0.4.3 | MIT OR Apache-2.0 |
| html5ever | 0.38.0 | MIT OR Apache-2.0 |
| http | 1.5.0 | MIT OR Apache-2.0 |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 |
| ico | 0.5.0 | MIT |
| icu_collections | 2.3.0 | Unicode-3.0 |
| icu_locale_core | 2.3.0 | Unicode-3.0 |
| icu_normalizer | 2.3.0 | Unicode-3.0 |
| icu_normalizer_data | 2.3.0 | Unicode-3.0 |
| icu_properties | 2.3.0 | Unicode-3.0 |
| icu_properties_data | 2.3.0 | Unicode-3.0 |
| icu_provider | 2.3.1 | Unicode-3.0 |
| ident_case | 1.0.1 | MIT/Apache-2.0 |
| idna | 1.1.0 | MIT OR Apache-2.0 |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT |
| image | 0.25.5 | MIT OR Apache-2.0 |
| indexmap | 1.9.3 | Apache-2.0 OR MIT |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| infer | 0.19.0 | MIT |
| inout | 0.2.2 | MIT OR Apache-2.0 |
| itertools | 0.14.0 | MIT OR Apache-2.0 |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| jiff | 0.2.37 | Unlicense OR MIT |
| jiff-core | 0.1.1 | Unlicense OR MIT |
| jiff-static | 0.2.37 | Unlicense OR MIT |
| jiff-tzdb | 0.1.8 | Unlicense OR MIT |
| jiff-tzdb-platform | 0.1.3 | Unlicense OR MIT |
| json-patch | 3.0.1 | MIT/Apache-2.0 |
| jsonptr | 0.6.3 | MIT OR Apache-2.0 |
| keyboard-types | 0.7.0 | MIT OR Apache-2.0 |
| leb128fmt | 0.1.0 | MIT OR Apache-2.0 |
| libc | 0.2.189 | MIT OR Apache-2.0 |
| libm | 0.2.16 | MIT |
| libsqlite3-sys | 0.38.2 | MIT |
| litemap | 0.8.3 | Unicode-3.0 |
| lock_api | 0.4.14 | MIT OR Apache-2.0 |
| log | 0.4.34 | MIT OR Apache-2.0 |
| lopdf | 0.45.0 | MIT |
| markup5ever | 0.38.0 | MIT OR Apache-2.0 |
| md-5 | 0.11.0 | MIT OR Apache-2.0 |
| memchr | 2.8.3 | Unlicense OR MIT |
| memmap2 | 0.9.11 | MIT OR Apache-2.0 |
| mime | 0.3.17 | MIT OR Apache-2.0 |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 |
| mio | 1.2.3 | MIT |
| muda | 0.19.3 | Apache-2.0 OR MIT |
| multiversion_no_op | 1.0.0 | Apache-2.0 OR MIT |
| new_debug_unreachable | 1.0.6 | MIT |
| nom | 8.0.0 | MIT |
| nonempty | 0.12.0 | MIT |
| ntapi | 0.4.3 | Apache-2.0 OR MIT |
| num-conv | 0.2.2 | MIT OR Apache-2.0 |
| num-traits | 0.2.19 | MIT OR Apache-2.0 |
| object | 0.40.0 | Apache-2.0 OR MIT |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |
| openssl-sys | 0.9.117 | MIT |
| option-ext | 0.2.0 | MPL-2.0 |
| pa-agents | 0.1.0 | Apache-2.0 |
| pa-code | 0.1.0 | Apache-2.0 |
| pa-core | 0.1.0 | Apache-2.0 |
| pa-export | 0.1.0 | Apache-2.0 |
| pa-inference | 0.1.0 | Apache-2.0 |
| pa-launcher | 0.1.0 | Apache-2.0 |
| pa-memory | 0.1.0 | Apache-2.0 |
| pa-pci | 0.1.0 | Apache-2.0 |
| pa-policy | 0.1.0 | Apache-2.0 |
| pa-scheduler | 0.1.0 | Apache-2.0 |
| pa-skills | 0.1.0 | Apache-2.0 |
| pa-tools | 0.1.0 | Apache-2.0 |
| pa-types | 0.1.0 | Apache-2.0 |
| pa-update | 0.1.0 | Apache-2.0 |
| pa-vault | 0.1.0 | Apache-2.0 |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 |
| password-hash | 0.5.0 | MIT OR Apache-2.0 |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 |
| phf | 0.13.1 | MIT |
| phf_generator | 0.13.1 | MIT |
| phf_macros | 0.13.1 | MIT |
| phf_shared | 0.13.1 | MIT |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT |
| plist | 1.10.1 | MIT |
| png | 0.17.16 | MIT OR Apache-2.0 |
| png | 0.18.1 | MIT OR Apache-2.0 |
| postcard | 1.1.3 | MIT OR Apache-2.0 |
| potential_utf | 0.1.6 | Unicode-3.0 |
| powerfmt | 0.2.0 | MIT OR Apache-2.0 |
| precomputed-hash | 0.1.1 | MIT |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| prodash | 31.0.0 | MIT |
| pulley-interpreter | 49.0.2 | Apache-2.0 WITH LLVM-exception |
| pulley-macros | 49.0.2 | Apache-2.0 WITH LLVM-exception |
| quick-xml | 0.42.0 | MIT |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| rand | 0.10.3 | MIT OR Apache-2.0 |
| rand_core | 0.10.1 | MIT OR Apache-2.0 |
| rand_core | 0.6.4 | MIT OR Apache-2.0 |
| rangemap | 1.8.0 | MIT/Apache-2.0 |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib |
| ref-cast | 1.0.27 | MIT OR Apache-2.0 |
| ref-cast-impl | 1.0.27 | MIT OR Apache-2.0 |
| regalloc2 | 0.15.2 | Apache-2.0 WITH LLVM-exception |
| regex | 1.13.1 | MIT OR Apache-2.0 |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 |
| rpassword | 7.5.4 | Apache-2.0 |
| rtoolbox | 0.0.6 | Apache-2.0 |
| rusqlite | 0.40.2 | MIT |
| rustc-demangle | 0.1.28 | MIT/Apache-2.0 |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT |
| rustversion | 1.0.23 | MIT OR Apache-2.0 |
| same-file | 1.0.6 | Unlicense/MIT |
| schannel | 0.1.29 | MIT |
| schemars | 0.8.22 | MIT |
| schemars | 0.9.0 | MIT |
| schemars | 1.2.2 | MIT |
| schemars_derive | 0.8.22 | MIT |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 |
| selectors | 0.36.1 | MPL-2.0 |
| semver | 1.0.28 | MIT OR Apache-2.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive_internals | 0.29.1 | MIT OR Apache-2.0 |
| serde_json | 1.0.151 | MIT OR Apache-2.0 |
| serde_repr | 0.1.21 | MIT OR Apache-2.0 |
| serde_spanned | 1.1.1 | MIT OR Apache-2.0 |
| serde_with | 3.23.0 | MIT OR Apache-2.0 |
| serde_with_macros | 3.23.0 | MIT OR Apache-2.0 |
| serde-untagged | 0.1.9 | MIT OR Apache-2.0 |
| serialize-to-javascript | 0.1.2 | MIT OR Apache-2.0 |
| serialize-to-javascript-impl | 0.1.2 | MIT OR Apache-2.0 |
| servo_arc | 0.4.3 | MIT OR Apache-2.0 |
| sha1 | 0.10.7 | MIT OR Apache-2.0 |
| sha1-checked | 0.10.0 | MIT OR Apache-2.0 |
| sha2 | 0.10.9 | MIT OR Apache-2.0 |
| sha2 | 0.11.0 | MIT OR Apache-2.0 |
| simd-adler32 | 0.3.10 | MIT |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 |
| siphasher | 1.0.3 | MIT/Apache-2.0 |
| slab | 0.4.12 | MIT |
| smallvec | 1.16.1 | MIT OR Apache-2.0 |
| socket2 | 0.6.5 | MIT OR Apache-2.0 |
| softbuffer | 0.4.8 | MIT OR Apache-2.0 |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 |
| string_cache | 0.9.0 | MIT OR Apache-2.0 |
| stringprep | 0.1.5 | MIT/Apache-2.0 |
| strsim | 0.11.1 | MIT |
| subtle | 2.6.1 | BSD-3-Clause |
| syn | 2.0.119 | MIT OR Apache-2.0 |
| syn | 3.0.5 | MIT OR Apache-2.0 |
| synstructure | 0.13.2 | MIT |
| sysinfo | 0.37.2 | MIT |
| tao | 0.35.3 | Apache-2.0 |
| target-lexicon | 0.13.5 | Apache-2.0 WITH LLVM-exception |
| tauri | 2.11.5 | Apache-2.0 OR MIT |
| tauri-codegen | 2.6.3 | Apache-2.0 OR MIT |
| tauri-macros | 2.6.3 | Apache-2.0 OR MIT |
| tauri-plugin-global-shortcut | 2.3.2 | Apache-2.0 OR MIT |
| tauri-runtime | 2.11.3 | Apache-2.0 OR MIT |
| tauri-runtime-wry | 2.11.4 | Apache-2.0 OR MIT |
| tauri-utils | 2.9.3 | Apache-2.0 OR MIT |
| tempfile | 3.27.0 | MIT OR Apache-2.0 |
| tendril | 0.5.1 | MIT OR Apache-2.0 |
| termcolor | 1.4.1 | Unlicense OR MIT |
| thiserror | 1.0.69 | MIT OR Apache-2.0 |
| thiserror | 2.0.20 | MIT OR Apache-2.0 |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 |
| thiserror-impl | 2.0.20 | MIT OR Apache-2.0 |
| time | 0.3.55 | MIT OR Apache-2.0 |
| time-core | 0.1.9 | MIT OR Apache-2.0 |
| time-macros | 0.2.32 | MIT OR Apache-2.0 |
| tinystr | 0.8.4 | Unicode-3.0 |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT |
| tokio | 1.53.1 | MIT |
| toml | 0.9.12+spec-1.1.0 | MIT OR Apache-2.0 |
| toml | 1.1.6+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_datetime | 0.7.5+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_datetime | 1.1.1+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_parser | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_writer | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 |
| tracing | 0.1.44 | MIT |
| tracing-core | 0.1.36 | MIT |
| tray-icon | 0.24.2 | MIT OR Apache-2.0 |
| typeid | 1.0.3 | MIT OR Apache-2.0 |
| typenum | 1.20.1 | MIT OR Apache-2.0 |
| unic-char-property | 0.9.0 | MIT/Apache-2.0 |
| unic-char-range | 0.9.0 | MIT/Apache-2.0 |
| unic-common | 0.9.0 | MIT/Apache-2.0 |
| unic-ucd-ident | 0.9.0 | MIT/Apache-2.0 |
| unic-ucd-version | 0.9.0 | MIT/Apache-2.0 |
| unicode-bidi | 0.3.18 | MIT OR Apache-2.0 |
| unicode-bom | 2.0.3 | Apache-2.0 |
| unicode-ident | 1.0.24 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| unicode-normalization | 0.1.25 | MIT OR Apache-2.0 |
| unicode-properties | 0.1.4 | MIT/Apache-2.0 |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 |
| unicode-width | 0.2.2 | MIT OR Apache-2.0 |
| url | 2.5.8 | MIT OR Apache-2.0 |
| urlpattern | 0.3.0 | MIT |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT |
| uuid | 1.26.1 | Apache-2.0 OR MIT |
| walkdir | 2.5.0 | Unlicense/MIT |
| wasm-encoder | 0.258.3 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wasm-encoder | 0.259.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wasmparser | 0.258.3 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wasmparser | 0.259.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wasmprinter | 0.258.3 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wasmtime | 49.0.2 | Apache-2.0 WITH LLVM-exception |
| wasmtime-environ | 49.0.2 | Apache-2.0 WITH LLVM-exception |
| wasmtime-internal-component-util | 49.0.2 | Apache-2.0 WITH LLVM-exception |
| wasmtime-internal-core | 49.0.2 | Apache-2.0 WITH LLVM-exception |
| wasmtime-internal-cranelift | 49.0.2 | Apache-2.0 WITH LLVM-exception |
| wasmtime-internal-fiber | 49.0.2 | Apache-2.0 WITH LLVM-exception |
| wasmtime-internal-jit-debug | 49.0.2 | Apache-2.0 WITH LLVM-exception |
| wasmtime-internal-jit-icache-coherence | 49.0.2 | Apache-2.0 WITH LLVM-exception |
| wasmtime-internal-unwinder | 49.0.2 | Apache-2.0 WITH LLVM-exception |
| wasmtime-internal-versioned-export-macros | 49.0.2 | Apache-2.0 WITH LLVM-exception |
| wast | 259.0.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wat | 1.259.0 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| web_atoms | 0.2.6 | MIT OR Apache-2.0 |
| webview2-com | 0.38.2 | MIT |
| webview2-com-macros | 0.8.1 | MIT |
| webview2-com-sys | 0.38.2 | MIT |
| weezl | 0.2.1 | MIT OR Apache-2.0 |
| winapi | 0.3.9 | MIT/Apache-2.0 |
| winapi-util | 0.1.11 | Unlicense OR MIT |
| window-vibrancy | 0.6.0 | Apache-2.0 OR MIT |
| windows | 0.61.3 | MIT OR Apache-2.0 |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 |
| windows-collections | 0.2.0 | MIT OR Apache-2.0 |
| windows-core | 0.61.2 | MIT OR Apache-2.0 |
| windows-future | 0.2.1 | MIT OR Apache-2.0 |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 |
| windows-link | 0.1.3 | MIT OR Apache-2.0 |
| windows-link | 0.2.1 | MIT OR Apache-2.0 |
| windows-numerics | 0.2.0 | MIT OR Apache-2.0 |
| windows-result | 0.3.4 | MIT OR Apache-2.0 |
| windows-strings | 0.4.2 | MIT OR Apache-2.0 |
| windows-sys | 0.59.0 | MIT OR Apache-2.0 |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 |
| windows-threading | 0.1.0 | MIT OR Apache-2.0 |
| windows-version | 0.1.7 | MIT OR Apache-2.0 |
| winnow | 0.7.15 | MIT |
| winnow | 1.0.4 | MIT |
| writeable | 0.6.4 | Unicode-3.0 |
| wry | 0.55.1 | Apache-2.0 OR MIT |
| yoke | 0.8.3 | Unicode-3.0 |
| yoke-derive | 0.8.2 | Unicode-3.0 |
| zerofrom | 0.1.8 | Unicode-3.0 |
| zerofrom-derive | 0.1.7 | Unicode-3.0 |
| zeroize | 1.9.0 | Apache-2.0 OR MIT |
| zeroize_derive | 1.5.0 | Apache-2.0 OR MIT |
| zerotrie | 0.2.5 | Unicode-3.0 |
| zerovec | 0.11.8 | Unicode-3.0 |
| zerovec-derive | 0.11.6 | Unicode-3.0 |
| zip | 4.6.1 | MIT |
| zlib-rs | 0.6.7 | Zlib |
| zlib-rs | 0.6.8 | Zlib |
| zmij | 1.0.23 | MIT |
<!-- END:rust -->

## 5. npm-Pakete (Laufzeit des Frontends)

<!-- BEGIN:npm -->
47 Pakete aus `app/package-lock.json` ohne Entwicklungsabhängigkeiten.

| Paket | Version | Lizenz |
|---|---|---|
| @codemirror/autocomplete | 6.20.3 | MIT |
| @codemirror/commands | 6.11.1 | MIT |
| @codemirror/lang-css | 6.3.1 | MIT |
| @codemirror/lang-html | 6.4.12 | MIT |
| @codemirror/lang-javascript | 6.2.5 | MIT |
| @codemirror/lang-markdown | 6.5.2 | MIT |
| @codemirror/lang-python | 6.2.1 | MIT |
| @codemirror/lang-rust | 6.0.2 | MIT |
| @codemirror/language | 6.12.4 | MIT |
| @codemirror/lint | 6.9.7 | MIT |
| @codemirror/search | 6.7.2 | MIT |
| @codemirror/state | 6.7.5 | MIT |
| @codemirror/view | 6.43.12 | MIT |
| @jridgewell/gen-mapping | 0.3.13 | MIT |
| @jridgewell/remapping | 2.3.5 | MIT |
| @jridgewell/resolve-uri | 3.1.2 | MIT |
| @jridgewell/sourcemap-codec | 1.6.0 | MIT |
| @jridgewell/trace-mapping | 0.3.31 | MIT |
| @lezer/common | 1.5.2 | MIT |
| @lezer/css | 1.3.6 | MIT |
| @lezer/highlight | 1.2.3 | MIT |
| @lezer/html | 1.3.13 | MIT |
| @lezer/javascript | 1.5.4 | MIT |
| @lezer/lr | 1.4.10 | MIT |
| @lezer/markdown | 1.7.2 | MIT |
| @lezer/python | 1.1.19 | MIT |
| @lezer/rust | 1.0.2 | MIT |
| @marijn/find-cluster-break | 1.0.4 | MIT |
| @sveltejs/acorn-typescript | 1.0.13 | MIT |
| @tauri-apps/api | 2.11.1 | Apache-2.0 OR MIT |
| @types/estree | 1.0.9 | MIT |
| acorn | 8.18.0 | MIT |
| aria-query | 5.3.1 | Apache-2.0 |
| axobject-query | 4.1.0 | Apache-2.0 |
| clsx | 2.1.1 | MIT |
| codemirror | 6.0.2 | MIT |
| crelt | 1.0.7 | MIT |
| devalue | 5.9.2 | MIT |
| esm-env | 1.2.2 | MIT |
| esrap | 2.3.7 | MIT |
| is-reference | 3.0.3 | MIT |
| locate-character | 3.0.0 | MIT |
| magic-string | 0.30.21 | MIT |
| style-mod | 4.1.4 | MIT |
| svelte | 5.57.0 | MIT |
| w3c-keyname | 2.2.8 | MIT |
| zimmerframe | 1.1.5 | MIT |
<!-- END:npm -->
