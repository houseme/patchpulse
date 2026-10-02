# PatchPulse Implementation Plan

This plan refines the original combat-solutions document. The original is historical input; these English task specifications and architecture decisions govern the implementation.

Scope: all F1-F9 requirements plus the user-approved v0.2-v0.5 extensions: CSV
export, agent/hub aggregation, OpenTelemetry trace export and compliance baselines.
The original v0.1 boundary remains historical; Tasks 09-12 define current delivery.

Project constraints: Apache-2.0 only for PatchPulse; English code comments; current registry releases, including acceptable RC/beta releases; jiff for dates; no chrono or time crates. The user confirmed that dependencies retain upstream licenses and notices; jiff is used under MIT.

| Task | Specification | Dependencies | Implementation status |
| --- | --- | --- | --- |
| 01 | [Foundation, Configuration, and License Policy](01-foundation.md) | None | Implemented; local gates pass |
| 02 | [Patch Identity, Dates, and Reconciliation](02-domain.md) | 01 | Implemented; local gates pass |
| 03 | [Read-Only Windows Collection Backends](03-collectors.md) | 01, 02 | Implemented; live Windows gate pending |
| 04 | [Scheduling, Atomic Snapshots, and Failure Degradation](04-snapshots.md) | 02, 03 | Implemented; local gates pass |
| 05 | [HTTP Contracts and Input Validation](05-api.md) | 04 | Implemented; local gates pass |
| 06 | [Structured Logging, Prometheus, and Alerts](06-observability.md) | 03, 04, 05 | Implemented; local gates pass |
| 07 | [Windows Service Lifecycle and Deployment](07-service.md) | 01, 04, 05 | Implemented; live Windows gate pending |
| 08 | [CI, Dependency Audit, and Docker Delivery](08-delivery.md) | 01-07 | Implemented; Linux image verified; remote CI pending |
| 09 | [CSV Inventory Export](09-csv-export.md) | 02, 04, 05 | Implemented; export contracts pass |
| 10 | [Agent and Hub Aggregation](10-agent-hub.md) | 03-06 | Implemented; real HTTP/TLS and lifecycle contracts pass |
| 11 | [OpenTelemetry Trace Export](11-opentelemetry.md) | 03, 05, 06, 10 | Implemented; real local OTLP delivery and failure/privacy contracts pass |
| 12 | [Patch Baseline Comparison](12-baseline.md) | 02, 04, 05, 10 | Agent and fleet implemented; contracts pass |

## Completion audit

See [requirements-audit.md](../requirements-audit.md) and
[completion-validation.md](../completion-validation.md) for every step,
acceptance criterion, source-proposal deviation and pending gate. F1-F9 and
Tasks 09-12 have implemented paths; production Windows acceptance is incomplete.

## Delivery sequence

Original implementation sequence: 01-02, 03-04, 05-07, then 08. The approved
extensions followed: 09 and 10, then 11 and 12 using the Hub publication.
Update the validation record as checks execute. No task is considered live-Windows
validated solely because it compiles on macOS or Linux.

## Corrections to the source proposal

- Date-only WMI InstalledOn values are not necessarily CIM timestamps; do not invent a timezone.
- WMI QuickFixEngineering exposes a subset of CBS updates; LCU/SSU visibility varies. WUA complements coverage, but no backend is a universal inventory.
- A timeout around spawn_blocking does not terminate a COM call. Each backend requires single-flight protection.
- Preserve failed backend data on partial success; do not replace it with empty vectors.
- Successful zero-patch collection establishes readiness.
- Native SCM service registration requires the service dispatcher, not --foreground.
