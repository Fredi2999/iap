//! Ausführung eines Skills in der Wasmtime-Sandbox (Konzept 8.2).
//!
//! Vertrag (ABI) eines Skills, bewusst ohne WASI:
//! - Export `memory`: der lineare Speicher des Moduls.
//! - Export `pa_alloc(len: i32) -> i32`: reserviert `len` Bytes und liefert den Zeiger.
//! - Export `pa_skill_invoke(tool_ptr, tool_len, args_ptr, args_len: i32) -> i64`:
//!   liefert `(ptr << 32) | len` eines UTF-8-JSON-Ergebnisses im Speicher des Moduls.
//!
//! Ein Skill darf **keine Importe** haben. Er sieht damit weder Dateien noch
//! Netzwerk, Uhr oder Umgebungsvariablen; alles, was er bekommt, sind der
//! Werkzeugname und die Argumente. Laufzeit und Speicher sind hart begrenzt.

use std::{
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::Duration,
};

use serde_json::Value;
use thiserror::Error;
use wasmtime::{
    Config, Engine, Instance, Memory, Module, Store, StoreLimits, StoreLimitsBuilder, Trap,
    TypedFunc,
};

/// Obergrenzen für einen einzelnen Aufruf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SandboxLimits {
    pub max_memory_bytes: usize,
    pub max_runtime: Duration,
    pub max_output_bytes: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SandboxError {
    #[error("Modul ist kein gültiges WebAssembly: {0}")]
    Compile(String),
    #[error("Skill verlangt Importe ({0}); erlaubt sind keine")]
    Imports(String),
    #[error("Export `{0}` fehlt")]
    MissingExport(&'static str),
    #[error("Zeitlimit überschritten")]
    Timeout,
    #[error("Skill ist abgebrochen: {0}")]
    Trap(String),
    #[error("Ungültige Antwort des Skills: {0}")]
    Output(String),
}

/// Führt einen Werkzeugaufruf eines Skills isoliert aus und liefert dessen JSON-Antwort.
///
/// Warum eine eigene Engine pro Aufruf: Skills werden selten aufgerufen, und
/// eine frische Engine macht den Zeit-Wächter (Epoch) unabhängig von anderen
/// Aufrufen. Die Übersetzung kostet dabei nur wenige Millisekunden.
pub fn execute(
    wasm: &[u8],
    tool: &str,
    arguments_json: &str,
    limits: &SandboxLimits,
) -> Result<Value, SandboxError> {
    let mut config = Config::new();
    config.epoch_interruption(true);
    config.max_wasm_stack(512 * 1024);
    let engine = Engine::new(&config).map_err(|error| SandboxError::Compile(error.to_string()))?;
    let module =
        Module::new(&engine, wasm).map_err(|error| SandboxError::Compile(error.to_string()))?;
    let imports: Vec<String> = module
        .imports()
        .map(|import| format!("{}::{}", import.module(), import.name()))
        .collect();
    if !imports.is_empty() {
        return Err(SandboxError::Imports(imports.join(", ")));
    }

    let store_limits = StoreLimitsBuilder::new()
        .memory_size(limits.max_memory_bytes)
        .instances(1)
        .memories(1)
        .tables(4)
        .trap_on_grow_failure(true)
        .build();
    let mut store = Store::new(&engine, store_limits);
    store.limiter(|state: &mut StoreLimits| state as &mut dyn wasmtime::ResourceLimiter);
    store.set_epoch_deadline(1);

    // Zeit-Wächter: läuft der Skill zu lange, wird die Epoche erhöht und
    // Wasmtime bricht an der nächsten Prüfstelle mit `Trap::Interrupt` ab.
    let (done, finished) = mpsc::channel::<()>();
    let watchdog_engine = engine.clone();
    let max_runtime = limits.max_runtime;
    let watchdog = thread::spawn(move || {
        if let Err(RecvTimeoutError::Timeout) = finished.recv_timeout(max_runtime) {
            watchdog_engine.increment_epoch();
        }
    });

    let result = invoke(
        &mut store,
        &module,
        tool,
        arguments_json,
        limits.max_output_bytes,
    );
    drop(done);
    let _ = watchdog.join();
    result
}

fn invoke(
    store: &mut Store<StoreLimits>,
    module: &Module,
    tool: &str,
    arguments_json: &str,
    max_output_bytes: usize,
) -> Result<Value, SandboxError> {
    let instance = Instance::new(&mut *store, module, &[]).map_err(map_error)?;
    let memory = instance
        .get_memory(&mut *store, "memory")
        .ok_or(SandboxError::MissingExport("memory"))?;
    let alloc: TypedFunc<i32, i32> = instance
        .get_typed_func(&mut *store, "pa_alloc")
        .map_err(|_| SandboxError::MissingExport("pa_alloc"))?;
    let entry: TypedFunc<(i32, i32, i32, i32), i64> = instance
        .get_typed_func(&mut *store, "pa_skill_invoke")
        .map_err(|_| SandboxError::MissingExport("pa_skill_invoke"))?;

    let (tool_ptr, tool_len) = write_input(store, &memory, &alloc, tool.as_bytes())?;
    let (args_ptr, args_len) = write_input(store, &memory, &alloc, arguments_json.as_bytes())?;
    let packed = entry
        .call(&mut *store, (tool_ptr, tool_len, args_ptr, args_len))
        .map_err(map_error)? as u64;

    let pointer = (packed >> 32) as usize;
    let length = (packed & 0xffff_ffff) as usize;
    if length > max_output_bytes {
        return Err(SandboxError::Output(format!(
            "{length} Bytes, erlaubt sind {max_output_bytes}"
        )));
    }
    let mut buffer = vec![0u8; length];
    memory
        .read(&*store, pointer, &mut buffer)
        .map_err(|_| SandboxError::Output("liegt außerhalb des Speichers".to_owned()))?;
    let text =
        String::from_utf8(buffer).map_err(|_| SandboxError::Output("kein UTF-8".to_owned()))?;
    serde_json::from_str(&text)
        .map_err(|error| SandboxError::Output(format!("kein JSON ({error})")))
}

/// Legt Eingabebytes über den Allokator des Skills in dessen Speicher.
fn write_input(
    store: &mut Store<StoreLimits>,
    memory: &Memory,
    alloc: &TypedFunc<i32, i32>,
    bytes: &[u8],
) -> Result<(i32, i32), SandboxError> {
    let length = i32::try_from(bytes.len())
        .map_err(|_| SandboxError::Output("Eingabe zu groß".to_owned()))?;
    let pointer = alloc.call(&mut *store, length).map_err(map_error)?;
    let offset = u32::try_from(pointer)
        .map_err(|_| SandboxError::Trap("pa_alloc lieferte einen ungültigen Zeiger".to_owned()))?;
    memory
        .write(&mut *store, offset as usize, bytes)
        .map_err(|_| {
            SandboxError::Trap("pa_alloc lieferte einen Bereich außerhalb des Speichers".to_owned())
        })?;
    Ok((pointer, length))
}

fn map_error(error: wasmtime::Error) -> SandboxError {
    match error.downcast_ref::<Trap>() {
        Some(Trap::Interrupt) => SandboxError::Timeout,
        Some(trap) => SandboxError::Trap(trap.to_string()),
        None => SandboxError::Trap(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALLOC: &str = r#"
      (memory (export "memory") 1)
      (global $heap (mut i32) (i32.const 1024))
      (func (export "pa_alloc") (param $len i32) (result i32)
        (local $ptr i32)
        global.get $heap
        local.set $ptr
        global.get $heap
        local.get $len
        i32.add
        global.set $heap
        local.get $ptr)
    "#;

    fn limits() -> SandboxLimits {
        SandboxLimits {
            max_memory_bytes: 4 * 1024 * 1024,
            max_runtime: Duration::from_millis(500),
            max_output_bytes: 64 * 1024,
        }
    }

    fn module(body: &str) -> Vec<u8> {
        // Mit dem Test-Feature `wat` nimmt Wasmtime auch Textformat an.
        format!("(module {ALLOC} {body})").into_bytes()
    }

    #[test]
    fn echo_skill_returns_its_arguments() {
        let wasm = module(
            r#"(func (export "pa_skill_invoke") (param i32 i32 i32 i32) (result i64)
                 local.get 2
                 i64.extend_i32_u
                 i64.const 32
                 i64.shl
                 local.get 3
                 i64.extend_i32_u
                 i64.or)"#,
        );
        let value = execute(&wasm, "echo", r#"{"text":"hallo"}"#, &limits()).expect("Aufruf");
        assert_eq!(value["text"], "hallo");
    }

    #[test]
    fn endless_loop_hits_time_limit() {
        let wasm = module(
            r#"(func (export "pa_skill_invoke") (param i32 i32 i32 i32) (result i64)
                 (loop $forever br $forever)
                 i64.const 0)"#,
        );
        assert_eq!(
            execute(&wasm, "x", "{}", &limits()),
            Err(SandboxError::Timeout)
        );
    }

    #[test]
    fn memory_growth_beyond_limit_is_stopped() {
        let wasm = module(
            r#"(func (export "pa_skill_invoke") (param i32 i32 i32 i32) (result i64)
                 (drop (memory.grow (i32.const 1000)))
                 i64.const 0)"#,
        );
        assert!(matches!(
            execute(&wasm, "x", "{}", &limits()),
            Err(SandboxError::Trap(_))
        ));
    }

    #[test]
    fn modules_with_imports_are_rejected() {
        let wasm = br#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32))))"#.to_vec();
        assert!(matches!(
            execute(&wasm, "x", "{}", &limits()),
            Err(SandboxError::Imports(_))
        ));
    }

    #[test]
    fn output_must_be_json() {
        let wasm = module(
            r#"(data (i32.const 16) "kein json")
               (func (export "pa_skill_invoke") (param i32 i32 i32 i32) (result i64)
                 i64.const 68719476745)"#,
        );
        assert!(matches!(
            execute(&wasm, "x", "{}", &limits()),
            Err(SandboxError::Output(_))
        ));
    }
}
