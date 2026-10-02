//! Versioned coherent snapshots exchanged between configured agents and hubs.
use super::snapshot::PatchSnapshot;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentSnapshot {
    pub schema_version: u32,
    pub observed_at: jiff::Timestamp,
    pub stale_after_secs: u64,
    pub is_stale: bool,
    pub snapshot: Arc<PatchSnapshot>,
}
