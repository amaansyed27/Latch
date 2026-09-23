param(
    [int]$FixturePort = 4177,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    if (-not $IsWindows) { throw 'V0.6 physical acceptance must run on Windows.' }

    if (-not $SkipBuild) {
        cargo fmt --all -- --check
        if ($LASTEXITCODE) { throw 'cargo fmt failed' }
        cargo test --workspace
        if ($LASTEXITCODE) { throw 'cargo test failed' }
        cargo build --workspace
        if ($LASTEXITCODE) { throw 'cargo build failed' }
    }

    node fixtures/v0.6-developer-loop/test.mjs
    if ($LASTEXITCODE) { throw 'Developer-loop fixture self-test failed' }

    $approved = Join-Path $env:TEMP 'latch-v0.6-acceptance'
    New-Item -ItemType Directory -Path $approved -Force | Out-Null
    Remove-Item (Join-Path $approved 'latch-uia-test.txt') -Force -ErrorAction SilentlyContinue

    $stdout = Join-Path $approved 'fixture.stdout.log'
    $stderr = Join-Path $approved 'fixture.stderr.log'
    Remove-Item $stdout, $stderr -Force -ErrorAction SilentlyContinue
    $fixture = Start-Process node -ArgumentList @('fixtures/v0.6-developer-loop/server.mjs', $FixturePort) -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    Start-Sleep -Milliseconds 500
    if ($fixture.HasExited) {
        throw "Fixture exited early. See $stderr"
    }

    Write-Host ''
    Write-Host 'Latch V0.6 physical acceptance environment is ready.' -ForegroundColor Green
    Write-Host "Temporary folder: $approved"
    Write-Host "Browser fixture: http://127.0.0.1:$FixturePort"
    Write-Host "Fixture PID: $($fixture.Id)"
    Write-Host ''
    Write-Host 'Approve the temporary folder in Latch Desktop, then execute docs/physical-acceptance-v0.6.md from a normal ChatGPT conversation.'
    Write-Host 'The fixture intentionally starts with animationFixed=false in fixtures/v0.6-developer-loop/app.js.'
    Write-Host ''
    Write-Host 'Press Enter after the physical checklist is complete to clean up the fixture.'
    [void](Read-Host)

    if (-not $fixture.HasExited) {
        Stop-Process -Id $fixture.Id -Force
        $fixture.WaitForExit()
    }

    $notepadFile = Join-Path $approved 'latch-uia-test.txt'
    if (Test-Path $notepadFile) {
        $contents = Get-Content $notepadFile -Raw
        if ($contents -ne 'Latch semantic test') {
            throw "Notepad acceptance file did not contain the exact expected text: $notepadFile"
        }
        Write-Host 'Notepad file readback: PASS' -ForegroundColor Green
    } else {
        Write-Warning 'Notepad acceptance file was not found; physical UIA acceptance is not complete.'
    }

    Write-Host 'Harness cleanup complete. Record the physical result against the exact tested main SHA.'
} finally {
    Pop-Location
}
