use pa_types::{
    chat::{ServerTimingsDto, StreamErrorKind, StreamOutcomeDto},
    ipc::{
        AuditEntryView, AuditFilter, AvailableModel, ConnectorConfig, MailReplyMode,
        ManifestSummary, MemoryExport, MemoryRetrieveResponse, MemoryUpsertRequest,
        SendMessageRequest, SettingsSnapshot, SettingsUpdate, StreamEvent, ThemePreference,
        TierOverrideChange, ToolPromptResponse, ToolStreamEvent, VaultSwitchRequest,
        WorkspaceEntry, WorkspaceListing,
    },
    memory::{Fact, FactCategory, ItemType, MemoryHit},
    model::{HardwareTier, KvQuantization},
};

#[test]
fn manifest_summary_round_trips_byte_counts_and_version() {
    let summary = ManifestSummary {
        version: "phase0-b10930".to_owned(),
        verified_files: 33,
        verified_bytes: 3_152_509_382,
        embedding_model_id: None,
    };
    let json = serde_json::to_string(&summary).expect("serialize");
    let decoded: ManifestSummary = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded, summary);
}

#[test]
fn available_model_marks_default_explicitly() {
    let model = AvailableModel {
        id: "gemma-4-e2b-q4-k-m".to_owned(),
        display_name: "Gemma 4 E2B Instruct Q4_K_M".to_owned(),
        family: "gemma4".to_owned(),
        gguf_bytes: 3_106_738_272,
        sha256: "740185b21d22ceb83a11c3aa62ad5842ef32c70f6096d756bbee85a1e4ec34b8".to_owned(),
        max_context_tokens: 8192,
        is_default: true,
    };
    let decoded: AvailableModel =
        serde_json::from_str(&serde_json::to_string(&model).expect("serialize"))
            .expect("deserialize");
    assert_eq!(decoded, model);
    assert!(decoded.is_default);
}

#[test]
fn stream_event_variants_use_snake_case_kind_discriminator() {
    let events = vec![
        StreamEvent::Started {
            conversation_id: "c".into(),
            user_message_id: "u".into(),
            assistant_message_id: "a".into(),
            engine_restarted: true,
        },
        StreamEvent::Delta {
            assistant_message_id: "a".into(),
            text: "Hallo".into(),
        },
        StreamEvent::Finished {
            assistant_message_id: "a".into(),
            outcome: StreamOutcomeDto {
                text: "Hallo Welt".into(),
                aborted: false,
                timings: ServerTimingsDto::default(),
                prompt_tokens: Some(42),
                completion_tokens: Some(7),
            },
            dropped_older_turns: 1,
        },
        StreamEvent::Failed {
            assistant_message_id: Some("a".into()),
            error_kind: StreamErrorKind::Transport,
            message: "Verbindung verloren".into(),
            partial_text: "T".into(),
            timings: ServerTimingsDto::default(),
        },
    ];
    for event in events {
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("\"kind\":\""));
        let decoded: StreamEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, event);
    }
}

#[test]
fn send_message_request_allows_new_conversation_without_id() {
    let request = SendMessageRequest {
        conversation_id: None,
        content: "Neue Runde".into(),
        thinking_level: pa_types::chat::ThinkingLevel::Standard,
        skill_id: None,
    };
    let json = serde_json::to_string(&request).expect("serialize");
    let decoded: SendMessageRequest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded, request);
    assert!(decoded.conversation_id.is_none());
}

#[test]
fn send_message_request_without_skill_id_still_decodes() {
    // Ältere Oberflächen kennen das Feld nicht.
    let decoded: SendMessageRequest =
        serde_json::from_str(r#"{"conversation_id":null,"content":"Hallo"}"#).expect("decode");
    assert!(decoded.skill_id.is_none());
    let request = SendMessageRequest {
        skill_id: Some("alpha".into()),
        ..decoded
    };
    let json = serde_json::to_string(&request).expect("serialize");
    assert!(json.contains("\"skill_id\":\"alpha\""));
}

#[test]
fn settings_snapshot_round_trip_preserves_tier_override_and_theme() {
    let snapshot = SettingsSnapshot {
        tier_override: Some(HardwareTier::T0),
        model_id: "gemma-4-e2b-q4-k-m".into(),
        context_tokens: 8192,
        kv_quantization: KvQuantization::F16,
        theme: ThemePreference::Dark,
        vault_path: "AI/data/vault.db".into(),
    };
    let decoded: SettingsSnapshot =
        serde_json::from_str(&serde_json::to_string(&snapshot).expect("serialize"))
            .expect("deserialize");
    assert_eq!(decoded, snapshot);
}

#[test]
fn settings_update_defaults_leave_all_fields_untouched() {
    let update = SettingsUpdate::default();
    let json = serde_json::to_string(&update).expect("serialize");
    let decoded: SettingsUpdate = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded, update);
    assert!(update.tier_override.is_none());
    assert!(update.context_tokens.is_none());
    assert!(update.theme.is_none());
}

