//! Release-mode handler benchmark using the real router and a deterministic inventory.
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use patchpulse::{
    api::{self, ApiState},
    cache::SnapshotStore,
    collector::{
        orchestrator::Orchestrator,
        traits::{CollectBatch, CollectError, Collector},
    },
    domain::patch::{PatchRecord, PatchSource, PatchStatus},
    observability::Metrics,
    scheduler,
};
use std::{
    hint::black_box,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tower::ServiceExt;

struct Inventory {
    records: Vec<PatchRecord>,
    fail: AtomicBool,
}
impl Collector for Inventory {
    fn name(&self) -> &'static str {
        "wua_installed"
    }
    fn collect(&self) -> Result<CollectBatch, CollectError> {
        if self.fail.load(Ordering::Relaxed) {
            return Err(CollectError::Backend("benchmark failure".into()));
        }
        Ok(CollectBatch {
            records: self.records.clone(),
            reboot_required: Some(false),
        })
    }
}

async fn request(router: &axum::Router, path: &str) -> usize {
    let response = router
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert!(response.status().is_success());
    let bytes = to_bytes(response.into_body(), 32 * 1024 * 1024)
        .await
        .unwrap();
    black_box(bytes.len())
}

fn main() {
    let count: usize = std::env::args()
        .nth(1)
        .map(|s| s.parse().unwrap())
        .unwrap_or(800);
    let repetitions: usize = std::env::args()
        .nth(2)
        .map(|s| s.parse().unwrap())
        .unwrap_or(1000);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let records = (0..count).map(|i| {
            let mut record = PatchRecord::new(format!("KB{:07}", 1_000_000 + i), PatchStatus::Installed, PatchSource::Wua);
            record.title = Some(format!("Cumulative security update {i}: {}", "Windows Server ".repeat(4)));
            record.description = Some("Deterministic fixture metadata. ".repeat(8));
            record.category = Some("Security Updates".into());
            record.severity = Some("Critical".into());
            record.installed_on = Some(if i % 2 == 0 { "2026-09-01T00:00:00Z" } else { "2026-09-02T00:00:00Z" }.parse().unwrap());
            record
        }).collect();
        let inventory = Arc::new(Inventory { records, fail: false.into() });
        let orchestrator = Orchestrator::new(vec![inventory.clone()], Duration::from_secs(5));
        let store = SnapshotStore::new(7200, &orchestrator.enabled_names());
        let metrics = Metrics::default();
        let _ = scheduler::tick_once(&store, &orchestrator, &metrics).await;
        let router = api::build(ApiState { store: store.clone(), metrics: metrics.clone() }, true, 15);
        let mut results = Vec::new();
        for path in ["/patches", "/patches?status=installed", "/patches?since=2026-09-02T00:00:00Z", "/patches/summary", "/ready", "/metrics"] {
            for _ in 0..25 { request(&router, path).await; }
            let mut samples = Vec::with_capacity(repetitions);
            let mut bytes = 0;
            for _ in 0..repetitions {
                let started = Instant::now();
                bytes = request(&router, path).await;
                samples.push(started.elapsed().as_nanos() as u64);
            }
            samples.sort_unstable();
            results.push(serde_json::json!({"case":path,"p50_ns":samples[repetitions/2],"p95_ns":samples[repetitions*95/100],"body_bytes":bytes}));
        }
        for fail in [false, true] {
            inventory.fail.store(fail, Ordering::Relaxed);
            let mut samples = Vec::new();
            for _ in 0..50 {
                let started = Instant::now();
                let _ = scheduler::tick_once(&store, &orchestrator, &metrics).await;
                samples.push(started.elapsed().as_nanos() as u64);
            }
            samples.sort_unstable();
            results.push(serde_json::json!({"case":if fail {"publish_failure"} else {"publish_success"},"p50_ns":samples[25],"p95_ns":samples[47]}));
        }
        println!("{}", serde_json::json!({"records":count,"repetitions":repetitions,"results":results}));
    });
    runtime.shutdown_timeout(Duration::from_secs(5));
}
