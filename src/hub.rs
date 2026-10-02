//! Bounded configured-agent polling and immutable machine-scoped fleet publication.
use crate::{
    config::HubConfig,
    domain::{agent::AgentSnapshot, patch::PatchStatus},
    observability::{InventoryGauges, Metrics},
};
use anyhow::{Context, ensure};
use jiff::Timestamp;
use serde::Serialize;
use serde_json::value::RawValue;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Semaphore, watch};
use tracing::Instrument as _;

#[derive(Debug, Clone, Default)]
pub struct AgentState {
    pub snapshot: Option<Arc<AgentSnapshot>>,
    pub raw: Option<Arc<RawValue>>,
    pub last_fetch: Option<Timestamp>,
    pub last_error: Option<String>,
    pub consecutive_failures: u32,
    size: usize,
}
impl AgentState {
    pub fn is_stale(&self, now: Timestamp, stale_after_secs: u64) -> bool {
        self.last_error.is_some()
            || self.last_fetch.is_none_or(|fetched| {
                now < fetched
                    || now.as_second().saturating_sub(fetched.as_second()) > stale_after_secs as i64
            })
            || self.snapshot.as_ref().is_none_or(|wire| {
                wire.is_stale || wire.snapshot.is_stale(now, wire.stale_after_secs)
            })
    }
}

#[derive(Debug, Clone, Default)]
pub struct FleetSnapshot {
    pub agents: BTreeMap<String, AgentState>,
    pub ready: bool,
    pub last_refreshed: Option<Timestamp>,
}

#[derive(Debug, Serialize)]
pub struct FleetSummary {
    pub mode: &'static str,
    pub configured_agents: usize,
    pub available_agents: usize,
    pub fresh_agents: usize,
    pub stale_agents: usize,
    pub is_ready: bool,
    pub is_stale: bool,
    pub total_installed: usize,
    pub total_pending: usize,
    pub reboot_required: bool,
    pub last_refreshed: Option<Timestamp>,
}

impl FleetSnapshot {
    pub fn summary(&self, now: Timestamp, stale_after_secs: u64) -> FleetSummary {
        let fresh = self
            .agents
            .values()
            .filter(|agent| !agent.is_stale(now, stale_after_secs))
            .count();
        FleetSummary {
            mode: "hub",
            configured_agents: self.agents.len(),
            available_agents: self
                .agents
                .values()
                .filter(|agent| agent.snapshot.is_some())
                .count(),
            fresh_agents: fresh,
            stale_agents: self.agents.len() - fresh,
            is_ready: self.ready,
            is_stale: fresh != self.agents.len() || !self.ready,
            total_installed: self
                .agents
                .values()
                .filter_map(|agent| agent.snapshot.as_ref())
                .map(|wire| wire.snapshot.installed.len())
                .sum(),
            total_pending: self
                .agents
                .values()
                .filter_map(|agent| agent.snapshot.as_ref())
                .map(|wire| wire.snapshot.pending.len())
                .sum(),
            reboot_required: self
                .agents
                .values()
                .filter_map(|agent| agent.snapshot.as_ref())
                .any(|wire| wire.snapshot.reboot_required),
            last_refreshed: self.last_refreshed,
        }
    }
    pub fn gauges(&self, now: Timestamp, stale_after_secs: u64) -> InventoryGauges {
        let summary = self.summary(now, stale_after_secs);
        let age = self
            .agents
            .values()
            .filter_map(|agent| agent.snapshot.as_ref()?.snapshot.age_seconds(now))
            .max();
        InventoryGauges {
            age_seconds: age,
            installed: summary.total_installed,
            pending: summary.total_pending,
            is_stale: summary.is_stale,
            reboot_required: summary.reboot_required,
        }
    }
}

#[derive(Clone)]
pub struct FleetStore {
    published: watch::Sender<Arc<FleetSnapshot>>,
    pub stale_after_secs: u64,
}
impl FleetStore {
    pub fn view(&self) -> Arc<FleetSnapshot> {
        Arc::clone(&self.published.borrow())
    }
}

