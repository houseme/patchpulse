mod support;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use patchpulse::{
    api::{self, ApiState},
    config::ObservabilityConfig,
    domain::patch::PatchStatus,
    observability::{self, Metrics},
    scheduler,
};
use std::time::{Duration, Instant};
use support::*;
use tower::ServiceExt;

#[test]
fn failed_trace_export_preserves_http_and_shutdown_progress() {
    let unused = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = unused.local_addr().unwrap();
    drop(unused);
    let mut config = ObservabilityConfig::default();
    config.traces.enabled = true;
    config.traces.endpoint = format!("http://{address}/v1/traces");
    config.traces.sample_ratio = 1.0;
    config.traces.max_queue_size = 16;
    config.traces.export_timeout_secs = 1;
    let guard = observability::init_logging(&config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (store, orchestrator, metrics) = setup(vec![mock(
            "wua_installed",
            vec![Some(vec![record("KB1", PatchStatus::Installed)])],
        )]);
        scheduler::tick_once(&store, &orchestrator, &metrics)
            .await
            .unwrap();
        let router = api::build(
            ApiState {
                store,
                metrics: Metrics::default(),
            },
            true,
            15,
        );
        for route in ["/health", "/ready", "/patches"] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(route).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{route}");
        }
    });
    drop(runtime);
    let started = Instant::now();
    drop(guard);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "trace export must not delay service shutdown"
    );
}
