//! Bindet den `pa-inference`-Serversupervisor an den `pa-core::ChatEngine`-Port.
//!
//! Der Adapter kapselt Supervisor, Loopback-Endpunkt und Modelladapter hinter
//! einer einzigen austauschbaren Struktur. `pa-core` sieht ausschließlich den
//! Trait, während der Launcher die konkrete llama.cpp-Verbindung besitzt und
//! sie über [`LlamaServerEngine::reload`] neu starten kann.

use std::time::Duration;

use pa_core::{
    engine::{ChatEngine, EngineReady},
    CoreError,
};
use pa_inference::{
    adapter::ModelAdapter,
    chat::{
        stream_chat_cancelable, stream_chat_cancelable_with_options, ChatOptions, ChatOutcome,
        ChatStreamError, ServerTimings,
    },
    config::ServerConfig,
    loopback::LoopbackEndpoint,
    supervisor::{EnsureStatus, Supervisor},
    InferenceError,
};
use pa_types::chat::{
    Message, ServerTimingsDto, StreamErrorDto, StreamErrorKind, StreamOutcomeDto, ThinkingLevel,
};
use serde_json::Value;

/// Nutzerwerte für Sampling. Der Adapter behält seine Family-spezifischen
/// Vorgaben, bis ausdrücklich ein Profil für dieses Modell gespeichert wurde.
#[derive(Debug, Clone, Copy)]
pub struct SamplingSettings {
    pub temperature: f32,
    pub top_p: f32,
}

struct ConfiguredAdapter<'a> {
    inner: &'a dyn ModelAdapter,
    sampling: Option<SamplingSettings>,
    thinking_level: ThinkingLevel,
}

impl ModelAdapter for ConfiguredAdapter<'_> {
    fn request(&self, messages: &[Message]) -> Result<Value, InferenceError> {
        let mut request = self.inner.request(messages)?;
        if let (Some(sampling), Some(object)) = (self.sampling, request.as_object_mut()) {
            object.insert("temperature".to_owned(), Value::from(sampling.temperature));
            object.insert("top_p".to_owned(), Value::from(sampling.top_p));
        }
        if let Some(object) = request.as_object_mut() {
            // Both fields are supported by the bundled llama-server: the template
            // receives the effort, while the token budget enforces a real limit.
            let (effort, budget) = match self.thinking_level {
                ThinkingLevel::Kurz => ("none", 0),
                ThinkingLevel::Standard => ("low", 128),
                ThinkingLevel::Sorgfaeltig => ("medium", 384),
                ThinkingLevel::Vertieft => ("high", 768),
                ThinkingLevel::Maximal => ("max", 1536),
            };
            object.insert("reasoning_effort".to_owned(), Value::from(effort));
            object.insert("thinking_budget_tokens".to_owned(), Value::from(budget));
        }
        Ok(request)
    }

    fn stop_sequences(&self) -> &[&str] {
        self.inner.stop_sequences()
    }
}

/// Besitzt Supervisor, Endpoint und Modelladapter für einen laufenden llama.cpp-Prozess.
///
/// Aus dem CLI/UI heraus gibt es dadurch nur ein einziges Handle für Server-
/// gesundheit, Streaming und Modell-Reload; das entspricht dem Ablauf, den
/// Schritt 3 im `pa-launcher::main` inline hatte.
pub struct LlamaServerEngine {
    supervisor: Supervisor,
    endpoint: LoopbackEndpoint,
    adapter: pa_inference::adapter::AdapterKind,
    config: ServerConfig,
    ready_timeout: Duration,
    sampling: Option<SamplingSettings>,
    thinking_level: ThinkingLevel,
}

impl LlamaServerEngine {
    /// Übernimmt einen bereits gestarteten Supervisor mit passendem Endpoint.
    ///
    /// Trennt bewusst die Startmessung (die der CLI beim Ladevorgang anzeigt)
    /// vom Adapter, damit die dokumentierten „Bereit nach X s"-Werte nicht in
    /// eine Bibliotheksfunktion verschwinden.
    pub fn from_running(
        supervisor: Supervisor,
        endpoint: LoopbackEndpoint,
        adapter: pa_inference::adapter::AdapterKind,
        config: ServerConfig,
        ready_timeout: Duration,
    ) -> Self {
        Self {
            supervisor,
            endpoint,
            adapter,
            config,
            ready_timeout,
            sampling: None,
            thinking_level: ThinkingLevel::default(),
        }
    }

