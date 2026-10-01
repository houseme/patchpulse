use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

use super::traits::{CollectBatch, CollectError, Collector};
use crate::domain::patch::{
    PatchRecord, PatchSource, PatchStatus, normalize_kb, parse_installed_date,
};

pub struct PowerShellCollector {
    pub name: &'static str,
    pub status: PatchStatus,
    pub script: Option<PathBuf>,
    pub timeout_secs: u64,
    pub(crate) cancelled: AtomicBool,
}

impl Collector for PowerShellCollector {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn collect(&self) -> Result<CollectBatch, CollectError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(CollectError::Cancelled);
        }
        #[cfg(windows)]
        {
            let text = execute(self)?;
            parse_output(&text, self.status)
        }
        #[cfg(not(windows))]
        {
            Err(CollectError::Unsupported(self.name))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Update {
    #[serde(default)]
    kb_ids: Vec<String>,
    update_id: Option<String>,
    #[serde(default)]
    revision: u32,
    title: Option<String>,
    description: Option<String>,
    msrc_severity: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    reboot_required: bool,
    installed_on: Option<String>,
}

/// Parse both the current envelope and legacy singleton/array scripts.
pub fn parse_output(text: &str, status: PatchStatus) -> Result<CollectBatch, CollectError> {
    let text = text.trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return Err(CollectError::Parse(
            "empty output is not an authoritative success".into(),
        ));
    }
    // Probe only the envelope shape; RawValue borrows the records without a JSON DOM.
    #[derive(Deserialize)]
    struct Envelope<'a> {
        #[serde(rename = "Records", borrow, default, deserialize_with = "present_raw")]
        records: Option<&'a serde_json::value::RawValue>,
        #[serde(
            rename = "RebootRequired",
            borrow,
            default,
            deserialize_with = "present_raw"
        )]
        reboot: Option<&'a serde_json::value::RawValue>,
    }
    fn present_raw<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<&'de serde_json::value::RawValue>, D::Error> {
        <&serde_json::value::RawValue>::deserialize(deserializer).map(Some)
    }
    fn rows(text: &str) -> Result<Vec<Update>, CollectError> {
        let text = text.trim();
        match text.as_bytes().first() {
            Some(b'[') => {
                serde_json::from_str(text).map_err(|error| CollectError::Parse(error.to_string()))
            }
            Some(b'{') => serde_json::from_str(text)
                .map(|row| vec![row])
                .map_err(|error| CollectError::Parse(error.to_string())),
            _ if text == "null" => Ok(Vec::new()),
            _ => Err(CollectError::Parse(
                "expected an update object or array".into(),
            )),
        }
    }
    let (updates, reboot_required) = if text.starts_with('{') {
        let envelope: Envelope<'_> =
            serde_json::from_str(text).map_err(|error| CollectError::Parse(error.to_string()))?;
        if let Some(records) = envelope.records {
            let reboot = envelope
                .reboot
                .map(|value| serde_json::from_str::<bool>(value.get()))
                .transpose()
                .map_err(|error| CollectError::Parse(format!("RebootRequired: {error}")))?;
            (rows(records.get())?, reboot)
        } else {
            (rows(text)?, None)
        }
    } else {
        (rows(text)?, None)
    };
    let mut batch = CollectBatch {
        reboot_required,
        ..CollectBatch::default()
    };
    for update in updates {
        let identity = update.update_id.as_deref().filter(|s| !s.trim().is_empty());
        let keys = if update.kb_ids.is_empty() {
            vec![format!(
                "WUA:{}:{}",
                identity.ok_or_else(|| CollectError::Parse(
                    "update without KB must have UpdateId".into()
                ))?,
                update.revision
            )]
        } else {
            update
                .kb_ids
                .iter()
                .map(|kb| {
                    normalize_kb(kb)
                        .ok_or_else(|| CollectError::Parse(format!("invalid KB identifier: {kb}")))
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        for key in keys {
            let mut record = PatchRecord::new(key, status, PatchSource::PowerShell);
            record.update_id = update.update_id.clone();
            record.title = update.title.clone().filter(|s| !s.is_empty());
            record.description = update.description.clone().filter(|s| !s.is_empty());
            record.severity = update.msrc_severity.clone().filter(|s| !s.is_empty());
            if !update.categories.is_empty() {
                record.category = Some(update.categories.join(", "));
            }
            record.reboot_required = update.reboot_required;
            if status == PatchStatus::Installed
                && let Some(date) = &update.installed_on
            {
                (record.installed_on, record.installed_date) = parse_installed_date(date);
                record.installed_on_raw = Some(date.clone());
            }
            batch.records.push(record);
        }
    }
    Ok(batch)
}

#[cfg(windows)]
fn execute(collector: &PowerShellCollector) -> Result<String, CollectError> {
    use std::process::Command;
    let root = std::env::var_os("SystemRoot")
        .ok_or_else(|| CollectError::Backend("SystemRoot is missing".into()))?;
    let mut command =
        Command::new(PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe"));
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
    ]);
    let mode = if collector.status == PatchStatus::Installed {
        "Installed"
    } else {
        "Pending"
    };
    if let Some(script) = &collector.script {
        if !script.is_absolute() {
            return Err(CollectError::Backend("script path must be absolute".into()));
        }
        command.arg("-File").arg(script).arg("-Mode").arg(mode);
    } else {
        // Only the embedded script and a fixed enum literal enter the command expression.
        command.arg("-Command").arg(format!(
            "& {{ {} }} -Mode {mode}",
            include_str!("../../scripts/query-patches.ps1")
        ));
    }
    super::process::run(
        &mut command,
        std::time::Duration::from_secs(collector.timeout_secs),
        &collector.cancelled,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_single_array_and_envelope_are_supported() {
        for text in ["[]", "null", r#"{"Records":[],"RebootRequired":false}"#] {
            assert!(
                parse_output(text, PatchStatus::Pending)
                    .unwrap()
                    .records
                    .is_empty()
            );
        }
        let one = r#"{"KbIds":["1","KB2"],"Title":"Update"}"#;
        assert_eq!(
            parse_output(one, PatchStatus::Pending)
                .unwrap()
                .records
                .len(),
            2
        );
        assert_eq!(
            parse_output(&format!("[{one}]"), PatchStatus::Pending)
                .unwrap()
                .records
                .len(),
            2
        );
        assert!(parse_output("", PatchStatus::Pending).is_err());
        assert!(parse_output("{}", PatchStatus::Pending).is_err());
        assert!(parse_output("garbage", PatchStatus::Pending).is_err());
    }

    #[test]
    fn no_kb_uses_update_identity_and_retains_system_reboot() {
        let batch = parse_output(
            r#"{"Records":[{"UpdateId":"guid","Revision":4}],"RebootRequired":true}"#,
            PatchStatus::Pending,
        )
        .unwrap();
        assert_eq!(batch.records[0].kb_id, "WUA:guid:4");
        assert_eq!(batch.reboot_required, Some(true));
    }

    #[test]
    fn envelope_reboot_state_must_be_a_boolean() {
        for raw in ["null", "42", "\"false\""] {
            let text = format!("{{\"Records\":[],\"RebootRequired\":{raw}}}");
            assert!(matches!(
                parse_output(&text, PatchStatus::Pending),
                Err(CollectError::Parse(_))
            ));
        }
        assert!(
            parse_output(
                r#"{"Records":null,"RebootRequired":false}"#,
                PatchStatus::Pending
            )
            .unwrap()
            .records
            .is_empty()
        );
    }
}
