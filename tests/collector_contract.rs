use patchpulse::{
    collector::{
        orchestrator::Orchestrator,
        traits::{CollectBatch, CollectError, Collector},
    },
    observability::Metrics,
};
use std::{sync::Arc, time::Duration};

struct ConcurrentCollector {
    name: &'static str,
    barrier: Arc<(std::sync::Mutex<usize>, std::sync::Condvar)>,
}
impl Collector for ConcurrentCollector {
    fn name(&self) -> &'static str {
        self.name
    }
    fn collect(&self) -> Result<CollectBatch, CollectError> {
        let (lock, condition) = &*self.barrier;
        let mut count = lock.lock().unwrap();
        *count += 1;
        condition.notify_all();
        let (count, waited) = condition
            .wait_timeout_while(count, Duration::from_secs(1), |count| *count < 2)
            .unwrap();
        if waited.timed_out() && *count < 2 {
            return Err(CollectError::Timeout(1));
        }
        Ok(CollectBatch::default())
    }
}

#[tokio::test]
async fn independent_backends_actually_run_concurrently() {
    let barrier = Arc::new((std::sync::Mutex::new(0), std::sync::Condvar::new()));
    let orchestrator = Orchestrator::new(
        vec![
            Arc::new(ConcurrentCollector {
                name: "first",
                barrier: barrier.clone(),
            }),
            Arc::new(ConcurrentCollector {
                name: "second",
                barrier,
            }),
        ],
        Duration::from_secs(2),
    );
    let outcomes = orchestrator.run(&Metrics::default()).await;
    assert_eq!(outcomes.len(), 2);
    assert!(outcomes.iter().all(|outcome| outcome.result.is_ok()));
}

#[cfg(not(windows))]
#[tokio::test]
async fn every_enabled_windows_backend_reports_unsupported() {
    let config = patchpulse::config::CollectorConfig {
        enable_powershell_installed: true,
        enable_powershell_pending: true,
        ..Default::default()
    };
    let outcomes = patchpulse::collector::build(&config)
        .run(&Metrics::default())
        .await;
    assert_eq!(outcomes.len(), 5);
    assert!(
        outcomes
            .iter()
            .all(|outcome| matches!(outcome.result, Err(CollectError::Unsupported(_))))
    );
}
