# Completion Review and Validation

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
MSVC linking, SCM install/start/stop/restart/reboot and account permissions need
execution on suitable Windows hosts. The Windows container recipe also needs a
compatible daemon; a Linux container cannot read host Windows patches. Validate
the multi-host deployment and external OTLP collector under its actual TLS and
credential policy. Windows RSS, idle/collection CPU, collection latency and
network throughput goals remain unmeasured. Remote GitHub CI has not run in
this local delivery.

The earlier [validation record](validation.md), [refactor evidence](review-refactor.md)
and [tool migration evidence](rust-tooling.md) retain their original historical
commands and measurements. The original proposal remains historical design
input; implementation contracts live in [tasks/README.md](tasks/README.md).
