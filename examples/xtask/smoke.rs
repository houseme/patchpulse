use std::{collections::BTreeMap, net::SocketAddr, path::Path, process::Command, time::Duration};

use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};

use super::{http, metadata::command_output};

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Image {
    id: String,
    architecture: String,
    size: u64,
    config: ImageConfig,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ImageConfig {
    user: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ContainerState {
    host_config: HostConfig,
    state: ExitState,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct HostConfig {
    #[serde(rename = "ReadonlyRootfs")]
    read_only: bool,
    cap_drop: Vec<String>,
    security_opt: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ExitState {
    exit_code: i32,
}

#[derive(Serialize)]
struct Report<'a> {
    image: &'a str,
    image_id: String,
    architecture: String,
    size_bytes: u64,
    non_root: bool,
    read_only: bool,
    healthcheck: &'static str,
    sigterm_exit_code: i32,
    endpoints: BTreeMap<&'static str, u16>,
}

struct Container {
    name: String,
    removed: bool,
}

impl Container {
    fn remove(&mut self) -> anyhow::Result<()> {
        command_output(
            Command::new("docker").args(["rm", "-f", &self.name]),
            "remove smoke container",
        )?;
        self.removed = true;
        Ok(())
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        if !self.removed {
            let _ = Command::new("docker")
                .args(["rm", "-f", &self.name])
                .output();
        }
    }
}

async fn docker(args: &[&str]) -> anyhow::Result<String> {
    let args: Vec<_> = args.iter().map(|arg| (*arg).to_owned()).collect();
    // Docker waits must not block the executor driving HTTP probes.
    let output = tokio::task::spawn_blocking(move || {
        command_output(Command::new("docker").args(&args), "Docker smoke command")
    })
    .await
    .context("Docker command worker failed")??;
    Ok(String::from_utf8(output)
        .context("Docker returned non-UTF-8 output")?
        .trim()
        .to_owned())
}

fn parse_inspect<T: serde::de::DeserializeOwned>(input: &str) -> anyhow::Result<T> {
    let mut entries: Vec<T> = serde_json::from_str(input).context("parse Docker inspect")?;
    ensure!(
        entries.len() == 1,
        "Docker inspect must return exactly one object"
    );
    entries.pop().context("missing Docker inspect object")
}

async fn verify_endpoints(address: SocketAddr) -> anyhow::Result<BTreeMap<&'static str, u16>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if http::request(address, "GET", "/health")
            .await
            .is_ok_and(|response| response.status == 200)
        {
            break;
        }
        ensure!(
            tokio::time::Instant::now() < deadline,
            "container HTTP startup timed out"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let mut checks = BTreeMap::new();
    for (path, expected) in [
        ("/health", 200),
        ("/ready", 503),
        ("/version", 200),
        ("/patches", 200),
        ("/patches/pending", 200),
        ("/patches/summary", 200),
        ("/metrics", 200),
        ("/patches?since=invalid", 400),
        ("/missing", 404),
    ] {
        let response = http::request(address, "GET", path)
            .await
            .with_context(|| format!("probe {path}"))?;
        ensure!(
            response.status == expected,
            "{path}: expected {expected}, got {}",
            response.status
        );
        checks.insert(path, response.status);
        if path == "/metrics" {
            ensure!(
                response
                    .headers
                    .get("content-type")
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| value.contains("text/plain; version=0.0.4")),
                "incorrect metrics content type"
            );
            ensure!(
                std::str::from_utf8(&response.body)?
                    .lines()
                    .any(|line| line == "patchpulse_stale 1"),
                "unsupported host must be stale"
            );
        }
        if path == "/patches/summary" {
            let summary: serde_json::Value = serde_json::from_slice(&response.body)?;
            ensure!(
                summary["is_stale"] == true && summary["total_installed"] == 0,
                "Linux must not fabricate Windows inventory"
            );
        }
    }
    ensure!(
        http::request(address, "POST", "/patches").await?.status == 405,
        "write methods must be rejected"
    );
    ensure!(
        http::request(address, "HEAD", "/health").await?.status == 200,
        "HEAD liveness probe failed"
    );
    checks.insert("POST /patches", 405);
    checks.insert("HEAD /health", 200);
    Ok(checks)
}

fn verify_logs(logs: &str) -> anyhow::Result<()> {
    ensure!(
        logs.contains("requires Windows"),
        "unsupported collectors must log failures"
    );
    let mut count = 0;
    for line in logs.lines().filter(|line| !line.trim().is_empty()) {
        let entry: serde_json::Value =
            serde_json::from_str(line).context("parse structured container log")?;
        ensure!(
            entry["timestamp"].is_string() && entry["fields"].is_object(),
            "JSON logs must include timestamps and structured fields"
        );
        count += 1;
    }
    ensure!(count > 0, "container logs must not be empty");
    Ok(())
}

async fn verify<'a>(name: &str, image: &'a str) -> anyhow::Result<Report<'a>> {
    docker(&[
        "run",
        "-d",
        "--name",
        name,
        "--read-only",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "-p",
        "127.0.0.1::9100",
        image,
    ])
    .await?;
    let port = docker(&["port", name, "9100/tcp"]).await?;
    let address: SocketAddr = port
        .lines()
        .next()
        .context("Docker published no port")?
        .parse()
        .context("parse published loopback address")?;
    let endpoints = verify_endpoints(address).await?;
    docker(&[
        "exec",
        name,
        "patchpulse",
        "--config",
        "/etc/patchpulse/patchpulse.toml",
        "--healthcheck",
    ])
    .await?;
    let details: Image = parse_inspect(&docker(&["image", "inspect", image]).await?)?;
    ensure!(
        details.config.user == "65532:65532",
        "runtime must use non-root UID/GID 65532"
    );
    verify_logs(&docker(&["logs", name]).await?)?;
    docker(&["stop", "--time", "15", name]).await?;
    let state: ContainerState = parse_inspect(&docker(&["inspect", name]).await?)?;
    ensure!(
        state.host_config.read_only
            && state.host_config.cap_drop == ["ALL"]
            && state
                .host_config
                .security_opt
                .iter()
                .any(|option| option == "no-new-privileges" || option == "no-new-privileges=true"),
        "runtime hardening options were not applied"
    );
    ensure!(
        state.state.exit_code == 0,
        "SIGTERM shutdown failed with exit code {}",
        state.state.exit_code
    );
    Ok(Report {
        image,
        image_id: details.id,
        architecture: details.architecture,
        size_bytes: details.size,
        non_root: true,
        read_only: true,
        healthcheck: "passed",
        sigterm_exit_code: state.state.exit_code,
        endpoints,
    })
}

