#[cfg(windows)]
mod windows {
    use patchpulse::{collector, config::CollectorConfig, observability::Metrics};

    #[tokio::test]
    #[ignore = "requires a real Windows host with WMI and WUA services"]
    async fn native_collectors_query_real_windows_without_writing_updates() {
        let orchestrator = collector::build(&CollectorConfig::default());
        let results = orchestrator.run(&Metrics::default()).await;
        assert_eq!(results.len(), 3);
        for result in results {
            assert!(
                result.result.is_ok(),
                "{}: {:?}",
                result.name,
                result.result
            );
        }
    }

    #[tokio::test]
    #[ignore = "requires Windows PowerShell 5.1 and WUA services"]
    async fn powershell_collectors_query_real_windows_without_external_modules() {
        let config = CollectorConfig {
            enable_wmi_installed: false,
            enable_wua_installed: false,
            enable_wua_pending: false,
            enable_powershell_installed: true,
            enable_powershell_pending: true,
            ..CollectorConfig::default()
        };
        for result in collector::build(&config).run(&Metrics::default()).await {
            assert!(
                result.result.is_ok(),
                "{}: {:?}",
                result.name,
                result.result
            );
        }
    }
}
