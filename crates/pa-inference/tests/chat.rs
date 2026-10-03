use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use pa_inference::{
    chat::{stream_chat, stream_chat_cancelable},
    gemma4::Gemma4Adapter,
    loopback::LoopbackEndpoint,
};

fn read_complete_request(stream: &mut std::net::TcpStream) {
    let mut request_bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    while !request_bytes.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = stream.read(&mut buffer).expect("request headers");
        assert_ne!(count, 0, "request ended before headers");
        request_bytes.extend_from_slice(&buffer[..count]);
    }
    let header_end = request_bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("header end");
    let headers = String::from_utf8_lossy(&request_bytes[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .and_then(|value| value.parse::<usize>().ok())
        .expect("content length");
    while request_bytes.len() < header_end + 4 + content_length {
        let count = stream.read(&mut buffer).expect("request body");
        assert_ne!(count, 0, "request body ended early");
        request_bytes.extend_from_slice(&buffer[..count]);
    }
}

#[test]
fn streams_chunked_deltas_and_server_timings_over_loopback_only() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let port = listener.local_addr().expect("address").port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut request_bytes = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request_bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).expect("request");
            assert_ne!(count, 0, "request ended before headers");
            request_bytes.extend_from_slice(&buffer[..count]);
        }
        let header_end = request_bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("header end");
        let headers = String::from_utf8_lossy(&request_bytes[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|value| value.parse::<usize>().ok())
            .expect("content length");
        while request_bytes.len() < header_end + 4 + content_length {
            let count = stream.read(&mut buffer).expect("request body");
            assert_ne!(count, 0, "request body ended early");
            request_bytes.extend_from_slice(&buffer[..count]);
        }
        let request = String::from_utf8_lossy(&request_bytes);
        assert!(
            request.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"),
            "request was {request:?}"
        );
        assert!(request.contains("Host: 127.0.0.1"));
        assert!(request.contains("Authorization: Bearer local-secret"));
        let events = [
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hal\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}],\"timings\":{\"prompt_ms\":480.0,\"prompt_per_second\":12.5,\"predicted_per_second\":7.25}}\n\n",
            "data: [DONE]\n\n",
        ];
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n")
            .expect("headers");
        for event in events {
            write!(stream, "{:X}\r\n{}\r\n", event.len(), event).expect("chunk");
        }
        let _ = stream.write_all(b"0\r\n\r\n");
    });
    let endpoint = LoopbackEndpoint::new(port, "local-secret").expect("endpoint");
    let adapter = Gemma4Adapter::new("gemma");
    let mut deltas = Vec::new();

    let outcome = stream_chat(&endpoint, &adapter, &[], |delta| {
        deltas.push(delta.to_owned());
        true
    })
    .expect("chat");

    server.join().expect("server");
    assert_eq!(deltas, vec!["Hal", "lo"]);
    assert_eq!(outcome.text, "Hallo");
    assert!(!outcome.aborted);
    assert_eq!(outcome.timings.prompt_ms, Some(480.0));
    assert_eq!(outcome.timings.prompt_per_second, Some(12.5));
    assert_eq!(outcome.timings.predicted_per_second, Some(7.25));
}

#[test]
fn callback_abort_keeps_partial_text_and_closes_the_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let port = listener.local_addr().expect("address").port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        read_complete_request(&mut stream);
        let event = "data: {\"choices\":[{\"delta\":{\"content\":\"Teil\"}}]}\n\n";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{}",
            event.len(),
            event
        )
        .expect("response");
    });
    let endpoint = LoopbackEndpoint::new(port, "secret").expect("endpoint");

    let outcome = stream_chat(&endpoint, &Gemma4Adapter::new("gemma"), &[], |_| false)
        .expect("aborted stream");

    server.join().expect("server");
    assert_eq!(outcome.text, "Teil");
    assert!(outcome.aborted);
}

#[test]
fn cancellation_does_not_wait_for_the_first_server_event() {
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let port = listener.local_addr().expect("address").port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        read_complete_request(&mut stream);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n")
            .expect("headers");
        thread::sleep(Duration::from_secs(1));
    });
    let endpoint = LoopbackEndpoint::new(port, "secret").expect("endpoint");
    let started = Instant::now();

    let outcome = stream_chat_cancelable(
        &endpoint,
        &Gemma4Adapter::new("gemma"),
        &[],
        || false,
        |_| true,
    )
    .expect("cancelled stream");

    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(outcome.aborted);
    assert!(outcome.text.is_empty());
    server.join().expect("server");
}

#[test]
fn transport_error_returns_already_streamed_partial_text() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let port = listener.local_addr().expect("address").port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        read_complete_request(&mut stream);
        let event = "data: {\"choices\":[{\"delta\":{\"content\":\"Sichtbar\"}}]}\n\n";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{}\r\nZZ\r\n",
            event.len(),
            event
        )
        .expect("response");
    });
    let endpoint = LoopbackEndpoint::new(port, "secret").expect("endpoint");

    let error = stream_chat(&endpoint, &Gemma4Adapter::new("gemma"), &[], |_| true)
        .expect_err("invalid second chunk");

    server.join().expect("server");
    assert_eq!(error.partial_text, "Sichtbar");
}

#[test]
fn eof_without_done_is_an_error_with_partial_text() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener");
    let port = listener.local_addr().expect("address").port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        read_complete_request(&mut stream);
        let event = "data: {\"choices\":[{\"delta\":{\"content\":\"Unvollständig\"}}]}\n\n";
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            event.len(),
            event
        )
        .expect("response");
    });
    let endpoint = LoopbackEndpoint::new(port, "secret").expect("endpoint");

    let error = stream_chat(&endpoint, &Gemma4Adapter::new("gemma"), &[], |_| true)
        .expect_err("missing DONE");

    server.join().expect("server");
    assert_eq!(error.partial_text, "Unvollständig");
}
