//! Native Windows SCM dispatch and stop notifications.

use crate::{
    app,
    config::{Cli, Config},
    observability,
};
use clap::Parser;
use std::{
    ffi::OsString,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};
use tokio::sync::watch;
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle},
    service_dispatcher,
};

const NAME: &str = "PatchPulse";
static FAILURE: Mutex<Option<String>> = Mutex::new(None);

pub fn run() -> anyhow::Result<()> {
    service_dispatcher::start(NAME, ffi_service_main)?;
    if let Some(error) = FAILURE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    {
        anyhow::bail!(error);
    }
    Ok(())
}

define_windows_service!(ffi_service_main, service_main);

fn status(state: ServiceState, exit: u32) -> ServiceStatus {
    let pending = matches!(
        state,
        ServiceState::StartPending | ServiceState::StopPending
    );
    ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: if state == ServiceState::Running {
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
        } else {
            ServiceControlAccept::empty()
        },
        exit_code: if exit == 0 {
            ServiceExitCode::Win32(0)
        } else {
            ServiceExitCode::ServiceSpecific(exit)
        },
        checkpoint: u32::from(pending),
        wait_hint: if pending {
            Duration::from_secs(30)
        } else {
            Duration::ZERO
        },
        process_id: None,
    }
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(error) = run_service() {
        tracing::error!(error = %error, "Windows service failed");
        *FAILURE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
    }
}

fn run_service() -> anyhow::Result<()> {
    let (sender, receiver) = watch::channel(false);
    let shared_handle: Arc<OnceLock<ServiceStatusHandle>> = Arc::new(OnceLock::new());
    let control_handle = Arc::clone(&shared_handle);
    let handle = service_control_handler::register(NAME, move |event| match event {
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        ServiceControl::Stop | ServiceControl::Shutdown => {
            if let Some(handle) = control_handle.get() {
                let _ = handle.set_service_status(status(ServiceState::StopPending, 0));
            }
            let _ = sender.send(true);
            ServiceControlHandlerResult::NoError
        }
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    let _ = shared_handle.set(handle);
    handle.set_service_status(status(ServiceState::StartPending, 0))?;
    let result = (|| -> anyhow::Result<()> {
        let cli = Cli::try_parse()?;
        let mut config = Config::load(cli.config.as_deref())?;
        if let Some(bind) = cli.bind {
            config.server.bind = bind;
        }
        observability::init_logging(&config.observability)?;
        app::block_on(app::run(config, receiver, || {
            handle.set_service_status(status(ServiceState::Running, 0))?;
            Ok(())
        }))
    })();
    handle.set_service_status(status(ServiceState::Stopped, u32::from(result.is_err())))?;
    result
}
