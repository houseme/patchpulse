# Windows Server Manual Acceptance Guide

Use this checklist independently on **Windows Server 2016, 2019 and 2022**, on
x86_64 hosts with the normal WMI and Windows Update Agent (WUA) services. Record
one result set per version; a green `windows-latest` CI job proves compilation
and runner tests, not behavior on these three server releases. Run operational
checks on staging machines whose service restart and scheduled reboot are
approved. PatchPulse only reads patch state; this guide does not install,
download, hide or approve Windows updates.

## 1. Record the host and prepare evidence

Open an elevated Windows PowerShell 5.1 session. Record the OS build, PowerShell
version, update policy (Windows Update or WSUS), account, time zone and the
PatchPulse commit/build identifier. Protect the evidence directory because
inventory and logs can contain host information. Do not include bearer tokens,
collector credentials or private target URLs in shared reports.

```powershell
Get-CimInstance Win32_OperatingSystem |
  Select-Object Caption, Version, BuildNumber, OSArchitecture
$PSVersionTable.PSVersion
Get-CimInstance Win32_Service -Filter "Name='Winmgmt' OR Name='wuauserv'" |
  Select-Object Name, State, StartMode
Get-TimeZone
New-Item -ItemType Directory -Force C:\PatchPulse\validation | Out-Null
```

Keep the OS edition/build and servicing policy alongside all results. An empty
WUA pending list is valid only when the backend succeeded; catalog freshness
depends on the host's own Windows Update/WSUS policy.

## 2. Build or stage the exact executable

Use the repository revision under test and Rust 1.98.1 with the Visual Studio
x64 MSVC C++ build tools, or transfer a SHA-256-verified executable produced
from that same revision on a trusted Windows/MSVC runner. A source checkout and
toolchain are required for the live integration tests below. From the repository
root, in an x64 developer shell:

```powershell
rustup show active-toolchain
cargo build --release --locked --target x86_64-pc-windows-msvc
cargo test --all-targets --all-features --locked
cargo test --locked --test windows_collectors -- --ignored --nocapture
New-Item -ItemType Directory -Force C:\PatchPulse\logs | Out-Null
Copy-Item .\target\x86_64-pc-windows-msvc\release\patchpulse.exe C:\PatchPulse\patchpulse.exe
Copy-Item .\config\patchpulse.toml C:\PatchPulse\patchpulse.toml
Copy-Item .\LICENSE, .\THIRD_PARTY_NOTICES.md C:\PatchPulse\
Get-FileHash C:\PatchPulse\patchpulse.exe -Algorithm SHA256
```

The ignored test command must report both native and embedded-PowerShell
collector tests as **passed**; a zero-test result, cross-target Clippy, or a
Windows CI run without `--ignored` is insufficient. Investigate WMI/WUA service
or account errors instead of marking a failed collector as an empty inventory.
Record compiler/toolchain versions, build exit code, tests and executable hash.

## 3. Validate configuration and service account

Keep the default `127.0.0.1:9100` bind for single-host checks. Edit
`C:\PatchPulse\patchpulse.toml` so `[observability]` has
`log_file = "C:/PatchPulse/logs/patchpulse.jsonl"`. Ensure the selected service
account can write that directory, while ordinary users cannot alter the binary,
configuration or optional trusted script. The installer defaults to LocalSystem
and refuses to replace an existing service.

```powershell
Get-Acl C:\PatchPulse\patchpulse.exe | Format-List
Get-Acl C:\PatchPulse\patchpulse.toml | Format-List
Get-Acl C:\PatchPulse\logs | Format-List
& C:\PatchPulse\patchpulse.exe --check-config --config C:\PatchPulse\patchpulse.toml
Get-Service PatchPulse -ErrorAction SilentlyContinue
```

Expected: configuration validation succeeds and no old `PatchPulse` service
exists before first installation. Check the ACLs against your deployment policy;
do not put secrets in the TOML or grant write access to untrusted users.

## 4. Install and exercise the Windows service

From the repository root, run the project scripts in the elevated session under
the organization's script-execution policy. The installer validates the config,
registers `--service` with absolute paths, selects delayed automatic start and
configures restart recovery.

```powershell
.\scripts\install-service.ps1 -BinaryPath C:\PatchPulse\patchpulse.exe `
  -ConfigPath C:\PatchPulse\patchpulse.toml
Get-Service PatchPulse
Get-CimInstance Win32_Service -Filter "Name='PatchPulse'" |
  Select-Object Name, State, StartMode, StartName, PathName
