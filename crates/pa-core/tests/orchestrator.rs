use std::collections::HashMap;

use pa_core::{
    budget::BudgetPolicy,
    conversation::Conversations,
    engine::{ChatEngine, EngineReady},
    orchestrator::{ChatOrchestrator, TurnCallbacks, TurnError, TurnRequest},
    prompt::PromptPrefix,
    CoreError,
};
use pa_types::chat::{
    Conversation, Message, MessageRole, MessageStatus, ServerTimingsDto, StreamErrorDto,
    StreamErrorKind, StreamOutcomeDto,
};

#[derive(Default)]
struct MemoryConversations {
    conversations: Vec<Conversation>,
    messages: HashMap<String, Vec<Message>>,
}

impl Conversations for MemoryConversations {
    fn create(&mut self, id: &str, title: &str, now_unix_ms: i64) -> Result<(), CoreError> {
        self.conversations.push(Conversation {
            id: id.to_owned(),
            title: title.to_owned(),
            created_at_unix_ms: now_unix_ms,
            updated_at_unix_ms: now_unix_ms,
            project_id: None,
        });
        self.messages.insert(id.to_owned(), Vec::new());
        Ok(())
    }

    fn list(&self) -> Result<Vec<Conversation>, CoreError> {
        Ok(self.conversations.clone())
    }

    fn messages(&self, conversation_id: &str) -> Result<Vec<Message>, CoreError> {
        Ok(self
            .messages
            .get(conversation_id)
            .cloned()
            .unwrap_or_default())
    }

    fn rename(&mut self, id: &str, title: &str, now_unix_ms: i64) -> Result<(), CoreError> {
        for conversation in &mut self.conversations {
            if conversation.id == id {
                conversation.title = title.to_owned();
                conversation.updated_at_unix_ms = now_unix_ms;
            }
        }
        Ok(())
    }

    fn delete(&mut self, id: &str) -> Result<(), CoreError> {
        self.conversations
            .retain(|conversation| conversation.id != id);
        self.messages.remove(id);
        Ok(())
    }

    fn append(
        &mut self,
        id: &str,
        conversation_id: &str,
        role: MessageRole,
        content: &str,
        status: MessageStatus,
        now_unix_ms: i64,
    ) -> Result<(), CoreError> {
        let entry = self.messages.entry(conversation_id.to_owned()).or_default();
        let position = entry.len() as i64;
        entry.push(Message {
            id: id.to_owned(),
            conversation_id: conversation_id.to_owned(),
            position,
            role,
            content: content.to_owned(),
            status,
            created_at_unix_ms: now_unix_ms,
        });
        Ok(())
    }

