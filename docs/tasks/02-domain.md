# Task 02: Patch Identity, Dates, and Reconciliation

Requirements: F1, F2. Dependencies: 01.

## Implementation steps

1. Model installed, pending, failed, and unknown states; retain the original public field names.
2. Normalize numeric KB identifiers; use WUA update GUID and revision for records without KB numbers.
3. Merge complementary fields deterministically and retain all source provenance. Never discard richer metadata merely because another field is present.
4. Parse RFC3339 and offset-aware CIM dates with jiff. Preserve date-only installation values separately without inventing UTC instants.
5. Deduplicate records in stable order; installed records take precedence over pending duplicates.

## Acceptance criteria

- Unit tests cover malformed KBs, complementary metadata, duplicate records, CIM offsets, locale dates, and non-ASCII input.

## Evidence

See [validation.md](../validation.md) for executed checks and platform limitations.

## Implementation record

`src/domain/patch.rs` and `src/domain/snapshot.rs`. KB normalization, metadata merge, CIM offset, FILETIME, and civil-date tests pass. Ambiguous dates retain their original value.

## Comprehensive audit

Step-by-step implementation and acceptance status is recorded in [requirements-audit.md](../requirements-audit.md). Code presence does not complete the live Windows release gates.
