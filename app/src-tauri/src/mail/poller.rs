//! Takt und Schalter der Auto-Antwort.
//!
//! Der Schalter lebt nur im Arbeitsspeicher: Nach jedem Entsperren ist er AUS, und es gibt keine
//! stille Wiederaufnahme. Jeder Start erhöht eine Generation; ein laufender Takt beendet sich,
//! sobald seine Generation nicht mehr aktuell ist. So kann es nie zwei Takte gleichzeitig geben,
//! und ein Not-Aus wirkt auch dann, wenn der Takt gerade schläft.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Kürzester und längster Abstand zwischen zwei Prüfungen in Minuten.
pub const MIN_INTERVAL_MINUTES: u32 = 1;
pub const MAX_INTERVAL_MINUTES: u32 = 60;

/// Warum der Takt endet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Nutzer hat ausgeschaltet oder Not-Aus gedrückt (Generation überholt).
    Switched,
    /// Air Gap wurde eingeschaltet.
    AirGap,
    /// Tresor gesperrt oder App beendet.
    SessionClosed,
}

/// Nächster Schritt des Takts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Stop(StopReason),
    Run,
    Wait,
}

/// Entscheidet, was der Takt jetzt tut. Rein und deshalb testbar.
///
/// Reihenfolge: ein überholter Takt endet zuerst, dann Air Gap, dann Sitzung, erst danach zählt die Zeit.
pub fn next_action(
    my_generation: u64,
    current_generation: u64,
    air_gap: bool,
    session_open: bool,
    now_ms: i64,
    due_ms: i64,
) -> Action {
    if my_generation != current_generation {
        Action::Stop(StopReason::Switched)
    } else if air_gap {
        Action::Stop(StopReason::AirGap)
    } else if !session_open {
        Action::Stop(StopReason::SessionClosed)
    } else if now_ms >= due_ms {
        Action::Run
    } else {
        Action::Wait
    }
}

/// Abstand zwischen zwei Prüfungen in Millisekunden, auf den erlaubten Bereich begrenzt.
pub fn interval_ms(minutes: u32) -> i64 {
    i64::from(minutes.clamp(MIN_INTERVAL_MINUTES, MAX_INTERVAL_MINUTES)) * 60_000
}

/// Sichtbarer Laufzustand.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunStatus {
    pub running: bool,
    pub last_run_unix_ms: Option<i64>,
    pub next_run_unix_ms: Option<i64>,
    pub last_error: Option<String>,
}

/// Gemeinsamer Zustand von Takt und Befehlen.
#[derive(Default)]
pub struct MailRuntime {
    generation: AtomicU64,
    polling: Mutex<bool>,
    pub status: Mutex<RunStatus>,
    /// Mails, die wegen eines Limits warten (nur im Speicher, damit das Protokoll sie einmal nennt).
    pub deferred: Mutex<HashSet<String>>,
}

impl MailRuntime {
    /// Schaltet ein und liefert die Generation für den neuen Takt. `None`, wenn schon eingeschaltet.
    pub fn start(&self) -> Option<u64> {
        let mut polling = self.polling.lock().ok()?;
        if *polling {
            return None;
        }
        *polling = true;
        Some(self.generation.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Schaltet aus; ein laufender Takt beendet sich beim nächsten Schritt.
    pub fn stop(&self) {
        if let Ok(mut polling) = self.polling.lock() {
            *polling = false;
        }
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut status) = self.status.lock() {
            status.next_run_unix_ms = None;
        }
    }

    pub fn is_polling(&self) -> bool {
        self.polling.lock().map(|p| *p).unwrap_or(false)
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runtime_starts_off() {
        let runtime = MailRuntime::default();
        assert!(!runtime.is_polling());
    }

    #[test]
    fn start_is_exclusive_and_stop_obsoletes_the_running_loop() {
        let runtime = MailRuntime::default();
        let generation = runtime.start().expect("erster Start");
        assert!(runtime.is_polling());
        assert!(runtime.start().is_none(), "kein zweiter Takt");
        assert_eq!(
            next_action(generation, runtime.generation(), false, true, 0, 0),
            Action::Run
        );
        runtime.stop();
        assert!(!runtime.is_polling());
        assert_eq!(
            next_action(generation, runtime.generation(), false, true, 0, 0),
            Action::Stop(StopReason::Switched)
        );
        // Neuer Start liefert eine neue Generation, der alte Takt bleibt überholt.
        let again = runtime.start().expect("neuer Start");
        assert_ne!(again, generation);
        assert_eq!(
            next_action(generation, runtime.generation(), false, true, 0, 0),
            Action::Stop(StopReason::Switched)
        );
    }

    #[test]
    fn air_gap_and_a_closed_session_stop_the_loop_before_anything_runs() {
        assert_eq!(
            next_action(1, 1, true, true, 100, 0),
            Action::Stop(StopReason::AirGap)
        );
        assert_eq!(
            next_action(1, 1, false, false, 100, 0),
            Action::Stop(StopReason::SessionClosed)
        );
        // Air Gap hat Vorrang vor „fällig“.
        assert_eq!(
            next_action(1, 1, true, false, 100, 0),
            Action::Stop(StopReason::AirGap)
        );
    }

    #[test]
    fn the_loop_waits_until_due() {
        assert_eq!(next_action(1, 1, false, true, 99, 100), Action::Wait);
        assert_eq!(next_action(1, 1, false, true, 100, 100), Action::Run);
    }

    #[test]
    fn the_interval_is_clamped_to_one_to_sixty_minutes() {
        assert_eq!(interval_ms(0), 60_000);
        assert_eq!(interval_ms(5), 300_000);
        assert_eq!(interval_ms(10_000), 3_600_000);
    }
}
