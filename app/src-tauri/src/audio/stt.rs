//! Lokale Spracherkennung über `whisper-server` (whisper.cpp).
//!
//! Der Server läuft nur für die Dauer einer Erkennung als eigener Prozess auf
//! `127.0.0.1` und wird danach beendet. Auf schwacher Hardware (T0) läuft er
//! dadurch nie gleichzeitig mit dem Sprachmodell: Erkennung, Inferenz und
//! Synthese werden seriell geplant, und der Speicher wird zwischen den
//! Schritten frei. Das Audio geht direkt aus dem Arbeitsspeicher an den
//! Server; es wird nie in eine Datei geschrieben.

use std::{
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use pa_inference::{
    config::ServerConfig,
    loopback::LoopbackEndpoint,
    multipart::{self, FilePart},
};
use thiserror::Error;

use super::pcm::{encode_wav, CAPTURE_RATE};
use crate::packs::Pack;

/// Windows-Flag: kein Konsolenfenster für Hilfsprozesse.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Fehler der Sprachfunktionen (Erkennung und Synthese).
#[derive(Debug, Error)]
pub enum SpeechError {
    /// Paket fehlt oder ist beschädigt.
    #[error("{0}")]
    Pack(#[from] crate::packs::PackError),
    /// Der Hilfsprozess ließ sich nicht starten oder antwortete nicht.
    #[error("Sprachdienst: {0}")]
    Process(String),
    /// Der Vorgang wurde abgebrochen.
    #[error("Der Vorgang wurde abgebrochen")]
    Cancelled,
}

/// Startet einen Hilfsprozess ohne Fenster, ohne geerbte Ein-/Ausgabe.
pub fn spawn_hidden(command: &mut Command) -> std::io::Result<Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.spawn()
}

/// Laufender `whisper-server`. Beim Fallenlassen wird der Prozess beendet.
pub struct WhisperServer {
    child: Arc<Mutex<Option<Child>>>,
    endpoint: LoopbackEndpoint,
}

/// Griff, mit dem ein anderer Thread die Erkennung abbrechen kann.
#[derive(Clone)]
pub struct WhisperKill(Arc<Mutex<Option<Child>>>);

impl WhisperKill {
    /// Beendet den Server sofort; eine laufende Erkennung bricht mit Fehler ab.
    pub fn kill(&self) {
        if let Ok(mut guard) = self.0.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            *guard = None;
        }
    }
}

