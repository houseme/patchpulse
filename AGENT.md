# AGENT.md

Read this file before changing PatchPulse. It describes the current implementation,
not the illustrative code in the historical combat-solutions proposal.

## Purpose and constraints

PatchPulse is a read-only Windows patch health agent with an optional configured
aggregation hub. Agents collect installed and pending updates, publish immutable
snapshots and serve HTTP queries and Prometheus metrics. Hubs poll configured
agents, preserve per-machine identities/failures and publish immutable fleet
views. Agent production targets are Windows Server 2016/2019/2022 x86_64;
Windows 10/11 are potential extensions. Domain, API, and fixture tests run on
Linux/macOS; Windows backends return explicit Unsupported errors there.

- Use Rust edition 2024 and the pinned Rust 1.98.1 toolchain.
- License PatchPulse only under Apache-2.0. Preserve upstream dependency notices.
- Use jiff for calendar dates and timestamps. Never add chrono or time crates,
  including through transitive or Windows-only dependencies. Standard-library
  Instant and Duration remain appropriate for elapsed time and scheduling.
- Keep all new code comments and task/implementation documentation in English.
- Check direct dependency versions against current registry releases; RC/beta
  versions are allowed. Preserve upstream compatibility constraints and commit
  Cargo.lock. Do not fork upstream packages merely to bypass a version pin.
- Never download, install, approve, hide, or remove updates. Do not expose command
  execution or collection triggers through HTTP.

## Architecture and ownership

Dependency direction: `main/service -> app -> api/scheduler -> cache/collector ->
domain`. The domain layer uses serde, jiff, and the standard library without
platform APIs or HTTP/runtime dependencies.

| Module | Responsibility |
| --- | --- |
| src/main.rs | CLI dispatch, configuration, logging, foreground/probe startup |
| src/app.rs | Application assembly, bounded runtime, shutdown and liveness probe |
| src/config.rs | Strict TOML defaults, validation, CLI actions and path resolution |
| src/domain/ | Patch identity, coherent metadata merge, dates and snapshot rules |
| src/collector/traits.rs | Synchronous Collector contract, batches and errors |
| src/collector/orchestrator.rs | Concurrent workers, timeout/single-flight, telemetry |
| src/collector/windows_native.rs | Official generated WMI/WUA COM interfaces |
| src/collector/powershell.rs | Trusted command construction and typed JSON parsing |
| src/collector/process.rs | Output limits, execution/drain deadline and process jobs |
| src/cache/mod.rs | Transactional assembly and immutable prepared publication |
| src/export.rs | Prepared spreadsheet-safe CSV bodies and complete row spans |
| src/hub.rs | Bounded configured-agent polling and immutable machine-scoped fleet state |
| src/net.rs | Outbound certificate-validating TLS client policy |
| src/scheduler/mod.rs | Immediate/periodic ticks; sole snapshot publisher |
| src/api/mod.rs | Read-only handlers, cached JSON, filters and contracts |
| src/observability/mod.rs | Tracing, bounded HTTP counters and Prometheus formatting |
| src/observability/telemetry.rs | Optional bounded OTLP/HTTP JSON exporter and W3C context propagation |
| src/service.rs | Windows SCM dispatch and service control/status lifecycle |
| src/preflight.rs | Read-only token elevation and enabled-administrator startup diagnostics |
| examples/xtask/ | Rust development commands for dependency policy, offline cache and Docker verification |

Only scheduler::tick_once calls SnapshotStore::publish. A writer mutex protects
staged backend data while a watch channel publishes one coherent view. Prepare
aggregation, encoded JSON, row boundaries, and latest-installation indexing on a
blocking worker. Readers must not wait for assembly. Preserve the last successful
data of failed backends; successful empty batches replace their own data.

HTTP handlers read one immutable view, never collect patches, and must preserve
JSON field names and endpoint paths. Full lists share prepared buffers; filters
copy encoded rows from the same publication. Summary freshness is evaluated at
request time rather than cached indefinitely.

## Collection and lifecycle invariants

- Invoke blocking WMI/COM/process work through spawn_blocking. COM initialization,
  interface use/drop, and cleanup stay on the same worker thread.
- Use official windows bindings. The implementation does not use the wmi crate
  or manually declared COM vtables.
- Keep at most one active blocking worker per backend. A timeout does not kill
  an in-progress COM call; retain the single-flight guard inside that worker.
- Return Result with backend/operation context for recoverable failures. Do not
  use unwrap/expect in recoverable production paths; they are acceptable in tests.
- PowerShell comes from the system executable path and a trusted embedded script
  by default. Configured scripts must be trusted local files; never derive a
  script or command from an HTTP request.
- Drain stdout/stderr concurrently and limit each to 8 MiB. The timeout covers
  startup, execution, and draining. Windows jobs contain assigned helper processes.
- Record each backend's actual completion timestamp before asynchronous logging
  or result aggregation delays.
- Collect once immediately, skip missed interval ticks, and avoid duplicate
  startup collection. The current design uses one shared interval.
- Foreground and SCM modes share two runtime workers and at most eight blocking
  workers. HTTP draining is bounded to ten seconds and runtime shutdown to five.
- Propagate signal failures, request cancellation once, and report SCM Running
  only after HTTP bind. Service errors use a nonzero service-specific exit code.

