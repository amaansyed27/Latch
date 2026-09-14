param([string]$Output = (Join-Path (Get-Location) 'latch-dogfood.md'))
$severity = Read-Host 'Severity (P0/P1/P2/P3)'
if ($severity -notmatch '^P[0-3]$') { throw 'Severity must be P0, P1, P2, or P3.' }
$entry = @"

## $((Get-Date).ToString('yyyy-MM-dd HH:mm zzz')) — $severity

- Task/command: $(Read-Host 'Task or command')
- Expected: $(Read-Host 'Expected')
- Actual: $(Read-Host 'Actual')
- Reproducible: $(Read-Host 'Reproducible? yes/no/sometimes')
- Logs/reference (no secrets): $(Read-Host 'Logs or reference')
"@
Add-Content -LiteralPath $Output -Value $entry
Write-Output "Saved locally: $Output"
