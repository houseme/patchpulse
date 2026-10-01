use crate::{
    cache::{EMPTY_LIST_JSON, SnapshotStore},
    domain::{patch::PatchStatus, snapshot::BackendStatus},
    observability::{HttpRoute, Metrics},
};
use axum::{
    Json, Router,
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

pub fn build(state: ApiState, metrics_enabled: bool, timeout_secs: u64) -> Router {
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
        )
        .route("/patches", get(installed))
        .route("/patches/pending", get(pending))
        .route("/patches/summary", get(summary));
    if metrics_enabled {
        router = router.route("/metrics", get(metrics));
    }
    router
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
            Duration::from_secs(timeout_secs),
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            record_request,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
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
    state
        .metrics
        .record_request(route, response.status().as_u16());
    response
}

async fn ready(State(state): State<ApiState>) -> Response {
    if state.store.view().snapshot.is_ready() {
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

async fn metrics(State(state): State<ApiState>) -> Response {
    let view = state.store.view();
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.metrics.render(
            &view.snapshot,
            state.store.stale_after_secs,
            jiff::Timestamp::now(),
        ),
    )
        .into_response()
}
