# Changelog

Changes are grouped by functional area. Unreleased entries describe the current
implementation and do not announce a published release.

## [Unreleased]

### Patch collection and reconciliation

- Add read-only installed-patch collection through Windows WMI and native WUA,
  and pending-patch collection through native WUA.
- Add optional PowerShell collectors with an embedded WUA script and no
  PSWindowsUpdate module requirement.
- Normalize KB identifiers and preserve stable identities for updates without
  KB numbers, including non-KB WMI records.
- Merge complementary metadata deterministically, retain source provenance,
  combine categories, and preserve the highest reported severity.
- Use jiff for installation dates and timestamps, including RFC3339, CIM offsets,
  and FILETIME. Preserve ambiguous date strings without guessing their timezone
  or date order, and merge installation fields as one coherent observation.
- Search the local WUA catalog without downloading or installing updates.

### Snapshots and scheduling

- Collect immediately at startup and then on a configurable interval, skipping
  missed ticks without duplicate startup collection.
- Run backends concurrently with independent timeouts and single-flight
  protection for blocking calls that continue after a timeout.
- Retain each failed backend's last successful inventory while healthy backends
  refresh; treat successful empty results as authoritative.
- Publish immutable snapshots transactionally and give installed records
  precedence over pending duplicates.
- Expose backend freshness, failures, readiness, and reboot requirements;
  conservatively mark future observations stale after a clock rollback.

### HTTP queries and performance

- Add `/health`, `/ready`, `/version`, `/patches`, `/patches/pending`,
  `/patches/summary`, and `/metrics` endpoints.
- Support status and inclusive RFC3339 `since` filters with structured query,
  unknown-route, and unsupported-method errors.
- Prepare JSON and encoded row boundaries in the background. Full lists share
  cached buffers; filters reuse encoded records without copying patch metadata
  or repeatedly serializing it.
- Prepare latest-installation indexing and coverage once per snapshot, and
  serialize summary fields directly from immutable data.
- Reuse inventory and query buffers on failure-only collection cycles.
- Record release-mode before/after measurements with an 800-record ABBA
  real-router benchmark. These measure handler/body production rather than
  network throughput or Windows collection speed; results and publication
  cost/memory tradeoffs are documented in [the review](docs/review-refactor.md).

### Observability

- Add structured JSON and pretty logs with jiff timestamps and optional file
  output for Windows service mode.
- Export collector duration histograms, success/failure counters, snapshot age,
  installed/pending counts, stale state, reboot state, and HTTP response counts.
- Use bounded route/status atomic counters without allocation or registry locks
  on requests; format coherent histogram snapshots outside the collector lock.
- Add Prometheus scrape configuration and alert examples, and support disabling
  the metrics endpoint.

### Configuration and service lifecycle

- Add strict TOML configuration, safe loopback defaults, validated durations,
  collector switches, bind overrides, and configuration/liveness CLI probes.
- Resolve relative script and log paths against the configuration file and
  reject conflicting primary CLI actions.
- Add native Windows SCM dispatch and start/stop/shutdown status handling, plus
  administrator installation and uninstallation scripts with delayed startup
  and recovery configuration.
- Share a bounded runtime between foreground and SCM modes, propagate signal
  listener errors, cancel collection once, and bound HTTP draining and runtime
  shutdown. Use a current-thread runtime for liveness probes.
- Bound PowerShell output and include pipe draining in the collection deadline;
  contain assigned helper processes in a private Windows job.

### Packaging, licensing, and verification

- Add a multi-stage Linux Docker image with a non-root runtime, required dynamic
  libraries, binary health checks, and a hardened Compose example.
- Add a Windows runtime image recipe and an optional offline Docker dependency
  cache containing only public locked crate archives and registry indexes.
- License PatchPulse exclusively under Apache-2.0 and retain dependency, Rust,
  and system-library notices. Add automated license/notice checks and bans on
  the chrono and time crates.
- Add Linux/macOS/Windows CI, opt-in live Windows collector checks, regression
  and HTTP contract tests, and real-image Docker smoke verification.
- Add English implementation tasks, architecture/API/deployment documentation,
  review findings, benchmark evidence, and explicit validation records.
- Verify 33 host tests, strict host/Windows-target Clippy, documentation,
  dependency policies, advisory checks, and Linux Docker smoke behavior.
  Live Windows collection, SCM/job behavior, MSVC linking, and Windows resource
  targets remain release gates rather than claimed validation.
