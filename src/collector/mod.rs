//! Collectors return data and never mutate the shared snapshot.

pub mod orchestrator;
pub mod powershell;
#[cfg(any(windows, test))]
mod process;
pub mod traits;
#[cfg(windows)]
mod windows_native;

use self::{
    orchestrator::Orchestrator,
    traits::{CollectBatch, CollectError, Collector},
};
use crate::{config::CollectorConfig, domain::patch::PatchStatus};
use std::sync::Arc;

pub const BACKEND_NAMES: [&str; 5] = [
    "wmi_installed",
    "wua_installed",
    "wua_pending",
    "powershell_installed",
    "powershell_pending",
];

struct NativeCollector {
    name: &'static str,
    status: PatchStatus,
    timeout_secs: u64,
}
impl Collector for NativeCollector {
    fn name(&self) -> &'static str {
        self.name
    }
    fn collect(&self) -> Result<CollectBatch, CollectError> {
        #[cfg(windows)]
        {
            windows_native::collect(self.name, self.status, self.timeout_secs)
        }
        #[cfg(not(windows))]
        {
            let _ = (self.status, self.timeout_secs);
            Err(CollectError::Unsupported(self.name))
        }
    }
}

pub fn build(config: &CollectorConfig) -> Orchestrator {
    let mut collectors: Vec<Arc<dyn Collector>> = Vec::new();
    for (enabled, name, status) in [
        (
            config.enable_wmi_installed,
            "wmi_installed",
            PatchStatus::Installed,
        ),
        (
            config.enable_wua_installed,
            "wua_installed",
            PatchStatus::Installed,
        ),
        (
            config.enable_wua_pending,
            "wua_pending",
            PatchStatus::Pending,
        ),
    ] {
        if enabled {
            collectors.push(Arc::new(NativeCollector {
                name,
                status,
                timeout_secs: config.collector_timeout_secs,
            }));
        }
    }
    for (enabled, name, status) in [
        (
            config.enable_powershell_installed,
            "powershell_installed",
            PatchStatus::Installed,
        ),
        (
            config.enable_powershell_pending,
            "powershell_pending",
            PatchStatus::Pending,
        ),
    ] {
        if enabled {
            collectors.push(Arc::new(powershell::PowerShellCollector {
                name,
                status,
                script: config.powershell_script.clone(),
                timeout_secs: config.collector_timeout_secs,
                cancelled: std::sync::atomic::AtomicBool::new(false),
            }));
        }
    }
    Orchestrator::new(
        collectors,
        std::time::Duration::from_secs(config.collector_timeout_secs),
    )
}
