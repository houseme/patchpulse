# Task 11: OpenTelemetry Trace Export

Requirements: approved v0.4 roadmap; depends on Tasks 03, 05, 06, 10.

## Implementation

1. Add disabled-by-default OTLP/HTTP trace configuration: collector endpoint,
   service identity, sampling ratio, export timeout and bounded batch queue.
2. Use official OpenTelemetry SDK/exporter and tracing integration; keep JSON
   logging and Prometheus available independently. Do not add chrono/time crates.
3. Trace HTTP, collector cycles and hub polls, propagate W3C trace context across
   HTTP boundaries, and avoid scripts, command lines, credentials and inventory
   payloads in span attributes.
4. Export on SDK workers, bound retries/timeouts and flush on foreground/SCM
   shutdown. Export failure must not stop the service or change patch readiness.
5. Verify actual OTLP payload receipt at a loopback collector, service resource,
   parent context, disabled mode, invalid configuration and graceful flush.

## Acceptance

Trace export works through the configured protocol, not solely an in-memory
exporter. Existing logging/metrics contracts and unsupported-platform behavior
remain intact. Production collector deployment remains an external acceptance gate.

## Implementation record

src/observability/telemetry.rs and the optional trace layer use official
OpenTelemetry crates. config/patchpulse.toml documents the disabled default.
tests/otel_contract.rs receives real OTLP/HTTP JSON, verifies parent/injected
trace context and excludes sensitive inventory/query text. The failure and
privacy contracts cover unavailable collectors, bounded flush and sanitized
structured errors even with debug logging. See [telemetry.md](../telemetry.md).
