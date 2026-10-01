# Changelog

Changes are grouped by functional area. Each functional commit adds its own entry.
Unreleased entries do not announce a published release.

## [Unreleased]

### Foundation, configuration, and patch model

- Add the edition-2024 Rust library, pinned toolchain and dependency lockfile; license PatchPulse under Apache-2.0 and retain upstream notices.
- Use jiff calendar dates and ban chrono/time throughout the resolved dependency graph.
- Add strict TOML defaults, configuration-relative paths, collector switches, bind validation and mutually exclusive CLI actions.
- Add KB and stable update identities, source provenance, category union, severity risk ordering, coherent installation dates and stale/readiness rules.

### Observability

- Add JSON/pretty tracing with jiff timestamps and optional append-only file output.
- Export collector histograms/counters and inventory/freshness/reboot gauges with Prometheus scrape and alert examples.
- Count HTTP route/status responses with bounded atomics and format coherent collector samples outside the registry lock.

### Windows collection and backend execution

- Add native WMI installed, WUA installed/pending and optional embedded PowerShell installed/pending collection; preserve non-KB identities and actual query errors.
- Use official COM interfaces and read-only cached-catalog searches without downloading, installing, hiding or approving patches.
- Run backends concurrently with deadlines, per-backend single-flight, cooperative cancellation, and actual worker-completion timestamps.
- Bound process output and execution/drain time; contain assigned helpers in private Windows jobs. Decode borrowed envelopes and strict typed rows.
- Provide explicit non-Windows Unsupported outcomes and opt-in live Windows integration tests.

### Snapshots and scheduling

- Collect immediately once, then on a configurable skip-on-missed-tick interval.
- Retain failed-source data/timestamps, apply healthy or empty batches, and reconcile installed/pending duplicates.
- Assemble immutable snapshots, encoded JSON and row boundaries in the background; publish one coherent view without blocking readers on assembly.
- Reuse prior inventory/query buffers on failure-only cycles; expose accurate readiness, stale/error and reboot state.
