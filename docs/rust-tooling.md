# Rust Development Tools

PatchPulse uses `cargo xtask` for repository maintenance. The alias in
`.cargo/config.toml` builds the `examples/xtask.rs` development executable with
the pinned Rust toolchain and locked graph. These commands require Rust/Cargo;
Docker verification also requires a running Linux Docker daemon. Python is not
required. The production executable and images do not include the development
tool or its HTTP client features.

## Commands

| Command | Behavior |
| --- | --- |
| `cargo xtask dependency-policy` | Verify the exact generated license inventory/notices and dependency bans. |
| `cargo xtask dependency-policy --write` | Regenerate docs/dependency-licenses.md and THIRD_PARTY_NOTICES.md. |
| `cargo xtask dependency-policy --metadata target/metadata.json` | Check previously captured Cargo metadata instead of spawning Cargo. |
| `cargo xtask prepare-docker-cache` | Replace target/docker-cargo-cache with only locked public archives/indexes. |
| `cargo xtask smoke-docker --image patchpulse:0.1.0` | Verify a real Linux image and write target/docker-smoke.json. |

Optional global `--root PATH` selects the repository root. Relative metadata and
report paths resolve against that root. `smoke-docker --report PATH` changes the
report destination. Run commands from the repository so Cargo can find the alias.

## Dependency policy and notices

Metadata covers all resolved targets, including Windows-only and development
dependencies. Reject chrono/time, missing license metadata, missing upstream
license files and any project license other than Apache-2.0. Preserve upstream
SPDX expressions and license text. Sort by crate name/version and filename;
normalize line endings to LF, as the previous generator did. If a package archive
omits notices, use its exact-version `licenses/upstream` fallback. Missing or
stale documents cause a nonzero exit; regeneration is explicit.

## Offline Docker cache

First fetch the locked dependency graph using `cargo fetch --locked`. The cache
command supports a single crates.io or rsproxy sparse registry cache. Copy only
resolved package archives, their sparse index entries and public registry
config.json. For rsproxy, generate a minimal public source replacement; never
copy user Cargo configuration or credentials. Prepare a temporary directory
before replacing the output, retain the previous cache on input failure and
remove stale registry configuration when changing registries.

```sh
cargo xtask prepare-docker-cache
docker build --build-context cargo_cache=./target/docker-cargo-cache \
  --build-arg CARGO_NET_OFFLINE=true -t patchpulse:0.1.0 .
cargo xtask smoke-docker --image patchpulse:0.1.0
```

The builder's Rust/base image must already be available for a fully offline
build. Only generated target cache files are replaced.

## Docker verification

Start a uniquely named temporary container with loopback port publication,
read-only root, all capabilities dropped and no-new-privileges. Use Hyper HTTP/1
with a three-second per-request timeout and a one-MiB response limit. Validate
all seven endpoints, expected Linux readiness 503, zero fabricated inventory,
Prometheus type/staleness, invalid-query 400, missing-route 404, POST 405 and HEAD
200. Check the binary health probe, UID/GID 65532, structured timestamped logs,
runtime hardening and graceful SIGTERM exit 0. Remove the temporary container
on success or failure; write a report only after verification and cleanup pass.

Docker subprocess waits run on blocking workers. The current-thread runtime
drives HTTP connections; connection tasks are aborted when each probe completes.
Production collectors, Windows platform behavior and deployment contracts are
unchanged. Linux image checks do not validate Windows collection or SCM.

## Migration validation

Validation is recorded after running the migrated commands. The original Python
helpers were executed once against the same locked graph for output comparison;
after normalizing generator command names, both generated documents matched
byte-for-byte. The helpers were then removed. Historical validation and baseline
hash records remain unchanged as provenance rather than executable instructions.

Checked on 2026-10-02 (Asia/Shanghai):

| Gate | Actual result |
| --- | --- |
| `cargo test --all-targets --all-features --locked` | 50 passed: 39 service/domain contracts and 11 development-tool regressions. |
| `cargo fmt --all --check` | Passed. |
| Strict host and x86_64-pc-windows-gnu Clippy, all targets/features | Passed; Windows execution is not implied. |
| `cargo doc --no-deps --locked` | Passed. |
| `cargo xtask dependency-policy`, including captured metadata | Passed; both generated documents match the retired generator after command-name normalization. |
| `cargo xtask prepare-docker-cache` | Passed against the real locked registry cache; the resulting cache built the image offline. |
| Offline cargo-deny licenses/bans/sources/advisories | Passed; only the existing upstream syn 2/3 duplicate warning. |
| `cargo audit --db target/advisory-snapshot --no-fetch` | Passed for 126 dependencies using the previously recorded 1,278-advisory snapshot; no current online freshness claim. |
| Docker release build and `cargo xtask smoke-docker` | Passed on Linux ARM64, including temporary-container removal. |

The rebuilt `patchpulse:0.1.0` image has Docker ID
`sha256:4d6d7e723021c1afc22d7909b02f508fa27c042df381937de114d0aebf8bc9d8`
and reported size 3,523,955 bytes. Its application executable layer was unchanged
by the development-tool migration; updated upstream notices are included.
Machine-readable results are in [docker-verification.json](docker-verification.json).
Live Windows acceptance and remote CI remain pending as documented in the audit.
