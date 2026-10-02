use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use clap::{ArgGroup, Parser};
use serde::{Deserialize, Serialize};

/// Command-line controls for foreground and SCM startup.
#[derive(Debug, Parser)]
#[command(version, about, group(ArgGroup::new("action").args(["service", "check_config", "healthcheck"])))]
pub struct Cli {
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    #[arg(long, conflicts_with = "service")]
    pub foreground: bool,
    #[arg(long, conflicts_with = "foreground")]
    pub service: bool,
    #[arg(long)]
    pub bind: Option<SocketAddr>,
    #[arg(long)]
    pub check_config: bool,
    #[arg(long)]
    pub healthcheck: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub mode: Mode,
    pub server: ServerConfig,
    pub collector: CollectorConfig,
    pub cache: CacheConfig,
    pub observability: ObservabilityConfig,
    pub baseline: BaselineConfig,
    pub hub: HubConfig,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Agent,
    Hub,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HubConfig {
    pub interval_secs: u64,
    pub request_timeout_secs: u64,
    pub stale_after_secs: u64,
    pub max_concurrency: usize,
    pub max_response_bytes: usize,
    pub max_total_snapshot_bytes: usize,
    pub agents: Vec<AgentConfig>,
}
impl Default for HubConfig {
    fn default() -> Self {
        Self {
            interval_secs: 60,
            request_timeout_secs: 10,
            stale_after_secs: 180,
            max_concurrency: 4,
            max_response_bytes: 8 * 1024 * 1024,
            max_total_snapshot_bytes: 64 * 1024 * 1024,
            agents: Vec::new(),
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentConfig {
    pub id: String,
    pub url: String,
    pub bearer_token_env: Option<String>,
}
impl HubConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        for value in [
            self.interval_secs,
            self.request_timeout_secs,
            self.stale_after_secs,
        ] {
            anyhow::ensure!(
                (1..=31_536_000).contains(&value),
                "hub durations must be between 1 and 31536000"
            );
        }
        anyhow::ensure!(
            (1..=8).contains(&self.max_concurrency),
            "hub max_concurrency must be between 1 and 8"
        );
        anyhow::ensure!(
            (1024..=64 * 1024 * 1024).contains(&self.max_response_bytes),
            "hub max_response_bytes must be between 1 KiB and 64 MiB"
        );
        anyhow::ensure!(
            self.max_total_snapshot_bytes >= self.max_response_bytes
                && self.max_total_snapshot_bytes <= 512 * 1024 * 1024,
            "hub total snapshot budget must cover one response and not exceed 512 MiB"
        );
        anyhow::ensure!(
            self.agents.len() <= 256,
            "hub supports at most 256 configured agents"
        );
        let mut ids = std::collections::BTreeSet::new();
        for agent in &self.agents {
            anyhow::ensure!(
                !agent.id.is_empty()
                    && agent.id.len() <= 64
                    && agent
                        .id
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_')),
                "agent IDs must contain 1..64 ASCII letters, digits, hyphens or underscores"
            );
            anyhow::ensure!(ids.insert(&agent.id), "duplicate agent ID: {}", agent.id);
            let endpoint = agent.endpoint()?;
            anyhow::ensure!(
                agent.bearer_token_env.is_none() || endpoint.scheme() == "https",
                "agent {} requires HTTPS when a bearer credential is configured",
                agent.id
            );
            if let Some(name) = &agent.bearer_token_env {
                anyhow::ensure!(
                    !name.is_empty()
                        && name.len() <= 128
                        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
                    "invalid bearer environment variable name"
                );
            }
        }
        Ok(())
    }
}
impl AgentConfig {
    pub fn endpoint(&self) -> anyhow::Result<reqwest::Url> {
        let mut url = reqwest::Url::parse(&self.url)
            .map_err(|_| anyhow::anyhow!("invalid configured agent URL for {}", self.id))?;
        anyhow::ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "agent {} requires an HTTP(S) URL without credentials, query or fragment",
            self.id
        );
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        url.join("snapshot").context("construct agent snapshot URL")
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct BaselineConfig {
    pub enabled: bool,
    pub name: String,
    pub required_kbs: Vec<String>,
}
impl Default for BaselineConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            name: "default".into(),
            required_kbs: Vec::new(),
        }
    }
}
impl BaselineConfig {
    pub fn prepare(&self) -> anyhow::Result<Option<crate::domain::baseline::Baseline>> {
        anyhow::ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 128,
            "baseline name must contain 1..128 bytes"
        );
        anyhow::ensure!(
            self.required_kbs.len() <= 10_000,
            "baseline contains more than 10000 KBs"
        );
        let required_kbs = self
            .required_kbs
            .iter()
            .map(|kb| {
                crate::domain::patch::normalize_kb(kb)
                    .with_context(|| format!("invalid baseline KB: {kb}"))
            })
            .collect::<anyhow::Result<std::collections::BTreeSet<_>>>()?;
        anyhow::ensure!(
            !self.enabled || !required_kbs.is_empty(),
            "an enabled baseline must contain at least one KB"
        );
        Ok(self.enabled.then(|| crate::domain::baseline::Baseline {
            name: self.name.clone(),
            required_kbs,
        }))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub request_timeout_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from(([127, 0, 0, 1], 9100)),
            request_timeout_secs: 15,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CollectorConfig {
    pub interval_secs: u64,
    pub collector_timeout_secs: u64,
    pub enable_wmi_installed: bool,
    pub enable_wua_installed: bool,
    pub enable_wua_pending: bool,
    pub enable_powershell_installed: bool,
    pub enable_powershell_pending: bool,
    pub powershell_script: Option<PathBuf>,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            interval_secs: 1800,
            collector_timeout_secs: 180,
            enable_wmi_installed: true,
            enable_wua_installed: true,
            enable_wua_pending: true,
            enable_powershell_installed: false,
            enable_powershell_pending: false,
            powershell_script: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CacheConfig {
    pub stale_after_secs: u64,
}
impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            stale_after_secs: 7200,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ObservabilityConfig {
    pub log_level: String,
    pub log_format: String,
    pub metrics_enabled: bool,
    pub log_file: Option<PathBuf>,
    pub traces: TraceConfig,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TraceConfig {
    pub enabled: bool,
    pub endpoint: String,
    pub service_name: String,
    pub sample_ratio: f64,
    pub max_queue_size: usize,
    pub export_timeout_secs: u64,
}
impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: "http://127.0.0.1:4318/v1/traces".into(),
            service_name: "patchpulse".into(),
            sample_ratio: 0.1,
            max_queue_size: 1024,
            export_timeout_secs: 2,
        }
    }
}
impl TraceConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        let url = reqwest::Url::parse(&self.endpoint).context("invalid OTLP trace URL")?;
        anyhow::ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path().ends_with("/v1/traces"),
            "OTLP trace endpoint requires HTTP(S) /v1/traces without credentials, query or fragment"
        );
        anyhow::ensure!(
            !self.service_name.is_empty()
                && self.service_name.len() <= 128
                && self
                    .service_name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-')),
            "trace service_name must contain 1..128 ASCII letters, digits, dots, hyphens or underscores"
        );
        anyhow::ensure!(
            self.sample_ratio.is_finite() && (0.0..=1.0).contains(&self.sample_ratio),
            "trace sample_ratio must be between 0 and 1"
        );
        anyhow::ensure!(
            (1..=8192).contains(&self.max_queue_size),
            "trace max_queue_size must be between 1 and 8192"
        );
        anyhow::ensure!(
            (1..=5).contains(&self.export_timeout_secs),
            "trace export_timeout_secs must be between 1 and 5"
        );
        Ok(())
    }
}
impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            log_level: "info".into(),
            log_format: "json".into(),
            metrics_enabled: true,
            log_file: None,
            traces: TraceConfig::default(),
        }
    }
}

