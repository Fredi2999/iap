//! Fenstererfassung und der Takt des Begleitprogramms.
//!
//! Nur die Betriebssystem-Zugriffe stehen hier. Die Verdichtung der Stichproben zu Journalzeilen
//! macht der reine, testbare [`Tracker`]. Gelesen werden ausschließlich der Name der
//! Vordergrund-Anwendung, deren Fenstertitel und die Leerlaufzeit (Zeit seit der letzten Eingabe);
//! nie Tastendrücke, Inhalte oder Bildschirm.

use crate::{clean_title, Record};

/// Ab so viel Leerlauf gilt der PC als unbenutzt; laufende Abschnitte werden beendet.
pub const IDLE_THRESHOLD_MS: i64 = 90_000;

/// Kürzeste Dauer, die als eigener Abschnitt ins Journal kommt (kürzere Vordergrund-Blitze
/// beim Fensterwechsel werden verworfen).
pub const MIN_SEGMENT_MS: i64 = 2_000;

/// So oft wird ein noch laufender Abschnitt als Zwischenstand ins Journal geschrieben. Das
/// Beenden des Begleiters (Fenster schließen, Prozess beenden) lässt sich nicht abfangen; so
/// gehen im schlimmsten Fall nur diese Sekunden verloren statt des ganzen Abschnitts.
pub const HEARTBEAT_MS: i64 = 30_000;

/// Was gerade im Vordergrund ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    pub app: String,
    pub title: String,
}

/// Verdichtet eine Folge von Stichproben zu Journal-Abschnitten. Rein und ohne Betriebssystem,
/// damit der Takt getestet werden kann.
#[derive(Debug, Default)]
pub struct Tracker {
    current: Option<Segment>,
    /// Ob Titel mitgeschrieben werden (vom Nutzer abschaltbar).
    record_titles: bool,
}

#[derive(Debug, Clone)]
struct Segment {
    app: String,
    title: String,
    start_unix_ms: i64,
    last_unix_ms: i64,
    /// Ein Teil dieses Abschnitts steht schon als Zwischenstand im Journal.
    continued: bool,
}

impl Tracker {
    pub fn new(record_titles: bool) -> Self {
        Self {
            current: None,
            record_titles,
        }
    }

    /// Verarbeitet eine Stichprobe. `foreground` ist `None`, wenn der PC leerläuft oder kein
    /// Fenster lesbar ist. Liefert einen fertigen Abschnitt, wenn einer abgeschlossen wurde.
    pub fn observe(
        &mut self,
        now_unix_ms: i64,
        foreground: Option<&Foreground>,
        idle_ms: i64,
    ) -> Option<Record> {
        let active = if idle_ms >= IDLE_THRESHOLD_MS {
            None
        } else {
            foreground
        };
        match active {
            None => self.flush(now_unix_ms),
            Some(fg) => {
                let title = if self.record_titles {
                    clean_title(&fg.title)
                } else {
                    String::new()
                };
                match &mut self.current {
                    Some(segment) if segment.app == fg.app && segment.title == title => {
                        segment.last_unix_ms = now_unix_ms;
                        if now_unix_ms - segment.start_unix_ms >= HEARTBEAT_MS {
                            // Zwischenstand sichern und mit dem Rest als Fortsetzung weitermachen.
                            let record = Record::Window {
                                at_unix_ms: segment.start_unix_ms,
                                app: segment.app.clone(),
                                title: segment.title.clone(),
                                ms: now_unix_ms - segment.start_unix_ms,
                                cont: segment.continued,
                            };
                            segment.start_unix_ms = now_unix_ms;
                            segment.continued = true;
                            Some(record)
                        } else {
                            None
                        }
                    }
                    _ => {
                        let finished = self.flush(now_unix_ms);
                        self.current = Some(Segment {
                            app: fg.app.clone(),
                            title,
                            start_unix_ms: now_unix_ms,
                            last_unix_ms: now_unix_ms,
                            continued: false,
                        });
                        finished
                    }
                }
            }
        }
    }

    /// Schließt den laufenden Abschnitt ab (beim Stoppen oder Leerlauf).
    pub fn flush(&mut self, now_unix_ms: i64) -> Option<Record> {
        let segment = self.current.take()?;
        let ms = (now_unix_ms.max(segment.last_unix_ms) - segment.start_unix_ms).max(0);
        // Ein Rest nach einem Zwischenstand wird auch dann festgehalten, wenn er kurz ist:
        // der Anfang des Abschnitts steht ja schon im Journal.
        if ms < MIN_SEGMENT_MS && !segment.continued {
            return None;
        }
        if ms == 0 {
            return None;
        }
        Some(Record::Window {
            at_unix_ms: segment.start_unix_ms,
            app: segment.app,
            title: segment.title,
            ms,
            cont: segment.continued,
        })
    }
}

/// Liest das aktuelle Vordergrundfenster. `None`, wenn keins lesbar ist.
#[cfg(windows)]
pub fn sample_foreground() -> Option<Foreground> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }

        // Titel lesen.
        let mut title = String::new();
        let len = GetWindowTextLengthW(hwnd);
        if len > 0 {
            let mut buffer = vec![0u16; (len + 1) as usize];
            let read = GetWindowTextW(hwnd, &mut buffer);
            if read > 0 {
                title = String::from_utf16_lossy(&buffer[..read as usize]);
            }
        }

        // Anwendungsname über die Prozess-ID.
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let app = (pid != 0)
            .then(|| process_name(pid))
            .flatten()
            .unwrap_or_else(|| "unbekannt".to_owned());
        Some(Foreground { app, title })
    }
}

