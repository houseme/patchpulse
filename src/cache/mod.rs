//! Transactional background assembly with constant-time snapshot publication.
use crate::{
    collector::{
        BACKEND_NAMES,
        traits::{BackendOutcome, CollectBatch},
    },
    domain::{
        patch::{PatchRecord, PatchStatus, dedupe},
        snapshot::{BackendStatus, PatchSnapshot},
    },
};
use bytes::Bytes;
use jiff::Timestamp;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::Arc,
};
use tokio::sync::{Mutex, watch};

type JsonSpans = Arc<Vec<Range<usize>>>;

pub const EMPTY_LIST_JSON: &[u8] = b"{\"count\":0,\"items\":[]}";

#[derive(Clone)]
pub struct PublishedSnapshot {
    pub snapshot: Arc<PatchSnapshot>,
    pub installed_json: Bytes,
    pub pending_json: Bytes,
    installed_csv: Bytes,
    pending_csv: Bytes,
    installed_csv_spans: JsonSpans,
    pending_csv_spans: JsonSpans,
    pub coverage: BTreeMap<String, bool>,
    latest_index: Option<usize>,
    installed_spans: JsonSpans,
    pending_spans: JsonSpans,
}
impl PublishedSnapshot {
    pub fn list_csv(&self, pending: bool, since: Option<Timestamp>) -> Bytes {
        let (records, csv, spans) = if pending {
            (
                &self.snapshot.pending,
                &self.pending_csv,
                &self.pending_csv_spans,
            )
        } else {
            (
                &self.snapshot.installed,
                &self.installed_csv,
                &self.installed_csv_spans,
            )
        };
        let Some(since) = since else {
            return csv.clone();
        };
        let selected: Vec<_> = records
            .iter()
            .zip(spans.iter())
            .filter(|(record, _)| record.installed_on.is_some_and(|instant| instant >= since))
            .map(|(_, span)| span)
            .collect();
        if selected.len() == records.len() {
            return csv.clone();
        }
        if selected.is_empty() {
            return Bytes::from_static(crate::export::HEADER);
        }
        let mut bytes = Vec::with_capacity(
            crate::export::HEADER.len() + selected.iter().map(|span| span.len()).sum::<usize>(),
        );
        bytes.extend_from_slice(crate::export::HEADER);
        for span in selected {
            bytes.extend_from_slice(&csv[span.clone()]);
        }
        Bytes::from(bytes)
    }
    pub fn latest_installed(&self) -> Option<&PatchRecord> {
        self.latest_index
            .and_then(|index| self.snapshot.installed.get(index))
    }
    /// Filters copy pre-encoded records instead of cloning or serializing patch metadata.
    pub fn list_json(&self, pending: bool, since: Option<Timestamp>) -> Bytes {
        let (records, json, spans) = if pending {
            (
                &self.snapshot.pending,
                &self.pending_json,
                &self.pending_spans,
            )
        } else {
            (
                &self.snapshot.installed,
                &self.installed_json,
                &self.installed_spans,
            )
        };
        let Some(since) = since else {
            return json.clone();
        };
        let selected: Vec<_> = records
            .iter()
            .zip(spans.iter())
            .filter(|(record, _)| record.installed_on.is_some_and(|instant| instant >= since))
            .map(|(_, span)| span)
            .collect();
        if selected.len() == records.len() {
            return json.clone();
        }
        if selected.is_empty() {
            return Bytes::from_static(EMPTY_LIST_JSON);
        }
        let header = format!("{{\"count\":{},\"items\":[", selected.len());
        let size: usize = selected.iter().map(|span| span.len()).sum();
        let mut bytes = Vec::with_capacity(header.len() + size + selected.len() + 2);
        bytes.extend_from_slice(header.as_bytes());
        for (index, span) in selected.into_iter().enumerate() {
            if index > 0 {
                bytes.push(b',');
            }
            bytes.extend_from_slice(&json[span.clone()]);
        }
        bytes.extend_from_slice(b"]}");
        Bytes::from(bytes)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("snapshot encoding failed: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("snapshot CSV encoding failed: {0}")]
    Csv(#[from] csv::Error),
    #[error("snapshot worker failed: {0}")]
    Worker(#[from] tokio::task::JoinError),
}

type BackendData = BTreeMap<String, Arc<CollectBatch>>;
#[derive(Clone)]
pub struct SnapshotStore {
    published: watch::Sender<Arc<PublishedSnapshot>>,
    writer: Arc<Mutex<BackendData>>,
    pub stale_after_secs: u64,
}
impl SnapshotStore {
    pub fn new(stale_after_secs: u64, enabled_names: &[&str]) -> Self {
        let mut snapshot = PatchSnapshot::default();
        for name in BACKEND_NAMES
            .into_iter()
            .chain(enabled_names.iter().copied())
        {
            snapshot.backends.insert(
                name.into(),
                BackendStatus {
                    enabled: enabled_names.contains(&name),
                    ..BackendStatus::default()
                },
            );
        }
        let coverage = snapshot
            .backends
            .iter()
            .map(|(name, status)| (name.clone(), status.enabled))
            .collect();
        let (published, _) = watch::channel(Arc::new(PublishedSnapshot {
            snapshot: Arc::new(snapshot),
            installed_json: Bytes::from_static(EMPTY_LIST_JSON),
            pending_json: Bytes::from_static(EMPTY_LIST_JSON),
            installed_csv: Bytes::from_static(crate::export::HEADER),
            pending_csv: Bytes::from_static(crate::export::HEADER),
            installed_csv_spans: Arc::default(),
            pending_csv_spans: Arc::default(),
            coverage,
            latest_index: None,
            installed_spans: Arc::default(),
            pending_spans: Arc::default(),
        }));
        Self {
            published,
            writer: Arc::new(Mutex::new(BTreeMap::new())),
            stale_after_secs,
        }
    }
    /// Acquire one immutable view; assembly never holds this publication lock.
    pub fn view(&self) -> Arc<PublishedSnapshot> {
        Arc::clone(&self.published.borrow())
    }
    pub async fn read(&self) -> Arc<PatchSnapshot> {
        Arc::clone(&self.view().snapshot)
    }

    /// Only scheduler::tick_once publishes. Cancellation leaves both data and publication unchanged.
    pub(crate) async fn publish(
        &self,
        outcomes: Vec<BackendOutcome>,
        now: Timestamp,
    ) -> Result<(), PublishError> {
        let mut writer = self.writer.lock().await;
        let staged = writer.clone();
        let previous = self.view();
        let (data, next) =
            tokio::task::spawn_blocking(move || assemble(staged, previous, outcomes, now))
                .await??;
        *writer = data;
        self.published.send_replace(Arc::new(next));
        Ok(())
    }
}

fn assemble(
    mut data: BackendData,
    previous: Arc<PublishedSnapshot>,
    outcomes: Vec<BackendOutcome>,
    now: Timestamp,
) -> Result<(BackendData, PublishedSnapshot), PublishError> {
    let mut next = (*previous).clone();
    let mut snapshot = (*previous.snapshot).clone();
    let mut successes = 0;
    let mut errors = Vec::new();
    for outcome in outcomes {
        let backend = snapshot.backends.entry(outcome.name.into()).or_default();
        backend.enabled = true;
        backend.in_flight = outcome.in_flight;
        match outcome.result {
            Ok(batch) => {
                successes += 1;
                backend.last_success = Some(outcome.finished_at);
                backend.last_error = None;
                backend.consecutive_failures = 0;
                data.insert(outcome.name.into(), Arc::new(batch));
            }
            Err(error) => {
                let error = error.to_string();
                errors.push(format!("{}: {error}", outcome.name));
                backend.last_error = Some(error);
                backend.consecutive_failures = backend.consecutive_failures.saturating_add(1);
            }
        }
    }
    if successes > 0 {
        snapshot.last_refreshed = Some(now);
    }
    snapshot.consecutive_failures = if errors.is_empty() {
        0
    } else {
        snapshot.consecutive_failures.saturating_add(1)
    };
    snapshot.last_error = if errors.is_empty() {
        None
    } else {
        Some(errors.join("; "))
    };
    if successes > 0 {
        snapshot.installed = Arc::new(dedupe(data.values().flat_map(|batch| {
            batch
                .records
                .iter()
                .filter(|record| record.status == PatchStatus::Installed)
        })));
        let installed: BTreeSet<_> = snapshot
            .installed
            .iter()
            .map(|record| record.kb_id.as_str())
            .collect();
        snapshot.pending = Arc::new(dedupe(data.values().flat_map(|batch| {
            batch.records.iter().filter(|record| {
                record.status == PatchStatus::Pending && !installed.contains(record.kb_id.as_str())
            })
        })));
        snapshot.reboot_required = data.values().any(|batch| {
            batch.reboot_required == Some(true)
                || batch.records.iter().any(|record| record.reboot_required)
        });
        (next.installed_json, next.installed_spans) = encode(&snapshot.installed)?;
        (next.pending_json, next.pending_spans) = encode(&snapshot.pending)?;
        (next.installed_csv, next.installed_csv_spans) =
            crate::export::encode(&snapshot.installed)?;
        (next.pending_csv, next.pending_csv_spans) = crate::export::encode(&snapshot.pending)?;
        next.latest_index = snapshot.latest_installed_index();
    }
    // Failure-only cycles keep every prepared buffer while updating diagnostics.
    next.coverage = snapshot
        .backends
        .iter()
        .map(|(name, status)| (name.clone(), status.enabled))
        .collect();
    next.snapshot = Arc::new(snapshot);
    Ok((data, next))
}

fn encode(records: &[PatchRecord]) -> Result<(Bytes, JsonSpans), serde_json::Error> {
    if records.is_empty() {
        return Ok((Bytes::from_static(EMPTY_LIST_JSON), Arc::default()));
    }
    let header = format!("{{\"count\":{},\"items\":[", records.len());
    let mut bytes = header.into_bytes();
    let mut spans = Vec::with_capacity(records.len());
    for (index, record) in records.iter().enumerate() {
        if index > 0 {
            bytes.push(b',');
        }
        let start = bytes.len();
        serde_json::to_writer(&mut bytes, record)?;
        spans.push(start..bytes.len());
    }
    bytes.extend_from_slice(b"]}");
    Ok((Bytes::from(bytes), Arc::new(spans)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn readers_do_not_wait_for_the_writer() {
        let store = SnapshotStore::new(30, &["wua_installed"]);
        let _writer = store.writer.lock().await;
        let snapshot = tokio::time::timeout(std::time::Duration::from_millis(50), store.read())
            .await
            .unwrap();
        assert!(!snapshot.is_ready());
    }
}
