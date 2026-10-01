> Superseded for the current branch by the [review and refactor record](review-refactor.md). Current host tests: 33; current Docker verification is in docker-verification.json. The sections below preserve the original implementation validation.

# Validation Record

Checked on 2026-10-01 (Asia/Shanghai). Implementation scope is F1-F9 from the source proposal. All eight English task specifications are in [tasks/README.md](tasks/README.md).

## Executed local gates

| Check | Actual result |
| --- | --- |
| cargo fmt --all --check | Passed |
| cargo test --all-features --locked | Passed: 21 tests (9 unit, 3 HTTP, 2 configuration, 7 pipeline) |
| cargo clippy --all-targets --all-features --locked -- -D warnings | Passed on macOS ARM64 |
| cargo check --all-targets --all-features --locked --target x86_64-pc-windows-gnu | Passed |
| cargo clippy --all-targets --all-features --locked --target x86_64-pc-windows-gnu -- -D warnings | Passed for final Windows collector and SCM code |
| cargo doc --no-deps --locked | Passed |
| cargo deny check licenses bans sources | Passed |
| cargo deny --offline --locked check --hide-inclusion-graph | Passed all four gates using the existing local advisory cache |
| Fresh RustSec cargo audit | Passed against the verified snapshot below; 124 dependency packages scanned |
| python3 scripts/dependency-policy.py | Passed; upstream notices generated and verified |
| Docker Compose config | Passed |
| Python compile and shell syntax checks | Passed |
| Docker build | Passed, release executable compiled in Linux ARM64 builder |
| python3 scripts/smoke-docker.py | Passed against the final image |
| git diff --check | Passed |

The vulnerability database's Git transport timed out. Recovery used authenticated read access to identify the current official RustSec commit and downloaded its exact source archive. `cargo audit --db target/advisory-snapshot --no-fetch` loaded 1,278 advisories and completed successfully. The temporary cache is not part of the project distribution.

RustSec snapshot: `3461c0d8f85d084552dd999c58d97c7123a9e0fd`, committed `2026-10-01T07:31:41Z`. This fresh snapshot audit is distinct from cargo-deny's offline cached advisory check.

The only duplicate dependency family is syn 2.0.119 and 3.0.6, required by upstream macro crates. Direct versions were checked against crates.io; no higher non-yanked RC/beta was available for the selected direct dependencies. Transitive dependencies use the newest versions allowed by upstream constraints. In particular, Axum 0.8.9 pins matchit 0.8.4, so matchit 0.8.6 cannot be selected without modifying Axum. The lockfile deliberately preserves that upstream constraint. See [dependency-versions.json](dependency-versions.json).

## Final Docker artifact

- Tag: `patchpulse:0.1.0`.
- Image ID: `sha256:a5b0153422107dde0677041269ce9a10e17464bbf861c5018487972c6219c7f1`.
- Architecture: `arm64`.
- Docker-reported size: 3522749 bytes.
- Runtime: scratch with required GNU/Linux libraries copied from the Rust Debian trixie builder.
- User: 65532:65532.
- Verified read-only root filesystem, dropped capabilities, binary health probe, and SIGTERM exit code 0.
- All seven endpoints verified: health/version/lists/summary/metrics return 200; readiness correctly returns 503 on Linux.
- Invalid filter returns JSON 400, unknown route returns JSON 404, write methods return 405, and HEAD health returns 200.
- Temporary smoke containers were removed; the requested local image remains available.

The ordinary online Docker build also compiled and produced an image. Final rebuilds used the exact locked public crate archives through the optional named build context to avoid repeated registry downloads. The cache preparation copies registry indexes and package archives only, never credentials or user configuration.

Machine-readable evidence: [docker-verification.json](docker-verification.json). The image includes PatchPulse's Apache-2.0 license, third-party crate notices, Rust standard-library notices, and copied system-library notices.

## Behavioral coverage

Tests cover partial/all failure retention, valid empty results, recovery, installed/pending reconciliation, startup scheduling without duplicate collection, single-flight after timeout, cooperative cancellation reaching active workers, strict configuration, relative path resolution, JSON endpoint contracts, query validation, metadata reconciliation, KB identity, CIM offsets, FILETIME, ambiguous date preservation, histogram buckets, and metric label escaping.

PowerShell cancellation is wired to process termination and wait, including a pre-launch cancellation check. Native synchronous COM calls cannot be forcibly cancelled; single-flight bounds active workers, and runtime shutdown waits at most five seconds before process exit. HTTP draining is capped at ten seconds.

## Remaining live-platform release gates

- Native WMI/WUA queries and PowerShell child termination on actual Windows Server 2016/2019/2022.
- MSVC executable linking, SCM installation, stop/start, recovery, service-account permissions, and reboot behavior on Windows.
- Windows container build/run and its WMI/WUA availability; a Windows runtime recipe is provided but was not built on this Linux Docker daemon.
- Windows resource/performance goals: under 30 MiB RSS, idle CPU, and collection latency/CPU targets. These were not measured.
- Remote GitHub CI execution. Workflow configuration is added, but no remote run is claimed.

The Windows integration tests are explicitly opt-in. Their absence on macOS is not counted as a successful live Windows test. A Linux container cannot validate host Windows patch inventory. Native WUA installation dates remain null when unavailable; WMI provides known dates. WUA searches its local cached catalog, whose freshness depends on the host's Windows Update/WSUS policy.

## Scope boundary

CSV export, multi-machine aggregation, OpenTelemetry export, and compliance baseline comparison are future version proposals in the original document, outside its F1-F9 implementation requirements. The source proposal remains preserved with a superseding implementation notice.
