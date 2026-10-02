# Requirements and Task Completion Audit

Audited against the complete historical `PatchPulse Combat Solutions.md`, the
original eight specifications and the user-approved Tasks 09-12 extensions,
implementation paths, tests, deployment recipes and evidence. Audit date:
2026-10-02 (Asia/Shanghai). Extension implementation is tracked below as it lands.

## Conclusion

F1-F9 and the approved v0.2-v0.5 extensions have implementation paths. This is
**not full production acceptance**: real Windows Server collection, SCM/job behavior,
Windows containers, resource limits, deployed multi-host and OTLP collector
operation remain unverified. The Windows MSVC release link and remote CI passed
on GitHub-hosted runners. Implemented
flags below refer to code and local behavior; acceptance refers to actual evidence.

This audit adds direct checks for simultaneous backend execution, non-Windows behavior of all five modes, read-only handlers, process output overflow, and active process cancellation. Backend success timestamps are now captured in the blocking worker before asynchronous result/logging delays.

## Functional requirements from section 2.2.1

| ID | Requirement | Status | Implementation | Evidence and limits |
| --- | --- | --- | --- | --- |
| F1 | Installed KB/description/installation observation | Implemented; live Windows acceptance pending | collector/windows_native.rs; collector/powershell.rs; domain/patch.rs | Domain/parser tests; Windows target compilation. Unknown installation times remain null, never inferred from publication dates. |
| F2 | Pending KB/title/category/severity | Implemented; catalog/platform acceptance pending | collector/windows_native.rs; scripts/query-patches.ps1 | Typed fixtures and severity/category regressions. Search uses the host local WUA catalog; freshness depends on Windows Update/WSUS. |
| F3 | Read-only HTTP health/readiness/list/summary | Implemented; host contracts verified | api/mod.rs; cache/mod.rs; tests/api_contract.rs | Seven endpoints, inclusive since/status, bad queries, write rejection, stable schema, cached filtering and no handler-triggered collection. |
| F4 | Prometheus metrics | Implemented; host verified | observability/mod.rs; config/prometheus.yml; config/alerts.yml | Histograms, labels, failure counters, bounded status counters, freshness/reboot gauges and disabled route. |
| F5 | Configuration and CLI | Implemented; host verified | config.rs; main.rs; config/patchpulse.toml | Defaults, unknown keys, duration/bind/log validation, action conflicts, path base and bind override. |
| F6 | Windows service registration | Implemented; live SCM acceptance pending | service.rs; scripts/install-service.ps1; scripts/uninstall-service.ps1 | Generated SCM entry, absolute quoted --service registration, delayed auto-start, recovery, Stop/Shutdown and error exit codes compile. Actual install/reboot/recovery not executed. |
| F7 | Failure retention and stale state | Implemented; host verified | cache/mod.rs; scheduler/mod.rs; tests/snapshot_pipeline.rs | Initial/all/partial failure, empty success, recovery, readiness retention, shared data/bodies and transactional publication. |
| F8 | Collector backend switches | Implemented; host verified | config.rs; collector/mod.rs | Five independent mode switches; all-disabled configuration fails; every enabled Windows backend reports Unsupported on other hosts. |
| F9 | Structured JSON logging | Implemented; Docker verified; SCM sink acceptance pending | observability/mod.rs; config/patchpulse.toml | Validated filter, JSON/pretty output, jiff timestamps and optional file sink; logging_contract.rs captures both formats on Unix. Docker JSON logs are parsed; live SCM rotation/file permissions remain operational gates. |

## Every task implementation step

A step can have implemented code and still require platform acceptance. Step numbers refer to the existing English task files.

