use pa_inference::{
    adapter::{AdapterKind, ModelAdapter},
    gemma4::Gemma4Adapter,
};
use pa_types::chat::{Message, MessageRole, MessageStatus};

#[test]
fn gemma_request_has_a_stable_prefix_and_explicitly_disables_tools() {
    let adapter = Gemma4Adapter::new("gemma-4-e2b-q4-k-m");
    let messages = vec![Message {
        id: "m1".to_owned(),
        conversation_id: "c1".to_owned(),
        position: 0,
        role: MessageRole::User,
        content: "Hallo".to_owned(),
        status: MessageStatus::Complete,
        created_at_unix_ms: 1,
    }];

    let request = adapter.request(&messages).expect("request");
    assert_eq!(request["model"], "gemma-4-e2b-q4-k-m");
    assert_eq!(request["stream"], true);
    assert_eq!(request["parse_tool_calls"], false);
    assert_eq!(request["messages"][0]["role"], "system");
    assert_eq!(request["messages"][1]["content"], "Hallo");
    assert_eq!(adapter.stop_sequences(), &["<end_of_turn>"]);
}

#[test]
fn qwen3_family_uses_its_chat_end_token() {
    let adapter = AdapterKind::from_family("qwen3", "qwen3-4b");
    let request = adapter
        .request(&[Message {
            id: "m1".to_owned(),
            conversation_id: "c1".to_owned(),
            position: 0,
            role: MessageRole::User,
            content: "Hallo".to_owned(),
            status: MessageStatus::Complete,
            created_at_unix_ms: 1,
        }])
        .expect("request");

    assert_eq!(request["model"], "qwen3-4b");
    assert_eq!(request["stop"], serde_json::json!(["<|im_end|>"]));
    assert_eq!(request["messages"][1]["content"], "Hallo");
}

#[test]
fn ministral3_family_uses_conservative_sampling_and_server_template() {
    let adapter = AdapterKind::from_family("ministral3", "ministral3-3b");
    let request = adapter
        .request(&[Message {
            id: "m1".to_owned(),
            conversation_id: "c1".to_owned(),
            position: 0,
            role: MessageRole::User,
            content: "Hallo".to_owned(),
            status: MessageStatus::Complete,
            created_at_unix_ms: 1,
        }])
        .expect("request");

    assert_eq!(request["model"], "ministral3-3b");
    assert_eq!(request["temperature"], 0.05);
    assert!(request.get("stop").is_none());
    assert_eq!(request["messages"][1]["content"], "Hallo");
}
