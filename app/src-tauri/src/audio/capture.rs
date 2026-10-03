//! Mikrofonaufnahme über WASAPI (Windows).
//!
//! Aufgenommen wird nur nach einem sichtbaren Start (siehe `voice.rs`) und nur
//! mit einem von `pa-policy` freigegebenen Ticket. Das Audio landet in einem
//! flüchtigen [`AudioBuffer`] im Arbeitsspeicher, nie in einer Datei.
//!
//! WASAPI wandelt im gemeinsamen Modus mit `AUTOCONVERTPCM` selbst in das
//! angeforderte Format (16 kHz, mono, 16 Bit) um; dadurch braucht IAP
//! keinen eigenen Resampler.

use thiserror::Error;

/// Fehler der Aufnahme mit verständlichem Klartext für die Oberfläche.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum CaptureError {
    /// Es gibt kein Aufnahmegerät.
    #[error("Es wurde kein Mikrofon gefunden.")]
    NoDevice,
    /// Das Betriebssystem verweigert den Zugriff.
    #[error(
        "Windows verweigert den Mikrofonzugriff. Prüfe Einstellungen > Datenschutz > Mikrofon."
    )]
    AccessDenied,
    /// Nur Stille aufgenommen, typisch bei gesperrtem Mikrofon.
    #[error("Das Mikrofon liefert nur Stille. Prüfe die Stummschaltung und Einstellungen > Datenschutz > Mikrofon.")]
    OnlySilence,
    /// Diese Plattform ist (noch) nicht geprüft.
    #[error("Die Sprachaufnahme ist auf dieser Plattform noch nicht geprüft.")]
    #[cfg_attr(windows, allow(dead_code))]
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
    /// Anderer Fehler des Audiosystems.
    #[error("Audiofehler: {0}")]
    Os(String),
}

#[cfg(windows)]
pub use imp::Recorder;
#[cfg(not(windows))]
pub use stub::Recorder;

#[cfg(windows)]
mod imp {
    use std::{
        ptr,
        sync::{
            atomic::{AtomicBool, AtomicU32, Ordering},
            mpsc, Arc, Mutex,
        },
        thread::{self, JoinHandle},
        time::{Duration, Instant},
    };

