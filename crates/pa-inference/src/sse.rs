use crate::InferenceError;

/// Gibt nur Nutzdaten und den expliziten Abschlussmarker an den Chatparser weiter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    Data(String),
    Done,
}

/// Puffert rohe Bytes, damit UTF-8- und Ereignisgrenzen unabhängig von TCP-Frames bleiben.
#[derive(Debug, Default)]
pub struct SseDecoder {
    pending: Vec<u8>,
}

impl SseDecoder {
    /// Beginnt ohne impliziten Zeichensatzersatz oder verlorene Restbytes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Liefert ausschließlich vollständig terminierte Ereignisse und behält Fragmente zurück.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>, InferenceError> {
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some((end, delimiter)) = event_boundary(&self.pending) {
            let block = self.pending[..end].to_vec();
            self.pending.drain(..end + delimiter);
            let text = std::str::from_utf8(&block)
                .map_err(|error| InferenceError::InvalidSse(error.to_string()))?;
            let mut data = Vec::new();
            for line in text.lines() {
                if let Some(value) = line.strip_prefix("data:") {
                    data.push(value.strip_prefix(' ').unwrap_or(value));
                }
            }
            if data.is_empty() {
                continue;
            }
            let joined = data.join("\n");
            if joined == "[DONE]" {
                events.push(SseEvent::Done);
            } else {
                events.push(SseEvent::Data(joined));
            }
        }
        Ok(events)
    }
}

fn event_boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes.windows(2).position(|window| window == b"\n\n");
    let crlf = bytes.windows(4).position(|window| window == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => Some((left, 2)),
        (Some(_), Some(right)) => Some((right, 4)),
        (Some(left), None) => Some((left, 2)),
        (None, Some(right)) => Some((right, 4)),
        (None, None) => None,
    }
}
