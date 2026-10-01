# Task 03: Read-Only Windows Collection Backends

Requirements: F1, F2, F8. Dependencies: 01, 02.

## Implementation steps

1. Implement WMI installed collection, native WUA installed/pending collection, and PowerShell WUA installed/pending collection.
2. Initialize and release COM on the same blocking worker thread. Use official generated WUA bindings instead of hand-written vtables.
3. Use read-only WUA Search and system reboot state; never download, approve, hide, or install updates.
4. Run blocking WMI/COM/process operations in spawn_blocking. Timeouts must not create unbounded abandoned workers.
5. For PowerShell, use a fixed trusted script, noninteractive arguments, UTF-8 output, bounded output, process termination, and explicit empty/single/array JSON handling.
6. Provide non-Windows Unsupported errors rather than fabricated data. Expose backend configuration and observed coverage independently.

## Acceptance criteria

- Fixture tests exercise output parsing, no-KB identity, multiple KBs, malformed output, and empty results.
- Windows cross-compilation checks generated bindings; live Windows collection is a separate release gate.

## Evidence

See [validation.md](../validation.md) for executed checks and platform limitations.

## Implementation record

`src/collector/windows_native.rs`, `src/collector/powershell.rs`, and `scripts/query-patches.ps1`. Native Windows code compiles and passes Clippy. Fixtures pass. Live Windows queries and PowerShell process cleanup require the platform integration gate.
