# HTTP API Contract

Default address: `127.0.0.1:9100`. All endpoints accept GET and HEAD. Request handlers read cached snapshots. Unsupported methods return 405 and unknown paths return 404, both with `{"error":{"code":"...","message":"..."}}`. Invalid filters return 400 with the same error envelope. Requests exceeding the configured timeout return 408.

| Endpoint | Status | Response |
| --- | --- | --- |
| /health | 200 | `{"status":"ok"}` |
| /ready | 200 or 503 | `{"status":"ready"}` or `{"status":"initializing"}` |
| /version | 200 | name, version, target OS, architecture, Apache-2.0 license |
| /patches | 200 | Installed list: count and items |
| /patches/pending | 200 | Pending list: count and items |
| /patches/summary | 200 | Counts, installation dates, freshness, failures, coverage, backend statuses, reboot state |
| /metrics | 200 or 404 when disabled | Prometheus 0.0.4 text |

## List filters

Both list endpoints accept optional `status` (`installed`, `pending`, `failed`, `unknown`) and `since` (RFC3339 instant). Each endpoint retains its list scope; `/patches?status=pending` returns zero items. `since` is inclusive and excludes records whose installation instant is unknown, including date-only values and pending records. Percent-encode a literal plus sign in timezone offsets. Unknown query keys, duplicate fields, invalid timestamps, and invalid status values are rejected.

A record contains `kb_id`, nullable `title`, `description`, `category`, `severity`, `installed_on`, `installed_date`, and `installed_on_raw`, plus `status`, `reboot_required`, `source`, `sources`, and nullable `update_id`. `source` is the deterministic primary source and `sources` retains all contributing sources. Lists are sorted by KB/identity and have no duplicate keys.

## Summary fields

- `total_installed`, `total_pending`: aggregate counts after deduplication and reconciliation.
- `latest_installed_kb`, `latest_installed_at`, `latest_installed_date`: newest known date, with nullable values when no reliable date is known.
- `last_refreshed`: last cycle containing any successful backend, including successful empty output.
- `is_stale`: true before first success or when any enabled backend is incomplete, failed, or beyond the age threshold.
- `consecutive_failures`, `last_error`: cycles containing failures and their combined error details.
- `coverage`: backend enablement flags, including disabled backends.
- `backends`: enabled, last_success, last_error, consecutive_failures, in_flight per backend.
- `reboot_required`: retained system/update observations across backend batches.
- `coverage_note`: limitations of CBS QuickFix and the cached WUA catalog.

Readiness remains 200 after the first successful batch even during later degradation; consumers must inspect freshness separately. Snapshots are in memory and restart in the initializing state. The service provides no authentication or TLS; keep it on loopback or behind the deployment's authenticated proxy and firewall.
