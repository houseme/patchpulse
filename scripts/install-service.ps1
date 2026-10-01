# SPDX-License-Identifier: Apache-2.0
#Requires -RunAsAdministrator
param(
    [string]$BinaryPath = 'C:\PatchPulse\patchpulse.exe',
    [string]$ConfigPath = 'C:\PatchPulse\patchpulse.toml'
)
$ErrorActionPreference = 'Stop'
$binary = (Resolve-Path -LiteralPath $BinaryPath).ProviderPath
$config = (Resolve-Path -LiteralPath $ConfigPath).ProviderPath
if ($binary.Contains('"') -or $config.Contains('"')) { throw 'Paths must not contain quotes.' }
if (Get-Service -Name PatchPulse -ErrorAction SilentlyContinue) {
    throw 'PatchPulse already exists. Use uninstall-service.ps1 before reinstalling.'
}
& $binary --config $config --check-config
if ($LASTEXITCODE -ne 0) { throw 'Configuration validation failed.' }
New-Service -Name PatchPulse -DisplayName 'PatchPulse Patch Health Service' `
    -BinaryPathName "`"$binary`" --service --config `"$config`"" `
    -StartupType Automatic -Description 'Read-only Windows patch health and Prometheus metrics.' | Out-Null
sc.exe config PatchPulse start= delayed-auto | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Delayed startup configuration failed.' }
sc.exe failure PatchPulse reset= 86400 actions= restart/5000/restart/10000/restart/30000 | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Recovery configuration failed.' }
Start-Service PatchPulse
Get-Service PatchPulse
