//! Sprachgespräch: Aufnahme, Erkennung und Vorlesen als gesteuerte Sitzung.
//!
//! Dieses Modul entscheidet, **ob** etwas starten darf; die Bausteine unter
//! `audio/` führen nur aus. Freigaben:
//!
//! - Tresor entsperrt (sonst keine Sprachverarbeitung),
//! - Paket vorhanden, eingeschaltet, Prüfsummen stimmen,
//! - T0-Tor: unter 10 GB RAM erst nach bestandenem Leistungstest auf diesem PC,
//! - ein `CaptureTicket` pro Aufnahme, ausgestellt aus einer sichtbaren
//!   Nutzeraktion und über `pa-policy` geprüft und protokolliert.
//!
//! Erkennung und Vorlesen gehen durch die serielle Modell-Queue, damit sie auf
//! schwacher Hardware nie gleichzeitig mit dem Sprachmodell rechnen.

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use pa_policy::{
    device::{authorize_capture, CaptureContext, CaptureTicket, DeviceKind},
    CapabilityAction, Decision,
};
use pa_types::{
    avatar::JobKind,
    model::HardwareTier,
    voice::{Transcript, VoiceBenchmark, VoiceBlock, VoiceState, VoiceStateEvent, VoiceStatus},
};
use tauri::{AppHandle, Emitter, Manager, State};
use zeroize::Zeroize;

use crate::{
    audio::{
        capture::{CaptureError, Recorder},
        command,
        pcm::{resample_linear, CAPTURE_RATE},
        stt::{whisper_language, SpeechError, WhisperKill, WhisperServer},
        text, tts,
    },
    ensure_bootstrap, lifecycle,
    pack_cmds::{all_status, open_usable, pack_enabled},
    packs::{PACK_PIPER, PACK_WHISPER},
    require_session, ui_language, AppError, AppResult, AppState,
};

/// Kürzeste sinnvolle Aufnahme; darunter wird nichts erkannt.
const MIN_AUDIO_MS: u64 = 400;
/// Mindestreserve an freiem Arbeitsspeicher während der Erkennung (T0-Tor).
const MIN_RAM_RESERVE_BYTES: u64 = 1_500_000_000;
/// Höchster erlaubter Echtzeitfaktor der Erkennung (Dauer / Audiodauer).
const MAX_STT_REAL_TIME_FACTOR: f64 = 1.0;
/// Längste akzeptierte Zeit bis zum ersten Ton der Sprachausgabe.
const MAX_TTS_FIRST_AUDIO_MS: u64 = 4_000;

/// Testsätze je Sprache für den Leistungstest.
fn benchmark_sentence(language: &str) -> &'static str {
    match language {
        "en" => "This is a short test of the local speech functions of IAP.",
        "es" => "Esta es una breve prueba de las funciones de voz locales de IAP.",
        "fr" => "Ceci est un court test des fonctions vocales locales de IAP.",
        _ => "Das ist ein kurzer Test der lokalen Sprachfunktionen von IAP.",
    }
}

struct Inner {
    state: VoiceState,
    recorder: Option<Recorder>,
    whisper: Option<WhisperKill>,
    last_block: Option<VoiceBlock>,
    benchmark: Option<VoiceBenchmark>,
}

/// Laufzeitzustand der Sprachsitzung; liegt als Feld in `AppState`.
pub struct VoiceRuntime {
    inner: Mutex<Inner>,
    speak_cancel: Arc<AtomicBool>,
    muted: AtomicBool,
}

