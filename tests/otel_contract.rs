mod support;
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
    routing::{get, post},
};
use bytes::Bytes;
use patchpulse::{
    api::{self, ApiState},
    config::{AgentConfig, HubConfig, ObservabilityConfig},
    domain::{agent::AgentSnapshot, patch::PatchStatus},
    hub::Hub,
    observability::{self, Metrics},
    scheduler,
};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use support::*;
use tower::ServiceExt;

#[test]
fn otlp_http_exports_parented_bounded_spans_and_flushes_on_shutdown() {
    let captured = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let recorder = captured.clone();
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
                post(move |body: Bytes| {
                    let recorder = recorder.clone();
                    async move {
                        recorder
                            .lock()
                            .unwrap()
                            .push(serde_json::from_slice(&body).unwrap());
                        Json(serde_json::json!({}))
                    }
                }),
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
    let mut config = ObservabilityConfig {
        log_level: "warn".into(),
        ..ObservabilityConfig::default()
    };
    config.traces.enabled = true;
    config.traces.endpoint = format!("http://{endpoint}/v1/traces");
    config.traces.service_name = "patchpulse-test".into();
    config.traces.sample_ratio = 1.0;
    config.traces.max_queue_size = 128;
    config.traces.export_timeout_secs = 2;
    config.traces.validate().unwrap();
    let guard = observability::init_logging(&config).unwrap();
    let captured_parent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let outgoing = Arc::new(Mutex::new(Vec::<String>::new()));
    let destination = outgoing.clone();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut item = record("KB42", PatchStatus::Installed);
        item.title = Some("SECRET-INVENTORY-PAYLOAD".into());
        let (store, orchestrator, metrics) =
            setup(vec![mock("wua_installed", vec![Some(vec![item])])]);
        scheduler::tick_once(&store, &orchestrator, &metrics)
            .await
            .unwrap();
        let router = api::build(
            ApiState {
                store: store.clone(),
                metrics: metrics.clone(),
            },
            true,
            15,
        );
        let request = Request::builder()
            .uri("/health?secret=SECRET-HTTP-QUERY")
            .header("traceparent", captured_parent)
            .body(Body::empty())
            .unwrap();
        let response = router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let _ = to_bytes(response.into_body(), 1024).await.unwrap();
        let wire = AgentSnapshot {
            schema_version: 1,
            observed_at: jiff::Timestamp::now(),
            stale_after_secs: 7200,
            is_stale: false,
            snapshot: store.view().snapshot.clone(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        let node = Router::new().route(
            "/snapshot",
            get(move |headers: HeaderMap| {
                let destination = destination.clone();
                let wire = wire.clone();
                async move {
                    if let Some(value) = headers
                        .get("traceparent")
                        .and_then(|value| value.to_str().ok())
                    {
                        destination.lock().unwrap().push(value.into());
                    }
                    Json(wire)
                }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, node).await.unwrap();
        });
        let hub = Hub::new(&HubConfig {
            agents: vec![AgentConfig {
                id: "node42".into(),
                url: format!("http://{endpoint}/"),
                bearer_token_env: None,
            }],
            ..Default::default()
        })
        .unwrap();
        hub.poll_once(&Metrics::default()).await.unwrap();
        assert!(hub.store.view().ready);
        server.abort();
    });
    drop(runtime);
    drop(guard);
    stop_tx.send(()).unwrap();
    collector.join().unwrap();
    let payloads = captured.lock().unwrap();
    assert!(
        !payloads.is_empty(),
        "SDK must send OTLP/HTTP JSON to the collector"
    );
    let payload = payloads
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<String>();
    assert!(payload.contains("patchpulse-test"));
    assert!(payload.contains("patchpulse.http"));
    assert!(payload.contains("patchpulse.collect"));
    assert!(payload.contains("patchpulse.hub"));
    assert!(payload.contains("4bf92f3577b34da6a3ce929d0e0e4736"));
    assert!(payload.contains("00f067aa0ba902b7"));
    assert!(!payload.contains("SECRET-INVENTORY-PAYLOAD"));
    assert!(!payload.contains("SECRET-HTTP-QUERY"));
    let headers = outgoing.lock().unwrap();
    assert_eq!(headers.len(), 1);
    assert!(headers[0].starts_with("00-"));
    let outbound_trace_id = headers[0].split('-').nth(1).unwrap();
    assert!(
        payload.contains(outbound_trace_id),
        "outgoing parent context must match an exported Hub span"
    );
}
