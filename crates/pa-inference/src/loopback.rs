use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    time::Duration,
};

use crate::{
    sse::{SseDecoder, SseEvent},
    InferenceError,
};

/// Speichert absichtlich keinen Hostnamen und kann daher weder DNS noch fremde Ziele ansprechen.
#[derive(Debug, Clone)]
pub struct LoopbackEndpoint {
    port: u16,
    token: String,
}

impl LoopbackEndpoint {
    /// Akzeptiert nur einen lokalen Port und einen header-sicheren Zugriffsschlüssel.
    pub fn new(port: u16, token: impl Into<String>) -> Result<Self, InferenceError> {
        let token = token.into();
        if port == 0 || token.is_empty() || token.contains(['\r', '\n']) {
            return Err(InferenceError::Http(
                "ungültiger Loopback-Endpunkt".to_owned(),
            ));
        }
        Ok(Self { port, token })
    }

    /// Sendet genau einen POST an 127.0.0.1 und folgt niemals Redirects.
    pub fn post_sse(
        &self,
        path: &str,
        body: &[u8],
        callback: impl FnMut(SseEvent) -> bool,
    ) -> Result<bool, InferenceError> {
        self.post_sse_cancelable(path, body, || true, callback)
    }

    /// Sendet einen JSON-POST und liefert den vollständigen Antwortkörper
    /// als Byte-Vektor zurück. Für /embeddings gedacht, das kein SSE nutzt.
    ///
    /// Wie `post_sse_cancelable` wird kein Redirect befolgt, und die
    /// Verbindung schließt nach genau einem Austausch.
    pub fn post_json_collect(
        &self,
        path: &str,
        body: &[u8],
        deadline: Duration,
    ) -> Result<Vec<u8>, InferenceError> {
        self.post_collect(path, "application/json", body, deadline)
    }

