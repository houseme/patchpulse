mod support;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use patchpulse::{
    api::{self, ApiState},
    domain::patch::PatchStatus,
    scheduler,
};
use support::*;
use tower::ServiceExt;

async fn response(
    router: &axum::Router,
    method: &str,
    uri: &str,
) -> (StatusCode, serde_json::Value) {
    let result = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = result.status();
    let bytes = to_bytes(result.into_body(), 1_000_000).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn every_json_endpoint_obeys_initial_and_collected_contracts() {
    let (store, orchestrator, metrics) = setup(vec![
        mock(
            "wua_installed",
            vec![Some(vec![record("KB1", PatchStatus::Installed)])],
        ),
        mock(
            "wua_pending",
            vec![Some(vec![record("KB2", PatchStatus::Pending)])],
        ),
    ]);
    let router = api::build(
        ApiState {
            store: store.clone(),
            metrics: metrics.clone(),
        },
        true,
        15,
    );
    assert_eq!(
        response(&router, "GET", "/health").await,
        (StatusCode::OK, serde_json::json!({"status":"ok"}))
    );
    assert_eq!(
        response(&router, "GET", "/ready").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let initial = response(&router, "GET", "/patches/summary").await.1;
    assert_eq!(initial["is_stale"], true);
    assert!(initial["last_refreshed"].is_null());
    let version = response(&router, "GET", "/version").await.1;
    assert_eq!(version["license"], "Apache-2.0");
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    assert_eq!(response(&router, "GET", "/ready").await.0, StatusCode::OK);
    for (uri, expected) in [("/patches", "installed"), ("/patches/pending", "pending")] {
        let (status, body) = response(&router, "GET", uri).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["count"], 1);
        assert_eq!(body["items"][0]["status"], expected);
        assert!(body["items"][0]["sources"].is_array());
    }
    let summary = response(&router, "GET", "/patches/summary").await.1;
    assert_eq!(summary["total_installed"], 1);
    assert_eq!(summary["total_pending"], 1);
    assert_eq!(summary["coverage"]["wua_installed"], true);
    assert_eq!(summary["coverage"]["wmi_installed"], false);
    assert!(summary["backends"]["wua_installed"]["last_success"].is_string());
    assert_eq!(summary["is_stale"], false);
}

#[tokio::test]
async fn filters_are_applied_and_invalid_queries_return_json_errors() {
    let (store, orchestrator, metrics) = setup(vec![mock(
        "wua_installed",
        vec![Some(vec![record("KB1", PatchStatus::Installed)])],
    )]);
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let router = api::build(ApiState { store, metrics }, true, 15);
    for uri in [
        "/patches?since=bad",
        "/patches?status=bad",
        "/patches?typo=value",
        "/patches?since=2026-09-01",
        "/patches?status=installed&status=pending",
        "/patches?since=2026-09-01T00:00:00Z&since=2026-09-02T00:00:00Z",
    ] {
        let (status, body) = response(&router, "GET", uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_query");
    }
    assert_eq!(
        response(&router, "GET", "/patches?since=2026-09-01T00:00:00Z")
            .await
            .1["count"],
        1
    );
    assert_eq!(
        response(&router, "GET", "/patches?since=2026-09-02T00:00:00Z")
            .await
            .1["count"],
        0
    );
    assert_eq!(
        response(&router, "GET", "/patches?status=pending").await.1["count"],
        0
    );
    assert_eq!(
        response(&router, "POST", "/patches").await.0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        response(&router, "GET", "/unknown").await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn metrics_have_correct_type_and_can_be_disabled() {
    let (store, orchestrator, metrics) = setup(vec![mock("wua_pending", vec![None])]);
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let state = ApiState { store, metrics };
    let router = api::build(state.clone(), true, 15);
    let result = router
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(
        result.headers()["content-type"],
        "text/plain; version=0.0.4; charset=utf-8"
    );
    let bytes = to_bytes(result.into_body(), 1_000_000).await.unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("patchpulse_collect_failure_total{collector=\"wua_pending\"} 1"));
    assert!(body.contains("patchpulse_stale 1"));
    assert_eq!(
        response(&api::build(state, false, 15), "GET", "/metrics")
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn encoded_filters_preserve_unicode_escaping_and_exact_schema() {
    let mut old = record("KB1", PatchStatus::Installed);
    old.title = Some("Quotes \" and slashes \\ with 中文".into());
    let mut recent = record("KB2", PatchStatus::Installed);
    recent.installed_on = Some("2026-09-02T00:00:00Z".parse().unwrap());
    recent.description = Some("Line one\nLine two 🦀".into());
    let (store, orchestrator, metrics) =
        setup(vec![mock("wua_installed", vec![Some(vec![old, recent])])]);
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let view = store.view();
    let router = api::build(ApiState { store, metrics }, true, 15);
    let full = response(&router, "GET", "/patches").await.1;
    let filtered = response(&router, "GET", "/patches?since=2026-09-02T00:00:00Z")
        .await
        .1;
    assert_eq!(filtered["count"], 1);
    assert_eq!(filtered["items"][0], full["items"][1]);
    let all = view.list_json(false, Some("2026-08-01T00:00:00Z".parse().unwrap()));
    assert_eq!(all.as_ptr(), view.installed_json.as_ptr());
}

#[tokio::test]
async fn handlers_never_trigger_collection() {
    use std::sync::atomic::Ordering;
    let backend = mock(
        "wua_installed",
        vec![Some(vec![record("KB1", PatchStatus::Installed)])],
    );
    let (store, orchestrator, metrics) = setup(vec![backend.clone()]);
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let calls = backend.calls.load(Ordering::SeqCst);
    let router = api::build(ApiState { store, metrics }, true, 15);
    for path in [
        "/patches",
        "/patches/pending",
        "/patches/summary",
        "/ready",
        "/version",
    ] {
        assert_eq!(response(&router, "GET", path).await.0, StatusCode::OK);
    }
    assert_eq!(backend.calls.load(Ordering::SeqCst), calls);
}

#[tokio::test]
async fn all_routes_support_head_without_response_bodies() {
    let (store, _, metrics) = setup(vec![mock("wua_installed", vec![])]);
    let router = api::build(ApiState { store, metrics }, true, 15);
    for (path, expected) in [
        ("/health", StatusCode::OK),
        ("/ready", StatusCode::SERVICE_UNAVAILABLE),
        ("/version", StatusCode::OK),
        ("/patches", StatusCode::OK),
        ("/patches/pending", StatusCode::OK),
        ("/patches/summary", StatusCode::OK),
        ("/metrics", StatusCode::OK),
    ] {
        let result = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("HEAD")
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(result.status(), expected, "{path}");
        assert!(
            to_bytes(result.into_body(), 1024).await.unwrap().is_empty(),
            "{path}"
        );
    }
}