#[test]
fn tier_override_change_distinguishes_clear_from_set() {
    let clear = TierOverrideChange::Clear;
    let set = TierOverrideChange::Set {
        tier: HardwareTier::T2,
    };
    let clear_json = serde_json::to_string(&clear).expect("serialize clear");
    let set_json = serde_json::to_string(&set).expect("serialize set");
    assert!(clear_json.contains("\"op\":\"clear\""));
    assert!(set_json.contains("\"op\":\"set\""));
    assert!(set_json.contains("\"tier\":\"t2\""));
    let clear_decoded: TierOverrideChange = serde_json::from_str(&clear_json).unwrap();
    let set_decoded: TierOverrideChange = serde_json::from_str(&set_json).unwrap();
    assert_eq!(clear_decoded, clear);
    assert_eq!(set_decoded, set);
}

#[test]
fn vault_switch_request_carries_passphrase_but_is_owned_by_backend() {
    let request = VaultSwitchRequest {
        new_vault_path: "D:/USB/AI/data/vault.db".into(),
        passphrase: "geheim".into(),
        create_if_missing: false,
    };
    let json = serde_json::to_string(&request).expect("serialize");
    let decoded: VaultSwitchRequest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded, request);
    assert_eq!(decoded.passphrase, "geheim");
}

#[test]
fn tool_stream_event_variants_use_snake_case_kind_discriminator() {
    let events = vec![
        ToolStreamEvent::ToolCall {
            assistant_message_id: "a".into(),
            tool: "read_file".into(),
            arguments_json: r#"{"path":"a.txt"}"#.into(),
        },
        ToolStreamEvent::ToolResult {
            assistant_message_id: "a".into(),
            tool: "read_file".into(),
            content: "geheim".into(),
            is_untrusted: true,
        },
        ToolStreamEvent::ToolError {
            assistant_message_id: "a".into(),
            tool: "write_file".into(),
            message: "M0 verbietet Schreiben".into(),
        },
        ToolStreamEvent::PermissionRequested {
            pending_id: "p-1".into(),
            assistant_message_id: "a".into(),
            tool: "write_file".into(),
            arguments_json: r#"{"path":"out.txt"}"#.into(),
            reason: "bewusste Bestätigung nötig".into(),
        },
    ];
    for event in events {
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(
            json.contains("\"kind\":\""),
            "erwartet snake-case-Discriminator, war {json}"
        );
        let decoded: ToolStreamEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded, event);
    }
}

#[test]
fn tool_prompt_response_carries_session_scope() {
    let response = ToolPromptResponse {
        pending_id: "p-1".into(),
        allow: true,
        remember_for_session: true,
    };
    let decoded: ToolPromptResponse =
        serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
    assert_eq!(decoded, response);
}

#[test]
fn memory_upsert_and_retrieve_round_trip() {
    let request = MemoryUpsertRequest {
        id: None,
        text: "Nutzer arbeitet mit Rust".into(),
        category: FactCategory::Skill,
        user_verified: true,
    };
    let decoded: MemoryUpsertRequest =
        serde_json::from_str(&serde_json::to_string(&request).unwrap()).unwrap();
    assert_eq!(decoded, request);

    let response = MemoryRetrieveResponse {
        query: "Rust".into(),
        hits: vec![MemoryHit {
            item_type: ItemType::Fact,
            item_id: "f-1".into(),
            score: 0.123,
            preview: "Nutzer arbeitet mit Rust".into(),
        }],
    };
    let decoded: MemoryRetrieveResponse =
        serde_json::from_str(&serde_json::to_string(&response).unwrap()).unwrap();
    assert_eq!(decoded, response);
}

#[test]
fn memory_export_serializes_all_bookkeeping_fields() {
    let export = MemoryExport {
        exported_at_unix_ms: 42,
        facts: vec![Fact {
            id: "f-1".into(),
            text: "hello".into(),
            category: FactCategory::Preference,
            confidence: 0.9,
            source_message_id: Some("m-1".into()),
            valid_from_unix_ms: 1,
            valid_until_unix_ms: None,
            superseded_by: None,
            user_verified: true,
            access_count: 3,
            last_accessed_unix_ms: Some(5),
        }],
    };
    let decoded: MemoryExport =
        serde_json::from_str(&serde_json::to_string(&export).unwrap()).unwrap();
    assert_eq!(decoded, export);
}

#[test]
fn audit_filter_defaults_are_empty() {
    let default = AuditFilter::default();
    assert!(default.actions.is_empty());
    assert!(default.outcomes.is_empty());
    assert!(default.contains.is_none());

    let filter = AuditFilter {
        actions: vec!["file_read".into()],
        outcomes: vec!["allow".into(), "deny".into()],
        contains: Some("Passphrase".into()),
        since_unix_ms: Some(1000),
        limit: Some(200),
    };
    let decoded: AuditFilter =
        serde_json::from_str(&serde_json::to_string(&filter).unwrap()).unwrap();
    assert_eq!(decoded, filter);
}

