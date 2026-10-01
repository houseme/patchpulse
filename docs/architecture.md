# PatchPulse Architecture

PatchPulse is an Apache-2.0 read-only Windows patch health service. Rust edition 2024 and Rust 1.98.1 are used for the initial implementation. The original combat-solutions document is retained as historical design input; the implementation and English task specifications supersede its illustrative code.

## Dependency direction

`main/service -> app -> api/scheduler -> cache/collector -> domain`.

The domain layer depends only on serde and jiff plus the standard library. Handlers read one immutable prepared snapshot; they never invoke collectors. The scheduler is the only publisher. A separate writer state stages backend batches; reconciliation, JSON preparation, and indexing run on a blocking worker. A watch channel publishes one immutable view. Readers never wait for the assembly lock. Failure-only cycles share prior inventory, encoded JSON, and row offsets. Configuration validation runs before HTTP startup. Native SCM dispatch starts before service-specific configuration loading.

## Collection pipeline

1. The Tokio interval fires once immediately, then every configured interval. Missed ticks are skipped.
2. The orchestrator runs enabled backends concurrently with `spawn_blocking` and a per-backend timeout.
3. A single-flight atomic guard remains inside each blocking worker. An expired COM call cannot be killed, so subsequent cycles report Busy until the existing call finishes. At most one worker per backend remains active.
4. Successful batches replace that backend's cached data, including valid empty batches. Failed batches preserve its prior data and timestamp.
5. The scheduler publishes an atomic aggregate: deterministic KB deduplication, complementary metadata merge, source provenance, and installed-over-pending reconciliation.
6. Backend errors remain visible after partial success. Readiness means at least one successful batch has occurred; incomplete, failed, or expired backends make the snapshot stale.

## Windows backends

| Backend | Data source | Default |
| --- | --- | --- |
| wmi_installed | Win32_QuickFixEngineering via generated IWbem interfaces | Enabled |
| wua_installed | Native WUA IUpdateSession, installed catalog search | Enabled |
| wua_pending | Native WUA IUpdateSession, visible pending catalog search | Enabled |
| powershell_installed | Embedded PowerShell WUA COM script | Disabled |
| powershell_pending | Embedded PowerShell WUA COM script | Disabled |

COM initialization and cleanup occur on the same worker, after all interface references are released. PowerShell uses a system executable path and a fixed embedded script by default. Custom scripts come only from trusted local configuration. Stdout/stderr are drained concurrently and limited to 8 MiB each. One deadline includes process startup, execution, and pipe draining. The process guard cleans up failures and cancellation; on Windows, a private job contains assigned helper processes.

WUA searches the local cached catalog (`Online=false`) and does not download or install updates. It therefore depends on Windows Update/WSUS having refreshed that catalog. Native WUA does not infer installation timestamps from publication or deployment-change dates. WMI contributes known installation dates during merging.

WMI reports CBS QuickFix records rather than a universal installed-software inventory. Non-KB identifiers are preserved with a WMI prefix; missing identifiers fail collection so prior data is retained. Do not assume that a specific LCU/SSU is universally present or absent. The configured `coverage` booleans indicate enabled backends; actual success, age, errors, and in-flight state are available in `backends`.

On Linux/macOS these collectors return Unsupported. HTTP stays available, `/ready` stays 503 until a genuine collection succeeds, and metrics report failures. A Linux Docker container cannot read its host's Windows inventory.

## Identity and dates

Numeric KBs normalize to `KB<digits>`. No-KB updates use `WUA:<UpdateID>:<revision>` in native and PowerShell paths. Duplicate data merges deterministically and preserves complementary fields and all sources.

Jiff represents UTC instants and civil dates. Offset-bearing CIM/RFC3339 and FILETIME values become instants. Unambiguous date-only formats become `installed_date`; ambiguous local formats remain in `installed_on_raw` without a guessed order or timezone. Native WUA records can have null installation dates. Installation fields merge as one observation; severity uses risk order and categories preserve their combined labels. The `since` filter includes only records with known instants.

## Failure and lifecycle semantics

| Condition | Readiness | Data |
| --- | --- | --- |
| Initial failure | 503 | Empty lists, errors and stale=true |
| Empty successful collection | 200 | Authoritative empty list |
| Partial failure | 200 after any success | Healthy backends update; failed backends retain data |
| All failed after success | 200 | Last successful data retained; errors and stale=true |
| Age beyond stale threshold | 200 after any success | Data retained; stale=true |

Foreground and SCM modes share two Tokio workers and a maximum of eight blocking workers. Health probes use a current-thread runtime. The process handles Ctrl+C/SIGTERM in foreground mode and Stop/Shutdown in SCM mode. SCM reports StartPending, Running only after HTTP bind, StopPending, and Stopped with a nonzero exit code after startup/server failure. HTTP shutdown drains for at most ten seconds, then runtime shutdown waits at most five seconds for remaining blocking workers. Service logs can append to a configured file; external rotation and access control are deployment responsibilities.

## Observability

Tracing emits JSON or pretty logs with jiff timestamps. A process-local metrics registry renders Prometheus 0.0.4 text without a global recorder. It tracks collector duration histograms, success/failure counters, snapshot age/count/stale/reboot gauges, and HTTP responses with bounded route labels. Unknown routes share the `unmatched` label.

The resource goals from the proposal (under 30 MiB RSS and low collection CPU) remain unverified targets until measured on Windows Server. Compilation and fixture tests are not live Windows collection evidence.

## References

- [Microsoft: Win32_QuickFixEngineering](https://learn.microsoft.com/en-us/windows/win32/cimwin32prov/win32-quickfixengineering)
- [Microsoft: WUA Online property](https://learn.microsoft.com/en-us/windows/win32/api/wuapi/nf-wuapi-iupdatesearcher-put_online)
- [Task specifications](tasks/README.md)
- [Validation evidence](validation.md)

## Prepared query representation

Full lists share pre-encoded Bytes buffers. Publication records each encoded object boundary; `since` filters copy selected encoded rows without metadata cloning or repeated serialization. Latest-installation indexing and coverage are prepared once. Summary serializes borrowed typed fields while freshness remains evaluated at request time. HTTP counters use bounded route/status atomics; collector histogram formatting occurs outside the registry lock.

See [review-refactor.md](review-refactor.md) for findings, measured gains, publication costs, and Windows validation limits.
