# Agent and Hub Operation

The default mode is agent. GET/HEAD `/snapshot` returns schema_version=1,
observed_at, stale_after_secs, is_stale and one coherent PatchSnapshot. The
snapshot contains installed/pending arrays, last_refreshed, last_error,
consecutive_failures, backends and reboot_required.

Run a hub with `cargo run --locked -- --config config/hub.toml --foreground`.
Hub mode skips local collectors and polls only static hub.agents targets. IDs
are unique ASCII letters/digits/hyphens/underscores. URLs support HTTP(S) without
embedded credentials, query or fragment; a configured path is a proxy prefix.
Use certificate-validating HTTPS and the deployment's authenticated proxy for
remote machines. Optional bearer_token_env names a protected environment
variable. Tokens and target URLs are absent from fleet responses and logs.
Bearer credentials require HTTPS; unencrypted HTTP is supported for agents
without a bearer credential, such as local testing behind host firewalls.

## Bounded polling and publication

Reuse one client, disable redirects and environment proxies, and bound
concurrency to 1..8, total request duration, response bytes and retained encoded
snapshot bytes. Defaults are 60-second polling, 10-second requests, 180-second
transport freshness, four requests, 8 MiB per response and 64 MiB retained input.
The retained input budget excludes decoded model overhead and is not an RSS
guarantee. At most 256 agents are configured. Validate schema, sorted unique
keys, record states, no installed/pending overlap and initial-empty semantics.

Each failed node keeps its prior data/last_fetch and increments failure count.
Healthy nodes update independently. Publish one immutable fleet state after a
cycle. Cancellation drops outstanding HTTP polling; handlers never configure
targets or trigger polling. The same KB on two machines remains two observations.

Source or transport failure/expiry makes a node stale. Readiness is historical
first source-ready remote collection; it does not imply universal freshness.
Metrics inventory counts sum per-host observations and use bounded path templates.
The Linux image includes native CA certificates and their upstream notice.

## Read-only Hub API

All routes support GET/HEAD; unsupported methods and unknown IDs use the common
JSON errors. Hub mode keeps health/ready/version/metrics and uses the following
machine-scoped routes instead of the agent /patches routes:

| Path | Behavior |
| --- | --- |
| `/agents` | Sorted IDs, fetch/error/failure/freshness/readiness and counts. |
| `/agents/{id}/snapshot` | agent_id, last_fetch, last_error, current is_stale and retained source envelope; 503 before any valid source, 404 for unknown ID. |
| `/fleet/summary` | Configured/available/fresh/stale agent counts, readiness, aggregate patch counts, reboot state and last_refreshed. |
| `/agents/{id}/baseline` | Named exact-KB report for this machine; absent or stale input is unknown. |
| `/fleet/baseline` | Named per-machine reports and overall conclusion. |

Baseline routes are disabled unless a baseline is configured. A failed agent
cannot establish compliance from its retained data. Hub mode does not recursively
expose /snapshot as though it were a single host. Existing agent JSON fields are
preserved. Collector execution, credentials and targets remain outside HTTP input.

## Verification

Real loopback HTTP tests exercise two hosts sharing a KB, partial failure and
recovery, per-machine/fleet compliance, budgets, strict schema/URL configuration,
redirect rejection, bounded concurrent polling and shutdown. A real TLS handshake
verifies rejection of an untrusted certificate. Host and Windows-target Clippy
pass; `sh scripts/check-windows-gnu.sh` uses installed Zig or MinGW C tools for
ring when checking Windows from another host. This is compilation evidence.
Live Windows collection, actual multi-host deployment and native resource targets
remain the external acceptance gates documented in the requirements audit.

The offline Linux ARM64 image was rebuilt with the public locked crate cache and
the Rust smoke tool verified its hardened agent mode, health probe and shutdown.
The report in [docker-verification.json](docker-verification.json) records image
`sha256:ef942a4d9e459e42652fe1160bd0c1e27fdd436a32008e83ad8c9e1b5d8ad22c`.