impl VoiceRuntime {
    /// Leerer Zustand: nichts läuft, nicht stumm.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                state: VoiceState::Idle,
                recorder: None,
                whisper: None,
                last_block: None,
                benchmark: None,
            }),
            speak_cancel: Arc::new(AtomicBool::new(false)),
            muted: AtomicBool::new(false),
        }
    }

    fn lock(&self) -> AppResult<std::sync::MutexGuard<'_, Inner>> {
        self.inner
            .lock()
            .map_err(|_| AppError::Internal("Sprachzustand gesperrt".to_owned()))
    }

    /// Aktueller Zustand.
    pub fn state(&self) -> VoiceState {
        self.inner
            .lock()
            .map(|inner| inner.state)
            .unwrap_or(VoiceState::Idle)
    }

    /// Beendet Aufnahme, Erkennung und Ausgabe sofort und verwirft alles Aufgenommene.
    ///
    /// Wird beim Wechsel zum Pet, beim Sperren des Tresors und beim
    /// vollständigen Beenden aufgerufen.
    pub fn stop_everything(&self, app: &AppHandle) {
        self.shutdown();
        emit(
            app,
            VoiceState::Idle,
            self.muted.load(Ordering::SeqCst),
            0,
            0.0,
            None,
        );
    }

    /// Wie [`Self::stop_everything`], aber ohne Ereignis (für das Beenden).
    pub fn shutdown(&self) {
        self.speak_cancel.store(true, Ordering::SeqCst);
        let (recorder, whisper) = match self.inner.lock() {
            Ok(mut inner) => {
                inner.state = VoiceState::Idle;
                (inner.recorder.take(), inner.whisper.take())
            }
            Err(_) => (None, None),
        };
        if let Some(recorder) = recorder {
            recorder.discard();
        }
        if let Some(kill) = whisper {
            kill.kill();
        }
    }
}

impl Default for VoiceRuntime {
    fn default() -> Self {
        Self::new()
    }
}

fn emit(
    app: &AppHandle,
    state: VoiceState,
    muted: bool,
    elapsed_ms: u64,
    level: f32,
    message: Option<String>,
) {
    let _ = app.emit(
        "voice-state",
        VoiceStateEvent {
            state,
            muted,
            elapsed_ms,
            level,
            message,
        },
    );
}

fn set_state(app: &AppHandle, runtime: &VoiceRuntime, state: VoiceState, message: Option<String>) {
    if let Ok(mut inner) = runtime.inner.lock() {
        inner.state = state;
    }
    emit(
        app,
        state,
        runtime.muted.load(Ordering::SeqCst),
        0,
        0.0,
        message,
    );
}

fn benchmark_key(host_identifier: &str) -> String {
    let prefix: String = host_identifier.chars().take(16).collect();
    format!("host.{prefix}.voice_benchmark")
}

fn effective_tier(state: &AppState) -> AppResult<HardwareTier> {
    let bootstrap = ensure_bootstrap(state)?;
    let override_tier = require_session(state)
        .ok()
        .and_then(|session| session.tier_override);
    Ok(override_tier.unwrap_or(bootstrap.plan.tier))
}

fn load_benchmark(state: &AppState) -> Option<VoiceBenchmark> {
    let bootstrap = ensure_bootstrap(state).ok()?;
    let session = require_session(state).ok()?;
    let vault = session.vault_runtime.lock().ok()?;
    let text = vault
        .repository()
        .setting(&benchmark_key(&bootstrap.host_identifier))
        .ok()??;
    serde_json::from_str(&text).ok()
}

/// Warum Sprache jetzt nicht starten darf, oder `None`, wenn sie darf.
fn block_reason(state: &AppState) -> AppResult<Option<(VoiceBlock, String)>> {
    if require_session(state).is_err() {
        return Ok(Some((
            VoiceBlock::VaultLocked,
            "Der Tresor ist gesperrt.".to_owned(),
        )));
    }
    if !pack_enabled(state, PACK_WHISPER) {
        return Ok(Some((
            VoiceBlock::PackMissing,
            "Die Spracherkennung ist ausgeschaltet.".to_owned(),
        )));
    }
    let statuses = all_status(state)?;
    if let Some(status) = statuses
        .iter()
        .find(|s| s.id == PACK_WHISPER && !s.installed)
    {
        return Ok(Some((
            VoiceBlock::PackMissing,
            status
                .missing_reason
                .clone()
                .unwrap_or_else(|| "Spracherkennung nicht eingerichtet.".to_owned()),
        )));
    }
    let tier = effective_tier(state)?;
    if matches!(tier, HardwareTier::T0 | HardwareTier::Unsupported) {
        let passed = load_benchmark(state).is_some_and(|b| b.passed);
        if !passed {
            return Ok(Some((
                VoiceBlock::BenchmarkPending,
                "Auf diesem Rechner ist Sprache erst nach einem bestandenen Leistungstest verfügbar (Einstellungen > Sprache)."
                    .to_owned(),
            )));
        }
    }
    Ok(None)
}

fn ui_lang(state: &AppState) -> String {
    match crate::lock(&state.package_root) {
        Ok(root) => ui_language::read(&root).to_owned(),
        Err(_) => "de".to_owned(),
    }
}

