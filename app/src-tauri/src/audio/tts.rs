//! Lokale Sprachausgabe über Piper.
//!
//! Piper bekommt den Text über die Standardeingabe und liefert rohes 16-Bit-PCM
//! auf der Standardausgabe. Beides bleibt im Arbeitsspeicher und geht direkt an
//! den [`Player`]; es entsteht keine Audiodatei. Jede Stimme gehört zu einer
//! Sprache; gibt es für die gewählte Sprache keine Stimme (etwa Japanisch),
//! meldet IAP das ehrlich, statt eine falsche Stimme zu benutzen.

use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use super::{
    pcm::bytes_to_samples,
    playback::Player,
    stt::{spawn_hidden, SpeechError},
};
use crate::packs::Pack;

/// Eine gefundene Stimme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    /// Dateiname ohne Endung, z. B. `de_DE-thorsten-medium`.
    pub name: String,
    pub model: String,
    pub sample_rate: u32,
}

/// Findet die Stimme zu einem Sprachcode (`de` → `de_*`) im Paket.
pub fn voice_for(pack: &Pack, language: &str) -> Option<Voice> {
    let prefix = format!("{language}_");
    let mut names: Vec<&str> = pack
        .manifest
        .files
        .iter()
        .filter_map(|f| f.path.strip_prefix("voices/"))
        .filter(|name| name.starts_with(&prefix) && name.ends_with(".onnx"))
        .collect();
    names.sort_unstable();
    let model = names.first()?;
    let json_path = pack.file(&format!("voices/{model}.json")).ok()?;
    let sample_rate = std::fs::read_to_string(json_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .pointer("/audio/sample_rate")
                .and_then(serde_json::Value::as_u64)
        })
        .and_then(|rate| u32::try_from(rate).ok())
        .unwrap_or(22_050);
    Some(Voice {
        name: model.trim_end_matches(".onnx").to_owned(),
        model: format!("voices/{model}"),
        sample_rate,
    })
}

/// Ergebnis einer Ausgabe.
#[derive(Debug, Clone, Copy)]
pub struct SpeakStats {
    /// Zeit bis zum ersten hörbaren Audio.
    pub first_audio_ms: u64,
    /// Gesamtdauer der Synthese.
    pub synth_ms: u64,
}

/// Startet Piper und reicht jedes Stück PCM an `sink` weiter, bis der Text
/// fertig ist oder `cancel` gesetzt wird. `sink` gibt `false` zurück, wenn
/// nicht weiter synthetisiert werden soll.
fn stream(
    pack: &Pack,
    voice: &Voice,
    text: &str,
    cancel: &AtomicBool,
    mut sink: impl FnMut(&[i16]) -> bool,
) -> Result<SpeakStats, SpeechError> {
    let exe = pack.file("piper.exe")?;
    let model = pack.file(&voice.model)?;
    let mut command = Command::new(&exe);
    command
        .current_dir(exe.parent().unwrap_or_else(|| pack.dir()))
        .arg("--model")
        .arg(&model)
        .arg("--output_raw")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let started = Instant::now();
    let mut child = spawn_hidden(&mut command)
        .map_err(|e| SpeechError::Process(format!("piper startet nicht: {e}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| SpeechError::Process("piper ohne Eingabe".to_owned()))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| SpeechError::Process("piper ohne Ausgabe".to_owned()))?;
    let payload = format!("{}\n", text.trim());
    // Eingabe in eigenem Thread, damit ein voller Ausgabekanal nicht blockiert.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(payload.as_bytes());
    });

    let mut first_audio_ms = 0_u64;
    let mut carry = None;
    let mut buffer = vec![0_u8; 16 * 1024];
    let outcome = loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            break Err(SpeechError::Cancelled);
        }
        match stdout.read(&mut buffer) {
            Ok(0) => break Ok(()),
            Ok(read) => {
                let samples = bytes_to_samples(&buffer[..read], &mut carry);
                if first_audio_ms == 0 && !samples.is_empty() {
                    first_audio_ms =
                        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                }
                if !sink(&samples) {
                    let _ = child.kill();
                    break Err(SpeechError::Cancelled);
                }
            }
            Err(error) => {
                let _ = child.kill();
                break Err(SpeechError::Process(format!("piper-Ausgabe: {error}")));
            }
        }
    };
    let _ = child.wait();
    let _ = writer.join();
    let synth_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    outcome.map(|()| SpeakStats {
        first_audio_ms,
        synth_ms,
    })
}

/// Spricht `text`, bis er fertig ist oder `cancel` gesetzt wird.
///
/// Der Player läuft nur während der Ausgabe; danach ist das Audiogerät frei
/// und der Speicher des Sprachprozesses zurückgegeben.
pub fn speak(
    pack: &Pack,
    voice: &Voice,
    text: &str,
    cancel: &AtomicBool,
    on_player: impl FnOnce(&Player),
) -> Result<SpeakStats, SpeechError> {
    if text.trim().is_empty() {
        return Ok(SpeakStats {
            first_audio_ms: 0,
            synth_ms: 0,
        });
    }
    let player =
        Player::start(voice.sample_rate).map_err(|e| SpeechError::Process(e.to_string()))?;
    on_player(&player);
    let result = stream(pack, voice, text, cancel, |samples| {
        player.push(samples);
        !player.is_finished()
    });
    match result {
        Ok(stats) => {
            player.close_input();
            // Auf das Ende der Wiedergabe warten, dabei Abbruch beachten.
            while !player.is_finished() {
                if cancel.load(Ordering::SeqCst) {
                    player.stop();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            player.wait();
            Ok(stats)
        }
        Err(error) => {
            player.stop();
            player.wait();
            Err(error)
        }
    }
}

/// Erzeugt Sprache nur im Speicher (für den Leistungstest), ohne sie abzuspielen.
pub fn synthesize(
    pack: &Pack,
    voice: &Voice,
    text: &str,
    cancel: &AtomicBool,
) -> Result<(Vec<i16>, SpeakStats), SpeechError> {
    let mut all = Vec::new();
    let stats = stream(pack, voice, text, cancel, |samples| {
        all.extend_from_slice(samples);
        true
    })?;
    Ok((all, stats))
}
