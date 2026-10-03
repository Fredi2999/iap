//! Sichtbares Job-Register mit serieller Modell-Warteschlange.
//!
//! Auf schwacher Hardware (T0) darf immer nur **eine** Arbeit die Engine, die
//! Spracherkennung oder die Sprachsynthese belegen: Erkennung, LLM-Inferenz und
//! Synthese laufen nacheinander (Konzept 10.5, AGENTS-Invariante 5). Jede
//! Arbeit meldet sich hier an, wartet FIFO auf ihren Platz, ist in der
//! Oberfläche mit Warteposition sichtbar und lässt sich abbrechen.
//!
//! Die Queue kennt weder Engine noch Modell; sie vergibt nur Plätze. Wer
//! einen Platz hält ([`RunGuard`]), darf die Ressource nutzen. Der Platz wird
//! auch bei Fehlern und Panik wieder frei.

use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex, MutexGuard,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use pa_types::avatar::{JobInfo, JobKind, JobStatus};
use thiserror::Error;

/// Wie viele abgeschlossene Einträge die Übersicht behält.
const FINISHED_HISTORY: usize = 20;

/// Fehler beim Warten auf einen Platz.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum JobError {
    /// Der Job wurde abgebrochen, bevor oder während er wartete.
    #[error("Der Auftrag wurde abgebrochen")]
    Cancelled,
}

type Listener = Arc<dyn Fn(&[JobInfo]) + Send + Sync>;

struct Entry {
    info: JobInfo,
    ticket: u64,
    cancel: Arc<AtomicBool>,
}

struct State {
    entries: Vec<Entry>,
    running: usize,
    capacity: usize,
    listener: Option<Listener>,
}

struct Shared {
    state: Mutex<State>,
    turn: Condvar,
    next_ticket: AtomicU64,
}

/// Gemeinsam genutzte Queue; klonen ist billig.
#[derive(Clone)]
pub struct JobQueue {
    shared: Arc<Shared>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    // Ein vergifteter Zustand ist hier unbedenklich: die Einträge sind
    // einfache Werte, und ein blockiertes Register wäre schlimmer.
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl JobQueue {
    /// Legt eine Queue mit `capacity` gleichzeitigen Plätzen an (T0: 1).
    pub fn new(capacity: usize) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    entries: Vec::new(),
                    running: 0,
                    capacity: capacity.max(1),
                    listener: None,
                }),
                turn: Condvar::new(),
                next_ticket: AtomicU64::new(1),
            }),
        }
    }

    /// Setzt die Rückmeldung, die bei jeder Änderung die aktuelle Liste erhält
    /// (die App macht daraus das Ereignis `jobs-changed`).
    pub fn set_listener(&self, listener: impl Fn(&[JobInfo]) + Send + Sync + 'static) {
        lock(&self.shared.state).listener = Some(Arc::new(listener));
    }

    /// Ändert die Zahl gleichzeitiger Plätze (stärkere Hardware, später gemessen).
    pub fn set_capacity(&self, capacity: usize) {
        lock(&self.shared.state).capacity = capacity.max(1);
        self.shared.turn.notify_all();
    }

    /// Meldet einen Job an. Er ist sofort sichtbar (`Waiting`), belegt aber
    /// noch keinen Platz.
    pub fn submit(&self, kind: JobKind, label: impl Into<String>, cancelable: bool) -> JobHandle {
        let ticket = self.shared.next_ticket.fetch_add(1, Ordering::SeqCst);
        let id = format!("job-{ticket}");
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut state = lock(&self.shared.state);
            state.entries.push(Entry {
                info: JobInfo {
                    id: id.clone(),
                    kind,
                    label: label.into(),
                    status: JobStatus::Waiting,
                    queue_position: None,
                    started_unix_ms: now_ms(),
                    cancelable,
                },
                ticket,
                cancel: Arc::clone(&cancel),
            });
            self.publish(&mut state);
        }
        JobHandle {
            queue: self.clone(),
            id,
            cancel,
            acquired: false,
        }
    }

    /// Aktuelle Übersicht.
    pub fn snapshot(&self) -> Vec<JobInfo> {
        let mut state = lock(&self.shared.state);
        renumber(&mut state);
        state.entries.iter().map(|e| e.info.clone()).collect()
    }

    /// Bricht einen Job ab; wartende Jobs enden sofort, laufende sehen das
    /// Abbruch-Flag. Gibt `false` zurück, wenn es ihn nicht gibt oder er nicht
    /// abbrechbar ist.
    pub fn cancel(&self, id: &str) -> bool {
        let mut state = lock(&self.shared.state);
        let Some(entry) = state.entries.iter_mut().find(|e| e.info.id == id) else {
            return false;
        };
        if !entry.info.cancelable || is_final(entry.info.status) {
            return false;
        }
        entry.cancel.store(true, Ordering::SeqCst);
        if entry.info.status == JobStatus::Waiting {
            entry.info.status = JobStatus::Cancelled;
        }
        self.publish(&mut state);
        drop(state);
        self.shared.turn.notify_all();
        true
    }

    /// Bricht alle laufenden und wartenden Jobs ab, deren Art zutrifft.
    ///
    /// Beim Wechsel zum Pet stoppen so Workflows und Agent Flow, während
    /// Chat-Antworten bewusst nicht betroffen sind.
    pub fn cancel_kinds(&self, kinds: &[JobKind]) -> usize {
        let ids: Vec<String> = lock(&self.shared.state)
            .entries
            .iter()
            .filter(|e| kinds.contains(&e.info.kind) && !is_final(e.info.status))
            .map(|e| e.info.id.clone())
            .collect();
        ids.iter().filter(|id| self.cancel(id)).count()
    }

    fn publish(&self, state: &mut State) {
        renumber(state);
        prune(state);
        if let Some(listener) = state.listener.clone() {
            let snapshot: Vec<JobInfo> = state.entries.iter().map(|e| e.info.clone()).collect();
            // Der Listener läuft außerhalb der Sperre nicht garantiert; er darf
            // daher nur kurz arbeiten (Ereignis absetzen).
            listener(&snapshot);
        }
    }
}

