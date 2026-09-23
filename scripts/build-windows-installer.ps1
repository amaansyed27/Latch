param([string]$OutputDirectory = 'artifacts')

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$output = Join-Path $root $OutputDirectory
$expectedMsiVersion = '0.6.0'
$expectedBinaryVersion = 'latch 0.6.0-beta.1'
$expectedUpgradeCode = '6B9638AD-38B8-4EA2-88EF-76D9961EBC4C'
$browserSource = Join-Path $root 'runtime\browser'
$browserStage = Join-Path $output 'browser-runtime-stage'

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

    cargo build --locked --release -p latch-link -p latch-desktop
    if ($LASTEXITCODE) { throw 'Release build failed.' }
    $binaryVersion = & 'target\release\latch-link.exe' --version
    if ($binaryVersion -ne $expectedBinaryVersion) {
        throw "Unexpected binary version: $binaryVersion; expected $expectedBinaryVersion"
    }

    New-Item -ItemType Directory -Path $output -Force | Out-Null
    Remove-Item "$output\LatchSetup-x64.msi", "$output\LatchSetup-x64.wixpdb", "$output\latch-windows-x64.zip", "$output\SHA256SUMS.txt", $browserStage -Recurse -Force -ErrorAction SilentlyContinue

    # Build exactly the pinned provider that will ship. The package contains
    # playwright@1.63.0 and Chromium is installed into playwright-core so the
    # runtime never resolves `@latest` on the user's machine.
    Push-Location $browserSource
    try {
        $oldBrowsersPath = $env:PLAYWRIGHT_BROWSERS_PATH
        $env:PLAYWRIGHT_BROWSERS_PATH = '0'
        npm install --omit=dev --ignore-scripts --package-lock=false
        if ($LASTEXITCODE) { throw 'Pinned Playwright dependency install failed.' }
        npx --no-install playwright install chromium
        if ($LASTEXITCODE) { throw 'Pinned Chromium installation failed.' }
    } finally {
        if ($null -eq $oldBrowsersPath) { Remove-Item Env:PLAYWRIGHT_BROWSERS_PATH -ErrorAction SilentlyContinue }
        else { $env:PLAYWRIGHT_BROWSERS_PATH = $oldBrowsersPath }
        Pop-Location
    }

    New-Item -ItemType Directory -Path $browserStage -Force | Out-Null
    Copy-Item "$browserSource\bridge.mjs" $browserStage
    Copy-Item "$browserSource\package.json" $browserStage
    Copy-Item "$browserSource\node_modules" "$browserStage\node_modules" -Recurse
    $nodePath = (Get-Command node.exe -ErrorAction Stop).Source
    if (-not (Test-Path "$browserStage\node_modules\playwright\package.json")) { throw 'Playwright runtime missing from browser stage.' }
    if (-not (Get-ChildItem "$browserStage\node_modules\playwright-core\.local-browsers" -Recurse -Filter chrome.exe -ErrorAction SilentlyContinue | Select-Object -First 1)) {
        throw 'Pinned Chromium payload missing from browser stage.'
    }

    wix build installer/Latch.wxs `
        -arch x64 `
        -pdbtype none `
        -d "SourceDir=$root\target\release" `
        -d "IconDir=$root\crates\latch-desktop\icons" `
        -d "BrowserRuntimeDir=$browserStage" `
        -d "NodeExePath=$nodePath" `
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
    Copy-Item $nodePath "$portable\node.exe"
    Copy-Item $browserStage "$portable\browser-runtime" -Recurse
    Compress-Archive -Path "$portable\*" -DestinationPath "$output\latch-windows-x64.zip" -Force
    Remove-Item $portable -Recurse -Force

    Get-FileHash "$output\LatchSetup-x64.msi", "$output\latch-windows-x64.zip" -Algorithm SHA256 |
        ForEach-Object { "{0}  {1}" -f $_.Hash.ToLowerInvariant(), (Split-Path $_.Path -Leaf) } |
        Set-Content "$output\SHA256SUMS.txt"

    Write-Output "Built $output\LatchSetup-x64.msi (MSI ProductVersion $productVersion)"
} finally {
    Remove-Item $browserStage -Recurse -Force -ErrorAction SilentlyContinue
    Pop-Location
}
