//! Trait-Port zwischen Orchestrator und Inferenzprozess.
//!
//! Der Core sieht Inferenz ausschließlich über [`ChatEngine`], damit
//! `pa-inference` (llama.cpp, HTTP, SSE) austauschbar bleibt und die
//! Crate-Grenze im Konzept Kapitel 5 gewahrt bleibt.

use pa_types::chat::{Message, StreamErrorDto, StreamOutcomeDto};

use crate::CoreError;

/// Unterscheidet einen unverändert laufenden Server von einem eben neu
/// gestarteten, damit die UI dem Nutzer den automatischen Restart nach
/// einem Absturz sichtbar melden kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineReady {
    AlreadyRunning,
    Restarted,
}

/// Ist die einzige Sicht von `pa-core` auf einen konkreten Inferenzserver.
///
/// Implementierungen liegen außerhalb des Cores (typisch in `pa-launcher`).
/// Der Trait ist bewusst objektsicher, damit der Orchestrator eine
/// `&mut dyn ChatEngine` halten kann.
pub trait ChatEngine {
    /// Prüft die Gesundheit des Servers und startet ihn bei Bedarf neu.
    ///
    /// Wird vor jedem Nutzerauftrag aufgerufen, damit ein im Leerlauf
    /// abgestürzter Server nicht erst mit einem verlorenen Request auffliegt.
    fn ensure_ready(&mut self) -> Result<EngineReady, CoreError>;

    /// Streamt den fertig gerenderten Nachrichtenverlauf zeichenweise.
    ///
    /// `on_delta` bekommt jedes Serverdelta und kann mit `false` den Stream
    /// beenden, ohne bereits ausgegebenen Text zu verwerfen. `should_continue`
    /// wird auch während Prompt-/Header-Warten geprüft und trägt so das
    /// externe Abbruchsignal (Ctrl+C, UI-Abbruch-Button) hinein.
    fn stream_chat(
        &self,
        messages: &[Message],
        should_continue: &mut dyn FnMut() -> bool,
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> Result<StreamOutcomeDto, StreamErrorDto>;
}
