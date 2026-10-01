# Task 08: CI, Dependency Audit, and Docker Delivery

Requirements: All F1-F9. Dependencies: 01-07.

## Implementation steps

1. Run fmt, strict clippy, unit/integration/API tests, rustdoc, dependency bans, license audit, and vulnerability audit.
2. Add Linux/macOS/Windows CI and opt-in live Windows collector tests.
3. Build a multi-stage Linux Docker image with a non-root runtime, read-only-compatible config, health check, and explicit bind.
4. Provide a Windows container recipe separately; a Linux container cannot access host WMI/WUA.
5. Build and smoke-test the local Docker image, recording health 200 and ready 503 for unsupported backends.
6. Generate third-party license inventory and notices from the resolved graph. Preserve upstream notices in distributions.
7. Write validation evidence and remaining platform/performance gates without claiming they were executed.

## Acceptance criteria

- A real local Docker image is built and tested.
- Validation report records actual commands and outcomes, separating implemented features from live Windows and performance evidence.

## Evidence

See [validation.md](../validation.md) for executed checks and platform limitations.

## Implementation record

`Dockerfile`, `Dockerfile.windows`, `compose.yaml`, `.github/workflows/ci.yml`, and dependency/Docker verification scripts. The Linux image is built locally and smoke-tested. Windows image execution and remote CI are not claimed as completed.

## Comprehensive audit

Step-by-step implementation and acceptance status is recorded in [requirements-audit.md](../requirements-audit.md). Code presence does not complete the live Windows release gates.