    fn finish(&mut self, id: &str, content: &str, status: MessageStatus) -> Result<(), CoreError> {
        for messages in self.messages.values_mut() {
            for message in messages.iter_mut() {
                if message.id == id {
                    message.content = content.to_owned();
                    message.status = status;
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

struct ScriptedEngine {
    ready: EngineReady,
    deltas: Vec<&'static str>,
    aborted: bool,
    fail_stream: Option<StreamErrorDto>,
}

impl ChatEngine for ScriptedEngine {
    fn ensure_ready(&mut self) -> Result<EngineReady, CoreError> {
        Ok(self.ready)
    }

    fn stream_chat(
        &self,
        _messages: &[Message],
        should_continue: &mut dyn FnMut() -> bool,
        on_delta: &mut dyn FnMut(&str) -> bool,
    ) -> Result<StreamOutcomeDto, StreamErrorDto> {
        if let Some(error) = &self.fail_stream {
            let mut partial = String::new();
            for delta in &self.deltas {
                if !should_continue() {
                    break;
                }
                on_delta(delta);
                partial.push_str(delta);
            }
            let mut error = error.clone();
            error.partial_text = partial;
            return Err(error);
        }
        let mut text = String::new();
        for delta in &self.deltas {
            if !should_continue() {
                break;
            }
            on_delta(delta);
            text.push_str(delta);
        }
        Ok(StreamOutcomeDto {
            text,
            aborted: self.aborted || !should_continue(),
            timings: ServerTimingsDto::default(),
            prompt_tokens: None,
            completion_tokens: None,
        })
    }
}

fn base_request<'a>(
    conversation_id: &'a str,
    user_id: &'a str,
    assistant_id: &'a str,
    input: &'a str,
) -> TurnRequest<'a> {
    TurnRequest {
        conversation_id,
        user_message_id: user_id,
        assistant_message_id: assistant_id,
        user_input: input,
        now_unix_ms: 1_000_000,
    }
}

#[test]
fn happy_path_streams_deltas_and_persists_final_content_as_complete() {
    let mut conversations = MemoryConversations::default();
    conversations.create("c1", "Test", 500_000).unwrap();
    let mut engine = ScriptedEngine {
        ready: EngineReady::AlreadyRunning,
        deltas: vec!["Hallo ", "Welt"],
        aborted: false,
        fail_stream: None,
    };

    let orchestrator = ChatOrchestrator::new(PromptPrefix::empty(), BudgetPolicy::new(1024));
    let mut collected = String::new();
    let mut engine_status_seen: Option<EngineReady> = None;
    let mut always_true = || true;
    let mut on_delta = |delta: &str| collected.push_str(delta);
    let mut on_ready = |status: EngineReady| engine_status_seen = Some(status);
    let mut callbacks = TurnCallbacks {
        should_continue: &mut always_true,
        on_delta: &mut on_delta,
        on_engine_ready: &mut on_ready,
    };

    let outcome = orchestrator
        .run_turn(
            &mut conversations,
            &mut engine,
            base_request("c1", "u1", "a1", "Ping?"),
            &mut callbacks,
        )
        .expect("turn");
    assert_eq!(collected, "Hallo Welt");
    assert_eq!(outcome.outcome.text, "Hallo Welt");
    assert!(!outcome.outcome.aborted);
    assert_eq!(engine_status_seen, Some(EngineReady::AlreadyRunning));

    let messages = conversations.messages("c1").unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "Ping?");
    assert_eq!(messages[0].status, MessageStatus::Complete);
    assert_eq!(messages[1].content, "Hallo Welt");
    assert_eq!(messages[1].status, MessageStatus::Complete);
}

#[test]
fn engine_restart_status_is_forwarded_before_message_persistence() {
    let mut conversations = MemoryConversations::default();
    conversations.create("c1", "Test", 500_000).unwrap();
    let mut engine = ScriptedEngine {
        ready: EngineReady::Restarted,
        deltas: vec!["ok"],
        aborted: false,
        fail_stream: None,
    };

    let orchestrator = ChatOrchestrator::new(PromptPrefix::empty(), BudgetPolicy::new(1024));
    let mut engine_status_seen: Option<EngineReady> = None;
    let mut always_true = || true;
    let mut on_delta = |_: &str| {};
    let mut on_ready = |status: EngineReady| engine_status_seen = Some(status);
    let mut callbacks = TurnCallbacks {
        should_continue: &mut always_true,
        on_delta: &mut on_delta,
        on_engine_ready: &mut on_ready,
    };

    orchestrator
        .run_turn(
            &mut conversations,
            &mut engine,
            base_request("c1", "u1", "a1", "Ping"),
            &mut callbacks,
        )
        .expect("turn");
    assert_eq!(engine_status_seen, Some(EngineReady::Restarted));
}

#[test]
fn abort_during_stream_preserves_partial_text_as_aborted_message() {
    let mut conversations = MemoryConversations::default();
    conversations.create("c1", "Test", 500_000).unwrap();
    let mut engine = ScriptedEngine {
        ready: EngineReady::AlreadyRunning,
        deltas: vec!["Beginn", "…"],
        aborted: true, // Server meldet Abbruch.
        fail_stream: None,
    };

    let orchestrator = ChatOrchestrator::new(PromptPrefix::empty(), BudgetPolicy::new(1024));
    let mut delta_count = 0_u8;
    let mut should_continue = || {
        delta_count += 1;
        delta_count <= 1 // Nach dem ersten Delta abbrechen.
    };
    let mut collected = String::new();
    let mut on_delta = |delta: &str| collected.push_str(delta);
    let mut on_ready = |_: EngineReady| {};
    let mut callbacks = TurnCallbacks {
        should_continue: &mut should_continue,
        on_delta: &mut on_delta,
        on_engine_ready: &mut on_ready,
    };

    let outcome = orchestrator
        .run_turn(
            &mut conversations,
            &mut engine,
            base_request("c1", "u1", "a1", "Frage"),
            &mut callbacks,
        )
        .expect("turn");
    assert!(outcome.outcome.aborted);
    assert_eq!(outcome.outcome.text, "Beginn");
    assert_eq!(collected, "Beginn");

    let messages = conversations.messages("c1").unwrap();
    assert_eq!(messages[1].status, MessageStatus::Aborted);
    assert_eq!(messages[1].content, "Beginn");
}

#[test]
fn stream_transport_error_persists_partial_text_and_returns_stream_error() {
    let mut conversations = MemoryConversations::default();
    conversations.create("c1", "Test", 500_000).unwrap();
    let mut engine = ScriptedEngine {
        ready: EngineReady::AlreadyRunning,
        deltas: vec!["Teil"],
        aborted: false,
        fail_stream: Some(StreamErrorDto {
            kind: StreamErrorKind::Transport,
            message: "Verbindung verloren".to_owned(),
            partial_text: String::new(),
            timings: ServerTimingsDto::default(),
        }),
    };

    let orchestrator = ChatOrchestrator::new(PromptPrefix::empty(), BudgetPolicy::new(1024));
    let mut always_true = || true;
    let mut collected = String::new();
    let mut on_delta = |delta: &str| collected.push_str(delta);
    let mut on_ready = |_: EngineReady| {};
    let mut callbacks = TurnCallbacks {
        should_continue: &mut always_true,
        on_delta: &mut on_delta,
        on_engine_ready: &mut on_ready,
    };

    let error = orchestrator
        .run_turn(
            &mut conversations,
            &mut engine,
            base_request("c1", "u1", "a1", "Ping"),
            &mut callbacks,
        )
        .expect_err("expected transport error");

    match error {
        TurnError::Stream(failure) => {
            assert_eq!(failure.source.kind, StreamErrorKind::Transport);
            assert_eq!(failure.source.partial_text, "Teil");
            assert_eq!(failure.assistant_message_id, "a1");
        }
        other => panic!("unerwartete Fehlerart: {other:?}"),
    }

    let messages = conversations.messages("c1").unwrap();
    assert_eq!(messages[1].content, "Teil");
    assert_eq!(messages[1].status, MessageStatus::Aborted);
}