| Task/step | Status | Evidence |
| --- | --- | --- |
| 01.1 | Verified | Cargo.toml, Cargo.lock, rust-toolchain.toml, LICENSE; library and binary build. |
| 01.2 | Verified at recorded version check | dependency-versions.json; newest direct releases recorded, upstream transitive pins retained. This is not a permanent newest-version guarantee. |
| 01.3 | Verified | deny.toml and full-target metadata contain no chrono/time; jiff calendars and std elapsed-time primitives. |
| 01.4 | Verified | config.rs, main.rs, config_contract.rs; foreground/service/probe dispatch. |
| 01.5 | Verified | One configuration-relative base, strict validation and configuration contract tests. |
| 02.1 | Verified | PatchRecord states and existing JSON fields; added source/date metadata. |
| 02.2 | Verified | KB parser and WUA identity/revision; WMI non-KB identity is also preserved. |
| 02.3 | Verified | Borrowed reconciliation, severity risk order, category union, provenance, coherent installation observation. |
| 02.4 | Verified | Jiff RFC3339/CIM/FILETIME and civil-date/non-ASCII/malformed-input tests. |
| 02.5 | Verified | Stable KB sort, dedupe and installed-over-pending pipeline regression. |
| 03.1 | Implemented; live gate pending | Five backends and both PowerShell modes; windows_collectors.rs is explicitly opt-in. |
| 03.2 | Implemented; compile verified | Apartment guard and official generated Windows COM bindings; live COM behavior not claimed. |
| 03.3 | Implemented; live gate pending | Only WUA Search/getters and SystemInfo; no downloader/installer APIs invoked. |
| 03.4 | Verified by fixtures/source | spawn_blocking, independent deadlines, barrier-style concurrency regression and retained single-flight guard. |
| 03.5 | Implemented; mixed evidence | Embedded trusted script, UTF-8, typed singleton/array/null/envelope handling, process supervision tests. Windows job containment needs real Windows evidence. |
| 03.6 | Verified | Explicit Unsupported for all five modes, coverage flags separated from backend errors/success. |
| 04.1 | Verified | Concurrent orchestrator and one scheduler publisher; no API writer. |
| 04.2 | Verified | Failed source keeps its prior batch/time; partial failure regression. |
| 04.3 | Verified | Empty success establishes readiness and clears only its own source. |
| 04.4 | Verified | Staged backend state and immutable coherent data/JSON publication; old views remain readable. |
| 04.5 | Verified | Readiness retained, stale/error/clock rollback tests. Native performance acceptance is separate. |
| 04.6 | Implemented; host behavior verified | Immediate/skip interval, no duplicate startup, cancellation and Docker SIGTERM; live SCM stop still pending. |
| 05.1 | Verified | All seven API routes and GET/HEAD Docker checks. |
| 05.2 | Verified | Status and inclusive RFC3339 since; invalid/duplicate/unknown keys produce JSON errors. |
| 05.3 | Verified | Handlers consume one immutable view and do not collect; summary exposes all documented fields. |
| 05.4 | Verified | JSON 404/405/408, bounded labels and the shared production TimeoutLayer. An indefinitely delayed handler is cancelled, produces JSON 408 and increments the exact HTTP counter. All route HEAD and duplicate-query contracts are tested. |
| 05.5 | Verified | api.md documents scope/date-only filtering, initialization/degradation and schemas. |
| 06.1 | Implemented; Docker JSON verified | JSON/pretty/file-sink builder and jiff clock. Both JSON and pretty file-sink output are exercised by isolated foreground processes on Unix; SCM permissions remain a Windows gate. |
| 06.2 | Verified | Collector paths emit logs and duration/success/failure telemetry, including Busy and Unsupported. |
| 06.3 | Verified | Grouped cumulative histograms and all required gauges/counters; no unbounded URI labels. |
| 06.4 | Verified | Disabled metrics route contracts; scrape and alert examples present. |
| 06.5 | Verified documentation boundary | Windows RSS/CPU/latency remain targets, not achieved results. |
| 07.1 | Implemented; compile verified | SCM dispatcher and StartPending/Running/StopPending/Stopped statuses. |
| 07.2 | Implemented; compile/host checks | Watch shutdown, shared runtime, failure exit, single cancellation; live control events pending. |
| 07.3 | Implemented; scripts need live Windows acceptance | Administrator scripts, canonical absolute quoted paths, delayed startup/recovery; no overwrite without uninstall. |
| 07.4 | Verified by source | New-Service uses --service and configuration validation uses --check-config. |
| 07.5 | Documented | LocalSystem/ACL/log sink/firewall/WSUS/native deployment limitations in deployment.md and AGENT.md. |
| 07.6 | Implemented; cross-platform contract verified | Read-only token/elevation diagnostics run on a startup blocking worker; unknown/restricted privileges warn without disabling HTTP. Real Windows token behavior remains a live gate. |
| 08.1 | Verified locally | Fmt, Clippy, tests, rustdoc, licenses/bans, cached advisory checks. Offline provenance recorded. |
| 08.2 | Verified on GitHub-hosted runners; live collectors pending | [Run 37002534605](https://github.com/houseme/patchpulse/actions/runs/37002534605) passed Linux/macOS/Windows, dependencies and Docker jobs, including the MSVC release link. Opt-in live Windows collector tests were not run. |
| 08.3 | Verified Linux ARM64 | Actual Docker build: non-root scratch runtime, notices, built-in probe, explicit bind. |
| 08.4 | Recipe provided; build/runtime pending | Dockerfile.windows requires Windows daemon and staged MSVC executable; no host-inventory guarantee. |
| 08.5 | Verified Linux ARM64 | cargo xtask smoke-docker validates original HTTP endpoints, CSV, agent snapshot, 503 readiness, hardening and SIGTERM; smoke-hub-docker verifies both roles, machine identity, stale/unknown baseline and cleanup. |
| 08.6 | Verified | cargo xtask dependency-policy inventory and upstream/Rust/system-library notices; normalized output matches the previous generator. |
| 08.7 | Verified | requirements-audit.md, review-refactor.md and validation.md distinguish code, host checks and live gates. |

## Every approved extension step

Step numbers follow the English specifications in docs/tasks/09-12.

| Task/step | Status | Evidence and remaining limit |
| --- | --- | --- |
| 09.1 | Verified | Agent GET/HEAD CSV route, strict format/status/since queries and JSON errors; export_contract.rs. |
| 09.2 | Verified | UTF-8 RFC 4180 rows and spans prepared in immutable publication; cached filtered rows retain stable order/schema. |
| 09.3 | Verified | Potential spreadsheet formulas gain an apostrophe; quotes, CRLF, Unicode and multiline text round-trip. |
| 09.4 | Verified | Content type, attachment header and structured invalid-query/write rejection through the real router. |
| 09.5 | Verified | Empty, pending, date-filtered, retained-after-failure, HEAD and unsupported-host CSV contracts. |
| 10.1 | Verified | Explicit agent/hub mode, unique static IDs/URLs, bounded durations, bytes and concurrency; hub_contract.rs. |
| 10.2 | Verified | Versioned coherent AgentSnapshot endpoint publishes backend diagnostics and evaluated freshness. |
| 10.3 | Verified locally | Pooled HTTP(S) client, disabled redirects/proxies, bounded requests, original-data retention and untrusted TLS rejection; remote deployment pending. |
| 10.4 | Verified | One immutable machine-keyed publication, agents/detail/fleet routes and no cross-host KB deduplication. |
| 10.5 | Verified locally | First real source success establishes historical readiness, stale nodes remain visible, and shutdown cancels an active cycle. |
| 10.6 | Verified locally | Two real loopback agents, full Hub runtime, partial failure/recovery, size budgets, cancellation and Linux two-container image smoke. Live multi-host operations remain external. |
| 11.1 | Verified | Disabled default and strict OTLP endpoint, identity, ratio, timeout and queue validation. |
| 11.2 | Verified | Official OpenTelemetry SDK/OTLP/tracing integration preserves JSON/pretty logs and Prometheus independently. |
| 11.3 | Verified locally | Bounded HTTP/collector/hub spans, W3C incoming parent and outbound propagation; OTLP payload excludes sensitive inventory/query text. |
| 11.4 | Verified locally | SDK worker, 1 MiB batches, one bounded retry, five-second flush; export failure leaves HTTP/readiness intact and emits sanitized errors. |
| 11.5 | Verified locally | Real OTLP/HTTP JSON reception, identity/context/linkage, disabled mode and debug-log response-body privacy. External collector acceptance remains pending. |
| 12.1 | Verified | Named exact-KB baseline normalization, deduplication and enabled-empty rejection. |
| 12.2 | Verified | Immutable agent comparison reports installed/missing/pending lists and counts. |
| 12.3 | Verified | Initial, failed and expired input is unknown; fresh missing requirements are non_compliant. No supersedence inferred. |
| 12.4 | Verified | Per-agent and fleet conclusions preserve identity and conclusive fresh non-compliance alongside unknown peers. |
| 12.5 | Verified | Real-router compliant/non-compliant/unknown, disabled/write, partial-failure and actual two-agent contracts. |

## Acceptance criteria review

| Task | Acceptance status |
| --- | --- |
| 01 | Host/configuration and license checks met; live versions are recorded snapshots. |
| 02 | Domain unit coverage met. |
| 03 | Fixtures and Windows target compilation met; live Windows collection remains pending. |
| 04 | Failure, empty-success, recovery, atomicity, readiness, concurrency and single-flight coverage met; actual Windows timing remains pending. |
| 05 | Endpoint/filter/error/readiness/metric contracts met, including cancellation/JSON 408/exact timeout counters and all-route HEAD behavior. |
| 06 | Histogram/label/failure/disabled-route checks met; live resource measurements and SCM log permissions pending. |
| 07 | Target compilation met; actual install/start/stop/reboot/recovery not met. |
| 08 | Local Linux image and remote CI passed; Windows image and target-server runtime acceptance remain pending. |
| 09 | CSV contracts and cached export path verified; no live Windows CSV data claim. |
| 10 | Hub code, real HTTP/TLS contracts and Linux two-role image verified; deployed multi-host and Windows acceptance pending. |
| 11 | Real local OTLP delivery, privacy and failure isolation verified; external collector and Windows acceptance pending. |
| 12 | Agent/fleet comparison and three-valued compliance verified; real Windows inventory comparison pending. |

## Historical proposal sections and intentional corrections

| Source section | Disposition |
| --- | --- |
| 1.1 language/license/MSRV | Superseded by user requirements: edition 2024, current pinned toolchain, Apache-2.0 project only and retained upstream licenses. |
| 1.4 no_std-style domain | Domain is platform-independent and testable; an actual no_std build is not implemented or required by F1-F9. |
| 2.3.2 different installed/pending intervals | Resolved using the explicit shared interval in 2.11. Separate intervals remain a future design option. |
| 2.4 old crate versions/date library/metrics stack | Current crates and jiff replace obsolete examples; local metrics registry preserves metric names. |
| 2.5 file layout | Equivalent responsibilities implemented with consolidated module files; old illustrative file names are not deliverables. |
| 2.6 domain merge | Semantics improved with severity priority, coherent dates, combined categories and source provenance. |
| 2.7 WMI coverage | CBS/QFE is a subset, not a guaranteed exclusion of every LCU/SSU. WUA catalog also has coverage/freshness limits. |
| 2.7 WUA skeleton | Implemented native interfaces and optional embedded PowerShell COM path; no placeholder collector remains. |
| 2.8-2.9 snapshots/scheduler | Implemented; immutable transactional publication replaces long-held snapshot locks. |
| 2.10 HTTP | Seven endpoints, queries, coverage and version fields present. No extra export route is part of F1-F9. |
| 2.11-2.12 config/assembly | Implemented safe defaults, validation, paths, CLI actions and bounded shared runtime. |
| 2.13 observability/alerts | Required metric names and JSON logging present; practical increase-over-hour alert replaces the implausible per-second failure threshold for 30-minute collection. |
| 2.14 foreground service example | Corrected to real --service SCM registration; native-host deployment recommended. Firewall allowlisting is an operator action, not an automatic network mutation. |
| 2.15 safety | Loopback, trusted configuration scripts, read-only built-in queries, dependency audit and no full command/script logging are implemented. There is no auth/TLS server or arbitrary-command HTTP endpoint. |
| 2.16 degradation | Retention, partial/initial/all failure, historical readiness and stale state are covered; 408 layer has source evidence. |
| 2.17 resource/QPS claims | Under-30-MB RSS, under-5-percent collection CPU, idle CPU, WMI under-5-s and PowerShell under-20-s remain unmeasured on Windows. Handler microbenchmarks do not prove these targets or network QPS. |
| 2.18 tests | Host domain/contracts/pipeline/process tests and opt-in Windows tests exist; ignored or cfg-excluded tests are not counted as passed Windows execution. |
| 2.19 privilege preflight recommendation | Implemented read-only startup token elevation and enabled-administrator diagnostics. Restricted/unobservable tokens warn; collection errors remain authoritative for WMI/WUA access. Installer requires admin and defaults to LocalSystem. |
| Part III AGENT.md | Replaced by an English guide with actual paths, current invariants, validation and delivery rules. |
| Part IV four-week rollout | Code artifacts exist; its Windows operational/performance acceptance cannot be claimed complete from macOS checks. |

## Roadmap and exclusions

| Item | Status |
| --- | --- |
| v0.2 CSV export | Implemented; prepared UTF-8 CSV, filters, formula escaping, failure retention and actual router contracts pass (Task 09). |
| v0.3 agent/hub aggregation | Implemented bounded HTTP(S) configured-agent polling, per-host retention/readiness/metrics and actual HTTP/TLS-rejection contracts; Task 10 supersedes the earlier non-goal. |
| v0.4 OpenTelemetry export | Implemented opt-in OTLP/HTTP JSON traces with bounded SDK worker, W3C propagation, sanitized failures and independent logs/metrics; a local collector receives real batches. External collector deployment remains pending. |
| v0.5 compliance baseline comparison | Agent and fleet exact-KB comparison implemented; missing and source/transport-stale observations yield unknown per-agent compliance. |
| Patch download/install and Web UI | Explicit non-goals; intentionally absent. |

## Required external acceptance evidence

1. Execute WMI/WUA/PowerShell and private-job cleanup on actual Server 2016/2019/2022 under the intended account, including denied permissions and cached-catalog behavior.
2. Build/link MSVC, install/start/stop/reboot the service, verify recovery and writable log destination/ACLs.
3. Build/run the Windows container on a compatible daemon and record WMI/WUA availability; validate the Linux image separately.
4. Measure Windows RSS/idle and collection CPU/latency with realistic inventory. Record network throughput separately from in-process CPU benchmarks.
5. Complete the [manual Windows Server guide](windows-server-manual-validation.md) on each target release. GitHub-hosted CI is recorded in the validation report and cannot replace these checks.

## Validation provenance

The audit is based on current source, host regression checks, Windows-target compilation and actual local Linux Docker behavior. Existing advisory provenance remains the verified snapshot recorded in validation.md; offline checks do not establish current online database freshness. Each rewritten functional commit receives a cumulative Changelog entry and an archived-tree build check. The old commit remains recoverable through houseme/backup-65866b7. Local rewriting does not rewrite origin/main.

Completion checkpoint: 70 macOS ARM64 host tests pass: 24 library tests,
6 API, 3 baseline, 2 CSV, 8 Hub, 2 collector, 2 configuration, 1 logging,
3 OTLP, 8 snapshot pipeline and 11 Rust tool tests. The first full test run
was blocked by the sandbox's loopback bind restriction; the same suite passed
with local loopback access. Host Clippy, upstream license notices, offline
cargo-deny and cargo-audit against official RustSec commit
`117edb3bed98e9be112f277b7615eea3252e7c43` pass. Windows target and
final SDK-inclusive image checks are recorded in completion-validation.md.

Final Linux image patchpulse:0.1.0 (arm64) passed Rust structured-log,
Agent endpoint/CSV/healthcheck and two-role Hub/TLS packaging verification.
It includes CA data and upstream notices; both roles passed hardened SIGTERM
checks. Image ID:
`sha256:f2939dc7507d64da140dd16e29816f16a2839c714ab75bb54d9b847a306fc7b1`.

## Rewritten functional history

The old aggregate commit remains reachable on `houseme/backup-65866b7`. For a
repository that allows rebase/squash but not merge commits, the 18 reviewed
functional trees were replayed in order on top of that aggregate commit. Each
replayed tree matches its original reviewed counterpart exactly; the first
commit folds removal of the superseded aggregate into the licensed foundation.
GitHub Actions runs on the PR branch before integration.

| Sequence | Commit | Functional boundary |
| --- | --- | --- |
| 1 | `19d3fa32c5` | build: replace aggregate with licensed Rust foundation |
| 2 | `3efb47eb09` | feat(observability): add structured logs and bounded Prometheus metrics |
| 3 | `631c7f3d65` | feat(collector): collect Windows updates with bounded backend execution |
| 4 | `8a7ba97872` | feat(snapshot): publish prepared inventories with failure retention |
| 5 | `653c809e27` | feat(api): expose cached patch queries and stable HTTP contracts |
| 6 | `e40e181cb0` | feat(service): add foreground and Windows SCM lifecycle |
| 7 | `7aef968cda` | ci: package hardened Docker images and validation workflows |
| 8 | `ca091ff8a3` | English agent guide, comprehensive requirement audit and cumulative Changelog |

## Subsequent functional commits

The original aggregate commit remains recoverable on the backup reference.
These later commits add independently reviewable capabilities. The separate
validation commit records the final image and exact branch status.

| Sequence | Commit | Functional boundary |
| --- | --- | --- |
| 9 | `2b8ebc522c` | refactor(tooling): replace Python helpers with Rust xtask |
| 10 | `dc9c204ee3` | feat(runtime): privilege preflight and complete HTTP timeout contracts |
| 11 | `e96c816fcf` | feat(export): prepared CSV patch inventories |
| 12 | `91bb4d9272` | feat(baseline): compare configured KB requirements |
| 13 | `7dca788c09` | feat(hub): bounded machine-scoped Agent/Hub aggregation |
| 14 | `dfccf33340` | fix(baseline): preserve conclusive fleet non-compliance |
| 15 | `9e9ae8c001` | feat(telemetry): bounded OTLP/HTTP trace export |
| 16 | `ec01d3d298` | ci(validation): final local Agent/Hub image and dependency evidence |
| 17 | `c5a6948810` | ci(windows): MSVC release executable link check |
| 18 | `2ca4efccd5` | docs(validation): Windows Server acceptance guide and remote CI evidence |
