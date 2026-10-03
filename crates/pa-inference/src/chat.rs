use pa_types::chat::Message;
use serde_json::Value;
use std::{error::Error, fmt};

use crate::{adapter::ModelAdapter, loopback::LoopbackEndpoint, sse::SseEvent, InferenceError};

/// Übernimmt ausschließlich vom Server tatsächlich gemeldete Raten.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ServerTimings {
    pub prompt_ms: Option<f64>,
    pub prompt_per_second: Option<f64>,
    pub predicted_per_second: Option<f64>,
}

/// Optionale Sampling-Zusatzparameter für den `/v1/chat/completions`-Endpunkt.
///
/// `grammar` wird als GBNF-Textfeld an den Server durchgereicht (llama.cpp-
/// Extension: Body-Feld `"grammar"`). `None` = keine Grammatik. `stream_usage`
/// schaltet die `stream_options.include_usage`-Signalisierung an, damit der
/// Server pro Antwort ein `usage`-Objekt mitliefert.
#[derive(Debug, Clone, Default)]
pub struct ChatOptions {
    pub grammar: Option<String>,
    pub stream_usage: bool,
    /// Bilder als `data:`-URLs für die **letzte Nutzernachricht**. Der Server
    /// braucht dafür einen geladenen Bildprojektor; ohne ihn lehnt er die
    /// Anfrage ab. Die Bilder bleiben im Arbeitsspeicher der Anfrage.
    pub images: Vec<String>,
}

/// Bewahrt auch bei bewusst geschlossenem Stream den bereits empfangenen Text.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatOutcome {
    pub text: String,
    pub aborted: bool,
    pub timings: ServerTimings,
    /// Prompt-Tokens laut Server-`usage`. `None`, wenn der Server keine
    /// `usage`-Statistik geschickt hat (z. B. weil `stream_usage=false` war).
    pub prompt_tokens: Option<u32>,
    /// Completion-Tokens laut Server-`usage`. Gleiche Regel.
    pub completion_tokens: Option<u32>,
}

/// Trägt Transportfehler zusammen mit bereits sichtbar ausgeliefertem Text zur Persistenzgrenze.
#[derive(Debug)]
pub struct ChatStreamError {
    pub source: Box<InferenceError>,
    pub partial_text: String,
    pub timings: ServerTimings,
}

impl fmt::Display for ChatStreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.source.fmt(formatter)
    }
}

impl Error for ChatStreamError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// Streamt jedes Serverdelta sofort und beendet bei Callback-Abbruch die TCP-Verbindung.
pub fn stream_chat(
    endpoint: &LoopbackEndpoint,
    adapter: &dyn ModelAdapter,
    messages: &[Message],
    on_delta: impl FnMut(&str) -> bool,
) -> Result<ChatOutcome, ChatStreamError> {
    stream_chat_cancelable(endpoint, adapter, messages, || true, on_delta)
}

/// Ermöglicht Abbruch bereits während Promptverarbeitung oder gepufferter HTTP-Ausgabe.
pub fn stream_chat_cancelable(
    endpoint: &LoopbackEndpoint,
    adapter: &dyn ModelAdapter,
    messages: &[Message],
    should_continue: impl FnMut() -> bool,
    on_delta: impl FnMut(&str) -> bool,
) -> Result<ChatOutcome, ChatStreamError> {
    stream_chat_cancelable_with_options(
        endpoint,
        adapter,
        messages,
        &ChatOptions::default(),
        should_continue,
        on_delta,
    )
}

/// Variante mit Sampling-Optionen; reicht `grammar` und `stream_options` an
/// den Server durch. `grammar=Some(...)` erzwingt strukturierte Ausgabe
/// nach der übergebenen GBNF-Grammatik. `stream_usage=true` bittet den
/// Server, ein `usage`-Objekt in den Stream zu mischen, damit
/// [`ChatOutcome::prompt_tokens`] / [`ChatOutcome::completion_tokens`]
/// echte Server-Zählungen führen können.
pub fn stream_chat_cancelable_with_options(
    endpoint: &LoopbackEndpoint,
    adapter: &dyn ModelAdapter,
    messages: &[Message],
    options: &ChatOptions,
    should_continue: impl FnMut() -> bool,
    mut on_delta: impl FnMut(&str) -> bool,
) -> Result<ChatOutcome, ChatStreamError> {
    let mut request = adapter
        .request(messages)
        .map_err(|source| stream_error(source, String::new(), ServerTimings::default()))?;
    apply_options_to_request(&mut request, options)?;
    let body = serde_json::to_vec(&request).map_err(|error| {
        stream_error(
            InferenceError::Adapter(error.to_string()),
            String::new(),
            ServerTimings::default(),
        )
    })?;
    let mut text = String::new();
    let mut timings = ServerTimings::default();
    let mut prompt_tokens: Option<u32> = None;
    let mut completion_tokens: Option<u32> = None;
    let mut saw_done = false;
    let mut protocol_error = None;
    let transport_completed = endpoint
        .post_sse_cancelable(
            "/v1/chat/completions",
            &body,
            should_continue,
            |event| match event {
                SseEvent::Done => {
                    saw_done = true;
                    false
                }
                SseEvent::Data(data) => {
                    let value: Value = match serde_json::from_str(&data) {
                        Ok(value) => value,
                        Err(error) => {
                            protocol_error = Some(InferenceError::InvalidSse(error.to_string()));
                            return false;
                        }
                    };
                    if let Some(value) = value.pointer("/timings/prompt_ms").and_then(Value::as_f64)
                    {
                        timings.prompt_ms = Some(value);
                    }
                    if let Some(value) = value
                        .pointer("/timings/prompt_per_second")
                        .and_then(Value::as_f64)
                    {
                        timings.prompt_per_second = Some(value);
                    }
                    if let Some(value) = value
                        .pointer("/timings/predicted_per_second")
                        .and_then(Value::as_f64)
                    {
                        timings.predicted_per_second = Some(value);
                    }
                    if let Some(count) = value
                        .pointer("/usage/prompt_tokens")
                        .and_then(Value::as_u64)
                    {
                        prompt_tokens = Some(u32::try_from(count).unwrap_or(u32::MAX));
                    }
                    if let Some(count) = value
                        .pointer("/usage/completion_tokens")
                        .and_then(Value::as_u64)
                    {
                        completion_tokens = Some(u32::try_from(count).unwrap_or(u32::MAX));
                    }
                    if let Some(delta) = value
                        .pointer("/choices/0/delta/content")
                        .and_then(Value::as_str)
                    {
                        text.push_str(delta);
                        on_delta(delta)
                    } else {
                        true
                    }
                }
            },
        )
        .map_err(|source| stream_error(source, text.clone(), timings))?;
    if let Some(source) = protocol_error {
        return Err(stream_error(source, text, timings));
    }
    if transport_completed && !saw_done {
        return Err(stream_error(
            InferenceError::InvalidSse("Stream endete ohne [DONE]".to_owned()),
            text,
            timings,
        ));
    }
    Ok(ChatOutcome {
        text,
        aborted: !transport_completed && !saw_done,
        timings,
        prompt_tokens,
        completion_tokens,
    })
}

