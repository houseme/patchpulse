# HTTP API Contract

Default address: `127.0.0.1:9100`. All endpoints accept GET and HEAD. Request handlers read cached snapshots. Unsupported methods return 405 and unknown paths return 404, both with `{"error":{"code":"...","message":"..."}}`. Invalid filters return 400 with the same error envelope. Requests exceeding the configured timeout return JSON 408 with code `request_timeout`; the timed-out handler is cancelled and the response is counted in HTTP metrics.

| Endpoint | Status | Response |
| --- | --- | --- |
| /health | 200 | `{"status":"ok"}` |
| /ready | 200 or 503 | `{"status":"ready"}` or `{"status":"initializing"}` |
| /version | 200 | name, version, target OS, architecture, Apache-2.0 license |
| /patches | 200 | Installed list: count and items |
| /patches/pending | 200 | Pending list: count and items |
| /patches/export | 200 | UTF-8 CSV attachment of installed or pending inventory |
| /patches/baseline | 200 or 404 when disabled | Named exact-KB comparison with explicit compliance/freshness |
| /patches/summary | 200 | Counts, installation dates, freshness, failures, coverage, backend statuses, reboot state |
| /metrics | 200 or 404 when disabled | Prometheus 0.0.4 text |

## List filters

Both list endpoints accept optional `status` (`installed`, `pending`, `failed`, `unknown`) and `since` (RFC3339 instant). Each endpoint retains its list scope; `/patches?status=pending` returns zero items. `since` is inclusive and excludes records whose installation instant is unknown, including date-only values and pending records. Percent-encode a literal plus sign in timezone offsets. Unknown query keys, duplicate fields, invalid timestamps, and invalid status values are rejected.

A record contains `kb_id`, nullable `title`, `description`, `category`, `severity`, `installed_on`, `installed_date`, and `installed_on_raw`, plus `status`, `reboot_required`, `source`, `sources`, and nullable `update_id`. `source` is the deterministic primary source and `sources` retains all contributing sources. Lists are sorted by KB/identity and have no duplicate keys.

## Summary fields

CSV export uses `/patches/export?format=csv`; see the export contract below.

- `total_installed`, `total_pending`: aggregate counts after deduplication and reconciliation.
- `latest_installed_kb`, `latest_installed_at`, `latest_installed_date`: newest known date, with nullable values when no reliable date is known.
- `last_refreshed`: last cycle containing any successful backend, including successful empty output.
- `is_stale`: true before first success or when any enabled backend is incomplete, failed, or beyond the age threshold.
- `consecutive_failures`, `last_error`: cycles containing failures and their combined error details.
- `coverage`: backend enablement flags, including disabled backends.
- `backends`: enabled, last_success, last_error, consecutive_failures, in_flight per backend (observed at the last published cycle, rather than a live worker probe).
- `reboot_required`: retained system/update observations across backend batches.
- `coverage_note`: limitations of CBS QuickFix and the cached WUA catalog.

Readiness remains 200 after the first successful batch even during later degradation; consumers must inspect freshness separately. Snapshots are in memory and restart in the initializing state. The service provides no authentication or TLS; keep it on loopback or behind the deployment's authenticated proxy and firewall.

## CSV export

`GET /patches/export?format=csv` exports installed records by default. Optional
status and since have the same validation and date semantics as list queries;
status=pending selects pending rows, while failed/unknown produce a header-only
file. Omitted format defaults to csv; any other format returns JSON 400.

The response has `text/csv; charset=utf-8` and an attachment filename. Columns are
kb_id, title, description, category, severity, installed_on, installed_date,
installed_on_raw, status, reboot_required, source, sources and update_id.
Null strings/dates become empty cells; sources use semicolons. Use RFC 4180
quoting, Unicode text and CRLF row endings. Potential spreadsheet formulas
(leading =, +, -, @ after whitespace, or leading tab/CR/LF) receive an apostrophe
prefix. This protects spreadsheet consumers and deliberately differs from the
unmodified JSON text. No export request starts collection.

CSV buffers and complete row spans are prepared alongside JSON during publication.
Failure-only cycles share previous buffers. Full exports share bytes; since
filters copy selected complete rows from the same immutable view.

## Baseline comparison

Configure `[baseline]` with enabled=true, a name and a nonempty required_kbs list.
Numeric KBs normalize to KB-prefixed keys and duplicates collapse. The default
disabled feature returns JSON 404. GET/HEAD `/patches/baseline` reports baseline,
compliance, required_count, installed_count, missing_count, pending_count,
installed, missing, pending, is_stale, last_refreshed and last_error. Lists are
sorted KB keys; pending is the subset of missing KBs observed as pending.

Compliance is compliant only when every required KB is installed and input is
fresh. Fresh input with missing KBs is non_compliant. Before readiness or after
source/transport failure or expiry, compliance is unknown even when retained
data previously satisfied the baseline. Counts/lists remain observations from
that same snapshot. Comparison uses exact KB membership; it does not infer
supersedence, security/vulnerability coverage or patch installation.

## Agent and hub protocol

Agent `/snapshot` and the mode-specific fleet routes, schemas, budgets, retained
failures and baseline semantics are specified in [fleet.md](fleet.md).
The fleet baseline reports non_compliant on any conclusive fresh-agent failure,
unknown if no failure is known but an agent is unknown, and compliant only when
every configured agent is fresh and compliant.

## Optional trace context

When OTLP traces are enabled, W3C traceparent/tracestate are accepted on HTTP
requests and forwarded by hub polls to configured agents. Trace attributes use
bounded route templates and method classes; API JSON and Prometheus schemas do
not change. See [telemetry.md](telemetry.md) for export configuration and limits.
