# Completion Review and Validation

## 2026-11-07 working-tree review

Reviewed F1-F9, Tasks 01-12 and roadmap v0.2-v0.5. The current source implements
every listed feature. Remaining acceptance covers Windows Server 2016/2019/2022
collection, SCM operations, Windows containers, deployed Agent/Hub and OTLP
integration, and Windows resource/performance measurements.

This review fixes pending-only baseline conclusions and remote freshness/readiness
without successful enabled backend evidence. The two regressions reproduce on
the `fb7cb06` source with the same current Cargo.lock, in a separate target
directory, and pass after the fixes. Agent snapshot JSON is prepared during
transactional publication and served with request-time freshness.

The workspace received concurrent Cargo.toml/Cargo.lock and stable-toolchain
updates during this review. Host checks use installed Rust 1.99.1 stable. The
Dockerfile builds with Rust 1.98.1. Dependency declarations and toolchain edits
belong to that concurrent work. This review restores the missing upstream license
materials and regenerates the dependency inventory against the resulting graph.

| Executed gate | Result |
| --- | --- |
| `cargo fmt --all --check` and `git diff --check` | Passed. |
| `cargo test --all-targets --all-features --locked` | 73 tests passed in an isolated current-source target directory with loopback access. Windows-only test binaries contain zero host tests. |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | Passed on macOS ARM64. |
| `sh scripts/check-windows-gnu.sh` | Passed with installed Zig and x86_64-pc-windows-gnu. |
| `cargo doc --no-deps --locked` | Passed. |
| `--check-config` for both shipped configurations | Passed. |
| `cargo xtask dependency-policy --write` followed by `cargo xtask dependency-policy` | Passed, including all resolved targets and upstream notices. |
| `cargo deny --offline --locked check --hide-inclusion-graph` | Advisories, bans, licenses and sources passed using cached database commit `26fd2d43f7280a1fb925a3743ec463017afe7119`, dated 2026-11-05. Upstream duplicate-version warnings remain. |
| `cargo audit --db target/roadmap-review/advisory-db --no-fetch` | Passed against verified official RustSec snapshot `309392b18f6b17022f82c701f300433126901068`, dated 2026-11-06T15:34:32Z, covering 1,422 advisories and 252 locked dependencies. |
| Docker build using the current Dockerfile and locked public crate cache | Passed on Linux ARM64. |
| `cargo xtask smoke-docker --image patchpulse:roadmap-review` | Passed health/readiness, CSV, snapshot, read-only methods, JSON logs, CA/notices, non-root/read-only runtime and SIGTERM exit zero. |
| `cargo xtask smoke-hub-docker --image patchpulse:roadmap-review` | Passed actual Agent/Hub polling, unknown baseline on unsupported Linux collectors and both SIGTERM exits at zero. |

Direct Git fetches of RustSec failed. Recovery used GitHub's official comparison
API from the clean cached commit to the current commit and fetched all 14 changed
files. Every file's computed Git blob ID matched both the comparison and contents
API. The verified snapshot resides in ignored target data. OpenTelemetry 0.33.1
licenses come from their recorded source commit and verified blob, while r-efi
6.0.0 uses the existing verified SPDX texts plus its current upstream AUTHORS.
Per-crate provenance is recorded under licenses/upstream.

Reproducible test logs, the isolated baseline source, benchmark executables and
Docker smoke reports reside in `target/roadmap-review`. The final Linux image
is `patchpulse:roadmap-review`, ID
`sha256:58e24b234e86a734e2a756c7dd61d538c31b01a25f3f4baacf95b6c383860143`.
The smoke commands clean up their temporary containers and network.

### Snapshot query measurement

The release-mode real-router benchmark uses 800 deterministic installed records,
10,000 requests per route and 50 publication samples. Baseline and candidate use
the same Cargo.lock and Rust 1.99.1, separate build directories, warm-up runs and
sequential A1/B1/B2/A2 execution after builds and container tests completed.

| Run | `/snapshot` p50 | `/snapshot` p95 | Successful publication p50 | Failure-only publication p50 |
| --- | --- | --- | --- | --- |
| A1, original | 294.042 us | 339.042 us | 690.583 us | 20.375 us |
| B1, updated | 7.625 us | 8.542 us | 962.166 us | 288.417 us |
| B2, updated | 7.417 us | 8.834 us | 942.125 us | 278.750 us |
| A2, original | 293.125 us | 342.375 us | 709.000 us | 19.125 us |

Baseline query p50 drift was 0.31%. Prepared JSON reduces the measured query
p50 by approximately 97.4%. The tradeoff is an additional encoded snapshot
(approximately 504 KB in this fixture) and approximately 0.25-0.27 ms extra
background publication work, including failure-only cycles that refresh error
diagnostics. This measures in-process handler execution on macOS. Network
throughput and Windows collector latency remain separate acceptance targets.
Run `cargo run --release --locked --example hotpath -- 800 10000` to measure the
current implementation. The archived source and A1/B1/B2/A2 JSON measurements
are in `target/roadmap-review`.

### Historical checkpoint

Checked on 2026-10-02 (Asia/Shanghai) against all eight original English tasks,
the F1-F9 requirements in the historical combat-solutions proposal, and the
user-approved Tasks 09-12. Code paths for CSV export, agent/hub aggregation,
OTLP traces and exact-KB baseline comparison are implemented. Step-by-step
evidence is in [requirements-audit.md](requirements-audit.md).

## Executed local gates

