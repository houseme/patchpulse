use crate::{
    cache::{EMPTY_LIST_JSON, SnapshotStore},
    domain::{patch::PatchStatus, snapshot::BackendStatus},
    observability::{HttpRoute, Metrics},
};
use axum::{
    Extension, Json, Router,
    extract::{Query, Request, State, rejection::QueryRejection},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};

#[derive(Clone)]
pub struct ApiState {
    pub store: SnapshotStore,
    pub metrics: Metrics,
}

#[derive(Clone, Default)]
pub struct ApiFeatures {
    pub baseline: Option<crate::domain::baseline::Baseline>,
    pub hub: Option<crate::hub::FleetStore>,
}

pub fn build(state: ApiState, metrics_enabled: bool, timeout_secs: u64) -> Router {
    build_with_features(state, metrics_enabled, timeout_secs, ApiFeatures::default())
}

pub fn build_with_features(
    state: ApiState,
    metrics_enabled: bool,
    timeout_secs: u64,
    features: ApiFeatures,
) -> Router {
    let mut router = Router::new()
        .route(
            "/health",
            get(|| async { json_bytes(Bytes::from_static(b"{\"status\":\"ok\"}")) }),
        )
        .route("/ready", get(ready))
        .route(
            "/version",
            get(|| async {
                #[derive(Serialize)]
                struct Version {
                    name: &'static str,
                    version: &'static str,
                    target: &'static str,
                    arch: &'static str,
                    license: &'static str,
                }
                Json(Version {
                    name: env!("CARGO_PKG_NAME"),
                    version: env!("CARGO_PKG_VERSION"),
                    target: std::env::consts::OS,
                    arch: std::env::consts::ARCH,
                    license: "Apache-2.0",
                })
            }),
        );
    if features.hub.is_none() {
        router = router
            .route("/patches", get(installed))
            .route("/patches/pending", get(pending))
            .route("/patches/export", get(export))
            .route("/patches/summary", get(summary))
            .route("/snapshot", get(agent_snapshot));
    } else {
        router = router
            .route("/agents", get(fleet::agents))
            .route("/agents/{id}/snapshot", get(fleet::agent_detail))
            .route("/fleet/summary", get(fleet::fleet_summary));
    }
    if metrics_enabled {
        router = router.route("/metrics", get(metrics));
    }
    if let Some(baseline) = &features.baseline {
        if features.hub.is_none() {
            router = router.route(
                "/patches/baseline",
                get(compare_baseline).layer(Extension(std::sync::Arc::new(baseline.clone()))),
            );
        } else {
            router = router
                .route("/agents/{id}/baseline", get(fleet::agent_baseline))
                .route("/fleet/baseline", get(fleet::fleet_baseline));
        }
    }
    router = router.layer(Extension(std::sync::Arc::new(features)));
    apply_policies(router, state, Duration::from_secs(timeout_secs))
}

fn apply_policies(router: Router<ApiState>, state: ApiState, timeout: Duration) -> Router {
    let router = router
        .fallback(|| async { api_error(StatusCode::NOT_FOUND, "not_found", "route not found") })
        .method_not_allowed_fallback(|| async {
            api_error(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "only GET and HEAD are supported",
            )
        })
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            timeout,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            record_request,
        ));
    let router = if crate::observability::telemetry::enabled() {
        router.layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request| {
                    crate::observability::telemetry::server_span(request)
                })
                .on_response(
                    |response: &Response, _elapsed: Duration, span: &tracing::Span| {
                        span.record(
                            "http.response.status_code",
                            u64::from(response.status().as_u16()),
                        );
                        if response.status().is_server_error() {
                            span.record("otel.status_code", "ERROR");
                        }
                    },
                ),
        )
    } else {
        router.layer(TraceLayer::new_for_http())
    };
    router.with_state(state)
}

fn json_bytes(bytes: Bytes) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], bytes).into_response()
}

fn api_error(status: StatusCode, code: &str, message: &str) -> Response {
    #[derive(Serialize)]
    struct Detail<'a> {
        code: &'a str,
        message: &'a str,
    }
    #[derive(Serialize)]
    struct Error<'a> {
        error: Detail<'a>,
    }
    (
        status,
        Json(Error {
            error: Detail { code, message },
        }),
    )
        .into_response()
}

async fn record_request(State(state): State<ApiState>, request: Request, next: Next) -> Response {
    let route = HttpRoute::from_path(request.uri().path());
    let response = next.run(request).await;
    let response = if response.status() == StatusCode::REQUEST_TIMEOUT {
        api_error(
            StatusCode::REQUEST_TIMEOUT,
            "request_timeout",
            "request exceeded the configured timeout",
        )
    } else {
        response
    };
    state
        .metrics
        .record_request(route, response.status().as_u16());
    response
}

async fn ready(
    State(state): State<ApiState>,
    Extension(features): Extension<std::sync::Arc<ApiFeatures>>,
) -> Response {
    let is_ready = features.hub.as_ref().map_or_else(
        || state.store.view().snapshot.is_ready(),
        |hub| hub.view().ready,
    );
    if is_ready {
        json_bytes(Bytes::from_static(b"{\"status\":\"ready\"}"))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            json_bytes(Bytes::from_static(b"{\"status\":\"initializing\"}")),
        )
            .into_response()
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    status: Option<PatchStatus>,
    since: Option<jiff::Timestamp>,
}

