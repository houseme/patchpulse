# PatchPulse

An Apache-2.0 read-only Windows patch health service. It combines WMI and Windows Update Agent data into cached HTTP responses and Prometheus metrics. Rust edition 2024, jiff dates, English code comments, and no chrono/time crates.

```sh
cargo run --locked -- --foreground --config config/patchpulse.toml
```

Windows collectors require a real Windows host. Linux/macOS builds and the Linux Docker image keep the API available while explicitly reporting unsupported collection and `/ready` = 503.

```sh
docker build -t patchpulse:0.1.0 .
docker run --rm -p 127.0.0.1:9100:9100 patchpulse:0.1.0
```

Endpoints: `/health`, `/ready`, `/version`, `/patches`, `/patches/pending`, `/patches/summary`, `/metrics`. No endpoint downloads or installs updates.

- [Requirements and completion audit](docs/requirements-audit.md)
- [English implementation tasks](docs/tasks/README.md)
- [Changelog](CHANGELOG.md)
- [Architecture](docs/architecture.md) and [API contract](docs/api.md)
- [Windows service and Docker deployment](docs/deployment.md)
- [Review findings and performance refactor](docs/review-refactor.md)
- [Validation evidence and remaining release gates](docs/validation.md)
- [Rust development tools](docs/rust-tooling.md)
- [Dependency license inventory](docs/dependency-licenses.md) and [third-party notices](THIRD_PARTY_NOTICES.md)

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
cargo deny check
cargo audit
cargo xtask dependency-policy
```

PatchPulse itself is licensed only under [Apache-2.0](LICENSE). Dependencies retain their upstream licenses and notices; jiff is used under MIT.
