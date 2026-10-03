use std::time::{Duration, Instant};

/// Ist die minimale Handlungsanweisung an den Hintergrund-Executor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDecision {
    Idle,
    Start,
    StartAndExit,
    Coalesced,
    Exit,
}

/// Koalesziert Timer-, UI- und Exit-Anforderungen, sodass nie zwei Snapshots parallel laufen.
#[derive(Debug)]
pub struct SyncWorkerState {
    interval: Duration,
    deadline: Instant,
    in_progress: bool,
    pending: bool,
    exit_requested: bool,
}

impl SyncWorkerState {
    /// Verwendet injizierte monotone Zeit, damit Tests und Systemzeitwechsel deterministisch bleiben.
    pub fn new(now: Instant, interval: Duration) -> Self {
        Self {
            interval,
            deadline: now + interval,
            in_progress: false,
            pending: false,
            exit_requested: false,
        }
    }

    /// Löst vor dem exakten Fälligkeitspunkt keinen unnötigen USB-Schreibzugriff aus.
    pub fn on_timer(&mut self, now: Instant) -> SyncDecision {
        if now < self.deadline {
            SyncDecision::Idle
        } else {
            self.request_sync()
        }
    }

    /// Verschmilzt gleichzeitige Anforderungen zu höchstens einem Folgelauf.
    pub fn request_sync(&mut self) -> SyncDecision {
        if self.in_progress {
            self.pending = true;
            SyncDecision::Coalesced
        } else {
            self.in_progress = true;
            SyncDecision::Start
        }
    }

    /// Erzwingt einen letzten Lauf und merkt den Exit auch während eines aktiven Syncs vor.
    pub fn request_exit(&mut self) -> SyncDecision {
        self.exit_requested = true;
        if self.in_progress {
            self.pending = true;
            SyncDecision::Coalesced
        } else {
            self.in_progress = true;
            SyncDecision::StartAndExit
        }
    }

    /// Aktualisiert Deadline nur nach Erfolg und startet genau einen koaleszierten Folgelauf.
    pub fn completed(&mut self, now: Instant, succeeded: bool) -> SyncDecision {
        self.in_progress = false;
        if succeeded {
            self.deadline = now + self.interval;
        } else {
            self.pending = true;
        }
        if self.exit_requested {
            self.pending = false;
            return SyncDecision::Exit;
        }
        if succeeded && self.pending {
            self.pending = false;
            self.in_progress = true;
            SyncDecision::Start
        } else {
            SyncDecision::Idle
        }
    }

    /// Verhindert, dass ein fehlgeschlagener Medien-Sync als erledigt angezeigt wird.
    pub fn has_pending_work(&self) -> bool {
        self.pending
    }
}