## Configuration and platform behavior

Default bind: 127.0.0.1:9100. Native WMI installed and WUA installed/pending
collectors are enabled; both PowerShell modes are optional. WUA queries the local
cached catalog, whose freshness depends on the host's Windows Update/WSUS policy.
Configured coverage flags do not imply complete or successful inventory coverage.

Resolve relative script/log paths against the configuration file's directory.
Windows services normally start in System32, so installation uses absolute binary
and configuration paths. Use --service for SCM registration, not --foreground.
The installer requires administrator rights and refuses to overwrite a service.
LocalSystem is the default service account; alternate accounts need explicit
permissions. Protect configuration/scripts with administrator-controlled ACLs.

Use stdout logging in read-only containers and a writable configured log file
for SCM deployments. External log rotation, firewall rules, authentication, and
TLS proxies are deployment responsibilities. Keep loopback binding unless the
operator explicitly configures monitored network exposure.

## HTTP contracts

| Method | Path | Expected status |
| --- | --- | --- |
| GET/HEAD | /health | 200 |
| GET/HEAD | /ready | 200 after any successful batch, otherwise 503 |
| GET/HEAD | /version | 200 |
| GET/HEAD | /patches | 200 |
| GET/HEAD | /patches/pending | 200 |
| GET/HEAD | /patches/summary | 200 |
| GET/HEAD | /metrics | 200, or 404 when disabled |

Lists accept status and inclusive RFC3339 since filters. Date-only/unknown
installation instants are excluded by since. Reject invalid/duplicate/unknown
query fields with JSON 400, unknown routes with JSON 404, and write methods with
JSON 405. The configured handler timeout returns 408. Historical readiness is
not freshness: consumers must also inspect is_stale, backends, and last_error.

## Observability and tests

Pair collector success/failure logs with duration histogram and success/failure
counters. Use structured tracing fields; do not log complete script contents or
command lines. JSON logs use jiff timestamps. HTTP labels must stay bounded;
unknown paths map to unmatched. Format coherent histogram snapshots outside the
collector registry lock and keep metric families grouped.
The optional OTLP exporter uses SDK batch workers and a five-second best-effort
flush. Export only project spans with bounded route/backend/agent attributes;
never attach inventory payloads, command/script text, credentials or raw URLs.
SDK WARN/ERROR export failures stay visible while its potentially sensitive
DEBUG/TRACE response-body records are suppressed from application logs.

Required validation commands:

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
sh scripts/check-windows-gnu.sh
cargo doc --no-deps --locked
cargo deny check
cargo audit
cargo xtask dependency-policy
```

For restricted networks, cargo-deny can use its offline cache and cargo-audit can
use a verified advisory snapshot. Record the exact database provenance; never
claim a fresh online fetch from an offline check. syn 2/3 coexist because of
upstream macro constraints and should not be rewritten without evidence.

TLS also adds compatibility-bound tower-http and Windows binding generations.
HTTPS uses ring C compilation; Windows checks on other hosts require a C cross
compiler. The local check uses existing Zig wrappers; native Windows CI uses its
installed compiler. Cross compilation still does not establish Windows execution.

Add focused tests for changed domain rules and HTTP contracts. Collector changes
must retain partial-failure, empty-success, timeout/single-flight, and cancellation
behavior. Do not count zero tests or cross-compilation as live Windows validation.
Development tasks use the `cargo xtask` alias and stay outside the production
binary/image. See docs/rust-tooling.md for cache and Docker smoke commands. Include
example tests with --all-targets; no Python installation is required by these tasks.
Run live collector tests on a real Windows host:

```sh
cargo test --locked --test windows_collectors -- --ignored --nocapture
cargo build --release --locked --target x86_64-pc-windows-msvc
```

SCM/job/container compatibility and Windows memory/CPU/latency goals require
recorded Windows execution evidence. Use release-mode controlled benchmarks for
performance claims; in-process handler results do not establish network QPS or
native collector speed.

## Delivery and documentation

Update code, tests, affected contracts/task specifications, and functional
CHANGELOG entries together. Split commits by coherent buildable feature boundaries
when requested; preserve uncommitted user work and verify the exact staged scope.
Append these trailers to each new commit in this order:

```text
Co-Authored-By: heihutu <heihutu@gmail.com>
Co-Authored-By: zhi22915 <qiuzgang@gmail.com>
```

Do not claim a release, tag, remote CI result, push, or Windows acceptance without
actual evidence. For an authorized history rewrite, retain a recovery reference,
preserve all working files, and report divergence from the remote branch.

Authoritative references: docs/requirements-audit.md, docs/tasks/README.md,
docs/completion-validation.md, docs/architecture.md, docs/api.md,
docs/deployment.md, docs/fleet.md and docs/telemetry.md. The earlier
docs/review-refactor.md and docs/validation.md retain historical evidence.
The historical PatchPulse Combat Solutions.md proposal
contains obsolete dependency, date-library, licensing, coverage, and foreground
service-registration examples; its F1-F9 requirements remain traceable in the audit.
CSV export, configured agent/hub aggregation, OpenTelemetry trace export and
baseline comparison are implemented under Tasks 09-12. Preserve read-only HTTP,
per-host inventory identity and explicit freshness/unknown compliance. Windows,
remote collector and production resource acceptance gates still apply.
