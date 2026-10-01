#[cfg(unix)]
#[test]
fn foreground_logs_honor_the_configured_format_and_file_sink() {
    use std::{
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };
    struct Process(Child);
    impl Drop for Process {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct Directory(std::path::PathBuf);
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = Directory(
        std::env::temp_dir().join(format!("patchpulse-log-contract-{}", std::process::id())),
    );
    std::fs::create_dir_all(&directory.0).unwrap();
    for format in ["json", "pretty"] {
        let config = directory.0.join(format!("{format}.toml"));
        let log = directory.0.join(format!("{format}.log"));
        std::fs::write(
            &config,
            format!("[observability]\nlog_format='{format}'\nlog_file='{format}.log'\n"),
        )
        .unwrap();
        let _child = Process(
            Command::new(env!("CARGO_BIN_EXE_patchpulse"))
                .args(["--foreground", "--bind", "127.0.0.1:0", "--config"])
                .arg(&config)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let started = Instant::now();
        let text = loop {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            if text.contains("PatchPulse listening") {
                break text;
            }
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "{format} log startup timed out"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        if format == "json" {
            let entry: serde_json::Value = serde_json::from_str(
                text.lines()
                    .find(|line| line.contains("PatchPulse listening"))
                    .unwrap(),
            )
            .unwrap();
            assert!(entry["timestamp"].is_string());
            assert_eq!(entry["level"], "INFO");
            assert_eq!(entry["fields"]["message"], "PatchPulse listening");
        } else {
            assert!(text.contains("INFO"));
            assert!(!text.trim_start().starts_with('{'));
        }
    }
}