async fn installed(
    State(state): State<ApiState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Response {
    list(state, query, false)
}
async fn pending(
    State(state): State<ApiState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Response {
    list(state, query, true)
}

fn list(
    state: ApiState,
    query: Result<Query<ListQuery>, QueryRejection>,
    pending: bool,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => {
            return api_error(StatusCode::BAD_REQUEST, "invalid_query", &error.body_text());
        }
    };
    let scope = if pending {
        PatchStatus::Pending
    } else {
        PatchStatus::Installed
    };
    if query.status.is_some_and(|status| status != scope) {
        return json_bytes(Bytes::from_static(EMPTY_LIST_JSON));
    }
    let view = state.store.view();
    json_bytes(view.list_json(pending, query.since))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ExportQuery {
    format: Option<String>,
    status: Option<PatchStatus>,
    since: Option<jiff::Timestamp>,
}

async fn export(
    State(state): State<ApiState>,
    query: Result<Query<ExportQuery>, QueryRejection>,
) -> Response {
    let Query(query) = match query {
        Ok(query) => query,
        Err(error) => {
            return api_error(StatusCode::BAD_REQUEST, "invalid_query", &error.body_text());
        }
    };
    if query
        .format
        .as_deref()
        .is_some_and(|format| format != "csv")
    {
        return api_error(
            StatusCode::BAD_REQUEST,
            "invalid_query",
            "format must be csv",
        );
    }
    let status = query.status.unwrap_or(PatchStatus::Installed);
    let csv = if matches!(status, PatchStatus::Installed | PatchStatus::Pending) {
        state
            .store
            .view()
            .list_csv(status == PatchStatus::Pending, query.since)
    } else {
        Bytes::from_static(crate::export::HEADER)
    };
    (
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"patchpulse-inventory.csv\"",
            ),
        ],
        csv,
    )
        .into_response()
}

async fn compare_baseline(
    State(state): State<ApiState>,
    Extension(baseline): Extension<std::sync::Arc<crate::domain::baseline::Baseline>>,
) -> Response {
    let view = state.store.view();
    Json(
        baseline.compare(
            &view.snapshot,
            view.snapshot
                .is_stale(jiff::Timestamp::now(), state.store.stale_after_secs),
        ),
    )
    .into_response()
}

#[derive(Serialize)]
struct Summary<'a> {
    total_installed: usize,
    total_pending: usize,
    latest_installed_kb: Option<&'a str>,
    latest_installed_at: Option<jiff::Timestamp>,
    latest_installed_date: Option<jiff::civil::Date>,
    last_refreshed: Option<jiff::Timestamp>,
    is_stale: bool,
    consecutive_failures: u32,
    last_error: Option<&'a str>,
    coverage: &'a BTreeMap<String, bool>,
    backends: &'a BTreeMap<String, BackendStatus>,
    reboot_required: bool,
    coverage_note: &'static str,
}

async fn summary(State(state): State<ApiState>) -> Response {
    let view = state.store.view();
    let snapshot = &view.snapshot;
    let latest = view.latest_installed();
    Json(Summary {
        total_installed: snapshot.installed.len(), total_pending: snapshot.pending.len(),
        latest_installed_kb: latest.map(|record| record.kb_id.as_str()), latest_installed_at: latest.and_then(|record| record.installed_on), latest_installed_date: latest.and_then(|record| record.installed_date),
        last_refreshed: snapshot.last_refreshed, is_stale: snapshot.is_stale(jiff::Timestamp::now(), state.store.stale_after_secs), consecutive_failures: snapshot.consecutive_failures, last_error: snapshot.last_error.as_deref(),
        coverage: &view.coverage, backends: &snapshot.backends, reboot_required: snapshot.reboot_required,
        coverage_note: "WMI exposes CBS QuickFixEngineering records; WUA searches the local cached catalog. Enabled backends do not imply complete inventory coverage.",
    }).into_response()
}

async fn metrics(
    State(state): State<ApiState>,
    Extension(features): Extension<std::sync::Arc<ApiFeatures>>,
) -> Response {
    let view = state.store.view();
    let now = jiff::Timestamp::now();
    let body = if let Some(hub) = &features.hub {
        state
            .metrics
            .render_gauges(hub.view().gauges(now, hub.stale_after_secs))
    } else {
        state
            .metrics
            .render(&view.snapshot, state.store.stale_after_secs, now)
    };
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

async fn agent_snapshot(State(state): State<ApiState>) -> Response {
    let view = state.store.view();
    let now = jiff::Timestamp::now();
    Json(crate::domain::agent::AgentSnapshot {
        schema_version: 1,
        observed_at: now,
        stale_after_secs: state.store.stale_after_secs,
        is_stale: view.snapshot.is_stale(now, state.store.stale_after_secs),
        snapshot: std::sync::Arc::clone(&view.snapshot),
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tower::ServiceExt;

    struct Cancelled(Arc<AtomicBool>);
    impl Drop for Cancelled {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[tokio::test]
    async fn request_timeout_cancels_handler_returns_json_and_records_408() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let state = ApiState {
            store: SnapshotStore::new(30, &[]),
            metrics: Metrics::default(),
        };
        let router = apply_policies(
            Router::new().route(
                "/health",
                get(move || async move {
                    let _guard = Cancelled(flag);
                    std::future::pending::<Response>().await
                }),
            ),
            state.clone(),
            Duration::from_millis(10),
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
        assert_eq!(result.status(), StatusCode::REQUEST_TIMEOUT);
        assert_eq!(result.headers()[header::CONTENT_TYPE], "application/json");
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(result.into_body(), 1024).await.unwrap()).unwrap();
        assert_eq!(body["error"]["code"], "request_timeout");
        assert!(cancelled.load(Ordering::Acquire));
        let rendered =
            state
                .metrics
                .render(&state.store.view().snapshot, 30, jiff::Timestamp::now());
        assert!(rendered.contains("path=\"/health\",status=\"408\"} 1"));
    }
}
mod fleet;
