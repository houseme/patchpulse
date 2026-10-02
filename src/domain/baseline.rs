//! Exact observed KB membership; no update supersedence or vulnerability inference.
use super::snapshot::PatchSnapshot;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct Baseline {
    pub name: String,
    pub required_kbs: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Compliance {
    Compliant,
    NonCompliant,
    Unknown,
}

#[derive(Debug, Serialize)]
pub struct BaselineReport<'a> {
    pub baseline: &'a str,
    pub compliance: Compliance,
    pub required_count: usize,
    pub installed_count: usize,
    pub missing_count: usize,
    pub pending_count: usize,
    pub installed: Vec<&'a str>,
    pub missing: Vec<&'a str>,
    pub pending: Vec<&'a str>,
    pub is_stale: bool,
    pub last_refreshed: Option<jiff::Timestamp>,
    pub last_error: Option<&'a str>,
}

impl Baseline {
    /// Transport failures can mark remote input stale even when its retained source is fresh.
    pub fn compare<'a>(
        &'a self,
        snapshot: &'a PatchSnapshot,
        input_stale: bool,
    ) -> BaselineReport<'a> {
        // Published inventories are sorted by KB; lookups avoid rebuilding an inventory index.
        let contains = |records: &[super::patch::PatchRecord], kb: &str| {
            records
                .binary_search_by(|record| record.kb_id.as_str().cmp(kb))
                .is_ok()
        };
        let mut installed = Vec::new();
        let mut missing = Vec::new();
        let mut pending = Vec::new();
        for kb in &self.required_kbs {
            if contains(&snapshot.installed, kb) {
                installed.push(kb.as_str());
            } else {
                missing.push(kb.as_str());
                if contains(&snapshot.pending, kb) {
                    pending.push(kb.as_str());
                }
            }
        }
        let is_stale = input_stale || !snapshot.is_ready();
        let compliance = if is_stale {
            Compliance::Unknown
        } else if missing.is_empty() {
            Compliance::Compliant
        } else {
            Compliance::NonCompliant
        };
        BaselineReport {
            baseline: &self.name,
            compliance,
            required_count: self.required_kbs.len(),
            installed_count: installed.len(),
            missing_count: missing.len(),
            pending_count: pending.len(),
            installed,
            missing,
            pending,
            is_stale,
            last_refreshed: snapshot.last_refreshed,
            last_error: snapshot.last_error.as_deref(),
        }
    }
}
