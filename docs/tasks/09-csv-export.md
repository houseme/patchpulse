# Task 09: CSV Inventory Export

Requirements: approved v0.2 roadmap; depends on Tasks 02, 04, 05.

## Implementation

1. Add GET/HEAD `/patches/export?format=csv` with installed scope by default,
   optional status and inclusive RFC3339 since filters, and strict query parsing.
2. Export one immutable publication, preserving the existing record fields in
   stable column/row order. Use UTF-8, RFC 4180 quoting and CRLF record endings.
3. Protect spreadsheet consumers by prefixing potentially executable text cells
   with an apostrophe. Document this deliberate presentation escaping.
4. Return text/csv and an attachment filename; use structured JSON errors.
5. Validate empty lists, pending/date filtering, commas, quotes, newlines,
   Unicode, formula text, HEAD and write rejection through the actual router.

## Acceptance

CSV round-trips through an independent reader. HTTP handlers never collect or
mutate inventory. CSV export is enabled in agent mode without external programs.

## Implementation record

src/export.rs prepares CSV rows during snapshot publication; the actual router
serves shared/filtered cached bodies. tests/export_contract.rs covers Unicode,
quoting, formula escaping, filtering, retained buffers, empty exports, HEAD,
duplicate/invalid queries and write rejection. Local contracts pass.
