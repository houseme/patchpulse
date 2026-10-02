mod support;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use patchpulse::{
    api::{self, ApiFeatures, ApiState},
    config::BaselineConfig,
    domain::patch::PatchStatus,
    scheduler,
};
use support::*;
use tower::ServiceExt;

async fn response(router: &axum::Router, method: &str) -> (u16, serde_json::Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri("/patches/baseline")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn baseline_distinguishes_pending_compliance_and_stale_retention() {
    let config = BaselineConfig {
        enabled: true,
        name: "monthly".into(),
        required_kbs: vec![" kb1 ".into(), "1".into(), "KB2".into()],
    };
    let (store, orchestrator, metrics) = setup(vec![
        mock(
            "wua_installed",
            vec![
                Some(vec![record("KB1", PatchStatus::Installed)]),
                Some(vec![
                    record("KB1", PatchStatus::Installed),
                    record("KB2", PatchStatus::Installed),
                ]),
                None,
            ],
        ),
        mock(
            "wua_pending",
            vec![
                Some(vec![record("KB2", PatchStatus::Pending)]),
                Some(vec![]),
                None,
            ],
        ),
    ]);
    let router = api::build_with_features(
        ApiState {
            store: store.clone(),
            metrics: metrics.clone(),
        },
        true,
        15,
        ApiFeatures {
            baseline: config.prepare().unwrap(),
            ..Default::default()
        },
    );
    assert_eq!(response(&router, "GET").await.1["compliance"], "unknown");
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let report = response(&router, "GET").await.1;
    assert_eq!(report["baseline"], "monthly");
    assert_eq!(report["required_count"], 2);
    assert_eq!(report["compliance"], "non_compliant");
    assert_eq!(report["installed"], serde_json::json!(["KB1"]));
    assert_eq!(report["missing"], serde_json::json!(["KB2"]));
    assert_eq!(report["pending"], serde_json::json!(["KB2"]));
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    assert_eq!(response(&router, "GET").await.1["compliance"], "compliant");
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let report = response(&router, "GET").await.1;
    assert_eq!(report["compliance"], "unknown");
    assert_eq!(report["installed_count"], 2);
    assert_eq!(report["is_stale"], true);
    assert_eq!(response(&router, "POST").await.0, 405);
}

#[test]
fn baseline_configuration_rejects_empty_invalid_and_unknown_inputs() {
    let mut config = BaselineConfig {
        enabled: true,
        ..Default::default()
    };
    assert!(config.prepare().is_err());
    config.required_kbs = vec!["not-a-kb".into()];
    assert!(config.prepare().is_err());
    config.required_kbs = vec!["23".into(), "KB23".into()];
    assert_eq!(config.prepare().unwrap().unwrap().required_kbs.len(), 1);
    assert!(toml::from_str::<patchpulse::config::Config>("[baseline]\ntypo=true").is_err());
}

#[tokio::test]
async fn disabled_baseline_is_not_exposed() {
    let (store, _, metrics) = setup(vec![]);
    assert_eq!(
        response(&api::build(ApiState { store, metrics }, true, 15), "GET")
            .await
            .0,
        404
    );
}
