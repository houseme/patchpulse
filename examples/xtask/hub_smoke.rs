use super::{http, metadata::command_output, smoke::docker};
use anyhow::{Context, ensure};
use serde::Serialize;
use std::{
    io::Write as _,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

struct Owned {
    network: String,
    agent: String,
    hub: String,
    config: PathBuf,
    network_created: bool,
}
impl Owned {
    fn cleanup(&mut self) -> anyhow::Result<()> {
        if self.network_created {
            for name in [&self.hub, &self.agent] {
                let _ = Command::new("docker").args(["rm", "-f", name]).output();
            }
            command_output(
                Command::new("docker").args(["network", "rm", &self.network]),
                "remove Hub smoke network",
            )?;
            self.network_created = false;
        }
        if self.config.exists() {
            std::fs::remove_file(&self.config)?;
        }
        Ok(())
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

#[derive(Serialize)]
struct Report<'a> {
    image: &'a str,
    image_id: String,
    agent_ready: u16,
    hub_ready: u16,
    configured_agents: u64,
    available_agents: u64,
    total_installed: u64,
    total_pending: u64,
    is_stale: bool,
    baseline: &'static str,
    non_root: bool,
    read_only: bool,
    agent_sigterm_exit_code: i64,
    hub_sigterm_exit_code: i64,
}

async fn wait_for_probe(name: &str) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if docker(&[
            "exec",
            name,
            "patchpulse",
            "--config",
            "/etc/patchpulse/patchpulse.toml",
            "--healthcheck",
        ])
        .await
        .is_ok()
        {
            return Ok(());
        }
        ensure!(
            tokio::time::Instant::now() < deadline,
            "container {name} health probe timed out"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn inspect_exit(name: &str) -> anyhow::Result<i64> {
    let state: serde_json::Value = serde_json::from_str(&docker(&["inspect", name]).await?)?;
    let state = state
        .as_array()
        .and_then(|items| items.first())
        .context("missing container inspection")?;
    ensure!(
        state["HostConfig"]["ReadonlyRootfs"] == true
            && state["HostConfig"]["CapDrop"] == serde_json::json!(["ALL"]),
        "Hub smoke containers must be read-only and drop capabilities"
    );
    let exit = state["State"]["ExitCode"]
        .as_i64()
        .context("missing container exit code")?;
    ensure!(
        exit == 0,
        "container {name} failed graceful SIGTERM: {exit}"
    );
    Ok(exit)
}

async fn verify<'a>(owned: &mut Owned, image: &'a str) -> anyhow::Result<Report<'a>> {
    docker(&["network", "create", &owned.network]).await?;
    owned.network_created = true;
    docker(&[
        "run",
        "-d",
        "--name",
        &owned.agent,
        "--network",
        &owned.network,
        "--read-only",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        image,
    ])
    .await?;
    wait_for_probe(&owned.agent).await?;
    let mount = format!(
        "{}:/etc/patchpulse/patchpulse.toml:ro",
        owned.config.display()
    );
    docker(&[
        "run",
        "-d",
        "--name",
        &owned.hub,
        "--network",
        &owned.network,
        "--read-only",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "-p",
        "127.0.0.1::9100",
        "-v",
        &mount,
        image,
    ])
    .await?;
    wait_for_probe(&owned.hub).await?;
    let published = docker(&["port", &owned.hub, "9100/tcp"]).await?;
    let address: SocketAddr = published
        .lines()
        .next()
        .context("Hub has no published port")?
        .parse()
        .context("parse Hub loopback port")?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let fleet = loop {
        let response = http::request(address, "GET", "/fleet/summary").await?;
        ensure!(
            response.status == 200,
            "Hub fleet summary returned {}",
            response.status
        );
        let body: serde_json::Value = serde_json::from_slice(&response.body)?;
        if body["available_agents"] == 1 {
            break body;
        }
        ensure!(
            tokio::time::Instant::now() < deadline,
            "Hub did not receive its configured agent snapshot"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    ensure!(
        fleet["configured_agents"] == 1
            && fleet["total_installed"] == 0
            && fleet["total_pending"] == 0
            && fleet["is_stale"] == true,
        "Linux Hub must retain one configured machine without fabricating Windows patches"
    );
    let agents = http::request(address, "GET", "/agents").await?;
    let agents: serde_json::Value = serde_json::from_slice(&agents.body)?;
    ensure!(
        agents["items"][0]["id"] == "linux-agent" && agents["items"][0]["is_stale"] == true,
        "Hub lost machine identity or source staleness"
    );
    let detail = http::request(address, "GET", "/agents/linux-agent/snapshot").await?;
    let detail: serde_json::Value = serde_json::from_slice(&detail.body)?;
    ensure!(
        detail["agent_id"] == "linux-agent" && detail["snapshot"]["schema_version"] == 1,
        "Hub detail must preserve the versioned agent envelope"
    );
    let baseline = http::request(address, "GET", "/fleet/baseline").await?;
    let baseline: serde_json::Value = serde_json::from_slice(&baseline.body)?;
    ensure!(
        baseline["compliance"] == "unknown"
            && baseline["agents"]["linux-agent"]["compliance"] == "unknown",
        "unsupported Linux inventory must never establish compliance"
    );
    ensure!(
        http::request(address, "GET", "/ready").await?.status == 503
            && http::request(address, "GET", "/patches").await?.status == 404,
        "Hub readiness and machine-scoped routes are incorrect"
    );
    ensure!(
        http::request(address, "HEAD", "/agents").await?.status == 200
            && http::request(address, "POST", "/agents").await?.status == 405,
        "Hub read-only HTTP methods are incorrect"
    );
    let metrics = http::request(address, "GET", "/metrics").await?;
    ensure!(
        metrics.status == 200
            && std::str::from_utf8(&metrics.body)?
                .lines()
                .any(|line| line == "patchpulse_stale 1"),
        "Hub metrics must mark incomplete fleet inventory stale"
    );
    let logs = docker(&["logs", &owned.hub]).await?;
    ensure!(
        logs.contains("agent snapshot received") && !logs.contains("requires Windows"),
        "Hub must poll agents without running Windows collectors locally"
    );
    for line in logs.lines().filter(|line| !line.trim().is_empty()) {
        let entry: serde_json::Value =
            serde_json::from_str(line).context("parse Hub structured log")?;
        ensure!(
            entry["timestamp"].is_string() && entry["fields"].is_object(),
            "Hub must emit structured jiff-timestamped logs"
        );
    }
    docker(&["stop", "--time", "15", &owned.hub]).await?;
    let hub_sigterm_exit_code = inspect_exit(&owned.hub).await?;
    docker(&["stop", "--time", "15", &owned.agent]).await?;
    let agent_sigterm_exit_code = inspect_exit(&owned.agent).await?;
    let image_details: serde_json::Value =
        serde_json::from_str(&docker(&["image", "inspect", image]).await?)?;
    let image_details = image_details
        .as_array()
        .and_then(|items| items.first())
        .context("missing image inspection")?;
    ensure!(
        image_details["Config"]["User"] == "65532:65532",
        "Hub image must run without root privileges"
    );
    Ok(Report {
        image,
        image_id: image_details["Id"]
            .as_str()
            .context("missing image ID")?
            .to_owned(),
        agent_ready: 503,
        hub_ready: 503,
        configured_agents: 1,
        available_agents: 1,
        total_installed: 0,
        total_pending: 0,
        is_stale: true,
        baseline: "unknown",
        non_root: true,
        read_only: true,
        agent_sigterm_exit_code,
        hub_sigterm_exit_code,
    })
}

pub(super) fn run(root: &Path, image: &str, report: &Path) -> anyhow::Result<()> {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        jiff::Timestamp::now().as_nanosecond()
    );
    let mut owned = Owned {
        network: format!("patchpulse-network-{suffix}"),
        agent: format!("patchpulse-agent-{suffix}"),
        hub: format!("patchpulse-hub-{suffix}"),
        config: root
            .join("target")
            .join(format!("patchpulse-hub-smoke-{suffix}.toml")),
        network_created: false,
    };
    std::fs::create_dir_all(root.join("target"))?;
    let config = format!(
        "mode = \"hub\"\n[server]\nbind = \"0.0.0.0:9100\"\n[hub]\n[[hub.agents]]\nid = \"linux-agent\"\nurl = \"http://{}:9100/\"\n[baseline]\nenabled = true\nname = \"smoke\"\nrequired_kbs = [\"KB1\"]\n",
        owned.agent
    );
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&owned.config)
        .context("create temporary Hub configuration")?
        .write_all(config.as_bytes())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let outcome = runtime.block_on(verify(&mut owned, image));
    let cleanup = owned.cleanup();
    let verified = outcome?;
    cleanup.context("Hub smoke passed but temporary Docker cleanup failed")?;
    let bytes = serde_json::to_string_pretty(&verified)? + "\n";
    if let Some(parent) = report
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(report, &bytes).context("write Hub smoke report")?;
    print!("{bytes}");
    Ok(())
}