fn is_final(status: JobStatus) -> bool {
    matches!(
        status,
        JobStatus::Finished | JobStatus::Failed | JobStatus::Cancelled
    )
}

fn renumber(state: &mut State) {
    let mut waiting: Vec<(u64, usize)> = state
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.info.status == JobStatus::Waiting)
        .map(|(i, e)| (e.ticket, i))
        .collect();
    waiting.sort_unstable();
    for entry in &mut state.entries {
        entry.info.queue_position = None;
    }
    for (position, (_, index)) in waiting.into_iter().enumerate() {
        state.entries[index].info.queue_position = u32::try_from(position + 1).ok();
    }
}

fn prune(state: &mut State) {
    let finished = state
        .entries
        .iter()
        .filter(|e| is_final(e.info.status))
        .count();
    let mut excess = finished.saturating_sub(FINISHED_HISTORY);
    state.entries.retain(|e| {
        if excess > 0 && is_final(e.info.status) {
            excess -= 1;
            false
        } else {
            true
        }
    });
}

/// Griff eines angemeldeten Jobs.
pub struct JobHandle {
    queue: JobQueue,
    id: String,
    cancel: Arc<AtomicBool>,
    acquired: bool,
}

impl JobHandle {
    /// ID für die Oberfläche.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Abbruch-Flag; laufende Arbeit prüft es regelmäßig.
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel)
    }

    /// Wartet FIFO, bis dieser Job an der Reihe ist und ein Platz frei ist.
    ///
    /// # Errors
    /// [`JobError::Cancelled`], wenn der Job vorher abgebrochen wurde.
    pub fn acquire(&mut self) -> Result<RunGuard, JobError> {
        let shared = &self.queue.shared;
        let mut state = lock(&shared.state);
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                if let Some(entry) = state.entries.iter_mut().find(|e| e.info.id == self.id) {
                    entry.info.status = JobStatus::Cancelled;
                }
                self.queue.publish(&mut state);
                drop(state);
                shared.turn.notify_all();
                return Err(JobError::Cancelled);
            }
            let my_ticket = state
                .entries
                .iter()
                .find(|e| e.info.id == self.id)
                .map(|e| e.ticket);
            let Some(my_ticket) = my_ticket else {
                return Err(JobError::Cancelled);
            };
            let first_in_line = !state
                .entries
                .iter()
                .any(|e| e.info.status == JobStatus::Waiting && e.ticket < my_ticket);
            if first_in_line && state.running < state.capacity {
                state.running += 1;
                if let Some(entry) = state.entries.iter_mut().find(|e| e.info.id == self.id) {
                    entry.info.status = JobStatus::Running;
                    entry.info.started_unix_ms = now_ms();
                }
                self.queue.publish(&mut state);
                self.acquired = true;
                return Ok(RunGuard {
                    queue: self.queue.clone(),
                    id: self.id.clone(),
                    outcome: JobStatus::Finished,
                    done: false,
                });
            }
            state = shared
                .turn
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        if self.acquired {
            return;
        }
        // Nie gestartet: aus der Warteschlange nehmen, damit andere nachrücken.
        let mut state = lock(&self.queue.shared.state);
        if let Some(entry) = state.entries.iter_mut().find(|e| e.info.id == self.id) {
            if !is_final(entry.info.status) {
                entry.info.status = JobStatus::Cancelled;
            }
        }
        self.queue.publish(&mut state);
        drop(state);
        self.queue.shared.turn.notify_all();
    }
}

