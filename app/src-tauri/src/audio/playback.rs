//! Wiedergabe von PCM über WASAPI (Windows), sofort abbrechbar.
//!
//! Die Ausgabe von Piper kommt als Datenstrom; sie wird satzweise
//! eingespeist und ohne Datei direkt an das Audiosystem gegeben. Ein Abbruch
//! (Unterbrechen, Stummschalten, Beenden) stoppt den Stream und verwirft den
//! bereits eingereihten Rest.

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
};

use thiserror::Error;

/// Fehler der Wiedergabe.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum PlaybackError {
    /// Es gibt kein Ausgabegerät.
    #[error("Es wurde kein Ausgabegerät gefunden.")]
    NoDevice,
    /// Diese Plattform ist (noch) nicht geprüft.
    #[error("Die Sprachausgabe ist auf dieser Plattform noch nicht geprüft.")]
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
    /// Anderer Fehler des Audiosystems.
    #[error("Audiofehler: {0}")]
    Os(String),
}

/// Gemeinsamer Zustand zwischen Einspeisung und Ausgabe-Thread.
struct Shared {
    queue: Mutex<VecDeque<i16>>,
    /// Es kommen keine weiteren Daten mehr.
    closed: AtomicBool,
    /// Sofort abbrechen und Rest verwerfen.
    stop: AtomicBool,
    /// Der Ausgabe-Thread ist zu Ende.
    finished: AtomicBool,
}

/// Laufende Ausgabe.
pub struct Player {
    shared: Arc<Shared>,
    join: Option<JoinHandle<()>>,
}

impl Player {
    /// Öffnet das Standard-Ausgabegerät für mono 16-Bit-PCM mit `sample_rate`.
    pub fn start(sample_rate: u32) -> Result<Self, PlaybackError> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::new()),
            closed: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            finished: AtomicBool::new(false),
        });
        let join = imp::spawn(Arc::clone(&shared), sample_rate)?;
        Ok(Self {
            shared,
            join: Some(join),
        })
    }

    /// Reiht weitere Samples ein.
    pub fn push(&self, samples: &[i16]) {
        if self.shared.stop.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut queue) = self.shared.queue.lock() {
            queue.extend(samples.iter().copied());
        }
    }

    /// Meldet, dass nichts mehr folgt; die Ausgabe endet, sobald sie leer gespielt ist.
    pub fn close_input(&self) {
        self.shared.closed.store(true, Ordering::SeqCst);
    }

    /// Bricht sofort ab und verwirft den Rest.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Ok(mut queue) = self.shared.queue.lock() {
            queue.clear();
        }
    }

    /// Ob die Ausgabe zu Ende ist (fertig gespielt oder abgebrochen).
    pub fn is_finished(&self) -> bool {
        self.shared.finished.load(Ordering::SeqCst)
    }

    /// Wartet auf das Ende der Ausgabe.
    pub fn wait(mut self) {
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::{
        ptr,
        sync::{atomic::Ordering, Arc},
        thread::{self, JoinHandle},
        time::Duration,
    };

    use windows::{
        core::IUnknown,
        Win32::{
            Media::Audio::{
                eConsole, eRender, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator,
                MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX, WAVE_FORMAT_PCM,
            },
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
            },
        },
    };

    use super::{PlaybackError, Shared};

    const E_NOTFOUND: i32 = 0x8007_0490_u32 as i32;

    fn map_error(error: &windows::core::Error) -> PlaybackError {
        if error.code().0 == E_NOTFOUND {
            PlaybackError::NoDevice
        } else {
            PlaybackError::Os(error.message())
        }
    }

    pub fn spawn(shared: Arc<Shared>, sample_rate: u32) -> Result<JoinHandle<()>, PlaybackError> {
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel::<Result<(), PlaybackError>>(1);
        let worker = Arc::clone(&shared);
        let join = thread::Builder::new()
            .name("iap-speaker".to_owned())
            .spawn(move || {
                // SAFETY: COM wird auf diesem Thread initialisiert und am Ende freigegeben.
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                    run(&worker, sample_rate, &ready_tx);
                    CoUninitialize();
                }
                worker.finished.store(true, Ordering::SeqCst);
            })
            .map_err(|e| PlaybackError::Os(e.to_string()))?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(join),
            Ok(Err(error)) => {
                let _ = join.join();
                Err(error)
            }
            Err(_) => {
                shared.stop.store(true, Ordering::SeqCst);
                let _ = join.join();
                Err(PlaybackError::Os(
                    "Das Ausgabegerät antwortete nicht.".to_owned(),
                ))
            }
        }
    }

    unsafe fn open(
        sample_rate: u32,
    ) -> Result<(IAudioClient, IAudioRenderClient, u32), PlaybackError> {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None::<&IUnknown>, CLSCTX_ALL)
                .map_err(|e| map_error(&e))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| map_error(&e))?;
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| map_error(&e))?;
        let format = WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_PCM as u16,
            nChannels: 1,
            nSamplesPerSec: sample_rate,
            nAvgBytesPerSec: sample_rate * 2,
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
        let frames = client.GetBufferSize().map_err(|e| map_error(&e))?;
        let render: IAudioRenderClient = client.GetService().map_err(|e| map_error(&e))?;
        Ok((client, render, frames))
    }

    unsafe fn run(
        shared: &Shared,
        sample_rate: u32,
        ready: &std::sync::mpsc::SyncSender<Result<(), PlaybackError>>,
    ) {
        let (client, render, capacity) = match open(sample_rate) {
            Ok(opened) => opened,
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

        loop {
            if shared.stop.load(Ordering::SeqCst) {
                break;
            }
            let padding = match client.GetCurrentPadding() {
                Ok(padding) => padding,
                Err(_) => break,
            };
            let free = capacity.saturating_sub(padding) as usize;
            let chunk: Vec<i16> = match shared.queue.lock() {
                Ok(mut queue) => {
                    let n = free.min(queue.len());
                    queue.drain(..n).collect()
                }
                Err(_) => break,
            };
            if !chunk.is_empty() {
                let Ok(target) = render.GetBuffer(chunk.len() as u32) else {
                    break;
                };
                ptr::copy_nonoverlapping(chunk.as_ptr(), target as *mut i16, chunk.len());
                if render.ReleaseBuffer(chunk.len() as u32, 0).is_err() {
                    break;
                }
            } else if shared.closed.load(Ordering::SeqCst) && padding == 0 {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        // Stop + Reset verwirft bereits an das Gerät übergebene Puffer sofort.
        let _ = client.Stop();
        let _ = client.Reset();
    }
}

#[cfg(not(windows))]
mod imp {
    use std::{sync::Arc, thread::JoinHandle};

    use super::{PlaybackError, Shared};

    pub fn spawn(_shared: Arc<Shared>, _sample_rate: u32) -> Result<JoinHandle<()>, PlaybackError> {
        Err(PlaybackError::Unsupported)
    }
}
