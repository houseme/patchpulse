use clap::Parser;
use patchpulse::config::{Cli, Config};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct TemporaryDirectory(PathBuf);
impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn configured_paths_are_relative_to_the_config_file() {
    let folder = TemporaryDirectory(std::env::temp_dir().join(format!(
        "patchpulse-config-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir_all(&folder.0).unwrap();
    let file = folder.0.join("patchpulse.toml");
    std::fs::write(&file, "[collector]\npowershell_script = 'scripts/query.ps1'\n[observability]\nlog_file = 'logs/output.jsonl'\n").unwrap();
    let config = Config::load(Some(&file)).unwrap();
    let base = std::fs::canonicalize(&folder.0).unwrap();
    assert_eq!(
        config.collector.powershell_script,
        Some(base.join("scripts/query.ps1"))
    );
    assert_eq!(
        config.observability.log_file,
        Some(base.join("logs/output.jsonl"))
    );
}

#[test]
fn all_disabled_invalid_bind_and_conflicting_modes_are_rejected() {
    let mut config = Config::default();
    config.collector.enable_wmi_installed = false;
    config.collector.enable_wua_installed = false;
    config.collector.enable_wua_pending = false;
    assert!(config.validate().is_err());
    assert!(toml::from_str::<Config>("[server]\nbind='example.com:9100'").is_err());
    assert!(Cli::try_parse_from(["patchpulse", "--foreground", "--service"]).is_err());
    assert!(Cli::try_parse_from(["patchpulse", "--service", "--check-config"]).is_err());
    assert!(Cli::try_parse_from(["patchpulse", "--healthcheck", "--check-config"]).is_err());
    assert_eq!(
        Cli::try_parse_from(["patchpulse", "--bind", "127.0.0.1:9101"])
            .unwrap()
            .bind
            .unwrap()
            .port(),
        9101
    );
}
