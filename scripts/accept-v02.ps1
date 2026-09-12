[CmdletBinding()]
param(
    [string]$RouterUrl = $(if ($env:LATCH_ROUTER_URL) { $env:LATCH_ROUTER_URL } else { 'https://latch-router.vercel.app' }),
    [switch]$FullSmoke
)

$ErrorActionPreference = 'Stop'

foreach ($name in 'LATCH_CONTROL_TOKEN', 'LATCH_DEVICE_NAME') {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
        throw "Required environment variable $name is not set"
    }
}

function Assert-True($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

$headers = @{ Authorization = "Bearer $env:LATCH_CONTROL_TOKEN" }
$requestNumber = 0
$workspace = Join-Path ([IO.Path]::GetTempPath()) "Latch-v02-acceptance-$([guid]::NewGuid())"
$processId = $null
$workspaceId = $null

function Invoke-Latch([string]$Method, [hashtable]$Params) {
    $script:requestNumber += 1
    $body = @{ id = "accept-$script:requestNumber"; version = 1; method = $Method; params = $Params } | ConvertTo-Json -Depth 8
    Invoke-RestMethod -Method Post -Headers $headers -ContentType 'application/json' -Body $body -Uri "$($RouterUrl.TrimEnd('/'))/api/devices/$deviceId/request" -TimeoutSec 40
}

try {
    $health = Invoke-RestMethod -Uri "$($RouterUrl.TrimEnd('/'))/api/health" -TimeoutSec 30
    Assert-True ($health.status -eq 'ok') 'Production health is not ok'

    $identityPath = Join-Path $env:LOCALAPPDATA 'Latch\device.json'
    Assert-True (Test-Path -LiteralPath $identityPath) 'Persisted device identity was not found'
    $deviceId = (Get-Content -Raw -LiteralPath $identityPath | ConvertFrom-Json).device_id
    $devices = (Invoke-RestMethod -Headers $headers -Uri "$($RouterUrl.TrimEnd('/'))/api/devices" -TimeoutSec 30).devices
    $matches = @($devices | Where-Object { $_.device_id -eq $deviceId -and $_.device_name -eq $env:LATCH_DEVICE_NAME })
    Assert-True ($matches.Count -eq 1 -and $matches[0].status -eq 'online') 'This persisted device is not online'

    New-Item -ItemType Directory -Path $workspace | Out-Null
    $open = Invoke-Latch 'workspace.open' @{ path = $workspace }
    Assert-True ($open.status -eq 'ok' -and $open.result.type -eq 'workspace') 'workspace.open failed'
    $workspaceId = $open.result.data.workspace_id

    $localNode = (node --version).Trim()
    $remoteNode = Invoke-Latch 'exec.run' @{ workspace_id = $workspaceId; program = 'node'; args = @('--version') }
    Assert-True ($remoteNode.status -eq 'ok' -and $remoteNode.result.type -eq 'exec') 'exec.run failed'
    Assert-True ($remoteNode.result.data.exit_code -eq 0 -and -not $remoteNode.result.data.timed_out) 'Remote node process did not exit cleanly'
    Assert-True ($remoteNode.result.data.stdout.Trim() -eq $localNode) 'Routed Node version does not match local Node'

    if ($FullSmoke) {
        $contents = 'Latch V0.2 production acceptance'
        $write = Invoke-Latch 'fs.write' @{ workspace_id = $workspaceId; path = 'proof.txt'; contents = $contents }
        Assert-True ($write.status -eq 'ok' -and $write.result.type -eq 'ack') 'fs.write failed'
        $read = Invoke-Latch 'fs.read' @{ workspace_id = $workspaceId; path = 'proof.txt' }
        Assert-True ($read.result.data.contents -ceq $contents) 'fs.read contents mismatch'
        $list = Invoke-Latch 'fs.list' @{ workspace_id = $workspaceId; path = '.' }
        Assert-True (@($list.result.data.entries | Where-Object name -eq 'proof.txt').Count -eq 1) 'fs.list did not contain proof.txt'
        $traversal = Invoke-Latch 'fs.read' @{ workspace_id = $workspaceId; path = '../outside.txt' }
        Assert-True ($traversal.status -eq 'error' -and $traversal.error.code -eq 'path_outside_workspace') 'Traversal was not rejected'
        $delete = Invoke-Latch 'fs.delete' @{ workspace_id = $workspaceId; path = 'proof.txt' }
        Assert-True ($delete.status -eq 'ok' -and $delete.result.type -eq 'ack') 'fs.delete failed'

        $started = Invoke-Latch 'exec.start' @{ workspace_id = $workspaceId; program = 'node'; args = @('-e', "console.log('ready'); setInterval(() => {}, 1000)") }
        Assert-True ($started.status -eq 'ok' -and $started.result.type -eq 'process_started') 'exec.start failed'
        $processId = $started.result.data.process_id
        $status = Invoke-Latch 'exec.status' @{ process_id = $processId }
        Assert-True ($status.result.data.state.state -eq 'running') 'Managed process was not running'
        $sawReady = $false
        foreach ($attempt in 1..20) {
            $output = Invoke-Latch 'exec.output' @{ process_id = $processId }
            if ($output.result.data.stdout.text -match 'ready') { $sawReady = $true; break }
            Start-Sleep -Milliseconds 100
        }
        Assert-True $sawReady 'Managed process output was not observable'
        $killed = Invoke-Latch 'exec.kill' @{ process_id = $processId }
        Assert-True ($killed.result.data.state.state -eq 'exited') 'Managed process did not terminate'
        $processId = $null
    }

    [pscustomobject]@{
        health = $health.status
        device_id = $deviceId
        local_node = $localNode
        routed_node = $remoteNode.result.data.stdout.Trim()
        filesystem_smoke = $(if ($FullSmoke) { 'passed' } else { 'skipped' })
        managed_process_smoke = $(if ($FullSmoke) { 'passed' } else { 'skipped' })
    } | ConvertTo-Json
} finally {
    if ($processId) {
        try { Invoke-Latch 'exec.kill' @{ process_id = $processId } | Out-Null } catch {}
    }
    if (Test-Path -LiteralPath $workspace) {
        $resolved = (Resolve-Path -LiteralPath $workspace).Path
        $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
        if ($resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path -Leaf $resolved).StartsWith('Latch-v02-acceptance-')) {
            try {
                Remove-Item -LiteralPath $resolved -Recurse -Force
            } catch [System.IO.IOException] {
                Write-Warning 'The empty acceptance workspace remains locked until latch-link exits.'
            }
        }
    }
}
