mod support;
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::get,
};
use patchpulse::{
    api::{self, ApiFeatures, ApiState},
    config::{AgentConfig, BaselineConfig, Config, HubConfig, Mode},
    domain::agent::AgentSnapshot,
    hub::Hub,
    observability::Metrics,
    scheduler,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use support::*;
use tower::ServiceExt;

struct Server {
    url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(router: Router) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { url, task }
}
async fn wire(kb: &str) -> AgentSnapshot {
    let (store, orchestrator, metrics) = setup(vec![mock(
        "wua_installed",
        vec![Some(vec![record(
            kb,
            patchpulse::domain::patch::PatchStatus::Installed,
        )])],
    )]);
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    AgentSnapshot {
        schema_version: 1,
        observed_at: jiff::Timestamp::now(),
        stale_after_secs: 7200,
        is_stale: false,
        snapshot: store.view().snapshot.clone(),
    }
}
fn agent(id: &str, url: &str) -> AgentConfig {
    AgentConfig {
        id: id.into(),
        url: url.into(),
        bearer_token_env: None,
    }
}
async fn response(router: &Router, path: &str, method: &str) -> (u16, serde_json::Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .method(method)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn real_agent_polls_preserve_identity_failure_data_and_fleet_compliance() {
    let failing = Arc::new(AtomicBool::new(false));
    let flag = failing.clone();
    let payload = wire("KB1").await;
    let first = server(Router::new().route(
        "/snapshot",
        get(move || {
            let flag = flag.clone();
            let payload = payload.clone();
            async move {
                if flag.load(Ordering::Acquire) {
                    StatusCode::SERVICE_UNAVAILABLE.into_response()
                } else {
                    Json(payload).into_response()
                }
            }
        }),
    ))
    .await;
    let payload = wire("KB1").await;
    let second = server(Router::new().route(
        "/snapshot",
        get(move || {
            let payload = payload.clone();
            async move { Json(payload) }
        }),
    ))
    .await;
    let config = HubConfig {
        agents: vec![agent("a", &first.url), agent("b", &second.url)],
        ..Default::default()
    };
    let hub = Hub::new(&config).unwrap();
    let metrics = Metrics::default();
    let baseline = BaselineConfig {
        enabled: true,
        name: "monthly".into(),
        required_kbs: vec!["1".into()],
    }
    .prepare()
    .unwrap();
    let router = api::build_with_features(
        ApiState {
            store: patchpulse::cache::SnapshotStore::new(30, &[]),
            metrics: metrics.clone(),
        },
        true,
        15,
        ApiFeatures {
            baseline,
            hub: Some(hub.store.clone()),
        },
    );
    assert_eq!(response(&router, "/ready", "GET").await.0, 503);
    assert_eq!(response(&router, "/agents/a/snapshot", "GET").await.0, 503);
    assert_eq!(
        response(&router, "/fleet/baseline", "GET").await.1["compliance"],
        "unknown"
    );
    hub.poll_once(&metrics).await.unwrap();
    let original = hub.store.view();
    assert_eq!(response(&router, "/ready", "GET").await.0, 200);
    let summary = response(&router, "/fleet/summary", "GET").await.1;
    assert_eq!(summary["configured_agents"], 2);
    assert_eq!(summary["total_installed"], 2);
    assert_eq!(summary["fresh_agents"], 2);
    assert_eq!(
        response(&router, "/fleet/baseline", "GET").await.1["compliance"],
        "compliant"
    );
    failing.store(true, Ordering::Release);
    hub.poll_once(&metrics).await.unwrap();
    let view = hub.store.view();
    assert!(Arc::ptr_eq(
        view.agents["a"].snapshot.as_ref().unwrap(),
        original.agents["a"].snapshot.as_ref().unwrap()
    ));
    let snapshot = response(&router, "/agents/a/snapshot", "GET").await.1;
    assert_eq!(snapshot["agent_id"], "a");
    assert_eq!(snapshot["is_stale"], true);
    assert_eq!(
        snapshot["snapshot"]["snapshot"]["installed"][0]["kb_id"],
        "KB1"
    );
    let report = response(&router, "/fleet/baseline", "GET").await.1;
    assert_eq!(report["compliance"], "unknown");
    assert_eq!(report["agents"]["a"]["compliance"], "unknown");
    assert_eq!(report["agents"]["b"]["compliance"], "compliant");
    assert_eq!(
        response(&router, "/agents/a/baseline", "GET").await.1["report"]["compliance"],
        "unknown"
    );
    assert_eq!(
        response(&router, "/agents/missing/snapshot", "GET").await.0,
        404
    );
    assert_eq!(response(&router, "/agents", "POST").await.0, 405);
    assert_eq!(response(&router, "/patches", "GET").await.0, 404);
    let result = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let text = String::from_utf8(
        to_bytes(result.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(text.contains("patchpulse_installed_patches 2"));
    assert!(text.contains("patchpulse_stale 1"));
    assert!(text.contains("path=\"/agents/{id}/snapshot\""));
    assert!(!text.contains(&first.url));
    failing.store(false, Ordering::Release);
    hub.poll_once(&metrics).await.unwrap();
    assert_eq!(
        response(&router, "/fleet/baseline", "GET").await.1["compliance"],
        "compliant"
    );
    let strict = BaselineConfig {
        enabled: true,
        name: "requires-two".into(),
        required_kbs: vec!["KB2".into()],
    }
    .prepare()
    .unwrap();
    let strict_router = api::build_with_features(
        ApiState {
            store: patchpulse::cache::SnapshotStore::new(30, &[]),
            metrics: metrics.clone(),
        },
        true,
        15,
        ApiFeatures {
            baseline: strict,
            hub: Some(hub.store.clone()),
        },
    );
    failing.store(true, Ordering::Release);
    hub.poll_once(&metrics).await.unwrap();
    let mixed = response(&strict_router, "/fleet/baseline", "GET").await.1;
    assert_eq!(mixed["agents"]["a"]["compliance"], "unknown");
    assert_eq!(mixed["agents"]["b"]["compliance"], "non_compliant");
    assert_eq!(mixed["compliance"], "non_compliant");
}

#[tokio::test]
async fn redirects_oversized_and_incoherent_snapshots_are_rejected() {
    let bad = server(Router::new().route(
        "/snapshot",
        get(|| async {
            (
                StatusCode::FOUND,
                [("location", "http://127.0.0.1:1/snapshot")],
            )
                .into_response()
        }),
    ))
    .await;
    let big = server(Router::new().route("/snapshot", get(|| async { "x".repeat(2048) }))).await;
    let mut invalid = wire("KB1").await;
    invalid.schema_version = 999;
    let invalid_server = server(Router::new().route(
        "/snapshot",
        get(move || {
            let invalid = invalid.clone();
            async move { Json(invalid) }
        }),
    ))
    .await;
    let config = HubConfig {
        max_response_bytes: 1024,
        agents: vec![
            agent("redirect", &bad.url),
            agent("oversize", &big.url),
            agent("schema", &invalid_server.url),
        ],
        ..Default::default()
    };
    let hub = Hub::new(&config).unwrap();
    hub.poll_once(&Metrics::default()).await.unwrap();
    let view = hub.store.view();
    assert!(!view.ready);
    assert!(
        view.agents
            .values()
            .all(|agent| agent.snapshot.is_none() && agent.last_error.is_some())
    );
    assert!(
        view.agents["redirect"]
            .last_error
            .as_ref()
            .unwrap()
            .contains("302")
    );
    assert!(
        view.agents["oversize"]
            .last_error
            .as_ref()
            .unwrap()
            .contains("byte limit")
    );
}

#[tokio::test]
async fn polling_is_bounded_and_shutdown_cancels_an_active_cycle() {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let starts = Arc::new(AtomicUsize::new(0));
    let (count, max, begun) = (active.clone(), peak.clone(), starts.clone());
    let payload = wire("KB1").await;
    let node = server(Router::new().route(
        "/snapshot",
        get(move || {
            let (count, max, begun, payload) =
                (count.clone(), max.clone(), begun.clone(), payload.clone());
            async move {
                let current = count.fetch_add(1, Ordering::SeqCst) + 1;
                max.fetch_max(current, Ordering::SeqCst);
                begun.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(30)).await;
                count.fetch_sub(1, Ordering::SeqCst);
                Json(payload)
            }
        }),
    ))
    .await;
    let config = HubConfig {
        max_concurrency: 2,
        agents: (0..5).map(|i| agent(&format!("a{i}"), &node.url)).collect(),
        ..Default::default()
    };
    let hub = Arc::new(Hub::new(&config).unwrap());
    hub.poll_once(&Metrics::default()).await.unwrap();
    assert_eq!(peak.load(Ordering::SeqCst), 2);
    let (sender, receiver) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(patchpulse::hub::run(hub, Metrics::default(), receiver));
    tokio::time::timeout(Duration::from_secs(1), async {
        while starts.load(Ordering::SeqCst) < 6 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    sender.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
}

#[test]
fn hub_configuration_is_strict_and_does_not_require_local_collectors() {
    let mut config = Config {
        mode: Mode::Hub,
        ..Default::default()
    };
    assert!(config.validate().is_err());
    config.hub.agents = vec![agent("a", "http://127.0.0.1:9100/")];
    config.collector.enable_wmi_installed = false;
    config.collector.enable_wua_installed = false;
    config.collector.enable_wua_pending = false;
    config.validate().unwrap();
    for url in [
        "file:///secret",
        "http://user:secret@localhost/",
        "http://localhost/?target=x",
    ] {
        config.hub.agents[0].url = url.into();
        assert!(config.validate().is_err());
    }
    config.hub.agents = vec![
        agent("a", "http://localhost/"),
        agent("a", "http://localhost/"),
    ];
    assert!(config.validate().is_err());
    config.hub.agents = vec![AgentConfig {
        id: "a".into(),
        url: "http://127.0.0.1:9100/".into(),
        bearer_token_env: Some("PATCHPULSE_AGENT_TOKEN".into()),
    }];
    assert!(config.validate().is_err());
}

#[tokio::test]
async fn https_rejects_an_untrusted_loopback_certificate() {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::io::Read;
    let _ = rustls::crypto::ring::default_provider().install_default();
    let tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(
                include_bytes!("fixtures/localhost-cert.der").to_vec(),
            )],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                include_bytes!("fixtures/localhost-key.der").to_vec(),
            )),
        )
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut stream = rustls::StreamOwned::new(
            rustls::ServerConnection::new(Arc::new(tls)).unwrap(),
            socket,
        );
        let _ = stream.read(&mut [0u8; 1]);
    });
    let config = HubConfig {
        request_timeout_secs: 3,
        agents: vec![agent("tls", &format!("https://{address}/"))],
        ..Default::default()
    };
    let hub = Hub::new(&config).unwrap();
    hub.poll_once(&Metrics::default()).await.unwrap();
    server.join().unwrap();
    let view = hub.store.view();
    assert!(!view.ready);
    assert!(view.agents["tls"].snapshot.is_none());
    assert!(view.agents["tls"].last_error.is_some());
}