    /// Ermöglicht `/model`- und `/reload`-Kommandos, ohne den Launcher neu zu starten.
    ///
    /// Vergibt einen neuen Port und ein neues Zugriffstoken, damit der alte
    /// Server keine offen bleibenden Verbindungen erreicht.
    pub fn reload(&mut self) -> Result<(), InferenceError> {
        let (port, token) = ServerConfig::random_endpoint()?;
        self.config.port = port;
        self.config.api_key = token.clone();
        self.supervisor
            .switch_model(self.config.clone(), self.ready_timeout)?;
        self.endpoint = LoopbackEndpoint::new(port, token)?;
        Ok(())
    }

    /// Wechselt erst nach einem erfolgreichen Health-Check auf einen neuen
    /// Modelladapter. Bei einem Startfehler wird die bisherige Konfiguration
    /// erneut gestartet, damit die Sitzung weiter nutzbar bleibt.
    pub fn switch_to(
        &mut self,
        config: ServerConfig,
        adapter: pa_inference::adapter::AdapterKind,
    ) -> Result<(), InferenceError> {
        let endpoint = LoopbackEndpoint::new(config.port, config.api_key.clone())?;
        let previous = self.config.clone();
        if let Err(error) = self
            .supervisor
            .switch_model(config.clone(), self.ready_timeout)
        {
            self.supervisor.switch_model(previous, self.ready_timeout)?;
            return Err(error);
        }
        self.config = config;
        self.endpoint = endpoint;
        self.adapter = adapter;
        Ok(())
    }

    /// Wendet ein gespeichertes Sampling-Profil ab dem nächsten Turn an;
    /// Kontext- und Modellspeicher bleiben vom Requestparameter getrennt.
    pub fn set_sampling(&mut self, sampling: Option<SamplingSettings>) {
        self.sampling = sampling;
    }

    /// Wählt die echte Denkstufe für den nächsten lokalen Chat-Request.
    pub fn set_thinking_level(&mut self, level: ThinkingLevel) {
        self.thinking_level = level;
    }

    /// Beendet den unterliegenden llama.cpp-Prozess sauber.
    pub fn stop(&mut self) -> Result<(), InferenceError> {
        self.supervisor.stop()
    }

    /// Gibt dem CLI Diagnose-Zugriff auf den Modelladapter, ohne die Grenze zu brechen.
    pub fn model_alias(&self) -> &str {
        &self.config.model_alias
    }

