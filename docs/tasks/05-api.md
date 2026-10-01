# Task 05: HTTP Contracts and Input Validation

Requirements: F3. Dependencies: 04.

## Implementation steps

1. Implement GET /health, /ready, /version, /patches, /patches/pending, /patches/summary, and /metrics.
2. Implement status and RFC3339 since filters with JSON 400 errors for invalid inputs.
3. Keep handlers read-only and platform-independent. Summary includes freshness, errors, reboot state, configured coverage, and backend status.
4. Return consistent JSON errors for unknown paths and unsupported methods; apply configured request timeout and bounded metric path labels.
5. Document exact response schemas, date-only filtering behavior, and initialization/degradation behavior.

## Acceptance criteria

- Contract tests assert status codes and JSON fields for every endpoint, invalid filters, readiness transitions, unknown routes, and metrics.

## Evidence

See [validation.md](../validation.md) for executed checks and platform limitations.

## Implementation record

`src/api/mod.rs`, `tests/api_contract.rs`, and `docs/api.md`. All seven endpoints, filters, structured errors, initialization, and metrics contracts pass. The real image smoke test verifies the shipped HTTP server.
