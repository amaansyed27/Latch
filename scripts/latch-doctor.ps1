param([string]$RouterUrl = 'https://latch-router.vercel.app')
$ErrorActionPreference = 'Stop'
$failed = $false
function Check($name, [scriptblock]$run) {
    try { $value = & $run; Write-Output ("{0,-24} OK {1}" -f $name, $value) }
    catch { $script:failed = $true; Write-Output ("{0,-24} FAIL {1}" -f $name, $_.Exception.Message) }
}
Write-Output 'Latch doctor (credentials are never displayed)'
Check 'Router health' { $h = Invoke-RestMethod "$RouterUrl/api/health"; if ($h.status -ne 'ok') { throw 'unhealthy' }; $h.transport }
Check 'OAuth metadata' { $m = Invoke-RestMethod "$RouterUrl/.well-known/oauth-authorization-server"; if (-not $m.token_endpoint) { throw 'incomplete' }; 'available' }
Check 'Protected resource' { $m = Invoke-RestMethod "$RouterUrl/.well-known/oauth-protected-resource"; if ($m.resource -ne "$RouterUrl/mcp") { throw 'wrong audience' }; 'available' }
Check 'Node' { node --version }
Check 'Cargo' { (cargo --version) }
Check 'Latch Link build' { cargo build -q -p latch-link; 'buildable' }
$identityPath = Join-Path $env:LOCALAPPDATA 'Latch\device.json'
Check 'Device identity' { $id = (Get-Content -Raw -LiteralPath $identityPath | ConvertFrom-Json).device_id; if (-not $id) { throw 'invalid' }; $id }
Check 'Credential Manager' { $id = (Get-Content -Raw -LiteralPath $identityPath | ConvertFrom-Json).device_id; if (-not ((cmdkey /list) -match [regex]::Escape($id))) { throw 'credential not found' }; 'credential present' }
if ($failed) { exit 1 }
