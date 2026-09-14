param([string]$OutputDirectory = 'artifacts')

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$output = Join-Path $root $OutputDirectory

Push-Location $root
try {
    cargo build --release -p latch-link
    if ($LASTEXITCODE) { throw 'Release build failed.' }
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    Remove-Item "$output\LatchSetup-x64.msi", "$output\LatchSetup-x64.wixpdb", "$output\latch-windows-x64.zip", "$output\SHA256SUMS.txt" -Force -ErrorAction SilentlyContinue
    wix build installer/Latch.wxs -arch x64 -pdbtype none -d "SourceDir=$root\target\release" -d "InstallerDir=$root\installer" -o "$output\LatchSetup-x64.msi"
    if ($LASTEXITCODE) { throw 'MSI build failed.' }
    Copy-Item 'target\release\latch-link.exe' "$output\latch.exe"
    Compress-Archive -LiteralPath "$output\latch.exe" -DestinationPath "$output\latch-windows-x64.zip" -Force
    Remove-Item "$output\latch.exe"
    Get-FileHash "$output\LatchSetup-x64.msi", "$output\latch-windows-x64.zip" -Algorithm SHA256 |
        ForEach-Object { "{0}  {1}" -f $_.Hash.ToLowerInvariant(), (Split-Path $_.Path -Leaf) } |
        Set-Content "$output\SHA256SUMS.txt"
    Write-Output "Built $output\LatchSetup-x64.msi"
} finally {
    Pop-Location
}