impl WhisperServer {
    /// Startet den Server mit dem Modell des Pakets und wartet, bis er bereit ist.
    pub fn start(pack: &Pack, threads: usize) -> Result<Self, SpeechError> {
        let exe = pack.file("whisper-server.exe")?;
        let model = pack.file("ggml-base.bin")?;
        let (port, token) =
            ServerConfig::random_endpoint().map_err(|e| SpeechError::Process(e.to_string()))?;
        let mut command = Command::new(&exe);
        command
            .current_dir(pack.dir())
            .args(["-m"])
            .arg(&model)
            .args(["--host", "127.0.0.1", "--port"])
            .arg(port.to_string())
            .args(["-t"])
            .arg(threads.max(1).to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = spawn_hidden(&mut command)
            .map_err(|e| SpeechError::Process(format!("whisper-server startet nicht: {e}")))?;
        let server = Self {
            child: Arc::new(Mutex::new(Some(child))),
            endpoint: LoopbackEndpoint::new(port, token)
                .map_err(|e| SpeechError::Process(e.to_string()))?,
        };
        server.wait_ready(port, Duration::from_secs(60))?;
        Ok(server)
    }

    /// Griff zum Abbrechen von außen.
    pub fn kill_handle(&self) -> WhisperKill {
        WhisperKill(Arc::clone(&self.child))
    }

    fn wait_ready(&self, port: u16, timeout: Duration) -> Result<(), SpeechError> {
        let deadline = Instant::now() + timeout;
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        while Instant::now() < deadline {
            if let Ok(mut guard) = self.child.lock() {
                match guard.as_mut() {
                    Some(child) => {
                        if let Ok(Some(status)) = child.try_wait() {
                            return Err(SpeechError::Process(format!(
                                "whisper-server beendete sich beim Start ({status})"
                            )));
                        }
                    }
                    None => return Err(SpeechError::Cancelled),
                }
            }
            if TcpStream::connect_timeout(&address.into(), Duration::from_millis(200)).is_ok() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(SpeechError::Process(
            "whisper-server wurde nicht rechtzeitig bereit".to_owned(),
        ))
    }

    /// Erkennt gesprochenen Text. `language` ist ein Sprachcode (`de`, `en`, …).
    pub fn transcribe(&self, samples: &[i16], language: &str) -> Result<String, SpeechError> {
        let wav = encode_wav(samples, CAPTURE_RATE);
        let body = multipart::build(
            &[
                ("response_format", "json"),
                ("language", language),
                ("temperature", "0.0"),
            ],
            &FilePart {
                field: "file",
                file_name: "audio.wav",
                content_type: "audio/wav",
                data: &wav,
            },
        );
        let response = self
            .endpoint
            .post_collect(
                "/inference",
                &body.content_type,
                &body.bytes,
                Duration::from_secs(180),
            )
            .map_err(|e| SpeechError::Process(e.to_string()))?;
        let value: serde_json::Value = serde_json::from_slice(&response)
            .map_err(|e| SpeechError::Process(format!("Antwort unlesbar: {e}")))?;
        let text = value
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        Ok(clean_transcript(text))
    }
}

impl Drop for WhisperServer {
    fn drop(&mut self) {
        self.kill_handle().kill();
    }
}

/// Bereinigt die Ausgabe von Whisper: Randleerzeichen und Marker wie `[BLANK_AUDIO]`.
pub fn clean_transcript(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0_u32;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Sprachcode für Whisper aus dem Code der Oberflächensprache.
pub fn whisper_language(ui_language: &str) -> &str {
    match ui_language {
        "de" | "en" | "es" | "fr" | "ja" => ui_language,
        _ => "auto",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcript_markers_and_spaces_are_cleaned() {
        assert_eq!(
            clean_transcript(" Hallo [BLANK_AUDIO]  Welt (Musik) \n"),
            "Hallo Welt"
        );
        assert_eq!(clean_transcript("[BLANK_AUDIO]"), "");
    }

    #[test]
    fn language_codes_map_and_unknown_becomes_auto() {
        assert_eq!(whisper_language("de"), "de");
        assert_eq!(whisper_language("ja"), "ja");
        assert_eq!(whisper_language("xx"), "auto");
    }
}

/// Ende-zu-Ende-Prüfung mit den echten Paketen (Piper erzeugt Sprache, Whisper
/// erkennt sie wieder). Läuft nur mit `IAP_DEV_PACKS=<Ordner AI/packs>` und
/// `cargo test -- --ignored`, weil die Pakete nicht im Repository liegen.
#[cfg(test)]
mod pack_roundtrip {
    use std::{path::PathBuf, sync::atomic::AtomicBool};

    use super::*;
    use crate::audio::{pcm::resample_linear, tts};

    #[test]
    #[ignore = "braucht die echten Zusatzpakete (IAP_DEV_PACKS)"]
    fn piper_speech_is_recognized_by_whisper() {
        let root = PathBuf::from(std::env::var("IAP_DEV_PACKS").expect("IAP_DEV_PACKS setzen"));
        let piper = Pack::open(&root, "piper").expect("piper");
        piper.verify_hashes().expect("piper-Prüfsummen");
        let whisper = Pack::open(&root, "whisper").expect("whisper");
        whisper.verify_hashes().expect("whisper-Prüfsummen");

        let voice = tts::voice_for(&piper, "de").expect("deutsche Stimme");
        assert_eq!(voice.sample_rate, 22_050);
        let (raw, stats) = tts::synthesize(
            &piper,
            &voice,
            "Guten Tag. Dies ist ein Test der Sprachfunktion.",
            &AtomicBool::new(false),
        )
        .expect("Synthese");
        assert!(raw.len() > 22_050, "mindestens eine Sekunde Audio");
        assert!(stats.first_audio_ms > 0);

        let samples = resample_linear(&raw, voice.sample_rate, CAPTURE_RATE);
        let server = WhisperServer::start(&whisper, 4).expect("whisper-server");
        let text = server
            .transcribe(&samples, "de")
            .expect("Erkennung")
            .to_lowercase();
        assert!(text.contains("guten tag"), "erkannt: {text}");
        assert!(text.contains("test"), "erkannt: {text}");
    }
}