    use windows::{
        core::IUnknown,
        Win32::{
            Foundation::E_ACCESSDENIED,
            Media::Audio::{
                eCapture, eConsole, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
                MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                WAVEFORMATEX, WAVE_FORMAT_PCM,
            },
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
            },
        },
    };

    use super::CaptureError;
    use crate::audio::pcm::{level, AudioBuffer, CAPTURE_RATE};

    /// Windows-HRESULT „Element nicht gefunden“: kein Gerät.
    const E_NOTFOUND: i32 = 0x8007_0490_u32 as i32;

    fn map_error(error: &windows::core::Error) -> CaptureError {
        let code = error.code();
        if code == E_ACCESSDENIED {
            CaptureError::AccessDenied
        } else if code.0 == E_NOTFOUND {
            CaptureError::NoDevice
        } else {
            CaptureError::Os(error.message())
        }
    }

    /// Eine laufende Aufnahme. Beim Fallenlassen wird gestoppt und der Puffer überschrieben.
    pub struct Recorder {
        stop: Arc<AtomicBool>,
        buffer: Arc<Mutex<AudioBuffer>>,
        level_bits: Arc<AtomicU32>,
        peak: Arc<AtomicU32>,
        failure: Arc<Mutex<Option<CaptureError>>>,
        started: Instant,
        join: Option<JoinHandle<()>>,
    }

    impl Recorder {
        /// Öffnet das Standardmikrofon und beginnt sofort. Fehler beim Öffnen
        /// (kein Gerät, verweigerter Zugriff) kommen synchron zurück.
        pub fn start() -> Result<Self, CaptureError> {
            let stop = Arc::new(AtomicBool::new(false));
            let buffer = Arc::new(Mutex::new(AudioBuffer::new()));
            let level_bits = Arc::new(AtomicU32::new(0));
            let peak = Arc::new(AtomicU32::new(0));
            let failure = Arc::new(Mutex::new(None));
            let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), CaptureError>>(1);

            let join = {
                let stop = Arc::clone(&stop);
                let buffer = Arc::clone(&buffer);
                let level_bits = Arc::clone(&level_bits);
                let peak = Arc::clone(&peak);
                let failure = Arc::clone(&failure);
                thread::Builder::new()
                    .name("iap-mic".to_owned())
                    .spawn(move || {
                        // SAFETY: COM wird im Arbeits-Thread initialisiert und am Ende
                        // wieder freigegeben; alle COM-Objekte leben nur in `run`.
                        unsafe {
                            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                            run(&stop, &buffer, &level_bits, &peak, &failure, &ready_tx);
                            CoUninitialize();
                        }
                    })
                    .map_err(|e| CaptureError::Os(e.to_string()))?
            };

            match ready_rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(())) => Ok(Self {
                    stop,
                    buffer,
                    level_bits,
                    peak,
                    failure,
                    started: Instant::now(),
                    join: Some(join),
                }),
                Ok(Err(error)) => {
                    let _ = join.join();
                    Err(error)
                }
                Err(_) => {
                    stop.store(true, Ordering::SeqCst);
                    let _ = join.join();
                    Err(CaptureError::Os(
                        "Das Mikrofon antwortete nicht.".to_owned(),
                    ))
                }
            }
        }

        /// Aktueller Pegel 0..1.
        pub fn level(&self) -> f32 {
            f32::from_bits(self.level_bits.load(Ordering::Relaxed))
        }

        /// Bisherige Aufnahmedauer.
        pub fn elapsed_ms(&self) -> u64 {
            u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
        }

        /// Ob die Höchstdauer erreicht wurde.
        pub fn overflowed(&self) -> bool {
            self.buffer.lock().map(|b| b.overflowed()).unwrap_or(false)
        }

        /// Fehler, der die Aufnahme unterwegs beendet hat (etwa Gerät entfernt).
        pub fn failure(&self) -> Option<CaptureError> {
            self.failure.lock().ok().and_then(|f| f.clone())
        }

        fn finish(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }

        /// Beendet die Aufnahme und übergibt die Samples.
        ///
        /// # Errors
        /// Der zuvor aufgetretene Fehler, oder `OnlySilence`, wenn länger als
        /// eine Sekunde nur Nullen ankamen.
        pub fn stop(mut self) -> Result<Vec<i16>, CaptureError> {
            self.finish();
            if let Some(error) = self.failure() {
                self.wipe();
                return Err(error);
            }
            let mut buffer = self
                .buffer
                .lock()
                .map_err(|_| CaptureError::Os("Puffer gesperrt".to_owned()))?;
            if buffer.duration_ms() > 1000 && self.peak.load(Ordering::Relaxed) == 0 {
                buffer.wipe();
                return Err(CaptureError::OnlySilence);
            }
            Ok(buffer.take())
        }

        /// Beendet die Aufnahme und überschreibt alles Aufgenommene.
        pub fn discard(mut self) {
            self.finish();
            self.wipe();
        }

        fn wipe(&self) {
            if let Ok(mut buffer) = self.buffer.lock() {
                buffer.wipe();
            }
        }
    }

    impl Drop for Recorder {
        fn drop(&mut self) {
            self.finish();
            self.wipe();
        }
    }

    /// Öffnet das Gerät und liest, bis `stop` gesetzt ist.
    ///
    /// # Safety
    /// Muss auf einem Thread mit initialisiertem COM laufen.
    unsafe fn run(
        stop: &AtomicBool,
        buffer: &Mutex<AudioBuffer>,
        level_bits: &AtomicU32,
        peak: &AtomicU32,
        failure: &Mutex<Option<CaptureError>>,
        ready: &mpsc::SyncSender<Result<(), CaptureError>>,
    ) {
        let opened = open_device();
        let (client, capture) = match opened {
            Ok(pair) => pair,
            Err(error) => {
                let _ = ready.send(Err(error));
                return;
            }
        };
        if let Err(error) = client.Start() {
            let _ = ready.send(Err(map_error(&error)));
            return;
        }
        let _ = ready.send(Ok(()));

        while !stop.load(Ordering::SeqCst) {
            match drain(&capture, buffer, level_bits, peak) {
                Ok(()) => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    if let Ok(mut slot) = failure.lock() {
                        *slot = Some(map_error(&error));
                    }
                    break;
                }
            }
        }
        let _ = client.Stop();
    }

    unsafe fn open_device() -> Result<(IAudioClient, IAudioCaptureClient), CaptureError> {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None::<&IUnknown>, CLSCTX_ALL)
                .map_err(|e| map_error(&e))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eCapture, eConsole)
            .map_err(|e| map_error(&e))?;
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| map_error(&e))?;
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 1,
            nSamplesPerSec: CAPTURE_RATE,
            nAvgBytesPerSec: CAPTURE_RATE * 2,
            nBlockAlign: 2,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                10_000_000,
                0,
                &format,
                None,
            )
            .map_err(|e| map_error(&e))?;
        let capture: IAudioCaptureClient = client.GetService().map_err(|e| map_error(&e))?;
        Ok((client, capture))
    }

    unsafe fn drain(
        capture: &IAudioCaptureClient,
        buffer: &Mutex<AudioBuffer>,
        level_bits: &AtomicU32,
        peak: &AtomicU32,
    ) -> windows::core::Result<()> {
        let mut packet = capture.GetNextPacketSize()?;
        while packet != 0 {
            let mut data: *mut u8 = ptr::null_mut();
            let mut frames = 0_u32;
            let mut flags = 0_u32;
            capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
            let count = frames as usize;
            let silent = flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0;
            let samples: Vec<i16> = if silent || data.is_null() {
                vec![0; count]
            } else {
                let base = data as *const i16;
                (0..count)
                    .map(|i| ptr::read_unaligned(base.add(i)))
                    .collect()
            };
            capture.ReleaseBuffer(frames)?;
            let max = samples
                .iter()
                .map(|s| u32::from(s.unsigned_abs()))
                .max()
                .unwrap_or(0);
            peak.fetch_max(max, Ordering::Relaxed);
            level_bits.store(level(&samples).to_bits(), Ordering::Relaxed);
            if let Ok(mut buffer) = buffer.lock() {
                buffer.push(&samples);
            }
            packet = capture.GetNextPacketSize()?;
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod stub {
    use super::CaptureError;

    /// Auf nicht geprüften Plattformen gibt es keine Aufnahme.
    pub struct Recorder;

    impl Recorder {
        pub fn start() -> Result<Self, CaptureError> {
            Err(CaptureError::Unsupported)
        }
        pub fn level(&self) -> f32 {
            0.0
        }
        pub fn elapsed_ms(&self) -> u64 {
            0
        }
        pub fn overflowed(&self) -> bool {
            false
        }
        pub fn failure(&self) -> Option<CaptureError> {
            None
        }
        pub fn stop(self) -> Result<Vec<i16>, CaptureError> {
            Err(CaptureError::Unsupported)
        }
        pub fn discard(self) {}
    }
}
