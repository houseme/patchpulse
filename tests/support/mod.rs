use patchpulse::{
    cache::SnapshotStore,
    collector::{
        orchestrator::Orchestrator,
        traits::{CollectBatch, CollectError, Collector},
    },
    domain::patch::{PatchRecord, PatchSource, PatchStatus},
    observability::Metrics,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub struct MockCollector {
    pub name: &'static str,
    pub calls: AtomicUsize,
    pub results: Vec<Option<Vec<PatchRecord>>>,
}

impl Collector for MockCollector {
    fn name(&self) -> &'static str {
        self.name
    }
    fn collect(&self) -> Result<CollectBatch, CollectError> {
        let index = self
            .calls
            .fetch_add(1, Ordering::SeqCst)
            .min(self.results.len() - 1);
        match &self.results[index] {
            Some(records) => Ok(CollectBatch {
                records: records.clone(),
                reboot_required: Some(false),
            }),
            None => Err(CollectError::Backend("mock failure".into())),
        }
    }
}

pub fn record(id: &str, status: PatchStatus) -> PatchRecord {
    let mut record = PatchRecord::new(id.into(), status, PatchSource::Wua);
    record.installed_on = if status == PatchStatus::Installed {
        Some("2026-09-01T00:00:00Z".parse().unwrap())
    } else {
        None
    };
    record
}

pub fn setup(collectors: Vec<Arc<dyn Collector>>) -> (SnapshotStore, Orchestrator, Metrics) {
    let orchestrator = Orchestrator::new(collectors, std::time::Duration::from_secs(1));
    let store = SnapshotStore::new(7200, &orchestrator.enabled_names());
    (store, orchestrator, Metrics::default())
}

pub fn mock(name: &'static str, results: Vec<Option<Vec<PatchRecord>>>) -> Arc<MockCollector> {
    Arc::new(MockCollector {
        name,
        calls: AtomicUsize::new(0),
        results,
    })
}
