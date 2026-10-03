use pa_types::chat::{ServerTimingsDto, StreamErrorDto, StreamErrorKind, StreamOutcomeDto};

#[test]
fn server_timings_round_trips_missing_values_as_none() {
    let timings = ServerTimingsDto {
        prompt_ms: Some(460.334),
        prompt_per_second: None,
        predicted_per_second: Some(16.869),
    };
    let json = serde_json::to_string(&timings).expect("timings must serialize");
    let decoded: ServerTimingsDto = serde_json::from_str(&json).expect("timings must deserialize");
    assert_eq!(decoded, timings);
}

#[test]
fn stream_outcome_dto_round_trips_aborted_partial_text() {
    let outcome = StreamOutcomeDto {
        text: "Antwort bis Listenpunkt 32".to_owned(),
        aborted: true,
        timings: ServerTimingsDto::default(),
        prompt_tokens: Some(120),
        completion_tokens: Some(32),
    };
    let json = serde_json::to_string(&outcome).expect("outcome must serialize");
    let decoded: StreamOutcomeDto = serde_json::from_str(&json).expect("outcome must deserialize");
    assert_eq!(decoded, outcome);
}

#[test]
fn stream_error_dto_preserves_partial_text_and_classification() {
    let error = StreamErrorDto {
        kind: StreamErrorKind::Transport,
        message: "Verbindung verloren".to_owned(),
        partial_text: "Teil bereits geschrieben".to_owned(),
        timings: ServerTimingsDto {
            prompt_ms: Some(770.968),
            prompt_per_second: None,
            predicted_per_second: None,
        },
    };
    let json = serde_json::to_string(&error).expect("error must serialize");
    let decoded: StreamErrorDto = serde_json::from_str(&json).expect("error must deserialize");
    assert_eq!(decoded, error);
    assert_eq!(decoded.partial_text, "Teil bereits geschrieben");
    assert_eq!(decoded.kind, StreamErrorKind::Transport);
}
