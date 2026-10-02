use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

use crate::{
    cache::{PublishError, SnapshotStore},
    collector::orchestrator::Orchestrator,
    observability::Metrics,
};

#[tracing::instrument(
    name = "patchpulse.collection_cycle",
    skip(store, orchestrator, metrics),fields(otel.status_code)
)]
pub async fn tick_once(
    store: &SnapshotStore,
    orchestrator: &Orchestrator,
    metrics: &Metrics,
) -> Result<(), PublishError> {
    let outcomes = orchestrator.run(metrics).await;
    if outcomes.iter().any(|outcome| outcome.result.is_err()) {
        tracing::Span::current().record("otel.status_code", "ERROR");
    }
    store.publish(outcomes, jiff::Timestamp::now()).await?;
    let snapshot = store.read().await;
    tracing::info!(
        installed = snapshot.installed.len(),
        pending = snapshot.pending.len(),
        degraded = snapshot.last_error.is_some(),
        "snapshot published"
    );
    Ok(())
}

pub async fn run(
    store: SnapshotStore,
    orchestrator: Arc<Orchestrator>,
    metrics: Metrics,
    interval_secs: u64,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if *shutdown.borrow() {
            break;
        }
        tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            _ = ticker.tick() => {
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => break,
                    result = tick_once(&store, &orchestrator, &metrics) => {
                        if let Err(error) = result { tracing::warn!(error = %error, "snapshot publication failed; retaining previous state"); }
                    }
                }
            }
        }
    }
}