/// Der gehaltene Platz. Beim Fallenlassen wird er frei.
pub struct RunGuard {
    queue: JobQueue,
    id: String,
    outcome: JobStatus,
    done: bool,
}

impl RunGuard {
    /// Markiert den Job als fehlgeschlagen (sonst gilt er als beendet).
    pub fn fail(&mut self) {
        self.outcome = JobStatus::Failed;
    }

    /// Markiert den Job als abgebrochen.
    pub fn cancelled(&mut self) {
        self.outcome = JobStatus::Cancelled;
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        let outcome = if std::thread::panicking() {
            JobStatus::Failed
        } else {
            self.outcome
        };
        let mut state = lock(&self.queue.shared.state);
        state.running = state.running.saturating_sub(1);
        if let Some(entry) = state.entries.iter_mut().find(|e| e.info.id == self.id) {
            entry.info.status = outcome;
        }
        self.queue.publish(&mut state);
        drop(state);
        self.queue.shared.turn.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, thread, time::Duration};

    fn status_of(queue: &JobQueue, id: &str) -> JobStatus {
        queue
            .snapshot()
            .into_iter()
            .find(|j| j.id == id)
            .map(|j| j.status)
            .expect("Job")
    }

    #[test]
    fn jobs_run_strictly_one_after_another_in_submission_order() {
        let queue = JobQueue::new(1);
        let (tx, rx) = mpsc::channel::<String>();
        let mut first = queue.submit(JobKind::Chat, "erster", true);
        let second = queue.submit(JobKind::Agent, "zweiter", true);
        let third = queue.submit(JobKind::Vision, "dritter", true);

        let first_guard = first.acquire().expect("erster darf sofort");
        let mut workers = Vec::new();
        for (name, mut handle) in [("zweiter", second), ("dritter", third)] {
            let tx = tx.clone();
            workers.push(thread::spawn(move || {
                let guard = handle.acquire().expect("Platz");
                tx.send(name.to_owned()).unwrap();
                drop(guard);
            }));
            thread::sleep(Duration::from_millis(30));
        }
        assert!(
            rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "niemand darf vorbei"
        );
        assert_eq!(
            queue
                .snapshot()
                .iter()
                .filter(|j| j.status == JobStatus::Running)
                .count(),
            1
        );
        drop(first_guard);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "zweiter");
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap(), "dritter");
        for worker in workers {
            worker.join().unwrap();
        }
    }

    #[test]
    fn waiting_jobs_show_queue_positions() {
        let queue = JobQueue::new(1);
        let mut running = queue.submit(JobKind::Chat, "läuft", true);
        let _guard = running.acquire().unwrap();
        let a = queue.submit(JobKind::Agent, "a", true);
        let b = queue.submit(JobKind::Agent, "b", true);
        let snapshot = queue.snapshot();
        let position = |id: &str| snapshot.iter().find(|j| j.id == id).unwrap().queue_position;
        assert_eq!(position(a.id()), Some(1));
        assert_eq!(position(b.id()), Some(2));
    }

    #[test]
    fn cancelling_a_waiting_job_lets_the_next_one_advance() {
        let queue = JobQueue::new(1);
        let mut running = queue.submit(JobKind::Chat, "läuft", true);
        let guard = running.acquire().unwrap();
        let mut a = queue.submit(JobKind::Workflow, "a", true);
        let mut b = queue.submit(JobKind::Workflow, "b", true);
        let a_id = a.id().to_owned();
        let waiter = thread::spawn(move || a.acquire().map(|_| ()));
        thread::sleep(Duration::from_millis(30));
        assert!(queue.cancel(&a_id));
        assert_eq!(waiter.join().unwrap(), Err(JobError::Cancelled));
        assert_eq!(status_of(&queue, &a_id), JobStatus::Cancelled);
        drop(guard);
        let _b_guard = b.acquire().expect("b rückt nach");
    }

    #[test]
    fn dropping_an_unstarted_handle_frees_the_line() {
        let queue = JobQueue::new(1);
        let mut running = queue.submit(JobKind::Chat, "läuft", true);
        let guard = running.acquire().unwrap();
        let abandoned = queue.submit(JobKind::Agent, "verwaist", true);
        let mut next = queue.submit(JobKind::Agent, "nächster", true);
        drop(abandoned);
        drop(guard);
        let _ok = next
            .acquire()
            .expect("nicht hinter einem toten Eintrag hängen");
    }

    #[test]
    fn panicking_holder_releases_the_slot() {
        let queue = JobQueue::new(1);
        let mut job = queue.submit(JobKind::Agent, "stürzt", true);
        let id = job.id().to_owned();
        let result = thread::spawn(move || {
            let _guard = job.acquire().unwrap();
            panic!("absichtlich");
        })
        .join();
        assert!(result.is_err());
        assert_eq!(status_of(&queue, &id), JobStatus::Failed);
        let mut next = queue.submit(JobKind::Agent, "danach", true);
        let _guard = next.acquire().expect("Platz wieder frei");
    }

    #[test]
    fn running_job_sees_cancel_flag_and_reports_cancelled() {
        let queue = JobQueue::new(1);
        let mut job = queue.submit(JobKind::AgentFlow, "läuft", true);
        let flag = job.cancel_flag();
        let mut guard = job.acquire().unwrap();
        assert!(queue.cancel(job.id()));
        assert!(flag.load(Ordering::SeqCst));
        guard.cancelled();
        drop(guard);
        assert_eq!(status_of(&queue, job.id()), JobStatus::Cancelled);
    }

    #[test]
    fn non_cancelable_job_refuses_cancel() {
        let queue = JobQueue::new(1);
        let job = queue.submit(JobKind::ModelSwitch, "Modellwechsel", false);
        assert!(!queue.cancel(job.id()));
    }

    #[test]
    fn cancel_kinds_hits_only_selected_kinds() {
        let queue = JobQueue::new(1);
        let mut chat = queue.submit(JobKind::Chat, "Chat", true);
        let _chat_guard = chat.acquire().unwrap();
        let workflow = queue.submit(JobKind::Workflow, "Workflow", true);
        let flow = queue.submit(JobKind::AgentFlow, "Flow", true);
        assert_eq!(
            queue.cancel_kinds(&[JobKind::Workflow, JobKind::AgentFlow]),
            2
        );
        assert_eq!(status_of(&queue, workflow.id()), JobStatus::Cancelled);
        assert_eq!(status_of(&queue, flow.id()), JobStatus::Cancelled);
        assert_eq!(status_of(&queue, chat.id()), JobStatus::Running);
    }

    #[test]
    fn listener_receives_updates_and_history_is_bounded() {
        let queue = JobQueue::new(1);
        let updates = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&updates);
        queue.set_listener(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        for index in 0..(FINISHED_HISTORY + 5) {
            let mut job = queue.submit(JobKind::Chat, format!("j{index}"), true);
            drop(job.acquire().unwrap());
        }
        assert!(updates.load(Ordering::SeqCst) > 0);
        assert!(queue.snapshot().len() <= FINISHED_HISTORY);
    }
}