    /// Gibt die aktuell aktive Modellkonfiguration frei (Kontext, Threads, KV).
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Variante mit optionaler GBNF-Grammatik; erzwingt strukturierte
    /// Modellausgabe (Konzept 7.2). Wird vom Agenten-Adapter aufgerufen,
    /// um Critic/Verifier deterministisch zu machen. Aktiviert außerdem
    /// `stream_options.include_usage`, damit
    /// [`StreamOutcomeDto::prompt_tokens`] /
    /// [`StreamOutcomeDto::completion_tokens`] echt gefüllt werden.
    pub fn stream_chat_with_grammar(
        &self,
        messages: &[Message],
        grammar: Option<&str>,
        should_continue: &mut dyn FnMut() -> bool,
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> Result<StreamOutcomeDto, StreamErrorDto> {
        let adapter = ConfiguredAdapter {
            inner: &self.adapter,
            sampling: self.sampling,
            thinking_level: self.thinking_level,
        };
        let options = ChatOptions {
            grammar: grammar.map(str::to_owned),
            stream_usage: true,
            images: Vec::new(),
        };
        stream_chat_cancelable_with_options(
            &self.endpoint,
            &adapter,
            messages,
            &options,
            should_continue,
            on_delta,
        )
        .map(convert_outcome)
        .map_err(convert_error)
    }
    /// Beantwortet die letzte Nutzernachricht zusammen mit Bildern (nur mit
    /// geladenem Bildprojektor, siehe `ServerConfig::mmproj`). Die Bilder
    /// stammen aus dem Arbeitsspeicher und werden nur in dieser Anfrage
    /// übertragen; es entsteht weder Datei noch Protokolleintrag.
    pub fn stream_chat_with_images(
        &self,
        messages: &[Message],
        images: &[String],
        should_continue: &mut dyn FnMut() -> bool,
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> Result<StreamOutcomeDto, StreamErrorDto> {
        let adapter = ConfiguredAdapter {
            inner: &self.adapter,
            sampling: self.sampling,
            thinking_level: self.thinking_level,
        };
        let options = ChatOptions {
            grammar: None,
            stream_usage: true,
            images: images.to_vec(),
        };
        stream_chat_cancelable_with_options(
            &self.endpoint,
            &adapter,
            messages,
            &options,
            should_continue,
            on_delta,
        )
        .map(convert_outcome)
        .map_err(convert_error)
    }
}

impl ChatEngine for LlamaServerEngine {
    fn ensure_ready(&mut self) -> Result<EngineReady, CoreError> {
        self.supervisor
            .ensure_running(self.ready_timeout)
            .map(convert_ready)
            .map_err(|error| CoreError::EngineUnavailable(error.to_string()))
    }

    fn stream_chat(
        &self,
        messages: &[Message],
        should_continue: &mut dyn FnMut() -> bool,
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> Result<StreamOutcomeDto, StreamErrorDto> {
        // Family-abhängiger Adapter (Gemma-4 als Default, Llama-3.2 als Zweitmodell).
        let adapter = ConfiguredAdapter {
            inner: &self.adapter,
            sampling: self.sampling,
            thinking_level: self.thinking_level,
        };
        stream_chat_cancelable(
            &self.endpoint,
            &adapter,
            messages,
            should_continue,
            on_delta,
        )
        .map(convert_outcome)
        .map_err(convert_error)
    }
}

fn convert_ready(status: EnsureStatus) -> EngineReady {
    match status {
        EnsureStatus::AlreadyRunning => EngineReady::AlreadyRunning,
        EnsureStatus::Restarted => EngineReady::Restarted,
    }
}

fn convert_outcome(outcome: ChatOutcome) -> StreamOutcomeDto {
    StreamOutcomeDto {
        text: outcome.text,
        aborted: outcome.aborted,
        timings: convert_timings(outcome.timings),
        prompt_tokens: outcome.prompt_tokens,
        completion_tokens: outcome.completion_tokens,
    }
}

fn convert_timings(timings: ServerTimings) -> ServerTimingsDto {
    ServerTimingsDto {
        prompt_ms: timings.prompt_ms,
        prompt_per_second: timings.prompt_per_second,
        predicted_per_second: timings.predicted_per_second,
    }
}

fn convert_error(error: ChatStreamError) -> StreamErrorDto {
    let kind = match error.source.as_ref() {
        InferenceError::Io(_) => StreamErrorKind::Transport,
        InferenceError::InvalidSse(_) => StreamErrorKind::Protocol,
        InferenceError::Http(_) => StreamErrorKind::Protocol,
        InferenceError::UnexpectedExit | InferenceError::Process { .. } => {
            StreamErrorKind::ProcessExited
        }
        InferenceError::Adapter(_) => StreamErrorKind::Adapter,
    };
    StreamErrorDto {
        kind,
        message: error.source.to_string(),
        partial_text: error.partial_text,
        timings: convert_timings(error.timings),
    }
}

#[cfg(test)]
mod sampling_tests {
    use super::*;

    #[test]
    fn model_specific_sampling_overrides_only_request_values() {
        let family = pa_inference::adapter::AdapterKind::from_family("llama-3.2", "local-model");
        let adapter = ConfiguredAdapter {
            inner: &family,
            sampling: Some(SamplingSettings {
                temperature: 0.2,
                top_p: 0.72,
            }),
            thinking_level: ThinkingLevel::Vertieft,
        };
        let request = adapter.request(&[]).expect("valid adapter request");
        assert_eq!(request["temperature"].as_f64(), Some(0.2_f32 as f64));
        assert_eq!(request["top_p"].as_f64(), Some(0.72_f32 as f64));
        assert_eq!(request["model"], "local-model");
        assert_eq!(request["parse_tool_calls"], false);
        assert_eq!(request["reasoning_effort"], "high");
        assert_eq!(request["thinking_budget_tokens"], 768);
    }

    #[test]
    fn thinking_levels_change_the_local_request_budget() {
        let family = pa_inference::adapter::AdapterKind::from_family("gemma-4", "local-model");
        for (level, effort, budget) in [
            (ThinkingLevel::Kurz, "none", 0),
            (ThinkingLevel::Standard, "low", 128),
            (ThinkingLevel::Sorgfaeltig, "medium", 384),
            (ThinkingLevel::Vertieft, "high", 768),
            (ThinkingLevel::Maximal, "max", 1536),
        ] {
            let adapter = ConfiguredAdapter {
                inner: &family,
                sampling: None,
                thinking_level: level,
            };
            let request = adapter.request(&[]).expect("valid adapter request");
            assert_eq!(request["reasoning_effort"], effort);
            assert_eq!(request["thinking_budget_tokens"], budget);
        }
    }
}