impl Config {
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        let mut config: Self = if let Some(path) = path {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("read config {}", path.display()))?;
            toml::from_str(&text).context("parse config")?
        } else {
            Self::default()
        };
        let paths = [
            &mut config.collector.powershell_script,
            &mut config.observability.log_file,
        ];
        if paths
            .iter()
            .any(|value| value.as_ref().is_some_and(|path| path.is_relative()))
        {
            let base = match path {
                Some(path) => std::fs::canonicalize(path)?
                    .parent()
                    .map(Path::to_path_buf)
                    .context("configuration has no parent")?,
                None => std::env::current_dir()?,
            };
            for path in paths {
                if let Some(relative) = path.as_ref().filter(|path| path.is_relative()) {
                    *path = Some(base.join(relative));
                }
            }
        }
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        self.baseline.prepare()?;
        self.hub.validate()?;
        self.observability.traces.validate()?;
        anyhow::ensure!(
            self.mode != Mode::Hub || !self.hub.agents.is_empty(),
            "hub mode requires at least one configured agent"
        );
        for (name, value) in [
            ("request_timeout_secs", self.server.request_timeout_secs),
            ("interval_secs", self.collector.interval_secs),
            (
                "collector_timeout_secs",
                self.collector.collector_timeout_secs,
            ),
            ("stale_after_secs", self.cache.stale_after_secs),
        ] {
            if value == 0 || value > 31_536_000 {
                bail!("{name} must be between 1 and 31536000");
            }
        }
        let c = &self.collector;
        if self.mode == Mode::Agent
            && !(c.enable_wmi_installed
                || c.enable_wua_installed
                || c.enable_wua_pending
                || c.enable_powershell_installed
                || c.enable_powershell_pending)
        {
            bail!("at least one collector must be enabled");
        }
        if !matches!(self.observability.log_format.as_str(), "json" | "pretty") {
            bail!("log_format must be json or pretty");
        }
        tracing_subscriber::EnvFilter::try_new(&self.observability.log_level)
            .context("invalid log filter")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe_and_valid() {
        let config = Config::load(None).unwrap();
        config.validate().unwrap();
        assert!(config.server.bind.ip().is_loopback());
    }

    #[test]
    fn typos_and_invalid_settings_are_rejected() {
        assert!(toml::from_str::<Config>("[server]\nbnid = '127.0.0.1:1'").is_err());
        let mut config = Config::default();
        config.collector.interval_secs = 0;
        assert!(config.validate().is_err());
        config.collector.interval_secs = 1;
        config.observability.log_level = "[".into();
        assert!(config.validate().is_err());
    }

    #[test]
    fn trace_configuration_rejects_unsafe_endpoints_and_unbounded_settings() {
        let mut config = TraceConfig {
            enabled: true,
            ..TraceConfig::default()
        };
        config.validate().unwrap();
        config.endpoint = "https://user:token@example.com/v1/traces".into();
        assert!(config.validate().is_err());
        config.endpoint = "http://127.0.0.1:4318/v1/traces".into();
        config.sample_ratio = f64::NAN;
        assert!(config.validate().is_err());
        config.sample_ratio = 1.0;
        config.max_queue_size = 0;
        assert!(config.validate().is_err());
        config.max_queue_size = 64;
        config.export_timeout_secs = 6;
        assert!(config.validate().is_err());
    }
}
