use super::{ApiFeatures, api_error};
use axum::{
    Extension, Json,
    extract::{Path, rejection::PathRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::collections::BTreeMap;

pub(super) async fn agents(
    Extension(features): Extension<std::sync::Arc<ApiFeatures>>,
) -> Response {
    let Some(hub) = &features.hub else {
        return api_error(StatusCode::NOT_FOUND, "not_found", "hub mode is disabled");
    };
    let view = hub.view();
    let now = jiff::Timestamp::now();
    #[derive(Serialize)]
    struct Agent<'a> {
        id: &'a str,
        last_fetch: Option<jiff::Timestamp>,
        last_error: Option<&'a str>,
        consecutive_failures: u32,
        is_stale: bool,
        is_ready: bool,
        total_installed: usize,
        total_pending: usize,
    }
    let items: Vec<_> = view
        .agents
        .iter()
        .map(|(id, agent)| Agent {
            id,
            last_fetch: agent.last_fetch,
            last_error: agent.last_error.as_deref(),
            consecutive_failures: agent.consecutive_failures,
            is_stale: agent.is_stale(now, hub.stale_after_secs),
            is_ready: agent
                .snapshot
                .as_ref()
                .is_some_and(|wire| wire.snapshot.is_ready()),
            total_installed: agent
                .snapshot
                .as_ref()
                .map_or(0, |wire| wire.snapshot.installed.len()),
            total_pending: agent
                .snapshot
                .as_ref()
                .map_or(0, |wire| wire.snapshot.pending.len()),
        })
        .collect();
    #[derive(Serialize)]
    struct List<T> {
        count: usize,
        items: T,
    }
    Json(List {
        count: items.len(),
        items,
    })
    .into_response()
}

pub(super) async fn fleet_summary(
    Extension(features): Extension<std::sync::Arc<ApiFeatures>>,
) -> Response {
    let Some(hub) = &features.hub else {
        return api_error(StatusCode::NOT_FOUND, "not_found", "hub mode is disabled");
    };
    Json(
        hub.view()
            .summary(jiff::Timestamp::now(), hub.stale_after_secs),
    )
    .into_response()
}

fn agent_id(path: Result<Path<String>, PathRejection>) -> Result<String, &'static str> {
    path.map(|Path(id)| id).map_err(|_| "invalid agent path")
}

pub(super) async fn agent_detail(
    Extension(features): Extension<std::sync::Arc<ApiFeatures>>,
    path: Result<Path<String>, PathRejection>,
) -> Response {
    let id = match agent_id(path) {
        Ok(id) => id,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, "invalid_agent", error),
    };
    let Some(hub) = &features.hub else {
        return api_error(StatusCode::NOT_FOUND, "not_found", "hub mode is disabled");
    };
    let view = hub.view();
    let Some(agent) = view.agents.get(&id) else {
        return api_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "agent is not configured",
        );
    };
    #[derive(Serialize)]
    struct Detail<'a> {
        agent_id: &'a str,
        last_fetch: Option<jiff::Timestamp>,
        last_error: Option<&'a str>,
        is_stale: bool,
        snapshot: Option<&'a serde_json::value::RawValue>,
    }
    let status = if agent.raw.is_some() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(Detail {
            agent_id: &id,
            last_fetch: agent.last_fetch,
            last_error: agent.last_error.as_deref(),
            is_stale: agent.is_stale(jiff::Timestamp::now(), hub.stale_after_secs),
            snapshot: agent.raw.as_deref(),
        }),
    )
        .into_response()
}

pub(super) async fn agent_baseline(
    Extension(features): Extension<std::sync::Arc<ApiFeatures>>,
    path: Result<Path<String>, PathRejection>,
) -> Response {
    let id = match agent_id(path) {
        Ok(id) => id,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, "invalid_agent", error),
    };
    let (Some(hub), Some(baseline)) = (&features.hub, &features.baseline) else {
        return api_error(StatusCode::NOT_FOUND, "not_found", "baseline is disabled");
    };
    let view = hub.view();
    let Some(agent) = view.agents.get(&id) else {
        return api_error(
            StatusCode::NOT_FOUND,
            "not_found",
            "agent is not configured",
        );
    };
    let empty = crate::domain::snapshot::PatchSnapshot::default();
    let snapshot = agent
        .snapshot
        .as_ref()
        .map_or(&empty, |wire| wire.snapshot.as_ref());
    #[derive(Serialize)]
    struct Detail<'a> {
        agent_id: &'a str,
        report: crate::domain::baseline::BaselineReport<'a>,
    }
    Json(Detail {
        agent_id: &id,
        report: baseline.compare(
            snapshot,
            agent.is_stale(jiff::Timestamp::now(), hub.stale_after_secs),
        ),
    })
    .into_response()
}

pub(super) async fn fleet_baseline(
    Extension(features): Extension<std::sync::Arc<ApiFeatures>>,
) -> Response {
    let (Some(hub), Some(baseline)) = (&features.hub, &features.baseline) else {
        return api_error(StatusCode::NOT_FOUND, "not_found", "baseline is disabled");
    };
    let view = hub.view();
    let now = jiff::Timestamp::now();
    let empty = crate::domain::snapshot::PatchSnapshot::default();
    let reports: BTreeMap<_, _> = view
        .agents
        .iter()
        .map(|(id, agent)| {
            let snapshot = agent
                .snapshot
                .as_ref()
                .map_or(&empty, |wire| wire.snapshot.as_ref());
            (
                id,
                baseline.compare(snapshot, agent.is_stale(now, hub.stale_after_secs)),
            )
        })
        .collect();
    use crate::domain::baseline::Compliance;
    let compliance = if reports
        .values()
        .any(|report| report.compliance == Compliance::Unknown)
    {
        Compliance::Unknown
    } else if reports
        .values()
        .any(|report| report.compliance == Compliance::NonCompliant)
    {
        Compliance::NonCompliant
    } else {
        Compliance::Compliant
    };
    #[derive(Serialize)]
    struct Reports<T> {
        baseline: String,
        compliance: Compliance,
        agents: T,
    }
    Json(Reports {
        baseline: baseline.name.clone(),
        compliance,
        agents: reports,
    })
    .into_response()
}
