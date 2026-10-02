# Changelog

Changes are grouped by functional area. Each functional commit adds its own entry.
Unreleased entries do not announce a published release.

## [Unreleased]

### Foundation, configuration, and patch model

- Add the edition-2024 Rust library, pinned toolchain and dependency lockfile; license PatchPulse under Apache-2.0 and retain upstream notices.
- Use jiff calendar dates and ban chrono/time throughout the resolved dependency graph.
- Add strict TOML defaults, configuration-relative paths, collector switches, bind validation and mutually exclusive CLI actions.
- Add KB and stable update identities, source provenance, category union, severity risk ordering, coherent installation dates and stale/readiness rules.
