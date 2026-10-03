//! Skelett eines PortableAI-WASM-Skills in Rust.
//!
//! Der Host ruft eine einzige Funktion `pa_skill_invoke(tool, args_json)` auf
//! und erwartet ein UTF-8-JSON-Ergebnis. `tool` ist der Werkzeug-Name aus
//! deinem `manifest.toml`, `args_json` die vom LLM erzeugten Argumente.
//!
//! Zwei Regeln, die dir Ärger sparen:
//! 1. **Keine Systemzeit, kein Netzwerk, keine Umgebungsvariablen.** Der
//!    Host reicht dir nur, was das Manifest explizit erlaubt.
//! 2. **Antworte immer als JSON.** Das Frontend zeigt den Text direkt an.
//!
//! Diese Datei kompiliert absichtlich als leerer Skill ohne wit-bindgen;
//! ersetze `pa_skill_invoke` und passe das Manifest an dein echtes
//! Werkzeug an.

#[no_mangle]
pub extern "C" fn pa_skill_invoke(_tool_ptr: *const u8, _tool_len: usize,
                                  _args_ptr: *const u8, _args_len: usize) -> u64 {
    // Später: Argumente einlesen, dispatchen, JSON zurückliefern.
    0
}