    /// Wie [`Self::post_json_collect`], aber mit frei wählbarem `Content-Type`
    /// (etwa `multipart/form-data` für die lokale Spracherkennung). Der
    /// Audiokörper bleibt im Arbeitsspeicher; es wird keine Datei angelegt.
    pub fn post_collect(
        &self,
        path: &str,
        content_type: &str,
        body: &[u8],
        deadline: Duration,
    ) -> Result<Vec<u8>, InferenceError> {
        if content_type.contains(['\r', '\n']) {
            return Err(InferenceError::Http("ungültiger Content-Type".to_owned()));
        }
        if !path.starts_with('/') || path.contains(['\r', '\n']) {
            return Err(InferenceError::Http(
                "ungültiger lokaler HTTP-Pfad".to_owned(),
            ));
        }
        let mut stream = TcpStream::connect_timeout(
            &SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.port).into(),
            Duration::from_secs(3),
        )?;
        stream.set_read_timeout(Some(deadline))?;
        write!(
            stream,
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: {content_type}\r\nAccept: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            self.token,
            body.len()
        )?;
        stream.write_all(body)?;
        stream.flush()?;
        let mut reader = BufReader::new(stream);
        let mut status = String::new();
        reader.read_line(&mut status)?;
        if !status.starts_with("HTTP/1.1 200 ") && !status.starts_with("HTTP/1.0 200 ") {
            return Err(InferenceError::Http(format!(
                "lokaler Server antwortete ohne Erfolg: {}",
                status.trim()
            )));
        }
        let mut chunked = false;
        let mut content_length = None;
        loop {
            let mut line = String::new();
            let read = reader.read_line(&mut line)?;
            if read == 0 {
                return Err(InferenceError::Http("Header endete vorzeitig".to_owned()));
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
                chunked = true;
            }
            if let Some(value) = lower.strip_prefix("content-length:") {
                content_length =
                    Some(value.trim().parse::<usize>().map_err(|_| {
                        InferenceError::Http("ungültige Content-Length".to_owned())
                    })?);
            }
            if lower.starts_with("location:") {
                return Err(InferenceError::Http("Redirect wird abgelehnt".to_owned()));
            }
        }
        let mut collected = Vec::new();
        if chunked {
            loop {
                let mut size_line = String::new();
                reader.read_line(&mut size_line)?;
                let size_text = size_line.trim().split(';').next().unwrap_or_default();
                let size = usize::from_str_radix(size_text, 16)
                    .map_err(|_| InferenceError::Http("ungültige Chunkgröße".to_owned()))?;
                if size == 0 {
                    break;
                }
                let mut chunk = vec![0_u8; size];
                reader.read_exact(&mut chunk)?;
                collected.extend_from_slice(&chunk);
                let mut terminator = [0_u8; 2];
                reader.read_exact(&mut terminator)?;
                if terminator != *b"\r\n" {
                    return Err(InferenceError::Http("Chunk ohne CRLF".to_owned()));
                }
            }
        } else if let Some(length) = content_length {
            collected.resize(length, 0);
            reader.read_exact(&mut collected)?;
        } else {
            reader.read_to_end(&mut collected)?;
        }
        Ok(collected)
    }

    /// Prüft Abbruch auch während der Server rechnet und noch kein SSE-Ereignis sendet.
    pub fn post_sse_cancelable(
        &self,
        path: &str,
        body: &[u8],
        mut should_continue: impl FnMut() -> bool,
        mut callback: impl FnMut(SseEvent) -> bool,
    ) -> Result<bool, InferenceError> {
        if !path.starts_with('/') || path.contains(['\r', '\n']) {
            return Err(InferenceError::Http(
                "ungültiger lokaler HTTP-Pfad".to_owned(),
            ));
        }
        let mut stream = TcpStream::connect_timeout(
            &SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.port).into(),
            Duration::from_secs(3),
        )?;
        stream.set_read_timeout(Some(Duration::from_millis(250)))?;
        write!(
            stream,
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nAccept: text/event-stream\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
            self.token,
            body.len()
        )?;
        stream.write_all(body)?;
        stream.flush()?;
        let mut reader = BufReader::new(stream);
        let mut status = String::new();
        if !read_line_cancelable(&mut reader, &mut status, &mut should_continue)? {
            return Ok(false);
        }
        if !status.starts_with("HTTP/1.1 200 ") && !status.starts_with("HTTP/1.0 200 ") {
            return Err(InferenceError::Http(format!(
                "llama-server antwortete ohne Erfolg: {}",
                status.trim()
            )));
        }
        let mut chunked = false;
        let mut content_length = None;
        loop {
            let mut line = String::new();
            if !read_line_cancelable(&mut reader, &mut line, &mut should_continue)? {
                return Ok(false);
            }
            if line == "\r\n" || line == "\n" {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
                chunked = true;
            }
            if let Some(value) = lower.strip_prefix("content-length:") {
                content_length =
                    Some(value.trim().parse::<usize>().map_err(|_| {
                        InferenceError::Http("ungültige Content-Length".to_owned())
                    })?);
            }
            if lower.starts_with("location:") {
                return Err(InferenceError::Http("Redirect wird abgelehnt".to_owned()));
            }
        }
        let mut decoder = SseDecoder::new();
        if chunked {
            loop {
                let mut size_line = String::new();
                if !read_line_cancelable(&mut reader, &mut size_line, &mut should_continue)? {
                    return Ok(false);
                }
                let size_text = size_line.trim().split(';').next().unwrap_or_default();
                let size = usize::from_str_radix(size_text, 16)
                    .map_err(|_| InferenceError::Http("ungültige Chunkgröße".to_owned()))?;
                if size == 0 {
                    break;
                }
                let mut remaining = size;
                let mut chunk = [0_u8; 4096];
                while remaining > 0 {
                    if !should_continue() {
                        return Ok(false);
                    }
                    let requested = remaining.min(chunk.len());
                    let count = match reader.read(&mut chunk[..requested]) {
                        Ok(0) => {
                            return Err(InferenceError::Http("Chunk endete vorzeitig".to_owned()))
                        }
                        Ok(count) => count,
                        Err(error) if is_timeout(&error) => continue,
                        Err(error) => return Err(error.into()),
                    };
                    remaining -= count;
                    if !dispatch(&mut decoder, &chunk[..count], &mut callback)? {
                        return Ok(false);
                    }
                }
                let mut terminator = [0_u8; 2];
                if !read_exact_cancelable(&mut reader, &mut terminator, &mut should_continue)? {
                    return Ok(false);
                }
                if terminator != *b"\r\n" {
                    return Err(InferenceError::Http("Chunk ohne CRLF".to_owned()));
                }
            }
        } else if let Some(length) = content_length {
            let mut remaining = length;
            let mut buffer = [0_u8; 4096];
            while remaining > 0 {
                let requested = remaining.min(buffer.len());
                if !should_continue() {
                    return Ok(false);
                }
                let count = match reader.read(&mut buffer[..requested]) {
                    Ok(0) => return Err(InferenceError::Http("Body endete vorzeitig".to_owned())),
                    Ok(count) => count,
                    Err(error) if is_timeout(&error) => continue,
                    Err(error) => return Err(error.into()),
                };
                remaining -= count;
                if !dispatch(&mut decoder, &buffer[..count], &mut callback)? {
                    return Ok(false);
                }
            }
        } else {
            let mut buffer = [0_u8; 4096];
            loop {
                if !should_continue() {
                    return Ok(false);
                }
                let count = match reader.read(&mut buffer) {
                    Ok(count) => count,
                    Err(error) if is_timeout(&error) => continue,
                    Err(error) => return Err(error.into()),
                };
                if count == 0 {
                    break;
                }
                if !dispatch(&mut decoder, &buffer[..count], &mut callback)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

fn read_line_cancelable(
    reader: &mut BufReader<TcpStream>,
    line: &mut String,
    should_continue: &mut impl FnMut() -> bool,
) -> Result<bool, InferenceError> {
    loop {
        if !should_continue() {
            return Ok(false);
        }
        match reader.read_line(line) {
            Ok(0) => return Err(InferenceError::Http("Body endete vorzeitig".to_owned())),
            Ok(_) => return Ok(true),
            Err(error) if is_timeout(&error) => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

fn read_exact_cancelable(
    reader: &mut BufReader<TcpStream>,
    target: &mut [u8],
    should_continue: &mut impl FnMut() -> bool,
) -> Result<bool, InferenceError> {
    let mut filled = 0;
    while filled < target.len() {
        if !should_continue() {
            return Ok(false);
        }
        match reader.read(&mut target[filled..]) {
            Ok(0) => return Err(InferenceError::Http("Body endete vorzeitig".to_owned())),
            Ok(count) => filled += count,
            Err(error) if is_timeout(&error) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(true)
}

fn is_timeout(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

fn dispatch(
    decoder: &mut SseDecoder,
    bytes: &[u8],
    callback: &mut impl FnMut(SseEvent) -> bool,
) -> Result<bool, InferenceError> {
    for event in decoder.push(bytes)? {
        if !callback(event) {
            return Ok(false);
        }
    }
    Ok(true)
}