struct Agent {
    id: String,
    endpoint: reqwest::Url,
    authorization: Option<reqwest::header::HeaderValue>,
}
pub struct Hub {
    pub store: FleetStore,
    config: HubConfig,
    client: reqwest::Client,
    agents: Vec<Arc<Agent>>,
    publisher: Mutex<()>,
}
impl Hub {
    pub fn new(config: &HubConfig) -> anyhow::Result<Self> {
        config.validate()?;
        ensure!(!config.agents.is_empty(), "hub requires configured agents");
        let mut agents = Vec::with_capacity(config.agents.len());
        let mut states = BTreeMap::new();
        for agent in &config.agents {
            let authorization = if let Some(name) = &agent.bearer_token_env {
                let token = std::env::var(name)
                    .with_context(|| format!("missing bearer credential for agent {}", agent.id))?;
                ensure!(
                    !token.is_empty(),
                    "empty bearer credential for agent {}",
                    agent.id
                );
                let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|_| {
                        anyhow::anyhow!("invalid bearer credential for agent {}", agent.id)
                    })?;
                value.set_sensitive(true);
                Some(value)
            } else {
                None
            };
            agents.push(Arc::new(Agent {
                id: agent.id.clone(),
                endpoint: agent.endpoint()?,
                authorization,
            }));
            states.insert(agent.id.clone(), AgentState::default());
        }
        let (published, _) = watch::channel(Arc::new(FleetSnapshot {
            agents: states,
            ..Default::default()
        }));
        Ok(Self {
            store: FleetStore {
                published,
                stale_after_secs: config.stale_after_secs,
            },
            config: config.clone(),
            client: crate::net::client(Duration::from_secs(config.request_timeout_secs))?,
            agents,
            publisher: Mutex::new(()),
        })
    }
    #[tracing::instrument(name = "patchpulse.hub_cycle", skip(self, metrics),fields(otel.status_code))]
    pub async fn poll_once(&self, metrics: &Metrics) -> anyhow::Result<()> {
        let _publisher = self.publisher.lock().await;
        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrency));
        let mut tasks = tokio::task::JoinSet::new();
        for agent in &self.agents {
            let agent = Arc::clone(agent);
            let client = self.client.clone();
            let semaphore = Arc::clone(&semaphore);
            let limit = self.config.max_response_bytes;
            let span = tracing::info_span!("patchpulse.hub_agent",otel.kind="client",otel.status_code=tracing::field::Empty,agent.id=%agent.id);
            tasks.spawn(
                async move {
                    let permit = semaphore.acquire_owned().await;
                    let started = Instant::now();
                    let result = match permit {
                        Ok(_permit) => fetch(&client, &agent, limit).await,
                        Err(_) => Err(anyhow::anyhow!("hub poll cancelled")),
                    };
                    if result.is_err() {
                        tracing::Span::current().record("otel.status_code", "ERROR");
                    }
                    (
                        agent.id.clone(),
                        Timestamp::now(),
                        started.elapsed().as_secs_f64(),
                        result,
                    )
                }
                .instrument(span),
            );
        }
        let mut next = (*self.store.view()).clone();
        let mut received = 0;
        let mut budget: usize = next.agents.values().map(|agent| agent.size).sum();
        while let Some(result) = tasks.join_next().await {
            let (id, finished, elapsed, result) = result.context("hub poll task failed")?;
            let agent = next
                .agents
                .get_mut(&id)
                .context("polled agent missing from configured publication")?;
            let result = result.and_then(|snapshot| {
                ensure!(
                    budget - agent.size + snapshot.size <= self.config.max_total_snapshot_bytes,
                    "fleet snapshot byte budget exceeded"
                );
                Ok(snapshot)
            });
            metrics.record_collection("hub_poll", elapsed, result.is_ok());
            match result {
                Ok(snapshot) => {
                    received += 1;
                    budget = budget - agent.size + snapshot.size;
                    next.ready |= snapshot.wire.snapshot.is_ready();
                    agent.snapshot = Some(snapshot.wire);
                    agent.raw = Some(snapshot.raw);
                    agent.size = snapshot.size;
                    agent.last_fetch = Some(finished);
                    agent.last_error = None;
                    agent.consecutive_failures = 0;
                    tracing::info!(agent=%id,"agent snapshot received");
                }
                Err(error) => {
                    agent.last_error = Some(error.to_string());
                    agent.consecutive_failures = agent.consecutive_failures.saturating_add(1);
                    tracing::warn!(agent=%id,error=%error,"agent snapshot failed; retaining previous data");
                }
            }
        }
        if received > 0 {
            next.last_refreshed = Some(Timestamp::now());
        }
        if next.agents.values().any(|agent| agent.last_error.is_some()) {
            tracing::Span::current().record("otel.status_code", "ERROR");
        }
        self.store.published.send_replace(Arc::new(next));
        Ok(())
    }
}

