mod support;

use patchpulse::{
    collector::{
        orchestrator::Orchestrator,
        traits::{CollectBatch, CollectError, Collector},
    },
    domain::patch::PatchStatus,
    observability::Metrics,
    scheduler,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use support::*;

#[tokio::test]
async fn partial_failure_retains_failed_source_and_updates_healthy_source() {
    let installed = mock(
        "wmi_installed",
        vec![
            Some(vec![record("KB1", PatchStatus::Installed)]),
            None,
            Some(vec![]),
        ],
    );
    let pending = mock(
        "wua_pending",
        vec![
            Some(vec![record("KB2", PatchStatus::Pending)]),
            Some(vec![record("KB3", PatchStatus::Pending)]),
            Some(vec![]),
        ],
    );
    let (store, orchestrator, metrics) = setup(vec![installed, pending]);
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let before = store.read().await;
    assert!(before.is_ready());
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let after = store.read().await;
    assert_eq!(after.installed[0].kb_id, "KB1");
    assert_eq!(after.pending[0].kb_id, "KB3");
    assert_eq!(
        after.backends["wmi_installed"].last_success,
        before.backends["wmi_installed"].last_success
    );
    assert!(after.last_error.is_some());
    assert!(after.is_stale(jiff::Timestamp::now(), 7200));
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let recovered = store.read().await;
    assert!(recovered.installed.is_empty() && recovered.pending.is_empty());
    assert!(recovered.is_ready());
    assert!(recovered.last_error.is_none());
    assert_eq!(recovered.consecutive_failures, 0);
}

#[tokio::test]
async fn all_failure_preserves_readiness_and_previous_snapshot() {
    let backend = mock(
        "wua_installed",
        vec![
            None,
            Some(vec![record("KB1", PatchStatus::Installed)]),
            None,
        ],
    );
    let (store, orchestrator, metrics) = setup(vec![backend]);
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    assert!(!store.read().await.is_ready());
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let good = store.read().await;
    let good_view = store.view();
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let failed = store.read().await;
    assert!(failed.is_ready());
    assert_eq!(failed.installed, good.installed);
    assert_eq!(failed.last_refreshed, good.last_refreshed);
    assert_eq!(failed.consecutive_failures, 1);
    let failed_view = store.view();
    assert!(Arc::ptr_eq(&failed.installed, &good.installed));
    assert!(Arc::ptr_eq(&failed.pending, &good.pending));
    assert_eq!(
        good_view.installed_json.as_ptr(),
        failed_view.installed_json.as_ptr()
    );
    assert_eq!(
        good_view.pending_json.as_ptr(),
        failed_view.pending_json.as_ptr()
    );
}

#[tokio::test]
async fn successful_empty_collection_establishes_readiness() {
    let (store, orchestrator, metrics) = setup(vec![mock("wua_pending", vec![Some(vec![])])]);
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    assert!(store.read().await.is_ready());
    assert!(!store.read().await.is_stale(jiff::Timestamp::now(), 7200));
}

#[tokio::test]
async fn installed_wins_over_pending_duplicates() {
    let (store, orchestrator, metrics) = setup(vec![
        mock(
            "wua_installed",
            vec![Some(vec![record("KB1", PatchStatus::Installed)])],
        ),
        mock(
            "wua_pending",
            vec![Some(vec![
                record("KB1", PatchStatus::Pending),
                record("KB2", PatchStatus::Pending),
            ])],
        ),
    ]);
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let snapshot = store.read().await;
    assert_eq!(snapshot.pending.len(), 1);
    assert_eq!(snapshot.pending[0].kb_id, "KB2");
}

struct BlockingCollector {
    calls: AtomicUsize,
    release: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}
impl Collector for BlockingCollector {
    fn name(&self) -> &'static str {
        "blocking"
    }
    fn collect(&self) -> Result<CollectBatch, CollectError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let (lock, notify) = &*self.release;
        let mut released = lock.lock().unwrap();
        while !*released {
            released = notify.wait(released).unwrap();
        }
        Ok(CollectBatch::default())
    }
}

#[tokio::test]
async fn timeout_keeps_single_flight_until_blocking_worker_finishes() {
    let release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let backend = Arc::new(BlockingCollector {
        calls: AtomicUsize::new(0),
        release: release.clone(),
    });
    let orchestrator = Orchestrator::new(vec![backend.clone()], Duration::from_millis(50));
    let metrics = Metrics::default();
    let first = orchestrator.run(&metrics).await;
    let second = orchestrator.run(&metrics).await;
    // Release before assertions so a failing assertion cannot hang runtime shutdown.
    *release.0.lock().unwrap() = true;
    release.1.notify_all();
    assert!(matches!(first[0].result, Err(CollectError::Timeout(_))));
    assert!(matches!(second[0].result, Err(CollectError::Busy)));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn scheduler_collects_once_at_startup_and_stops() {
    let backend = mock("wua_pending", vec![Some(vec![])]);
    let (store, orchestrator, metrics) = setup(vec![backend.clone()]);
    let (sender, receiver) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(scheduler::run(
        store.clone(),
        Arc::new(orchestrator),
        metrics,
        60,
        receiver,
    ));
    tokio::time::timeout(Duration::from_secs(1), async {
        while !store.read().await.is_ready() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    sender.send(true).unwrap();
    task.await.unwrap();
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
}

struct CancellableCollector {
    started: std::sync::atomic::AtomicBool,
    cancelled: std::sync::atomic::AtomicBool,
}

impl Collector for CancellableCollector {
    fn name(&self) -> &'static str {
        "cancellable"
    }
    fn collect(&self) -> Result<CollectBatch, CollectError> {
        self.started.store(true, Ordering::Release);
        while !self.cancelled.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(1));
        }
        Err(CollectError::Cancelled)
    }
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

#[tokio::test]
async fn shutdown_requests_reach_active_blocking_collectors() {
    let backend = Arc::new(CancellableCollector {
        started: false.into(),
        cancelled: false.into(),
    });
    let orchestrator = Arc::new(Orchestrator::new(
        vec![backend.clone()],
        Duration::from_secs(1),
    ));
    let worker = Arc::clone(&orchestrator);
    let task = tokio::spawn(async move { worker.run(&Metrics::default()).await });
    let startup = tokio::time::timeout(Duration::from_secs(1), async {
        while !backend.started.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await;
    orchestrator.cancel();
    startup.unwrap();
    let outcomes = task.await.unwrap();
    assert!(matches!(outcomes[0].result, Err(CollectError::Cancelled)));
    assert!(!outcomes[0].in_flight);
}

#[tokio::test]
async fn publication_replaces_inventory_and_encoded_body_together() {
    let backend = mock(
        "wua_installed",
        vec![
            Some(vec![record("KB1", PatchStatus::Installed)]),
            Some(vec![record("KB2", PatchStatus::Installed)]),
        ],
    );
    let (store, orchestrator, metrics) = setup(vec![backend]);
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let old = store.view();
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let current = store.view();
    let old_json: serde_json::Value = serde_json::from_slice(&old.installed_json).unwrap();
    let new_json: serde_json::Value = serde_json::from_slice(&current.installed_json).unwrap();
    assert_eq!(old_json["items"][0]["kb_id"], "KB1");
    assert_eq!(
        new_json["items"][0]["kb_id"],
        current.snapshot.installed[0].kb_id
    );
    assert_eq!(current.snapshot.installed[0].kb_id, "KB2");
    assert_eq!(current.latest_installed().unwrap().kb_id, "KB2");
}