/// Dateiname der ausführbaren Datei eines Prozesses, ohne Pfad.
#[cfg(windows)]
fn process_name(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buffer = vec![0u16; MAX_PATH as usize];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        result.ok()?;
        let full = String::from_utf16_lossy(&buffer[..size as usize]);
        Some(full.rsplit(['\\', '/']).next().unwrap_or(&full).to_owned())
    }
}

/// Leerlaufzeit in Millisekunden (Zeit seit der letzten Maus-/Tastatureingabe).
#[cfg(windows)]
pub fn idle_ms() -> i64 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info).as_bool() {
            let now = GetTickCount();
            i64::from(now.wrapping_sub(info.dwTime))
        } else {
            0
        }
    }
}

#[cfg(not(windows))]
pub fn sample_foreground() -> Option<Foreground> {
    None
}

#[cfg(not(windows))]
pub fn idle_ms() -> i64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fg(app: &str, title: &str) -> Foreground {
        Foreground {
            app: app.into(),
            title: title.into(),
        }
    }

    #[test]
    fn a_continuous_window_becomes_one_segment_on_change() {
        let mut t = Tracker::new(true);
        let chrome = fg("chrome.exe", "Wikipedia");
        assert!(t.observe(0, Some(&chrome), 0).is_none());
        assert!(t.observe(5000, Some(&chrome), 0).is_none());
        // Wechsel schließt den Chrome-Abschnitt ab.
        let code = fg("code.exe", "main.rs");
        let done = t.observe(10000, Some(&code), 0).unwrap();
        assert_eq!(
            done,
            Record::Window {
                at_unix_ms: 0,
                app: "chrome.exe".into(),
                title: "Wikipedia".into(),
                ms: 10000,
                cont: false
            }
        );
    }

    #[test]
    fn idle_flushes_the_current_segment() {
        let mut t = Tracker::new(true);
        let chrome = fg("chrome.exe", "Seite");
        t.observe(0, Some(&chrome), 0);
        let done = t.observe(20000, Some(&chrome), IDLE_THRESHOLD_MS).unwrap();
        assert_eq!(
            done.clone(),
            Record::Window {
                at_unix_ms: 0,
                app: "chrome.exe".into(),
                title: "Seite".into(),
                ms: 20000,
                cont: false
            }
        );
        // Nach Leerlauf gibt es keinen laufenden Abschnitt mehr.
        assert!(t.observe(25000, None, IDLE_THRESHOLD_MS).is_none());
    }

    #[test]
    fn very_short_flashes_are_dropped() {
        let mut t = Tracker::new(true);
        let a = fg("a.exe", "x");
        t.observe(0, Some(&a), 0);
        // Nach 500 ms direkt Wechsel: zu kurz, kein Abschnitt.
        let b = fg("b.exe", "y");
        assert!(t.observe(500, Some(&b), 0).is_none());
    }

    #[test]
    fn titles_can_be_turned_off() {
        let mut t = Tracker::new(false);
        let chrome = fg("chrome.exe", "Geheime Seite");
        t.observe(0, Some(&chrome), 0);
        let done = t.flush(5000).unwrap();
        if let Record::Window { title, .. } = done {
            assert_eq!(title, "");
        } else {
            panic!("Fenster erwartet");
        }
    }

    #[test]
    fn finishing_flushes_the_last_segment() {
        let mut t = Tracker::new(true);
        let a = fg("a.exe", "x");
        t.observe(1000, Some(&a), 0);
        let done = t.flush(9000).unwrap();
        assert_eq!(
            done,
            Record::Window {
                at_unix_ms: 1000,
                app: "a.exe".into(),
                title: "x".into(),
                ms: 8000,
                cont: false
            }
        );
    }

    #[test]
    fn a_long_window_is_saved_while_it_is_still_open() {
        // Regression: ohne Zwischenstand blieb ein einziges, langes Fenster bis zum Wechsel
        // unsichtbar; beim Schließen des Begleiters war das Journal leer und der Import schlug fehl.
        let mut t = Tracker::new(true);
        let a = fg("a.exe", "x");
        assert!(t.observe(0, Some(&a), 0).is_none());
        assert!(t.observe(HEARTBEAT_MS - 5_000, Some(&a), 0).is_none());
        let beat = t.observe(HEARTBEAT_MS, Some(&a), 0).expect("Zwischenstand");
        assert_eq!(
            beat,
            Record::Window {
                at_unix_ms: 0,
                app: "a.exe".into(),
                title: "x".into(),
                ms: HEARTBEAT_MS,
                cont: false
            }
        );
        // Das Weiterlaufen ist eine Fortsetzung und zählt nicht als neuer Wechsel.
        let b = fg("b.exe", "y");
        let done = t.observe(HEARTBEAT_MS + 8_000, Some(&b), 0).unwrap();
        assert_eq!(
            done,
            Record::Window {
                at_unix_ms: HEARTBEAT_MS,
                app: "a.exe".into(),
                title: "x".into(),
                ms: 8_000,
                cont: true
            }
        );
    }

    #[test]
    fn a_short_tail_after_a_heartbeat_is_not_lost() {
        let mut t = Tracker::new(true);
        let a = fg("a.exe", "x");
        t.observe(0, Some(&a), 0);
        t.observe(HEARTBEAT_MS, Some(&a), 0).expect("Zwischenstand");
        // Nur 1 s nach dem Zwischenstand: als Fortsetzung trotzdem festgehalten.
        let done = t.flush(HEARTBEAT_MS + 1_000).expect("Rest");
        assert!(matches!(
            done,
            Record::Window {
                ms: 1_000,
                cont: true,
                ..
            }
        ));
    }

    #[cfg(not(windows))]
    #[test]
    fn os_sampling_is_unsupported_off_windows() {
        assert!(sample_foreground().is_none());
        assert_eq!(idle_ms(), 0);
    }
}
