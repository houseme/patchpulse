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
    pub server: ServerConfig,
    pub collector: CollectorConfig,
    pub cache: CacheConfig,
    pub observability: ObservabilityConfig,
    pub baseline: BaselineConfig,
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
}
impl Default for ObservabilityConfig {
    fn default() -> Self {
        Self {
            log_level: "info".into(),
            log_format: "json".into(),
            metrics_enabled: true,
            log_file: None,
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
        if !(c.enable_wmi_installed
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
}
