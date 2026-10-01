# SPDX-License-Identifier: Apache-2.0
param(
    [ValidateSet('Installed', 'Pending')]
    [string]$Mode = 'Pending'
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$session = New-Object -ComObject Microsoft.Update.Session
$session.ClientApplicationID = 'PatchPulse'
$searcher = $session.CreateUpdateSearcher()
# Search the locally cached catalog. PatchPulse never initiates downloads or installs.
$searcher.Online = $false
$criteria = if ($Mode -eq 'Installed') { 'IsInstalled=1' } else { 'IsInstalled=0 and IsHidden=0' }
$result = $searcher.Search($criteria)
if ([int]$result.ResultCode -ne 2) {
    throw "WUA search did not fully succeed: $($result.ResultCode)"
}
$system = New-Object -ComObject Microsoft.Update.SystemInfo
$records = @(
    foreach ($update in $result.Updates) {
        [pscustomobject]@{
            KbIds = @($update.KBArticleIDs)
            UpdateId = [string]$update.Identity.UpdateID
            Revision = [int]$update.Identity.RevisionNumber
            Title = [string]$update.Title
            Description = [string]$update.Description
            MsrcSeverity = [string]$update.MsrcSeverity
            Categories = @($update.Categories | ForEach-Object { [string]$_.Name })
            RebootRequired = [bool]$update.RebootRequired
        }
    }
)
# The envelope keeps zero and one result unambiguous in Windows PowerShell 5.1.
ConvertTo-Json -InputObject ([pscustomobject]@{
    Records = $records
    RebootRequired = [bool]$system.RebootRequired
}) -Depth 6 -Compress