/// Status für Oberfläche, Avatar und Pet-Menü.
#[tauri::command]
pub fn voice_status(state: State<'_, AppState>) -> AppResult<VoiceStatus> {
    let block = block_reason(&state)?;
    let language = ui_lang(&state);
    let packs: Vec<_> = all_status(&state)?
        .into_iter()
        .filter(|p| p.id == PACK_WHISPER || p.id == PACK_PIPER)
        .collect();
    let voice_available = packs
        .iter()
        .find(|p| p.id == PACK_PIPER)
        .is_some_and(|p| p.installed && p.enabled)
        && crate::pack_cmds::packs_dir(&state)
            .ok()
            .and_then(|dir| crate::packs::Pack::open(&dir, PACK_PIPER).ok())
            .and_then(|pack| tts::voice_for(&pack, &language))
            .is_some();
    let inner = state.voice.lock()?;
    Ok(VoiceStatus {
        available: block.is_none(),
        block: block.map(|(kind, _)| kind).or(inner.last_block),
        state: inner.state,
        muted: state.voice.muted.load(Ordering::SeqCst),
        packs,
        language,
        voice_available,
        benchmark: inner.benchmark.clone().or_else(|| load_benchmark(&state)),
    })
}

/// Klartext, warum Sprache gerade gesperrt ist (leer, wenn sie frei ist).
#[tauri::command]
pub fn voice_block_reason(state: State<'_, AppState>) -> AppResult<Option<String>> {
    Ok(block_reason(&state)?.map(|(_, text)| text))
}

/// Startet eine einzelne, sichtbare Aufnahme.
#[tauri::command]
pub fn voice_start_capture(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    if let Some((_, reason)) = block_reason(&state)? {
        return Err(AppError::Invalid(reason));
    }
    state.flow.touch_activity();
    {
        let inner = state.voice.lock()?;
        if inner.recorder.is_some() || inner.state == VoiceState::Transcribing {
            return Err(AppError::Invalid(
                "Es läuft bereits eine Aufnahme oder Erkennung.".to_owned(),
            ));
        }
    }
    // Während vorgelesen wird, pausiert die Aufnahme: erst die Ausgabe beenden.
    if state.voice.state() == VoiceState::Speaking {
        state.voice.speak_cancel.store(true, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(150));
    }

    let context = CaptureContext {
        vault_unlocked: true,
        gate_open: true,
    };
    let mut ticket = CaptureTicket::issue(DeviceKind::Microphone);
    let decision = authorize_capture(&context, &mut ticket, DeviceKind::Microphone);
    lifecycle::audit_decision(
        &state,
        CapabilityAction::MicCapture,
        None,
        &decision,
        "Sprachaufnahme auf Knopfdruck",
    );
    if let Decision::Deny(reason) | Decision::Prompt(reason) = decision {
        return Err(AppError::Invalid(reason));
    }

    let recorder = match Recorder::start() {
        Ok(recorder) => recorder,
        Err(error) => {
            if let Ok(mut inner) = state.voice.lock() {
                inner.last_block =
                    (error == CaptureError::AccessDenied).then_some(VoiceBlock::MicrophoneDenied);
            }
            return Err(AppError::Invalid(error.to_string()));
        }
    };
    {
        let mut inner = state.voice.lock()?;
        inner.recorder = Some(recorder);
        inner.last_block = None;
        inner.state = VoiceState::Listening;
    }
    emit(&app, VoiceState::Listening, false, 0, 0.0, None);

    // Fortschrittsmeldungen für Pegelanzeige und Zeitgeber.
    let ticker_app = app.clone();
    thread::spawn(move || {
        let state = ticker_app.state::<AppState>();
        loop {
            thread::sleep(Duration::from_millis(100));
            let Ok(inner) = state.voice.inner.lock() else {
                break;
            };
            if inner.state != VoiceState::Listening {
                break;
            }
            let Some(recorder) = inner.recorder.as_ref() else {
                break;
            };
            let message = if recorder.overflowed() {
                Some("max_duration".to_owned())
            } else {
                recorder.failure().map(|error| error.to_string())
            };
            emit(
                &ticker_app,
                VoiceState::Listening,
                false,
                recorder.elapsed_ms(),
                recorder.level(),
                message,
            );
        }
    });
    Ok(())
}

