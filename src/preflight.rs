//! Read-only startup diagnostics; token privileges do not prove collector access.

#[derive(Debug, Clone, Copy)]
pub struct Privileges {
    pub supported: bool,
    pub elevated: Option<bool>,
    pub administrator: Option<bool>,
}

#[cfg(not(windows))]
pub fn inspect() -> anyhow::Result<Privileges> {
    Ok(Privileges {
        supported: false,
        elevated: None,
        administrator: None,
    })
}

#[cfg(windows)]
pub fn inspect() -> anyhow::Result<Privileges> {
    use anyhow::Context;
    use windows::{
        Win32::{
            Foundation::{CloseHandle, HANDLE},
            Security::{
                CheckTokenMembership, CreateWellKnownSid, GetTokenInformation, PSID,
                SECURITY_MAX_SID_SIZE, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
                WinBuiltinAdministratorsSid,
            },
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        },
        core::BOOL,
    };

    struct Token(HANDLE);
    impl Drop for Token {
        fn drop(&mut self) {
            // SAFETY: this guard owns the successfully opened token handle exactly once.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }
    let mut handle = HANDLE::default();
    // SAFETY: the current-process pseudo-handle is valid and handle is writable.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) }
        .context("open current process token for startup diagnostics")?;
    let token = Token(handle);
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0;
    // SAFETY: TOKEN_ELEVATION is the aligned, correctly sized output for TokenElevation.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenElevation,
            Some(std::ptr::from_mut(&mut elevation).cast()),
            std::mem::size_of_val(&elevation) as u32,
            &mut returned,
        )
    }
    .context("query process token elevation")?;
    anyhow::ensure!(
        returned as usize == std::mem::size_of_val(&elevation),
        "unexpected token elevation length"
    );
    let mut storage = [0u32; SECURITY_MAX_SID_SIZE as usize / std::mem::size_of::<u32>()];
    let sid = PSID(storage.as_mut_ptr().cast());
    let mut size = std::mem::size_of_val(&storage) as u32;
    // SAFETY: storage has SID alignment and the documented maximum SID size.
    unsafe { CreateWellKnownSid(WinBuiltinAdministratorsSid, None, Some(sid), &mut size) }
        .context("create administrators SID")?;
    let mut member = BOOL::default();
    // SAFETY: sid is initialized above and lives through this call. Startup does not impersonate;
    // a null token makes Windows use the effective token with the required impersonation type.
    unsafe { CheckTokenMembership(None, sid, &mut member) }
        .context("query enabled administrators membership")?;
    Ok(Privileges {
        supported: true,
        elevated: Some(elevation.TokenIsElevated != 0),
        administrator: Some(member.as_bool()),
    })
}

pub(crate) fn log_startup() {
    match inspect() {
        Ok(report)
            if report.supported
                && (report.elevated == Some(false) || report.administrator == Some(false)) =>
        {
            tracing::warn!(
                elevated = report.elevated,
                administrator = report.administrator,
                "startup privilege preflight: restricted token; collector access depends on account permissions"
            );
        }
        Ok(report) => {
            tracing::info!(
                platform_supported = report.supported,
                elevated = report.elevated,
                administrator = report.administrator,
                "startup privilege preflight completed"
            );
        }
        Err(error) => {
            tracing::warn!(error = %error, "startup privilege preflight failed; collectors will report actual access errors")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preflight_reports_only_observed_platform_privileges() {
        let report = inspect().unwrap();
        assert_eq!(report.supported, cfg!(windows));
        assert_eq!(report.elevated.is_some(), cfg!(windows));
        assert_eq!(report.administrator.is_some(), cfg!(windows));
    }
}
