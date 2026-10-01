# Task 01: Foundation, Configuration, and License Policy

Requirements: F5, F8. Dependencies: None.

## Implementation steps

1. Create an edition 2024 Rust library and binary with an Apache-2.0 project license and a committed lockfile.
2. Query the registry for current non-yanked releases; evaluate RC and beta releases when newer, and declare current direct versions explicitly and pin the resolved graph in Cargo.lock.
3. Use jiff for all calendar dates and timestamps. Ban chrono and time throughout the resolved graph, including Windows targets.
4. Load strict TOML configuration with safe defaults; support --config, --bind, --foreground, --service, --check-config, and --healthcheck.
5. Resolve configured script paths relative to the configuration file, never the service working directory. Reject zero durations, invalid binds, unknown keys, invalid log settings, and all-disabled collectors.

## Acceptance criteria

- Configuration defaults, typo rejection, CLI overrides, and path resolution have tests.
- The project is Apache-2.0 only; dependency license choices are recorded separately without relicensing upstream code.

## Evidence

See [validation.md](../validation.md) for executed checks and platform limitations.

## Implementation record

`Cargo.toml`, `rust-toolchain.toml`, `src/config.rs`, `config/patchpulse.toml`, `deny.toml`, and `tests/config_contract.rs`. Configuration and dependency policy checks pass.

## Comprehensive audit

Step-by-step implementation and acceptance status is recorded in [requirements-audit.md](../requirements-audit.md). Code presence does not complete the live Windows release gates.
