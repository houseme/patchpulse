use std::{collections::BTreeMap, sync::Arc};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::patch::PatchRecord;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackendStatus {
    pub enabled: bool,
    pub last_success: Option<Timestamp>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
    pub in_flight: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PatchSnapshot {
    pub installed: Arc<Vec<PatchRecord>>,
    pub pending: Arc<Vec<PatchRecord>>,
    pub last_refreshed: Option<Timestamp>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
    pub backends: BTreeMap<String, BackendStatus>,
    pub reboot_required: bool,
}

impl PatchSnapshot {
    pub fn is_ready(&self) -> bool {
        self.last_refreshed.is_some()
    }

    pub fn is_stale(&self, now: Timestamp, stale_after_secs: u64) -> bool {
        !self.is_ready()
            || self.backends.values().filter(|b| b.enabled).any(|b| {
                b.last_success.is_none_or(|t| {
                    now < t
                        || now.as_second().saturating_sub(t.as_second())
                            > i64::try_from(stale_after_secs).unwrap_or(i64::MAX)
                }) || b.last_error.is_some()
            })
    }

    pub fn age_seconds(&self, now: Timestamp) -> Option<i64> {
        let oldest = self
            .backends
            .values()
            .filter(|b| b.enabled)
            .filter_map(|b| b.last_success)
            .min()?;
        Some(now.as_second().saturating_sub(oldest.as_second()).max(0))
    }

    pub fn latest_installed(&self) -> Option<&PatchRecord> {
        self.latest_installed_index()
            .and_then(|index| self.installed.get(index))
    }

    pub fn latest_installed_index(&self) -> Option<usize> {
        self.installed
            .iter()
            .enumerate()
            .filter(|(_, p)| p.installed_on.is_some() || p.installed_date.is_some())
            .max_by_key(|(_, p)| (p.installation_key(), &p.kb_id))
            .map(|(index, _)| index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clock_rollback_cannot_report_a_future_observation_as_fresh() {
        let future: Timestamp = "2026-09-02T00:00:00Z".parse().unwrap();
        let mut snapshot = PatchSnapshot {
            last_refreshed: Some(future),
            ..PatchSnapshot::default()
        };
        snapshot.backends.insert(
            "wua_installed".into(),
            BackendStatus {
                enabled: true,
                last_success: Some(future),
                ..BackendStatus::default()
            },
        );
        assert!(snapshot.is_stale("2026-09-01T00:00:00Z".parse().unwrap(), 7200));
    }
}
