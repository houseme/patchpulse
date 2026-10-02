# Changelog

Changes are grouped by functional area. Each functional commit adds its own entry.
Unreleased entries do not announce a published release.

## [Unreleased]

### Completion review: runtime and HTTP contracts

- Add read-only startup token elevation and enabled-administrator diagnostics without disabling HTTP for restricted accounts.
- Return structured JSON 408 errors, cancel timed-out handlers and count timeout responses; exercise all-route HEAD and duplicate-query contracts.

### CSV inventory export

- Add read-only GET/HEAD CSV exports with installed/pending selection and inclusive since filtering, strict queries and attachment headers.
- Prepare RFC 4180 UTF-8 CSV and row spans during publication; share full bodies, copy filtered rows and retain buffers after collection failures.
- Escape spreadsheet formula cells and preserve commas, quotes, Unicode and multiline text; verify export contracts without external programs.

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

### HTTP queries and performance

- Add GET/HEAD health, readiness, version, installed/pending list, summary and optional metrics endpoints.
- Validate status and inclusive RFC3339 since filters, reject bad/unknown/duplicate fields, and return structured 400/404/405 errors with configured 408 timeout behavior.
- Share complete JSON buffers and copy only selected encoded rows for filters; prepare summary indexing once per publication and keep handlers read-only.
- Add real-router release benchmarking and contract regressions for schema, Unicode escaping, cached filtering and absence of handler-triggered collection.

### Configuration and service lifecycle

- Add foreground/configuration/liveness CLI dispatch and native SCM entry/control/status handling.
- Share a bounded two-worker/eight-blocking-worker runtime, propagate signal errors, request cancellation once, and cap HTTP/runtime shutdown.
- Use a current-thread runtime for liveness probes; exercise JSON/pretty file logging with isolated foreground processes.
- Add administrator installation/uninstallation with absolute quoted --service paths, delayed startup, recovery configuration and preserved files/logs.

### Packaging and continuous verification

- Add a non-root Linux image with required runtime libraries and project/dependency/Rust/system notices, built-in health checks and hardened Compose configuration.
- Provide a Windows runtime recipe and optional offline build cache of public locked crate archives/indexes.
- Add Linux/macOS/Windows CI and opt-in Windows checks, plus real-image endpoint/hardening/structured-log/SIGTERM smoke verification.
- Linux backends report Unsupported and readiness remains 503; Windows container and native operational acceptance remain separate gates.

### Rust development tooling

- Replace all three Python helpers with cargo xtask commands for dependency policy/notices, public offline Docker cache preparation and real-image smoke verification.
- Preserve deterministic upstream license text and enforce Apache-2.0 project licensing and chrono/time bans across all resolved targets.
- Stage cache updates before replacing the previous cache; exclude global configuration and credentials. Bound loopback HTTP probes and clean up temporary hardened containers.
- Keep tooling and HTTP client dependencies in development targets; update CI to exercise tool tests with --all-targets and run verification without Python.
- Verify 50 host tests, strict host/Windows-target Clippy, license-output parity and the rebuilt Linux ARM64 image using the Rust commands.

### Documentation, audit, and delivery

- Replace AGENT.md with an English guide for actual modules, current invariants, validation commands and functional-commit conventions.
- Trace F1-F9, all 44 implementation steps and 12 acceptance criteria, original-proposal corrections and deferred roadmap items in the completion audit.
- Retain measured 800-record ABBA handler evidence, including higher one-time publication cost and added query-buffer memory; this does not prove network throughput or native collector speed.
- Verify 39 host tests and host/Windows-target Clippy, documentation, licenses/bans/advisories and Linux Docker behavior; live Windows collection/SCM/jobs/MSVC/container/resource targets and remote CI remain pending.
