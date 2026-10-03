use std::time::{Duration, Instant};

use pa_vault::sync_worker::{SyncDecision, SyncWorkerState};

#[test]
fn starts_exactly_after_five_minutes_and_coalesces_parallel_requests() {
    let start = Instant::now();
    let mut worker = SyncWorkerState::new(start, Duration::from_secs(300));

    assert_eq!(
        worker.on_timer(start + Duration::from_secs(299)),
        SyncDecision::Idle
    );
    assert_eq!(
        worker.on_timer(start + Duration::from_secs(300)),
        SyncDecision::Start
    );
    assert_eq!(worker.request_sync(), SyncDecision::Coalesced);
    assert_eq!(worker.request_sync(), SyncDecision::Coalesced);
    assert_eq!(
        worker.completed(start + Duration::from_secs(301), true),
        SyncDecision::Start
    );
    assert_eq!(
        worker.completed(start + Duration::from_secs(302), true),
        SyncDecision::Idle
    );
}

#[test]
fn exit_requests_an_immediate_sync_even_before_deadline() {
    let start = Instant::now();
    let mut worker = SyncWorkerState::new(start, Duration::from_secs(300));

    assert_eq!(worker.request_exit(), SyncDecision::StartAndExit);
    assert_eq!(worker.request_exit(), SyncDecision::Coalesced);
    assert_eq!(worker.completed(start, true), SyncDecision::Exit);
}

#[test]
fn failed_sync_stays_pending_instead_of_claiming_success() {
    let start = Instant::now();
    let mut worker = SyncWorkerState::new(start, Duration::from_secs(300));
    assert_eq!(worker.request_sync(), SyncDecision::Start);

    assert_eq!(worker.completed(start, false), SyncDecision::Idle);
    assert!(worker.has_pending_work());
}