struct Received {
    wire: Arc<AgentSnapshot>,
    raw: Arc<RawValue>,
    size: usize,
}
async fn fetch(client: &reqwest::Client, agent: &Agent, limit: usize) -> anyhow::Result<Received> {
    let mut headers = reqwest::header::HeaderMap::new();
    crate::observability::telemetry::inject(&mut headers);
    let mut request = client.get(agent.endpoint.clone()).headers(headers);
    if let Some(value) = &agent.authorization {
        request = request.header(reqwest::header::AUTHORIZATION, value.clone());
    }
    let mut response = request
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("agent HTTP request failed: {}", error.without_url()))?;
    ensure!(
        response.status() == reqwest::StatusCode::OK,
        "agent HTTP status {}",
        response.status().as_u16()
    );
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size <= limit as u64),
        "agent response exceeded byte limit"
    );
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| anyhow::anyhow!("agent response failed: {}", error.without_url()))?
    {
        ensure!(
            chunk.len() <= limit.saturating_sub(body.len()),
            "agent response exceeded byte limit"
        );
        body.extend_from_slice(&chunk);
    }
    tokio::task::spawn_blocking(move || decode(body))
        .await
        .context("agent decode worker failed")?
}
fn decode(body: Vec<u8>) -> anyhow::Result<Received> {
    let wire: AgentSnapshot = serde_json::from_slice(&body).map_err(|error| {
        anyhow::anyhow!(
            "invalid agent snapshot JSON at line {} column {}",
            error.line(),
            error.column()
        )
    })?;
    ensure!(
        wire.schema_version == 1,
        "unsupported agent snapshot schema"
    );
    ensure!(
        (1..=31_536_000).contains(&wire.stale_after_secs),
        "invalid agent freshness threshold"
    );
    ensure!(
        wire.snapshot.backends.len() <= 64,
        "agent returned too many backends"
    );
    for (records, status) in [
        (&wire.snapshot.installed, PatchStatus::Installed),
        (&wire.snapshot.pending, PatchStatus::Pending),
    ] {
        ensure!(
            records.len() <= 100_000,
            "agent returned too many patch records"
        );
        ensure!(
            records
                .iter()
                .all(|record| record.status == status && !record.kb_id.trim().is_empty())
                && records.windows(2).all(|pair| pair[0].kb_id < pair[1].kb_id),
            "agent inventory must have unique sorted keys and matching status"
        );
    }
    ensure!(
        wire.snapshot.pending.iter().all(|record| wire
            .snapshot
            .installed
            .binary_search_by(|installed| installed.kb_id.cmp(&record.kb_id))
            .is_err()),
        "agent installed/pending inventories overlap"
    );
    ensure!(
        wire.snapshot.is_ready()
            || (wire.snapshot.installed.is_empty() && wire.snapshot.pending.is_empty()),
        "unready agent must not fabricate inventory"
    );
    let size = body.len();
    let raw = RawValue::from_string(String::from_utf8(body).context("agent snapshot UTF-8")?)?;
    Ok(Received {
        wire: Arc::new(wire),
        raw: Arc::from(raw),
        size,
    })
}

pub async fn run(hub: Arc<Hub>, metrics: Metrics, mut shutdown: watch::Receiver<bool>) {
    let mut ticker = tokio::time::interval(Duration::from_secs(hub.config.interval_secs));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if *shutdown.borrow() {
            break;
        }
        tokio::select! {biased; _=shutdown.changed()=>break, _=ticker.tick()=>{
            tokio::select! {biased; _=shutdown.changed()=>break, result=hub.poll_once(&metrics)=>{if let Err(error)=result {tracing::warn!(error=%error,"hub publication failed; retaining previous state");}}}
        }}
    }
}