fn apply_options_to_request(
    request: &mut Value,
    options: &ChatOptions,
) -> Result<(), ChatStreamError> {
    let object = request.as_object_mut().ok_or_else(|| {
        stream_error(
            InferenceError::Adapter("Adapter lieferte kein JSON-Objekt".to_owned()),
            String::new(),
            ServerTimings::default(),
        )
    })?;
    if let Some(grammar) = &options.grammar {
        // llama.cpp-Extension am OpenAI-kompatiblen Endpunkt: Feld `grammar`
        // mit GBNF-Grammatiktext (siehe llama-server-Referenz).
        object.insert("grammar".to_owned(), Value::String(grammar.clone()));
    }
    if !options.images.is_empty() {
        attach_images(object, &options.images)
            .map_err(|source| stream_error(source, String::new(), ServerTimings::default()))?;
    }
    if options.stream_usage {
        // OpenAI-Feld: `stream_options.include_usage=true` schaltet
        // pro-Request die `usage`-Statistik im Stream frei.
        let stream_options = object
            .entry("stream_options".to_owned())
            .or_insert_with(|| Value::Object(Default::default()));
        if let Some(map) = stream_options.as_object_mut() {
            map.insert("include_usage".to_owned(), Value::Bool(true));
        }
    }
    Ok(())
}

/// Macht aus dem Text der letzten Nutzernachricht eine Inhaltsliste mit Text
/// und Bildern (OpenAI-Format, das `llama-server` mit `--mmproj` versteht).
fn attach_images(
    request: &mut serde_json::Map<String, Value>,
    images: &[String],
) -> Result<(), InferenceError> {
    let messages = request
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| InferenceError::Adapter("Anfrage ohne Nachrichten".to_owned()))?;
    let last_user = messages
        .iter_mut()
        .rev()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .ok_or_else(|| InferenceError::Adapter("Keine Nutzernachricht für das Bild".to_owned()))?;
    let text = last_user
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mut parts = vec![serde_json::json!({"type": "text", "text": text})];
    for url in images {
        if !url.starts_with("data:image/") {
            return Err(InferenceError::Adapter(
                "Bilder müssen als data:image/-URL übergeben werden".to_owned(),
            ));
        }
        parts.push(serde_json::json!({"type": "image_url", "image_url": {"url": url}}));
    }
    last_user["content"] = Value::Array(parts);
    Ok(())
}

fn stream_error(
    source: InferenceError,
    partial_text: String,
    timings: ServerTimings,
) -> ChatStreamError {
    ChatStreamError {
        source: Box::new(source),
        partial_text,
        timings,
    }
}

#[cfg(test)]
mod image_tests {
    use super::*;
    use serde_json::json;

    fn request() -> serde_json::Map<String, Value> {
        json!({"messages": [
            {"role": "system", "content": "s"},
            {"role": "user", "content": "Frage"},
            {"role": "assistant", "content": "a"},
            {"role": "user", "content": "Was siehst du?"}
        ]})
        .as_object()
        .cloned()
        .expect("Objekt")
    }

    #[test]
    fn images_go_into_the_last_user_message_only() {
        let mut request = request();
        attach_images(&mut request, &["data:image/png;base64,AAAA".to_owned()]).expect("anhängen");
        let messages = request["messages"].as_array().expect("Liste");
        assert_eq!(messages[1]["content"], "Frage");
        let parts = messages[3]["content"].as_array().expect("Teile");
        assert_eq!(parts[0]["text"], "Was siehst du?");
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(parts[1]["image_url"]["url"], "data:image/png;base64,AAAA");
    }

    #[test]
    fn non_data_urls_are_refused() {
        let mut request = request();
        assert!(attach_images(&mut request, &["https://example.org/x.png".to_owned()]).is_err());
    }
}
