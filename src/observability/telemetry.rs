//! Bounded OTLP/HTTP trace export and W3C context propagation.
use crate::config::TraceConfig;
use opentelemetry::propagation::{Extractor, Injector, TextMapPropagator};
use opentelemetry::trace::TraceContextExt;
use opentelemetry_otlp::{Protocol, RetryPolicy, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{
    Resource,
    propagation::TraceContextPropagator,
    trace::{BatchConfigBuilder, BatchSpanProcessor, Sampler, SdkTracerProvider},
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tracing_opentelemetry::OpenTelemetrySpanExt;

static ACTIVE: AtomicBool = AtomicBool::new(false);
pub(crate) fn enabled() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}
pub(crate) fn activate() {
    ACTIVE.store(true, Ordering::Relaxed)
}

pub struct TraceGuard {
    pub(super) provider: Option<SdkTracerProvider>,
}
impl TraceGuard {
    pub(super) fn new(provider: Option<SdkTracerProvider>) -> Self {
        Self { provider }
    }
}
impl Drop for TraceGuard {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Relaxed);
        if let Some(provider) = &self.provider
            && let Err(error) = provider.shutdown_with_timeout(Duration::from_secs(5))
        {
            tracing::warn!(error=%error,"OTLP trace flush failed during shutdown");
        }
    }
}

pub(super) fn provider(config: &TraceConfig) -> anyhow::Result<Option<SdkTracerProvider>> {
    if !config.enabled {
        return Ok(None);
    }
    crate::net::install_crypto();
    let timeout = Duration::from_secs(config.export_timeout_secs);
    let client = reqwest::blocking::Client::builder()
        .tls_backend_rustls()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(timeout)
        .connect_timeout(timeout)
        .build()?;
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpJson)
        .with_endpoint(config.endpoint.clone())
        .with_timeout(timeout)
        .with_retry_policy(
            RetryPolicy::recommended()
                .with_max_retries(1)
                .with_max_delay(Duration::from_millis(100))
                .with_max_jitter(Duration::ZERO),
        )
        .with_max_request_body_size(1024 * 1024)
        .with_http_client(client)
        .build()?;
    let batch = BatchConfigBuilder::default()
        .with_max_queue_size(config.max_queue_size)
        .with_max_export_batch_size(config.max_queue_size.min(256))
        .with_scheduled_delay(Duration::from_millis(200))
        .build();
    let processor = BatchSpanProcessor::builder(exporter)
        .with_batch_config(batch)
        .build();
    let resource = Resource::builder()
        .with_service_name(config.service_name.clone())
        .with_attribute(opentelemetry::KeyValue::new(
            "service.version",
            env!("CARGO_PKG_VERSION"),
        ))
        .build();
    Ok(Some(
        SdkTracerProvider::builder()
            .with_span_processor(processor)
            .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
                config.sample_ratio,
            ))))
            .with_resource(resource)
            .with_max_events_per_span(0)
            .with_max_attributes_per_span(16)
            .build(),
    ))
}

struct HeaderExtractor<'a>(&'a axum::http::HeaderMap);
impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|value| value.to_str().ok())
    }
    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(axum::http::HeaderName::as_str).collect()
    }
}
struct HeaderInjector<'a>(&'a mut reqwest::header::HeaderMap);
impl Injector for HeaderInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(key.as_bytes()),
            reqwest::header::HeaderValue::from_str(&value),
        ) {
            self.0.insert(name, value);
        }
    }
}

pub(crate) fn server_span<B>(request: &axum::http::Request<B>) -> tracing::Span {
    let route = super::HttpRoute::from_path(request.uri().path()).path();
    let method = match request.method().as_str() {
        "GET" => "GET",
        "HEAD" => "HEAD",
        _ => "OTHER",
    };
    let span = tracing::info_span!(
        "patchpulse.http",
        otel.kind = "server",
        http.route = route,
        http.request.method = method,
        otel.status_code = tracing::field::Empty,
        http.response.status_code = tracing::field::Empty
    );
    let parent = TraceContextPropagator::new().extract(&HeaderExtractor(request.headers()));
    if parent.span().span_context().is_valid() {
        let _ = span.set_parent(parent);
    }
    span
}
pub(crate) fn inject(headers: &mut reqwest::header::HeaderMap) {
    if enabled() {
        let context = tracing::Span::current().context();
        TraceContextPropagator::new().inject_context(&context, &mut HeaderInjector(headers));
    }
}