#[tokio::test]
async fn fleet_byte_budget_and_poll_freshness_are_enforced() {
    let payload = wire("KB1").await;
    let limit = serde_json::to_vec(&payload).unwrap().len().max(1024) + 1;
    let node = server(Router::new().route(
        "/snapshot",
        get(move || {
            let payload = payload.clone();
            async move { Json(payload) }
        }),
    ))
    .await;
    let config = HubConfig {
        max_response_bytes: limit,
        max_total_snapshot_bytes: limit,
        stale_after_secs: 1,
        agents: vec![agent("a", &node.url), agent("b", &node.url)],
        ..Default::default()
    };
    let hub = Hub::new(&config).unwrap();
    hub.poll_once(&Metrics::default()).await.unwrap();
    let view = hub.store.view();
    assert_eq!(
        view.agents
            .values()
            .filter(|agent| agent.snapshot.is_some())
            .count(),
        1
    );
    assert!(view.agents.values().any(|agent|agent.last_error.as_deref()==Some("fleet snapshot byte budget exceeded")));
    let now = jiff::Timestamp::now()
        .checked_add(jiff::SignedDuration::from_secs(3))
        .unwrap();
    assert!(view.agents.values().all(|agent| agent.is_stale(now, 1)));
}

#[tokio::test]
async fn agent_snapshot_endpoint_exports_the_same_publication_and_live_freshness() {
    let (store, orchestrator, metrics) = setup(vec![mock(
        "wua_installed",
        vec![
            Some(vec![record(
                "KB1",
                patchpulse::domain::patch::PatchStatus::Installed,
            )]),
            None,
        ],
    )]);
    let router = api::build(
        ApiState {
            store: store.clone(),
            metrics: metrics.clone(),
        },
        true,
        15,
    );
    let initial = response(&router, "/snapshot", "GET").await.1;
    assert_eq!(initial["schema_version"], 1);
    assert_eq!(initial["is_stale"], true);
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let wire = response(&router, "/snapshot", "GET").await.1;
    assert_eq!(wire["is_stale"], false);
    assert_eq!(
        wire["snapshot"],
        serde_json::to_value(&store.view().snapshot).unwrap()
    );
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let failed = response(&router, "/snapshot", "GET").await.1;
    assert_eq!(failed["is_stale"], true);
    assert_eq!(failed["snapshot"]["installed"][0]["kb_id"], "KB1");
}

