use crate::domain::patch::PatchRecord;

#[derive(Debug, thiserror::Error)]
pub enum CollectError {
    #[error("{0} requires Windows")]
    Unsupported(&'static str),
    #[error("I/O failure: {0}")]
    Io(#[from] std::io::Error),
    #[error("backend failure: {0}")]
    Backend(String),
    #[error("collector timed out after {0}s")]
    Timeout(u64),
    #[error("previous blocking collector is still running")]
    Busy,
    #[error("collection cancelled during shutdown")]
    Cancelled,
    #[error("invalid collector output: {0}")]
    Parse(String),
}

#[derive(Debug, Clone, Default)]
pub struct CollectBatch {
    pub records: Vec<PatchRecord>,
    pub reboot_required: Option<bool>,
}

/// Synchronous backends are always invoked by the orchestrator on blocking workers.
pub trait Collector: Send + Sync + 'static {
    fn name(&self) -> &'static str;
    fn collect(&self) -> Result<CollectBatch, CollectError>;
    /// Request cooperative shutdown where the backend supports cancellation.
    fn cancel(&self) {}
}

#[derive(Debug)]
pub struct BackendOutcome {
    pub name: &'static str,
    pub finished_at: jiff::Timestamp,
    pub result: Result<CollectBatch, CollectError>,
    pub in_flight: bool,
}
