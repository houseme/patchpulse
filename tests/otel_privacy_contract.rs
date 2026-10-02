use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    routing::post,
};
use patchpulse::{
    api::{self, ApiState},
    cache::SnapshotStore,
    config::ObservabilityConfig,
    observability::{self, Metrics},
};
use std::{path::PathBuf, sync::mpsc, time::Duration};
use tower::ServiceExt;

struct LogFile(PathBuf);
impl Drop for LogFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn sdk_export_errors_are_logged_without_collector_response_bodies() {
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let collector = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            ready_tx.send(listener.local_addr().unwrap()).unwrap();
            let router = Router::new().route(
                "/v1/traces",
                post(|| async { (StatusCode::UNAUTHORIZED, "SECRET-COLLECTOR-ECHO") }),
            );
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = stop_rx.await;
                })
                .await
                .unwrap();
        });
    });
    let endpoint = ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let log = LogFile(std::env::temp_dir().join(format!(
        "patchpulse-otel-privacy-{}-{}.jsonl",
        std::process::id(),
        jiff::Timestamp::now().as_nanosecond()
    )));
    let mut config = ObservabilityConfig {
        log_level: "debug".into(),
        log_file: Some(log.0.clone()),
        ..Default::default()
    };
    config.traces.enabled = true;
    config.traces.endpoint = format!("http://{endpoint}/v1/traces");
    config.traces.sample_ratio = 1.0;
    config.traces.max_queue_size = 16;
    config.traces.export_timeout_secs = 1;
    let guard = observability::init_logging(&config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let router = api::build(
            ApiState {
                store: SnapshotStore::new(30, &[]),
                metrics: Metrics::default(),
            },
            true,
            15,
        );
        let result = router
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::OK);
    });
    drop(runtime);
    drop(guard);
    stop_tx.send(()).unwrap();
    collector.join().unwrap();
    let text = std::fs::read_to_string(&log.0).unwrap();
    assert!(text.contains("BatchSpanProcessor.ExportError"));
    assert!(text.contains("401"));
    assert!(!text.contains("SECRET-COLLECTOR-ECHO"));
}