/// Beendet die Aufnahme und liefert den erkannten Text (nicht gesendet).
#[tauri::command(async)]
pub async fn voice_stop_capture(app: AppHandle) -> AppResult<Transcript> {
    tauri::async_runtime::spawn_blocking(move || stop_capture_blocking(&app))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

fn stop_capture_blocking(app: &AppHandle) -> AppResult<Transcript> {
    let state = app.state::<AppState>();
    let recorder = state
        .voice
        .lock()?
        .recorder
        .take()
        .ok_or_else(|| AppError::Invalid("Es läuft keine Aufnahme.".to_owned()))?;
    set_state(app, &state.voice, VoiceState::Transcribing, None);

    let mut samples = match recorder.stop() {
        Ok(samples) => samples,
        Err(error) => {
            set_state(app, &state.voice, VoiceState::Idle, None);
            return Err(AppError::Invalid(error.to_string()));
        }
    };
    let audio_ms = (samples.len() as u64 * 1000) / u64::from(CAPTURE_RATE);
    let language = ui_lang(&state);
    if audio_ms < MIN_AUDIO_MS {
        samples.zeroize();
        set_state(app, &state.voice, VoiceState::Idle, None);
        return Ok(Transcript {
            text: String::new(),
            language,
            audio_ms,
            transcribe_ms: 0,
            command: None,
        });
    }

    let result = transcribe(app, &state, &samples, &language);
    samples.zeroize();
    drop(samples);
    set_state(app, &state.voice, VoiceState::Idle, None);
    let (text, transcribe_ms) = result?;
    let command = command::parse(&text, &language);
    Ok(Transcript {
        text,
        language,
        audio_ms,
        transcribe_ms,
        command,
    })
}

fn transcribe(
    app: &AppHandle,
    state: &AppState,
    samples: &[i16],
    language: &str,
) -> AppResult<(String, u64)> {
    let pack = open_usable(state, PACK_WHISPER)?;
    // Sichtbar in der Job-Liste; wartet, wenn gerade das Sprachmodell rechnet.
    let mut job = state
        .flow
        .jobs
        .submit(JobKind::SpeechToText, "Sprache erkennen", true);
    let mut slot = job
        .acquire()
        .map_err(|_| AppError::Invalid("Erkennung abgebrochen.".to_owned()))?;
    let threads = ensure_bootstrap(state).map(|b| b.plan.threads).unwrap_or(2);

    let started = Instant::now();
    let server = match WhisperServer::start(&pack, threads) {
        Ok(server) => server,
        Err(error) => {
            slot.fail();
            return Err(AppError::Invalid(error.to_string()));
        }
    };
    if let Ok(mut inner) = state.voice.lock() {
        inner.whisper = Some(server.kill_handle());
    }
    let outcome = server.transcribe(samples, whisper_language(language));
    if let Ok(mut inner) = state.voice.lock() {
        inner.whisper = None;
    }
    drop(server); // Speicher der Erkennung sofort freigeben.
    let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let _ = app;
    match outcome {
        Ok(text) => Ok((text, elapsed)),
        Err(error) => {
            slot.fail();
            Err(match error {
                SpeechError::Cancelled => AppError::Invalid("Erkennung abgebrochen.".to_owned()),
                other => AppError::Invalid(other.to_string()),
            })
        }
    }
}

/// Verwirft eine laufende Aufnahme oder Erkennung ohne Ergebnis.
#[tauri::command]
pub fn voice_discard(app: AppHandle, state: State<'_, AppState>) {
    let (recorder, whisper) = match state.voice.inner.lock() {
        Ok(mut inner) => (inner.recorder.take(), inner.whisper.take()),
        Err(_) => (None, None),
    };
    if let Some(recorder) = recorder {
        recorder.discard();
    }
    if let Some(kill) = whisper {
        kill.kill();
    }
    set_state(&app, &state.voice, VoiceState::Idle, None);
}

/// Liest `text` mit der lokalen Stimme vor. Unterbrechbar, stummschaltbar.
#[tauri::command(async)]
pub async fn voice_speak(app: AppHandle, text: String) -> AppResult<()> {
    tauri::async_runtime::spawn_blocking(move || speak_blocking(&app, &text))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

fn speak_blocking(app: &AppHandle, raw_text: &str) -> AppResult<()> {
    let state = app.state::<AppState>();
    if state.voice.muted.load(Ordering::SeqCst) {
        return Ok(());
    }
    if require_session(&state).is_err() {
        return Err(AppError::locked());
    }
    let language = ui_lang(&state);
    let pack = open_usable(&state, PACK_PIPER)?;
    let voice = tts::voice_for(&pack, &language).ok_or_else(|| {
        AppError::Invalid("Für diese Sprache gibt es keine lokale Stimme.".to_owned())
    })?;
    let speech = text::for_speech(raw_text);
    if speech.is_empty() {
        return Ok(());
    }
    // Aufnahme pausiert während der Ausgabe: laufende Aufnahme wird beendet.
    let recorder = state.voice.lock()?.recorder.take();
    if let Some(recorder) = recorder {
        recorder.discard();
    }

    state.voice.speak_cancel.store(false, Ordering::SeqCst);
    let mut job = state
        .flow
        .jobs
        .submit(JobKind::TextToSpeech, "Antwort vorlesen", true);
    let mut slot = job
        .acquire()
        .map_err(|_| AppError::Invalid("Vorlesen abgebrochen.".to_owned()))?;
    set_state(app, &state.voice, VoiceState::Speaking, None);
    let cancel = Arc::clone(&state.voice.speak_cancel);
    // Auch der Job-Abbruch (Job-Liste) unterbricht die Ausgabe.
    let job_flag = job.cancel_flag();
    let watcher_cancel = Arc::clone(&cancel);
    let watcher_done = Arc::new(AtomicBool::new(false));
    let watcher = {
        let done = Arc::clone(&watcher_done);
        thread::spawn(move || {
            while !done.load(Ordering::SeqCst) {
                if job_flag.load(Ordering::SeqCst) {
                    watcher_cancel.store(true, Ordering::SeqCst);
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
        })
    };
    let result = tts::speak(&pack, &voice, &speech, &cancel, |_| {});
    watcher_done.store(true, Ordering::SeqCst);
    let _ = watcher.join();
    set_state(app, &state.voice, VoiceState::Idle, None);
    match result {
        Ok(_) | Err(SpeechError::Cancelled) => Ok(()),
        Err(error) => {
            slot.fail();
            Err(AppError::Invalid(error.to_string()))
        }
    }
}

/// Unterbricht die laufende Sprachausgabe sofort.
#[tauri::command]
pub fn voice_stop_output(state: State<'_, AppState>) {
    state.voice.speak_cancel.store(true, Ordering::SeqCst);
}

/// Schaltet die Sprachausgabe stumm oder wieder ein.
#[tauri::command]
pub fn voice_set_muted(app: AppHandle, state: State<'_, AppState>, muted: bool) {
    state.voice.muted.store(muted, Ordering::SeqCst);
    if muted {
        state.voice.speak_cancel.store(true, Ordering::SeqCst);
    }
    emit(&app, state.voice.state(), muted, 0, 0.0, None);
}

/// Misst auf diesem PC, ob Sprache nutzbar ist, und merkt das Ergebnis im Tresor.
#[tauri::command(async)]
pub async fn voice_run_benchmark(app: AppHandle) -> AppResult<VoiceBenchmark> {
    tauri::async_runtime::spawn_blocking(move || benchmark_blocking(&app))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
}

fn benchmark_blocking(app: &AppHandle) -> AppResult<VoiceBenchmark> {
    let state = app.state::<AppState>();
    require_session(&state)?;
    let language = ui_lang(&state);
    let stt_pack = open_usable(&state, PACK_WHISPER)?;
    let tts_pack = open_usable(&state, PACK_PIPER)?;
    let voice = tts::voice_for(&tts_pack, &language).ok_or_else(|| {
        AppError::Invalid("Für diese Sprache gibt es keine lokale Stimme.".to_owned())
    })?;

    let mut job = state
        .flow
        .jobs
        .submit(JobKind::SpeechToText, "Sprach-Leistungstest", true);
    let mut slot = job
        .acquire()
        .map_err(|_| AppError::Invalid("Test abgebrochen.".to_owned()))?;
    let mut notes = Vec::new();
    let cancel = AtomicBool::new(false);

    let (raw, stats) = tts::synthesize(&tts_pack, &voice, benchmark_sentence(&language), &cancel)
        .map_err(|e| AppError::Invalid(e.to_string()))?;
    let mut samples = resample_linear(&raw, voice.sample_rate, CAPTURE_RATE);
    let audio_seconds = samples.len() as f64 / f64::from(CAPTURE_RATE);

    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let before = system.available_memory();

    let threads = ensure_bootstrap(&state)
        .map(|b| b.plan.threads)
        .unwrap_or(2);
    let load_started = Instant::now();
    let server = WhisperServer::start(&stt_pack, threads).map_err(|e| {
        slot.fail();
        AppError::Invalid(e.to_string())
    })?;
    let load_ms = u64::try_from(load_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    system.refresh_memory();
    let reserve = system.available_memory();

    let started = Instant::now();
    let transcript = server.transcribe(&samples, whisper_language(&language));
    let seconds = started.elapsed().as_secs_f64();
    drop(server);
    samples.zeroize();
    let text = transcript.map_err(|e| {
        slot.fail();
        AppError::Invalid(e.to_string())
    })?;

    let rtf = if audio_seconds > 0.0 {
        seconds / audio_seconds
    } else {
        f64::MAX
    };
    let mut passed = true;
    if reserve < MIN_RAM_RESERVE_BYTES {
        passed = false;
        notes.push(format!(
            "Freier Arbeitsspeicher während der Erkennung nur {} MB (nötig: {} MB).",
            reserve / 1_000_000,
            MIN_RAM_RESERVE_BYTES / 1_000_000
        ));
    }
    if rtf > MAX_STT_REAL_TIME_FACTOR {
        passed = false;
        notes.push(format!("Erkennung zu langsam (Faktor {rtf:.2}, nötig höchstens {MAX_STT_REAL_TIME_FACTOR:.1})."));
    }
    if stats.first_audio_ms > MAX_TTS_FIRST_AUDIO_MS {
        passed = false;
        notes.push(format!(
            "Sprachausgabe startet zu spät ({} ms).",
            stats.first_audio_ms
        ));
    }
    if text.trim().is_empty() {
        passed = false;
        notes.push("Die Erkennung lieferte keinen Text.".to_owned());
    }
    notes.push(format!(
        "Sprachausgabe: erster Ton nach {} ms, Synthese gesamt {} ms.",
        stats.first_audio_ms, stats.synth_ms
    ));
    notes.push(format!(
        "Freier Speicher vor dem Start {} MB, nach dem Laden {} MB.",
        before / 1_000_000,
        reserve / 1_000_000
    ));

    let benchmark = VoiceBenchmark {
        measured_unix_ms: crate::now_unix_ms(),
        ram_reserve_bytes: reserve,
        stt_real_time_factor: rtf,
        tts_first_audio_ms: stats.first_audio_ms,
        load_ms,
        passed,
        notes,
    };
    store_benchmark(&state, &benchmark)?;
    if let Ok(mut inner) = state.voice.lock() {
        inner.benchmark = Some(benchmark.clone());
    }
    Ok(benchmark)
}

fn store_benchmark(state: &AppState, benchmark: &VoiceBenchmark) -> AppResult<()> {
    let bootstrap = ensure_bootstrap(state)?;
    let session = require_session(state)?;
    let text = serde_json::to_string(benchmark).map_err(|e| AppError::Internal(e.to_string()))?;
    let mut vault = session.vault_runtime.lock()?;
    vault
        .repository_mut()
        .set_setting(&benchmark_key(&bootstrap.host_identifier), &text)?;
    let mut no_fault = pa_vault::hot_copy::NoFault;
    let _ = vault.sync(&mut no_fault);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_key_uses_short_host_prefix() {
        assert_eq!(
            benchmark_key("0123456789abcdef0123456789abcdef"),
            "host.0123456789abcdef.voice_benchmark"
        );
    }

    #[test]
    fn every_ui_language_with_a_voice_has_a_test_sentence() {
        for language in ["de", "en", "es", "fr"] {
            assert!(!benchmark_sentence(language).is_empty());
        }
    }

    #[test]
    fn new_runtime_is_idle_and_not_muted() {
        let runtime = VoiceRuntime::new();
        assert_eq!(runtime.state(), VoiceState::Idle);
        assert!(!runtime.muted.load(Ordering::SeqCst));
    }
}