sc.exe qc PatchPulse
sc.exe qfailure PatchPulse
Stop-Service PatchPulse
Start-Service PatchPulse
Get-Service PatchPulse
```

Expected: Running after install/start, Stopped after stop, an absolute quoted
binary/config command line, delayed automatic startup and configured recovery.
Check the JSON log for privilege preflight and collector outcomes. Restricted
tokens may warn without stopping HTTP; actual backend errors decide collection
success. On a staging host only, terminate the service process once to verify
SCM recovery, then observe that it restarts. Schedule a reboot within a
maintenance window to verify automatic startup and fresh collection afterward.
Record the service state and log timestamps before and after each transition.

## 5. Verify read-only API and source data

Wait up to the configured `collector_timeout_secs` plus a small startup margin
for the first collection. `/ready` may return 503 before the first success;
afterward it can remain 200 during a later partial failure, so always inspect
`is_stale`, `backends` and `last_error` in the summary.

```powershell
$base = 'http://127.0.0.1:9100'
(Invoke-WebRequest "$base/health" -UseBasicParsing).StatusCode
(Invoke-WebRequest "$base/version" -UseBasicParsing).Content
$summary = Invoke-RestMethod "$base/patches/summary"
$summary | Select-Object total_installed, total_pending, is_stale, last_refreshed, last_error, reboot_required
$summary.backends
$installed = Invoke-RestMethod "$base/patches"
$pending = Invoke-RestMethod "$base/patches/pending"
$installed.count
$pending.count
(Invoke-WebRequest "$base/snapshot" -UseBasicParsing).StatusCode
(Invoke-WebRequest "$base/metrics" -UseBasicParsing).Content |
  Select-String 'patchpulse_collect_(success|failure|duration_seconds)'
Get-CimInstance Win32_QuickFixEngineering |
  Select-Object -First 10 HotFixID, InstalledOn, Description
