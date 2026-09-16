param([string]$OutputDirectory = 'artifacts')

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$output = Join-Path $root $OutputDirectory
$expectedMsiVersion = '0.5.1'
$expectedUpgradeCode = '6B9638AD-38B8-4EA2-88EF-76D9961EBC4C'

Push-Location $root
try {
    [xml]$wixSource = Get-Content 'installer\Latch.wxs' -Raw
    $package = $wixSource.SelectSingleNode("//*[local-name()='Package']")
    if (-not $package) { throw 'installer/Latch.wxs does not contain a Package element.' }
    if ($package.Version -ne $expectedMsiVersion) {
        throw "Unexpected WiX Package Version: $($package.Version); expected $expectedMsiVersion"
    }
    if ($package.UpgradeCode -ne $expectedUpgradeCode) {
        throw "Unexpected WiX UpgradeCode: $($package.UpgradeCode)"
    }

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

    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase((Resolve-Path "$output\LatchSetup-x64.msi").Path, 0)
    $view = $database.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductVersion'")
    $view.Execute()
    $record = $view.Fetch()
    if (-not $record) { throw 'MSI ProductVersion is missing.' }
    $productVersion = $record.StringData(1)
    if ($productVersion -ne $expectedMsiVersion) {
        throw "Unexpected MSI ProductVersion: $productVersion; expected $expectedMsiVersion"
    }
    $view.Close()

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

    Write-Output "Built $output\LatchSetup-x64.msi (MSI ProductVersion $productVersion)"
} finally {
    Pop-Location
}
