# OpenTelemetry Trace Export

Trace export is opt-in. The default agent, hub, JSON/pretty logs and Prometheus
operate without an OTLP collector. Set `[observability.traces]` in the trusted
configuration to enable traces:

```toml
[observability.traces]
enabled = true
endpoint = "http://127.0.0.1:4318/v1/traces"
service_name = "patchpulse"
sample_ratio = 0.1
max_queue_size = 1024
export_timeout_secs = 2
```

The endpoint must be HTTP(S), end in `/v1/traces`, and contain no embedded
credentials, query or fragment. Use certificate-validating HTTPS for a remote
collector; use the deployment's protected OTLP header environment variables
when it needs authentication. Service names contain 1..128 ASCII letters,
digits, dots, hyphens or underscores. The sampling ratio is finite and 0..1;
the queue has 1..8192 slots and each export request times out within 1..5
seconds. Invalid settings fail configuration validation before startup.

The implementation uses OpenTelemetry's SDK and OTLP/HTTP JSON exporter with a
dedicated batch worker. At most 256 spans form an export batch, the serialized
request body is capped at 1 MiB, and a failed attempt is retried at most once
with bounded delay. Full queues drop traces rather than block HTTP or patch
collection. Shutdown attempts to flush for at most five seconds. An unavailable
collector produces sanitized WARN/ERROR logs and never changes patch readiness
or service exit status. Even at debug log level, SDK HTTP response bodies are
excluded from PatchPulse logs because a remote collector might echo credentials.

Application HTTP, collector cycles/backends and hub polls have structured
spans. HTTP spans use a bounded route template and method class, plus response
status. Collector spans carry a fixed backend name; hub spans carry a validated,
bounded configured agent ID. Span events are disabled and each span has at most
16 attributes. Inventory titles, script content, command lines, bearer tokens
and raw request URLs are not exported as span attributes. W3C `traceparent` and
`tracestate` arrive on HTTP and propagate to configured agent requests. Parent
sampling decisions are preserved; the configured ratio applies to root traces.
Logging filters can reduce local logs without suppressing sampled traces.

The Linux image includes system CA certificates for HTTPS. A local collector
test receives a real OTLP JSON batch and checks service identity, parent
context, server/collector/hub spans and outbound context. Other tests confirm
bounded shutdown and visible sanitized export errors when the collector is
unavailable or returns a sensitive error body. External collector deployment,
live Windows execution and resource targets remain separate acceptance gates.
