use pa_inference::sse::{SseDecoder, SseEvent};

#[test]
fn decodes_fragmented_utf8_multiline_keepalive_and_done() {
    let mut decoder = SseDecoder::new();
    let euro = "€".as_bytes();
    let mut first = b": keepalive\r\ndata: hello\r\ndata: ".to_vec();
    first.extend_from_slice(&euro[..2]);
    assert!(decoder.push(&first).expect("fragment").is_empty());
    let mut second = vec![euro[2]];
    second.extend_from_slice(b"\r\n\r\ndata: [DONE]\n\n");

    let events = decoder.push(&second).expect("events");
    assert_eq!(
        events,
        vec![SseEvent::Data("hello\n€".to_owned()), SseEvent::Done]
    );
}

#[test]
fn rejects_invalid_utf8_instead_of_replacing_bytes() {
    let mut decoder = SseDecoder::new();
    assert!(decoder.push(b"data: \xff\n\n").is_err());
}