| Gate | Actual result |
| --- | --- |
| `cargo fmt --all --check` | Passed. |
| `cargo clippy --all-targets --all-features --locked -- -D warnings` | Passed on macOS ARM64. |
| `sh scripts/check-windows-gnu.sh` | Passed for x86_64-pc-windows-gnu with the installed Zig C compiler for ring; this is a cross-target code check. |
| `cargo test --all-targets --all-features --locked` | 70 host tests passed: 24 library, 6 API, 3 baseline, 2 CSV, 8 Hub, 2 collector, 2 configuration, 1 logging, 3 OTLP, 8 snapshot pipeline and 11 Rust tool tests. |
| `cargo doc --no-deps --locked` | Passed. |
| `cargo xtask dependency-policy` | Passed for all resolved targets. No chrono/time crate; own package license is Apache-2.0. Upstream notices are preserved. |
| `cargo deny --offline --locked check --hide-inclusion-graph` | Advisories, bans, licenses and sources passed. Duplicate base64, getrandom, syn, tower-http and windows-sys lines are upstream compatibility constraints. |
| `cargo audit --db target/advisory-snapshot --no-fetch` | Passed: 1,280 advisories, 244 locked dependencies. |
| `cargo run --locked -- --check-config --config config/hub.toml` | Passed. |
| Offline Docker build and both Rust smoke commands | Passed on Linux ARM64; reports linked below. |
| [GitHub Actions run 37002534605](https://github.com/houseme/patchpulse/actions/runs/37002534605) on `8a6d2da25dd6c4edbb97e171d6455a84d3915b1e` | Passed on 2026-10-02: Linux, macOS, Windows, dependencies and Docker jobs. The Windows job linked the x86_64-pc-windows-msvc release executable. Ignored live collector tests were not run. |
| [Merged-main CI run 37009989774](https://github.com/houseme/patchpulse/actions/runs/37009989774) on `c5fbd1dff924e2c32e1ba593eade6463f85ec24e` | All five jobs passed after PR #1 was rebased into `main`; the PR head's push and pull-request runs also passed all ten checks before merge. |
| Windows Server manual guide | All nine PowerShell command blocks parsed with the PowerShell parser; the commands await execution on Server 2016/2019/2022. |

The first full test command ran under a sandbox that denied local TCP bind and
therefore failed six Hub fixtures with `PermissionDenied`. The identical suite
passed with loopback access. Zero-test Windows integration binaries and cross
compilation are not counted as live Windows execution.

## RustSec and license provenance

The fresh advisory snapshot uses the [official RustSec advisory-db commit
117edb3bed98e9be112f277b7615eea3252e7c43](https://github.com/RustSec/advisory-db/commit/117edb3bed98e9be112f277b7615eea3252e7c43),
committed 2026-10-02T08:58:33Z. Its clean local base was
`e2111519ba6d14a5da59a7b2e5c8083ae8a37c01`; all 12 changed files in the
official comparison were downloaded via GitHub's contents API and verified
against their Git blob IDs. The new snapshot lives in ignored target data. This
audit used that snapshot without pretending the old on-disk cargo-audit cache
was current.

PatchPulse's own [Apache-2.0 license](../LICENSE) is unchanged. Additional
ISC and CDLA-Permissive-2.0 allowances apply only to upstream code or CA data.
Official source-commit license texts were restored for archives that omitted
them. r-efi declares MIT OR Apache-2.0 OR LGPL-2.1-or-later without bundling
license text; its fallback uses pinned [official SPDX license-list
data](https://github.com/spdx/license-list-data) and the byte-identical upstream
AUTHORS file. Each fallback records its exact source and blob in provenance.json.

## Final Linux Docker image

`patchpulse:0.1.0`, ARM64, Docker ID
`sha256:f2939dc7507d64da140dd16e29816f16a2839c714ab75bb54d9b847a306fc7b1`,
Docker-reported size 5,397,393 bytes. The public locked crate cache built the
image offline. The scratch runtime carries required libraries, CA data and
project/dependency/Rust/system-library notices. Image metadata reports
Apache-2.0 for PatchPulse and runtime user 65532:65532.

The [agent smoke report](docker-verification.json) verifies built-in health,
read-only root, dropped capabilities, non-root execution, SIGTERM exit zero,
original HTTP/metrics, CSV export and versioned `/snapshot`. Unsupported Linux
collectors leave `/ready` at 503 and inventory empty. The Agent tool also copied
the runtime LICENSE and THIRD_PARTY_NOTICES.md for byte-for-byte comparison
against the repository and verified its nonempty CA bundle. The [Hub smoke
report](docker-hub-verification.json) verifies one actual Agent and Hub container
on a dedicated temporary Docker network, machine identity, receipt of its
snapshot, zero Linux patch counts, stale/unknown baseline, structured logs,
read-only methods and both SIGTERM exits at zero. Both tools removed their
temporary containers, network and configuration; no such Docker resources
remained after inspection.

## External acceptance still required

Real Windows Server 2016/2019/2022 WMI/WUA/PowerShell data, private job cleanup,
SCM install/start/stop/restart/reboot and account permissions need
execution on suitable Windows hosts. The Windows container recipe also needs a
compatible daemon; a Linux container cannot read host Windows patches. Validate
the multi-host deployment and external OTLP collector under its actual TLS and
credential policy. Windows RSS, idle/collection CPU, collection latency and
network throughput goals remain unmeasured. Follow the
[Windows Server manual acceptance guide](windows-server-manual-validation.md)
separately on each target release.

The Windows CI release executable link check passed on a GitHub-hosted
`windows-latest` runner. It does not prove Windows Server 2016/2019/2022 runtime
behavior, service control or host patch visibility.

The earlier [validation record](validation.md), [refactor evidence](review-refactor.md)
and [tool migration evidence](rust-tooling.md) retain their original historical
commands and measurements. The original proposal remains historical design
input; implementation contracts live in [tasks/README.md](tasks/README.md).
