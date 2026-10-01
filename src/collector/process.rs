//! Bounded process supervision includes pipe draining and cooperative cancellation.
use super::traits::CollectError;
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

struct Process {
    child: Child,
    #[cfg(windows)]
    job: Job,
}
impl Drop for Process {
    fn drop(&mut self) {
        #[cfg(windows)]
        self.job.terminate();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[cfg(windows)]
struct Job(windows::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Job {
    fn new() -> Result<Self, CollectError> {
        use windows::Win32::System::JobObjects::*;
        // The handle is owned by this guard and closed exactly once on the worker thread.
        let job =
            Self(unsafe { CreateJobObjectW(None, None) }.map_err(|error| {
                CollectError::Backend(format!("create PowerShell job: {error}"))
            })?);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        }
        .map_err(|error| CollectError::Backend(format!("configure PowerShell job: {error}")))?;
        Ok(job)
    }
    fn assign(&self, child: &Child) -> Result<(), CollectError> {
        use std::os::windows::io::AsRawHandle;
        // Child owns the valid process handle throughout this call.
        unsafe {
            windows::Win32::System::JobObjects::AssignProcessToJobObject(
                self.0,
                windows::Win32::Foundation::HANDLE(child.as_raw_handle()),
            )
        }
        .map_err(|error| CollectError::Backend(format!("assign PowerShell job: {error}")))
    }
    fn terminate(&self) {
        // Termination is confined to processes assigned to this collector's private job.
        let _ = unsafe { windows::Win32::System::JobObjects::TerminateJobObject(self.0, 1) };
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.0) };
    }
}

pub(super) fn run(
    command: &mut Command,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<String, CollectError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(CollectError::Cancelled);
    }
    let started = Instant::now();
    #[cfg(windows)]
    let job = Job::new()?;
    let mut process = Process {
        child: command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
        #[cfg(windows)]
        job,
    };
    #[cfg(windows)]
    process.job.assign(&process.child)?;
    let stdout = process
        .child
        .stdout
        .take()
        .ok_or_else(|| CollectError::Backend("stdout unavailable".into()))?;
    let stderr = process
        .child
        .stderr
        .take()
        .ok_or_else(|| CollectError::Backend("stderr unavailable".into()))?;
    let (sender, receiver) = mpsc::channel();
    fn reader(
        pipe: impl Read + Send + 'static,
        stderr: bool,
        sender: mpsc::Sender<(bool, Result<Vec<u8>, CollectError>)>,
    ) -> std::io::Result<()> {
        thread::Builder::new()
            .name(
                if stderr {
                    "patchpulse-stderr"
                } else {
                    "patchpulse-stdout"
                }
                .into(),
            )
            .spawn(move || {
                let result = (|| {
                    let mut bytes = Vec::new();
                    pipe.take((OUTPUT_LIMIT + 1) as u64)
                        .read_to_end(&mut bytes)?;
                    if bytes.len() > OUTPUT_LIMIT {
                        return Err(CollectError::Backend(
                            "PowerShell output exceeded 8 MiB".into(),
                        ));
                    }
                    Ok(bytes)
                })();
                let _ = sender.send((stderr, result));
            })?;
        Ok(())
    }
    reader(stdout, false, sender.clone())?;
    reader(stderr, true, sender)?;
    let mut output = None;
    let mut errors = None;
    let mut exit = None;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(CollectError::Cancelled);
        }
        if started.elapsed() >= timeout {
            return Err(CollectError::Timeout(timeout.as_secs()));
        }
        if exit.is_none()
            && let Some(status) = process.child.try_wait()?
        {
            exit = Some(status);
            // A helper inheriting stdout must not extend the collection beyond its parent.
            #[cfg(windows)]
            process.job.terminate();
        }
        if let Some(status) = exit
            && output.is_some()
            && errors.is_some()
        {
            let output: Vec<u8> = output.take().unwrap_or_default();
            let errors: Vec<u8> = errors.take().unwrap_or_default();
            if !status.success() {
                return Err(CollectError::Backend(format!(
                    "PowerShell exited with {status}: {}",
                    String::from_utf8_lossy(&errors[..errors.len().min(2048)])
                )));
            }
            return String::from_utf8(output)
                .map_err(|error| CollectError::Parse(format!("stdout was not UTF-8: {error}")));
        }
        match receiver.recv_timeout(
            timeout
                .saturating_sub(started.elapsed())
                .min(Duration::from_millis(10)),
        ) {
            Ok((stderr, result)) => {
                let bytes = result?;
                if stderr {
                    errors = Some(bytes);
                } else {
                    output = Some(bytes);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) if output.is_some() && errors.is_some() => {
                thread::sleep(Duration::from_millis(1))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(CollectError::Backend(
                    "output reader terminated without a result".into(),
                ));
            }
        }
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn shell(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", script]);
        command
    }
    #[test]
    fn captures_output_and_rejects_nonzero_exit() {
        assert_eq!(
            run(
                &mut shell("printf payload"),
                Duration::from_secs(2),
                &AtomicBool::new(false)
            )
            .unwrap(),
            "payload"
        );
        assert!(matches!(
            run(
                &mut shell("printf problem >&2; exit 7"),
                Duration::from_secs(2),
                &AtomicBool::new(false)
            ),
            Err(CollectError::Backend(_))
        ));
    }
    #[test]
    fn inherited_pipe_still_obeys_the_deadline() {
        let started = Instant::now();
        let result = run(
            &mut shell("sleep 0.2 & exit 0"),
            Duration::from_millis(30),
            &AtomicBool::new(false),
        );
        assert!(matches!(result, Err(CollectError::Timeout(_))));
        assert!(started.elapsed() < Duration::from_millis(150));
        // The Unix fixture's short-lived helper exits independently; Windows uses job containment.
        thread::sleep(Duration::from_millis(220));
    }
    #[test]
    fn cancelled_work_never_starts_a_process() {
        let mut command = Command::new("/nonexistent/command");
        assert!(matches!(
            run(&mut command, Duration::from_secs(1), &AtomicBool::new(true)),
            Err(CollectError::Cancelled)
        ));
    }
}
