use anyhow::{Context, ensure};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Deserialize)]
pub(super) struct Metadata {
    pub packages: Vec<Package>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct Package {
    pub name: String,
    pub version: String,
    pub license: Option<String>,
    pub manifest_path: PathBuf,
    pub source: Option<String>,
}

pub(super) fn load(root: &Path, file: Option<&Path>) -> anyhow::Result<Metadata> {
    let bytes = if let Some(file) = file {
        std::fs::read(super::relative_to(root, file)).context("read metadata input")?
    } else {
        command_output(
            Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
                .current_dir(root)
                .args([
                    "metadata",
                    "--locked",
                    "--all-features",
                    "--format-version",
                    "1",
                ])
                .arg("--manifest-path")
                .arg(root.join("Cargo.toml")),
            "cargo metadata",
        )?
    };
    serde_json::from_slice(&bytes).context("parse Cargo metadata")
}

pub(super) fn command_output(command: &mut Command, operation: &str) -> anyhow::Result<Vec<u8>> {
    let output = command
        .output()
        .with_context(|| format!("start {operation}"))?;
    ensure!(
        output.status.success(),
        "{operation} failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output.stdout)
}

pub(super) fn validate_identifier(value: &str, version: bool) -> anyhow::Result<()> {
    ensure!(
        !value.is_empty()
            && value.bytes().all(|c| c.is_ascii_alphanumeric()
                || c == b'-'
                || c == b'_'
                || (version && matches!(c, b'.' | b'+'))),
        "invalid package identifier: {value}"
    );
    Ok(())
}
