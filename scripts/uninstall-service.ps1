# SPDX-License-Identifier: Apache-2.0
#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
$service = Get-Service -Name PatchPulse -ErrorAction SilentlyContinue
if ($null -eq $service) { Write-Output 'PatchPulse is not installed.'; exit 0 }
if ($service.Status -ne 'Stopped') {
    Stop-Service -Name PatchPulse
    $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
}
sc.exe delete PatchPulse | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Service deletion failed.' }
Write-Output 'PatchPulse service removed. Configuration and logs are preserved.'
