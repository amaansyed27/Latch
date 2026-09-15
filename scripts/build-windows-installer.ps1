param([string]$OutputDirectory = 'artifacts')

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$output = Join-Path $root $OutputDirectory

Push-Location $root
try {
    cargo build --release -p latch-link -p latch-desktop
    if ($LASTEXITCODE) { throw 'Release build failed.' }

    New-Item -ItemType Directory -Path $output -Force | Out-Null
    Remove-Item "$output\LatchSetup-x64.msi", "$output\LatchSetup-x64.wixpdb", "$output\latch-windows-x64.zip", "$output\SHA256SUMS.txt" -Force -ErrorAction SilentlyContinue

    wix build installer/Latch.wxs `
        -arch x64 `
        -pdbtype none `
        -d "SourceDir=$root\target\release" `
        -d "IconDir=$root\crates\latch-desktop\icons" `
        -o "$output\LatchSetup-x64.msi"
    if ($LASTEXITCODE) { throw 'MSI build failed.' }

    $portable = Join-Path $output 'portable'
    Remove-Item $portable -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Path $portable -Force | Out-Null
    Copy-Item 'target\release\latch-link.exe' "$portable\latch.exe"
    Copy-Item 'target\release\LatchDesktop.exe' "$portable\LatchDesktop.exe"
    Compress-Archive -Path "$portable\*" -DestinationPath "$output\latch-windows-x64.zip" -Force
    Remove-Item $portable -Recurse -Force

    Get-FileHash "$output\LatchSetup-x64.msi", "$output\latch-windows-x64.zip" -Algorithm SHA256 |
        ForEach-Object { "{0}  {1}" -f $_.Hash.ToLowerInvariant(), (Split-Path $_.Path -Leaf) } |
        Set-Content "$output\SHA256SUMS.txt"

    Write-Output "Built $output\LatchSetup-x64.msi"
} finally {
    Pop-Location
}