#[tokio::test]
async fn full_hub_runtime_polls_configured_agent_without_local_collectors() {
    let payload = wire("KB42").await;
    let node = server(Router::new().route(
        "/snapshot",
        get(move || {
            let payload = payload.clone();
            async move { Json(payload) }
        }),
    ))
    .await;
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reserved.local_addr().unwrap();
    drop(reserved);
    let mut config = Config {
        mode: Mode::Hub,
        ..Default::default()
    };
    config.server.bind = address;
    config.hub.agents = vec![agent("server42", &node.url)];
    config.validate().unwrap();
    let (sender, receiver) = tokio::sync::watch::channel(false);
    let ready = Arc::new(AtomicBool::new(false));
    let listening = ready.clone();
    let task = tokio::spawn(patchpulse::app::run(config, receiver, move || {
        listening.store(true, Ordering::Release);
        Ok(())
    }));
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        while !ready.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
        loop {
            let response = reqwest::get(format!("http://{address}/fleet/summary"))
                .await
                .unwrap();
            let body: serde_json::Value =
                serde_json::from_slice(&response.bytes().await.unwrap()).unwrap();
            if body["is_ready"] == true {
                break body;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(result["total_installed"], 1);
    let agents_response = reqwest::get(format!("http://{address}/agents"))
        .await
        .unwrap();
    let agents: serde_json::Value =
        serde_json::from_slice(&agents_response.bytes().await.unwrap()).unwrap();
    assert_eq!(agents["items"][0]["id"], "server42");
    assert_eq!(
        reqwest::get(format!("http://{address}/patches"))
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::NOT_FOUND
    );
    sender.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
