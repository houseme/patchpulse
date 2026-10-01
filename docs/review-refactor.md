# Branch Review and Refactor

The review covers all tracked changes and untracked implementation files on the current branch. The original implementation contains concrete patch accumulation in snapshot mutation, per-request conversion, metrics locking, and shutdown cleanup. The domain/platform separation, read-only collectors, last-success retention, and single-flight COM guard are retained.

## Findings and applied fixes

Original locations below refer to the frozen input before refactoring. Source fingerprints are recorded in [review-baseline-source-hashes.json](review-baseline-source-hashes.json).

| Priority | Original location | Issue and resulting change |
| --- | --- | --- |
| P1 | src/cache/mod.rs:61 | The exclusive snapshot lock covered deep cloning, reconciliation, and aggregation, including all-failed cycles. Background transactional assembly now uses a separate writer lock, immutable shared inventory, and one short watch publication. Failed cycles reuse inventory and JSON buffers. |
| P1 | src/api/mod.rs:143 | Every list request deep-cloned metadata and serialized the full list. Lists now use one prepared representation with encoded row boundaries. Full lists share Bytes; filtered lists copy only selected encoded records. Unicode and JSON escaping remain covered by serde and regression tests. |
| P2 | src/api/mod.rs:152 | Summary scanned all installed records and built an intermediate JSON value tree on every request. Latest-record indexing and configured coverage are prepared once; response fields borrow immutable data. |
| P2 | src/observability/mod.rs:134 | Metric formatting held the registry mutex, while every HTTP request allocated String keys and acquired that mutex. HTTP route/status counters are bounded atomics with an occupancy bitmap; histogram snapshots are copied under a short lock and formatted outside it. Metric families are emitted together. |
| P1 | src/domain/patch.rs:81 | String length chose Important over Critical; date, timestamp, and raw date were merged independently. Severity follows risk order, categories are combined deterministically, and installation fields remain one coherent observation. CIM numeric fields are strict and future observations cannot appear fresh after clock rollback. |
| P1 | src/collector/powershell.rs:245 | After parent exit, unconditional reader joins bypassed timeout and cancellation when another process held the pipe. One process supervisor includes pipe draining in its deadline; Windows private jobs contain assigned helper processes. Oversize output, nonzero exit, cancellation, and pipe retention are covered by tests or platform compilation. |
| P2 | src/collector/powershell.rs:74 | The JSON value tree duplicated parsing work and silently accepted an invalid envelope reboot flag. A borrowed RawValue envelope and direct typed rows preserve legacy singleton/array/null input while rejecting malformed reboot state. |
| P2 | src/app.rs:64 and src/main.rs:30 | Cleanup requested cancellation twice; foreground signal errors were detached, and foreground/SCM separately created unbounded default runtimes. Cancellation now occurs once, signal errors propagate, both modes share two runtime workers and at most eight blocking workers, and health probes use a current-thread runtime. |
| P2 | src/config.rs:122 | Relative-path resolution was duplicated, and service/config-check/probe flags could conflict silently. Paths resolve from one base and primary CLI actions are mutually exclusive. |
| P2 | src/collector/windows_native.rs:121 | Non-KB WMI rows were discarded and empty identifiers could make incomplete data appear successful. Non-KB identifiers are preserved as WMI identities; missing identity fails the backend and retains prior data. Authoritative CIM installation data replaces its full observation. WUA result-code failures retain useful context. |

No observed critical unsafe-memory issue was found in the reviewed paths. All identified P1/P2 changes are implemented. This is a code/contract review rather than live Windows execution evidence.

## Performance evidence

A frozen baseline and the same deterministic real-router harness were compiled with Rust 1.98.1 in release mode. Both runs use two Tokio workers, 800 installed records, 1,500 samples per endpoint, 25 warmups, and 50 samples per publication case. A1-B1-B2-A2 ordering separates the frozen baseline from the refactor. Every baseline drift in this run is below 7%; the reporting gate is 10%.

| Case | Before median us | After median us | Speedup | A2/A1 drift |
| --- | --- | --- | --- | --- |
| `/patches` | 326.666 | 1.625 | 200.96x | -0.4% |
| `/patches?status=installed` | 324.041 | 1.208 | 268.25x | -0.1% |
| `/patches?since=2026-09-02T00:00:00Z` | 166.729 | 8.666 | 19.24x | 2.1% |
| `/patches/summary` | 6.854 | 2.229 | 3.07x | 4.4% |
| `/ready` | 1.104 | 1.020 | 1.08x | 3.9% |
| `/metrics` | 2.542 | 2.062 | 1.23x | 6.8% |
| `publish_success` | 291.791 | 447.312 | 0.65x | 6.3% |
| `publish_failure` | 204.688 | 32.479 | 6.30x | -0.5% |

These are in-process handler/body-production measurements, not network throughput or native Windows collection speed. The body sizes match for the compared endpoints. Exact raw runs and method are in [review-performance.json](review-performance.json). Run the retained harness with `cargo run --release --locked --example hotpath -- 800 1500`.

Successful publication costs more because it prepares cached JSON and row offsets once per collection cycle. The default cycle is 1,800 seconds, and preparation runs on a blocking worker without holding the reader publication lock. Readers continue serving the previous coherent view until the new view is ready. Failure-only publication becomes substantially cheaper. Cached JSON adds persistent memory approximately equal to each list's encoded body plus 16 bytes per row offset; Windows RSS targets remain unmeasured.

## Verification

- 33 host tests pass: 19 unit/process tests, four API contracts, two configuration contracts, eight pipeline tests.
- Strict Clippy passes on the host and x86_64-pc-windows-gnu, including the new Windows job and SCM paths.
- Rustdoc, license/notice policy, chrono/time bans, and cargo-deny's four offline gates pass.
- Cargo audit passes against the previously verified RustSec snapshot with 1,278 advisories. The database snapshot provenance is retained in validation.md; this review does not claim a newer online advisory fetch.
- Docker is rebuilt and smoke-tested after these changes; final image evidence is in docker-verification.json.

Windows job behavior, real WMI/WUA/PowerShell queries, SCM operation, MSVC linking, and Windows memory/CPU targets still require live Windows validation. `in_flight` remains the state observed at the last published collection cycle, not a live worker probe.

## Result

The code now has one consistent publication and representation path rather than repeated per-request repairs. The reviewed correctness and resource-lifecycle defects are fixed, and the read-path performance gains are measured. Production Windows readiness remains conditional on the existing live-platform gates.

## Delivered image

`patchpulse:0.1.0` (arm64), image ID `sha256:ff61b9d14ba75e0c6d7b65c2901f063b524f07cb683d9a92b5990a2523456bff`, Docker-reported size 3522838 bytes. Final smoke checks passed for all seven endpoints, invalid input, write rejection, HEAD, non-root/read-only operation, the binary health probe, and SIGTERM exit code 0. Temporary containers were removed.