#[test]
fn audit_entry_view_carries_hex_hashes_for_ui_display() {
    let entry = AuditEntryView {
        id: 1,
        created_unix_ms: 10,
        mode: "m1_workspace".into(),
        action: "file_read".into(),
        target: Some("hello.txt".into()),
        outcome: "allow".into(),
        reason: "OK".into(),
        hash_hex: "a".repeat(64),
        prev_hash_hex: "0".repeat(64),
    };
    let decoded: AuditEntryView =
        serde_json::from_str(&serde_json::to_string(&entry).unwrap()).unwrap();
    assert_eq!(decoded, entry);
}

#[test]
fn workspace_listing_is_stable_across_paths() {
    let listing = WorkspaceListing {
        root: "/workspace".into(),
        relative_path: "docs".into(),
        entries: vec![
            WorkspaceEntry {
                name: "readme.md".into(),
                relative_path: "docs/readme.md".into(),
                is_directory: false,
                bytes: 120,
                modified_unix_ms: Some(1),
            },
            WorkspaceEntry {
                name: "images".into(),
                relative_path: "docs/images".into(),
                is_directory: true,
                bytes: 0,
                modified_unix_ms: None,
            },
        ],
    };
    let decoded: WorkspaceListing =
        serde_json::from_str(&serde_json::to_string(&listing).unwrap()).unwrap();
    assert_eq!(decoded, listing);
}

#[test]
fn connector_config_round_trip_and_defaults() {
    let default_cfg = ConnectorConfig::default();
    assert!(default_cfg.offline_mode);
    assert!(!default_cfg.exa_enabled);
    assert!(!default_cfg.gmail_enabled);
    assert_eq!(default_cfg.gmail_check_interval_minutes, 5);
    assert_eq!(default_cfg.gmail_reply_mode, MailReplyMode::Draft);
    assert!(!default_cfg.gmail_send_acknowledged && !default_cfg.gmail_allow_read);
    assert_eq!(
        (
            default_cfg.gmail_max_per_hour,
            default_cfg.gmail_max_per_day
        ),
        (6, 30)
    );

    let custom_cfg = ConnectorConfig {
        offline_mode: false,
        exa_enabled: true,
        exa_api_key: "test-key-123".into(),
        wikipedia_enabled: true,
        open_meteo_enabled: true,
        brave_enabled: true,
        brave_api_key: "brave-key".into(),
        gmail_enabled: true,
        gmail_address: "bot@gmail.com".into(),
        gmail_target_email: "boss@example.com".into(),
        gmail_check_interval_minutes: 10,
        gmail_reply_mode: MailReplyMode::Send,
        gmail_send_acknowledged: true,
        gmail_max_per_hour: 3,
        gmail_max_per_day: 10,
        gmail_instruction: "Antworte auf Englisch.".into(),
        gmail_allow_read: true,
    };
    let json = serde_json::to_string(&custom_cfg).expect("serialize");
    let decoded: ConnectorConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded, custom_cfg);
}

#[test]
fn an_old_stored_connector_config_without_new_fields_still_loads() {
    // So sah die gespeicherte Konfiguration vor Wikipedia, Open-Meteo und Brave aus.
    let old = r#"{"offline_mode":true,"exa_enabled":true,"exa_api_key":"k","whatsapp_enabled":false,
        "whatsapp_phone_number":"","gmail_enabled":false,"gmail_auto_reply_enabled":false,
        "gmail_target_email":"","gmail_check_interval_minutes":10}"#;
    let decoded: ConnectorConfig = serde_json::from_str(old).expect("alte Konfiguration");
    assert!(decoded.exa_enabled && decoded.exa_api_key == "k");
    assert!(!decoded.wikipedia_enabled && !decoded.open_meteo_enabled && !decoded.brave_enabled);
    assert!(decoded.brave_api_key.is_empty());
    // Auch eine ganz leere Konfiguration fällt auf sichere Standardwerte zurück (Air Gap an).
    let empty: ConnectorConfig = serde_json::from_str("{}").expect("leer");
    assert!(empty.offline_mode);
    // Die alten Felder (WhatsApp, Auto-Antwort-Schalter) werden ohne Fehler ignoriert; die
    // Auto-Antwort startet nie aus einer gespeicherten Datei, sondern nur per Schalter.
    assert!(!decoded.gmail_enabled && decoded.gmail_reply_mode == MailReplyMode::Draft);
}

#[test]
fn debug_output_never_shows_mail_settings_beyond_the_switch() {
    let config = ConnectorConfig {
        gmail_address: "geheim@gmail.com".into(),
        ..ConnectorConfig::default()
    };
    assert!(!format!("{config:?}").contains("geheim@gmail.com"));
}

#[test]
fn the_debug_output_never_shows_any_key() {
    let cfg = ConnectorConfig {
        exa_api_key: "GEHEIM-EXA".into(),
        brave_api_key: "GEHEIM-BRAVE".into(),
        ..ConnectorConfig::default()
    };
    let text = format!("{cfg:?}");
    assert!(!text.contains("GEHEIM"), "{text}");
}
