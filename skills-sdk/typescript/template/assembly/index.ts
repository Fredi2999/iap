// Skelett eines PortableAI-WASM-Skills in AssemblyScript.
//
// Der Host ruft `pa_skill_invoke(tool, argsJson)` auf und erwartet einen
// UTF-8-JSON-String zurück. AssemblyScript hat keinen echten `String`
// ABI — echte Skills nutzen einen shared-memory-Ansatz, den der SDK-Loader
// bereitstellen wird. Diese Vorlage zeigt nur die Funktionssignatur.
//
// Regeln (wie im Rust-Template):
// 1. Keine externen Ressourcen ohne Manifest-Erlaubnis.
// 2. Antworten sind JSON.

export function pa_skill_invoke(_toolPtr: usize, _toolLen: usize,
                                _argsPtr: usize, _argsLen: usize): u64 {
  // Später: Argumente einlesen, dispatchen, JSON zurückliefern.
  return 0;
}
