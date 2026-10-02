# Task 07: Windows Service Lifecycle and Deployment

Requirements: F6. Dependencies: 01, 04, 05.

## Implementation steps

1. Implement the SCM dispatcher and service control handler with Starting, Running, StopPending, and Stopped transitions.
2. Propagate Stop/Shutdown to the shared application shutdown channel; report startup failures as service failures.
3. Provide administrator-only install/uninstall scripts with absolute quoted paths, delayed automatic startup, and restart policy.
4. Use --service in SCM registration. Do not register a foreground binary as a Windows service.
5. Document LocalSystem permissions, firewall access, loopback defaults, WSUS coexistence, and native-host deployment.
6. Log read-only startup token/elevation diagnostics in foreground and SCM modes;
   preserve HTTP availability and report actual collector access failures.

## Acceptance criteria

- Windows target compilation passes.
- Live installation, start/stop, reboot and collection evidence must be recorded on an actual Windows host before production release.

## Evidence

See [validation.md](../validation.md) for executed checks and platform limitations.

## Implementation record

`src/service.rs`, `scripts/install-service.ps1`, and `scripts/uninstall-service.ps1`. SCM code passes Windows target Clippy. Live SCM installation, stop/start, recovery, and service-account checks remain Windows release gates.

## Comprehensive audit

Step-by-step implementation and acceptance status is recorded in [requirements-audit.md](../requirements-audit.md). Code presence does not complete the live Windows release gates.