```

Expected: health/version/list/summary/snapshot/metrics return 200, version
reports Windows and Apache-2.0, counts match the respective `items` arrays,
and each enabled backend reports success or an explicit error. Compare a sample
of WMI `HotFixID` values to installed records and their `sources`; compare WUA
installed/pending identities to the host's cached catalog. The API reconciles
duplicates and has wider/different coverage than QuickFixEngineering, so total
counts need not equal `Get-HotFix`. Unknown or date-only installation dates may
remain null and must not be invented. Check that installed and pending lists do
not share the same update identity.

For a read-only independent WUA reference, in the same host session:

```powershell
$session = New-Object -ComObject Microsoft.Update.Session
$searcher = $session.CreateUpdateSearcher()
$searcher.Online = $false
$wuaInstalled = $searcher.Search('IsInstalled=1')
$wuaPending = $searcher.Search('IsInstalled=0 and IsHidden=0')
$wuaInstalled.ResultCode, $wuaInstalled.Updates.Count
$wuaPending.ResultCode, $wuaPending.Updates.Count
```

WUA search and the service use the local cached catalog. Sample KB IDs, update
IDs, titles, severity, category and reboot flags rather than requiring equal
post-deduplication counts. Do not trigger an online scan or install to make a
test pass.

Check method/filter contracts; non-200 `Invoke-WebRequest` responses raise an
exception, so inspect the HTTP status in `catch`:

```powershell
function Get-Status([string]$Path, [string]$Method = 'GET') {
  try { [int](Invoke-WebRequest "$base$Path" -Method $Method -UseBasicParsing).StatusCode }
  catch { [int]$_.Exception.Response.StatusCode }
}
Get-Status '/health' 'HEAD'             # 200
Get-Status '/patches?since=invalid'     # 400
Get-Status '/patches' 'POST'             # 405; no write occurs
Get-Status '/does-not-exist'            # 404
```

## 6. Verify CSV, baseline and optional backends

```powershell
$csvPath = 'C:\PatchPulse\validation\installed.csv'
Invoke-WebRequest "$base/patches/export?format=csv" -UseBasicParsing -OutFile $csvPath
$rows = @(Import-Csv -Path $csvPath -Encoding UTF8)
$rows.Count
Get-Status '/patches/export?format=json'  # 400
Get-Status '/patches/baseline'            # 404 while baseline is disabled
```

Expected: UTF-8 CSV header and installed rows correspond to one snapshot;
embedded commas/quotes/newlines survive CSV parsing and formula-like text is
apostrophe-prefixed. An inventory can change between separate API requests, so
compare counts against a snapshot from the same collection interval.

For a baseline trial, choose one KB known installed and one confirmed absent on
the host. Set `[baseline] enabled = true`, a name and
`required_kbs = ["KB<installed>", "KB<absent>"]`, validate config, and restart
the service. `GET /patches/baseline` must report exact installed/missing/pending
KB membership and `non_compliant` while input is fresh; if any enabled source is
stale or has failed, the result is `unknown`. Restore the production baseline
settings afterward. Supersedence is not inferred.

To validate the two optional embedded PowerShell backends, set
`enable_powershell_installed = true` and `enable_powershell_pending = true` in
`[collector]`, validate config and restart. Confirm both additional backend
statuses in `/patches/summary` and rerun the ignored live tests. The embedded
script needs no PSWindowsUpdate module. Restore the intended backend switches
after testing.

## 7. Failure retention and recovery on staging only

Use a separate validation config with only the two PowerShell backends enabled,
`interval_secs = 30`, `stale_after_secs = 60`, and
`powershell_script = "C:/PatchPulse/query-patches.ps1"`. Copy the trusted
`scripts/query-patches.ps1` to that protected path before starting. After one
successful cycle, record `/snapshot` and `/patches/summary`, then replace only
that validation script with `throw 'intentional validation failure'`. Wait one
interval and inspect the same endpoints. Expected: health remains 200, errors
and staleness become visible, last successful inventory is retained, and no
new successful refresh timestamp is invented. Restore the trusted script, wait
one more cycle and confirm recovery. Do not stop WMI/WUA or change the host's
update policy to simulate failure. Restore the original config and script when
done.

## 8. Measure each Windows release under realistic inventory

Record the exact inventory size and interval. Sample `WorkingSet64` (bytes),
`PrivateMemorySize64`, cumulative `CPU` and elapsed wall time for the service
process at idle and across at least three collections. Compute average CPU as
the process CPU-seconds delta divided by wall-seconds delta, and state whether
you normalized by logical processor count. Record per-backend histogram
`patchpulse_collect_duration_seconds` and success/failure counters from
`/metrics`; histogram buckets are cumulative. The original proposal targets
**<30 MB resident memory**, **<5% collection CPU**, **<5 s WMI** and
**<20 s PowerShell**; measure and report actual values and any miss. A 5-second
bucket only proves `<=5 s`, so use a timed run when a strict boundary matters.

```powershell
$before = Get-Process -Name patchpulse | Select-Object -First 1 Id, CPU, WorkingSet64, PrivateMemorySize64
$started = Get-Date
Start-Sleep -Seconds 60
$after = Get-Process -Id $before.Id
$wallSeconds = ((Get-Date) - $started).TotalSeconds
[pscustomobject]@{
  Pid = $after.Id
  ResidentMB = $after.WorkingSet64 / 1000000
  PrivateMB = $after.PrivateMemorySize64 / 1000000
  CpuPercentOneCore = 100 * ($after.CPU - $before.CPU) / $wallSeconds
}
```

Repeat this measurement around actual collection windows, not only idle time.
For a repeatable staging run, set `interval_secs = 60` under `[collector]` in a dedicated
validation config, validate it and restart the service; observe at least three
cycles, then restore the approved interval. Record that test interval alongside
the results. The <5 s WMI and <20 s PowerShell limits apply to individual
backend invocations, not an average over successes and failures.
If the PID changes due to SCM restart, discard that CPU delta and record the
restart separately. Measure network request rate/latency from a separate client;
in-process microbenchmarks do not prove multi-client throughput.

## 9. Optional multi-host and OTLP deployment

For fleet acceptance, run at least two Windows agents with distinct identities
and an authenticated HTTPS proxy or approved private network path. Configure a
Hub with static `[[hub.agents]]` entries as in [fleet.md](fleet.md), validate its
config and poll `/agents`, `/agents/{id}/snapshot`, `/fleet/summary` and, when
enabled, `/fleet/baseline`. Confirm per-host counts, same-KB separation, fresh
and stale states, and retained data after disconnecting one test agent. Never
put credentials in target URLs; use the documented protected environment
variable with HTTPS when a bearer token is required.

For trace acceptance, enable `[observability.traces]` with a real TLS-validated
OTLP/HTTP JSON `/v1/traces` endpoint. Confirm the external collector receives
bounded HTTP/collector/Hub spans, W3C parent linkage and no inventory titles,
credentials or raw target URLs. Temporarily make the collector unavailable and
confirm sanitized error logs and unchanged patch API readiness. Restore its
normal endpoint and sample ratio after testing; see [telemetry.md](telemetry.md).

## 10. Windows container compatibility, cleanup and sign-off

On a compatible Windows Docker daemon, build the MSVC executable, stage it at
`artifacts/patchpulse.exe`, and build `Dockerfile.windows` as described in
[deployment.md](deployment.md). Check `/health`, `/ready`, `/patches/summary`,
container identity and WMI/WUA backend errors. A running container does **not**
prove host patch visibility; record Unsupported/access limits separately. Keep
native SCM mode as the host-inventory acceptance path.

After the trials, restore the approved configuration, scripts, firewall/proxy
settings and service account. Remove a disposable service with
`.\scripts\uninstall-service.ps1` only if that host is meant to return to its
pre-test state; the script preserves files and logs. Do not run it on a retained
deployment.

Sign off each OS row with: OS build, commit, executable SHA-256, service account,
WMI/WUA/PowerShell results, API/CSV/baseline/Hub/OTLP results as applicable,
SCM/reboot/container results, RSS/CPU/latency numbers, failure-recovery results,
evidence location, tester, date and a clear **pass/fail/not run** outcome.
Never convert a missing host or unrun step into a pass.

| Target OS | Build and hash | Native/PowerShell collectors | API/CSV/baseline | SCM/reboot | Performance | Hub/OTLP/container | Sign-off |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Server 2016 | Not run | Not run | Not run | Not run | Not run | Not run | Not run |
| Server 2019 | Not run | Not run | Not run | Not run | Not run | Not run | Not run |
| Server 2022 | Not run | Not run | Not run | Not run | Not run | Not run | Not run |
