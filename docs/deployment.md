# Deployment and Operations

## Foreground

```sh
cargo run --locked -- --foreground --config config/patchpulse.toml
cargo run --locked -- --check-config --config config/patchpulse.toml
```

On Linux/macOS the API runs but Windows collectors explicitly report Unsupported. A live host with WMI/WUA and sufficient permissions is required for inventory readiness.

Configuration is strict TOML: unknown fields are rejected, all durations must be 1..31536000 seconds, at least one backend must be enabled, and logging must have a valid filter and json/pretty format. `--bind` overrides the configured socket. Relative custom script and log file paths resolve against the configuration file's directory. The embedded PowerShell script needs no PSWindowsUpdate module or runtime script file.

## Windows service

Build on Windows:

```powershell
cargo build --release --locked --target x86_64-pc-windows-msvc
```

Copy the executable and configuration into a protected directory. Configure `observability.log_file` to a writable absolute file path and create its parent directory. Use administrator-only writable permissions for the executable, configuration, and optional custom script. LocalSystem is the default service account; grant only the deployment's required access if choosing another account.

```powershell
.\scripts\install-service.ps1 -BinaryPath C:\PatchPulse\patchpulse.exe -ConfigPath C:\PatchPulse\patchpulse.toml
Get-Service PatchPulse
Stop-Service PatchPulse
Start-Service PatchPulse
.\scripts\uninstall-service.ps1
```

The installer validates configuration, registers `--service`, sets delayed automatic startup and restart recovery, and starts the service. It refuses to replace an existing service. Uninstallation preserves files and logs. A raw foreground process cannot be registered as an SCM service.

Foreground and SCM startup query the current token's elevation and enabled
Administrators membership and record structured privilege diagnostics. Restricted
or unobservable tokens produce warnings; they do not disable HTTP or claim that
WMI/WUA access is impossible. Actual collection errors remain authoritative.
Non-Windows preflight explicitly reports platform_supported=false.

Keep the default loopback bind unless monitoring requires a private network socket. Apply a firewall allowlist or an authenticated TLS reverse proxy when exposing it. PatchPulse performs no patch install/download operations and can coexist with WSUS and management tools. The cached WUA catalog must be refreshed by the existing Windows Update policy.

## Docker

The service also supports configured Agent/Hub operation; see [fleet.md](fleet.md)
and config/hub.toml for polling, HTTPS and machine-scoped baseline deployment.

```sh
docker build -t patchpulse:0.1.0 .
docker run --rm --read-only --cap-drop ALL --security-opt no-new-privileges \
  -p 127.0.0.1:9100:9100 patchpulse:0.1.0
docker compose up -d --build
```

The multi-stage Linux image uses Rust 1.98.1 and a scratch runtime containing only the required Debian trixie dynamic libraries, runs as numeric user 65532, includes project/dependency/Rust and system-library license notices, and probes liveness using the binary. The runtime contains no shell or curl.

For restricted networks, `cargo xtask prepare-docker-cache` prepares only the public locked crate archives and indexes. Build with `docker build --build-context cargo_cache=./target/docker-cargo-cache --build-arg CARGO_NET_OFFLINE=true -t patchpulse:0.1.0 .`. Verify the built image with `cargo xtask smoke-docker --image patchpulse:0.1.0`. See [rust-tooling.md](rust-tooling.md) for prerequisites and cache behavior. The ordinary build command remains online by default. The Linux image is useful for API integration and failure-mode verification; it does not expose a Windows host's patches and correctly returns ready=503.

To change ports, mount a matching configuration at `/etc/patchpulse/patchpulse.toml` so the health probe and server use the same port. The default CMD binds all container interfaces, while the example host port mapping binds only host loopback. Keep logs on stdout for read-only containers, or mount a writable log destination.

`Dockerfile.windows` is a separate runtime recipe. Build the Windows executable on a Windows runner and place it at `artifacts/patchpulse.exe`, then build with a compatible Windows Docker daemon:

```powershell
docker build -f Dockerfile.windows -t patchpulse:windows-0.1.0 .
```

Windows containers do not imply host patch visibility or WUA service availability. The recipe runs as ContainerUser; WMI/WUA access may be unavailable or require deployment-specific privileges. Validate container compatibility separately. Use the native SCM service on each Windows host for production host inventory.

## Monitoring and release checks

Prometheus scrape and alert examples are in `config/prometheus.yml` and `config/alerts.yml`. Monitor service liveness, stale state, collector errors, pending count, and reboot state. Historical readiness alone is not inventory freshness.

Before a Windows production release, run `cargo test --test windows_collectors -- --ignored --nocapture`, verify SCM start/stop and restart recovery on Server 2016/2019/2022, compare records with system tools, verify multiple date locales, and measure idle/collection RSS and CPU. These are live-platform gates and are not replaced by cross-compilation.
