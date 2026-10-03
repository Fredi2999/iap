# Modell-Evals

Feste Testfragen, die zeigen, wie ein Modell sich bei den Aufgaben verhält, auf die IAP sich verlässt:
gültige JSON-Hülle bei Werkzeugen, kein erfundener oder verbotener Werkzeugaufruf, Widerstand gegen
Anweisungen in gelesenem Fremdtext, Antworten auf Deutsch.

- Fälle: `cases.toml` (klein und deutsch, jederzeit ergänzbar)
- Auswertung: `crates/pa-launcher/src/eval.rs` (läuft ohne Modell, durch Tests abgesichert)
- Lauf gegen ein echtes Modell: `crates/pa-launcher/tests/model_eval.rs` (ignoriert)

## Aufruf

```powershell
$env:IAP_EVAL_SERVER = "D:\IAP\AI\bin\win-x64\llama-server.exe"
$env:IAP_EVAL_MODEL  = "D:\IAP\AI\models\Qwen2.5-Coder-7B-Instruct-abliterated-Q5_K_M.gguf"
$env:IAP_EVAL_FAMILY = "qwen3"      # family aus der .model.toml
cargo test -p pa-launcher --test model_eval -- --ignored --nocapture
```

Optional: `IAP_EVAL_THREADS`, `IAP_EVAL_MIN_PASS` (Standard 0,7), `IAP_EVAL_OUT` (Bericht als JSON).
Der Lauf nutzt denselben Adapter, dieselbe Werkzeug-Hülle und dieselbe Grammatik wie die App. Er startet
den Server nur auf 127.0.0.1 und wertet jede Antwort anhand der Erwartungen im Fall aus.

## Grenzen

- Der Lauf wurde bisher **nicht** gegen ein echtes Modell ausgeführt. Nur die Auswertung ist getestet.
- Wenige Fälle sind ein Anhaltspunkt. Eine hohe Quote heißt nicht, dass ein Modell gut ist, eine niedrige
  schon eher, dass man ihm keine Werkzeuge anvertrauen sollte.
- Modelle sind nicht deterministisch; ein einzelner Fehlschlag kann Zufall sein. Bei Auffälligkeiten
  mehrfach laufen lassen.
- Die Ergebnisse gehören in die `.model.toml` des Modells, sobald jemand sie gemessen hat (heute steht dort
  bei den `abliterated`-Modellen „Qualität nicht geprüft“).
