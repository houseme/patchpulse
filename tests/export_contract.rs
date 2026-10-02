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

#[tokio::test]
async fn csv_export_preserves_rows_filters_and_safe_spreadsheet_cells() {
    let mut installed = record("KB1", PatchStatus::Installed);
    installed.title = Some("Quoted \"title\", 中文\nnext line".into());
    installed.description = Some("  =HYPERLINK(\"unsafe\")".into());
    let (store, orchestrator, metrics) = setup(vec![
        mock("wua_installed", vec![Some(vec![installed]), None]),
        mock(
            "wua_pending",
            vec![Some(vec![record("KB2", PatchStatus::Pending)]), None],
        ),
    ]);
    scheduler::tick_once(&store, &orchestrator, &metrics)
        .await
        .unwrap();
    let original = store.view().list_csv(false, None);
    let router = api::build(
        ApiState {
            store: store.clone(),
            metrics,
        },
        true,
        15,
    );
    let result = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/patches/export?format=csv")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(result.headers()["content-type"], "text/csv; charset=utf-8");
    assert!(
        result.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .contains("attachment")
    );
    let bytes = to_bytes(result.into_body(), 1_000_000).await.unwrap();
    assert_eq!(bytes, original);
    let mut reader = csv::Reader::from_reader(bytes.as_ref());
    assert_eq!(reader.headers().unwrap().len(), 13);
    let rows: Vec<_> = reader.records().map(Result::unwrap).collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(&rows[0][0], "KB1");
    assert_eq!(&rows[0][1], "Quoted \"title\", 中文\nnext line");
    assert_eq!(&rows[0][2], "'  =HYPERLINK(\"unsafe\")");
    assert!(bytes.ends_with(b"\r\n"));
    for (query, kb) in [
        ("status=pending", Some("KB2")),
        ("status=failed", None),
        ("since=2026-09-02T00:00:00Z", None),
    ] {
        let result = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/patches/export?{query}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = to_bytes(result.into_body(), 1_000_000).await.unwrap();
        let rows: Vec<_> = csv::Reader::from_reader(bytes.as_ref())
            .records()
            .map(Result::unwrap)
            .collect();
        assert_eq!(rows.first().map(|row| &row[0]), kb);
    }
    scheduler::tick_once(
        &store,
        &orchestrator,
        &patchpulse::observability::Metrics::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        original.as_ptr(),
        store.view().list_csv(false, None).as_ptr()
    );
    let result = router
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/patches/export")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    assert!(to_bytes(result.into_body(), 1024).await.unwrap().is_empty());
}

#[tokio::test]
async fn empty_csv_and_invalid_queries_obey_http_contracts() {
    let (store, _, metrics) = setup(vec![mock("wua_installed", vec![])]);
    let router = api::build(ApiState { store, metrics }, true, 15);
    for (method, uri, expected) in [
        ("GET", "/patches/export", 200),
        ("GET", "/patches/export?format=json", 400),
        ("GET", "/patches/export?since=bad", 400),
        ("GET", "/patches/export?typo=1", 400),
        ("GET", "/patches/export?format=csv&format=csv", 400),
        ("POST", "/patches/export", 405),
    ] {
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
        assert_eq!(result.status().as_u16(), expected);
        let bytes = to_bytes(result.into_body(), 1024).await.unwrap();
        if expected == 200 {
            assert_eq!(
                csv::Reader::from_reader(bytes.as_ref()).records().count(),
                0
            );
        } else {
            assert!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["error"]["code"]
                    .is_string()
            );
        }
    }
}