pub(super) fn run(image: &str, report: &Path) -> anyhow::Result<()> {
    let mut container = Container {
        name: format!(
            "patchpulse-smoke-{}-{}",
            std::process::id(),
            jiff::Timestamp::now().as_nanosecond()
        ),
        removed: false,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let verification = runtime.block_on(verify(&container.name, image));
    let cleanup = container.remove();
    let verified = verification?;
    cleanup.context("verification passed but container cleanup failed")?;
    let output = serde_json::to_string_pretty(&verified)? + "\n";
    if let Some(parent) = report
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(report, &output).context("write Docker smoke report")?;
    print!("{output}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structured_logs_require_original_unsupported_diagnostic_and_fields() {
        verify_logs(r#"{"timestamp":"2026-10-02T00:00:00Z","fields":{"message":"collector requires Windows"}}"#).unwrap();
        assert!(verify_logs(r#"{"fields":{"message":"collector requires Windows"}}"#).is_err());
        assert!(verify_logs("collector requires Windows").is_err());
    }
    #[test]
    fn inspect_requires_exactly_one_object() {
        assert!(parse_inspect::<serde_json::Value>("[]").is_err());
        assert!(parse_inspect::<serde_json::Value>("[{},{}]").is_err());
        assert_eq!(
            parse_inspect::<serde_json::Value>("[{}]").unwrap(),
            serde_json::json!({})
        );
    }
}
