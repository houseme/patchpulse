use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use super::traits::{BackendOutcome, CollectError, Collector};
use crate::observability::Metrics;

struct Slot {
    collector: Arc<dyn Collector>,
    busy: Arc<AtomicBool>,
}

struct FlightGuard(Arc<AtomicBool>);
impl Drop for FlightGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub struct Orchestrator {
    slots: Vec<Arc<Slot>>,
    timeout: Duration,
}

impl Orchestrator {
    pub fn cancel(&self) {
        for slot in &self.slots {
            slot.collector.cancel();
        }
    }
    pub fn new(collectors: Vec<Arc<dyn Collector>>, timeout: Duration) -> Self {
        Self {
            slots: collectors
                .into_iter()
                .map(|collector| {
                    Arc::new(Slot {
                        collector,
                        busy: Arc::new(AtomicBool::new(false)),
                    })
                })
                .collect(),
            timeout,
        }
    }

    pub fn enabled_names(&self) -> Vec<&'static str> {
        self.slots.iter().map(|s| s.collector.name()).collect()
    }

    pub async fn run(&self, metrics: &Metrics) -> Vec<BackendOutcome> {
        let mut tasks = tokio::task::JoinSet::new();
        let mut results = Vec::new();
        for slot in &self.slots {
            let slot = Arc::clone(slot);
            let name = slot.collector.name();
            if slot
                .busy
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                metrics.record_collection(name, 0.0, false);
                tracing::warn!(collector = name, "previous collector still running");
                results.push(BackendOutcome {
                    name,
                    finished_at: jiff::Timestamp::now(),
                    result: Err(CollectError::Busy),
                    in_flight: true,
                });
                continue;
            }
            let guard = FlightGuard(Arc::clone(&slot.busy));
            let timeout = self.timeout;
            let metrics = metrics.clone();
            tasks.spawn(async move {
                let started = Instant::now();
                let backend = Arc::clone(&slot.collector);
                let worker = tokio::task::spawn_blocking(move || {
                    // The guard remains in the blocking task even after its caller times out.
                    let _guard = guard;
                    backend.collect()
                });
                let result = match tokio::time::timeout(timeout, worker).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(error)) => Err(CollectError::Backend(format!("{name} worker: {error}"))),
                    Err(_) => Err(CollectError::Timeout(timeout.as_secs())),
                };
                metrics.record_collection(name, started.elapsed().as_secs_f64(), result.is_ok());
                match &result {
                    Ok(batch) => tracing::info!(
                        collector = name,
                        records = batch.records.len(),
                        "collector succeeded"
                    ),
                    Err(error) => {
                        tracing::warn!(collector = name, error = %error, "collector failed")
                    }
                }
                BackendOutcome {
                    name,
                    finished_at: jiff::Timestamp::now(),
                    result,
                    in_flight: slot.busy.load(Ordering::Acquire),
                }
            });
        }
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok(result) => results.push(result),
                Err(error) => {
                    tracing::error!(error = %error, "collector orchestration task failed")
                }
            }
        }
        results.sort_by_key(|r| r.name);
        results
    }
}
