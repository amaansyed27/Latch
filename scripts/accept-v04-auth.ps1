param(
    [Parameter(Mandatory)]
    [ValidatePattern('^https://')]
    [string]$RouterUrl,
    [switch]$PairLocalDevice,
    [string]$AuthorizationUrl
)

$ErrorActionPreference = 'Stop'
$cookiePath = Join-Path ([System.IO.Path]::GetTempPath()) ("latch-auth-{0}.txt" -f [guid]::NewGuid().ToString('N'))
$headerPath = Join-Path ([System.IO.Path]::GetTempPath()) ("latch-oauth-{0}.txt" -f [guid]::NewGuid().ToString('N'))

try {
    $email = if ($env:LATCH_TEST_EMAIL) { $env:LATCH_TEST_EMAIL } else { "latch.acceptance.$([guid]::NewGuid().ToString('N'))@example.com" }
    $password = if ($env:LATCH_TEST_PASSWORD) { $env:LATCH_TEST_PASSWORD } else { "La!$([guid]::NewGuid().ToString('N'))9" }
    $payload = @{ email = $email; password = $password; name = 'Latch Acceptance' } | ConvertTo-Json -Compress
    $signup = $payload | vercel curl "$RouterUrl/api/auth/sign-up/email" -- --silent --cookie-jar $cookiePath --header "Origin: $RouterUrl" --header 'content-type: application/json' --data-binary '@-'
    $signupBody = $signup | ConvertFrom-Json
    if ($signupBody.user.id) {
        Write-Output 'managed_signup=ok'
    } elseif ($env:LATCH_TEST_EMAIL) {
        $signin = $payload | vercel curl "$RouterUrl/api/auth/sign-in/email" -- --silent --cookie-jar $cookiePath --header "Origin: $RouterUrl" --header 'content-type: application/json' --data-binary '@-'
        $signinBody = $signin | ConvertFrom-Json
        if (-not $signinBody.user.id) { throw 'Managed sign-in failed.' }
        Write-Output 'managed_signin=ok'
    } else {
        throw "Managed sign-up failed (fields: $($signupBody.psobject.Properties.Name -join ','))."
    }

    $session = vercel curl "$RouterUrl/api/auth/get-session" -- --silent --cookie $cookiePath --header "Origin: $RouterUrl"
    $sessionBody = $session | ConvertFrom-Json
    if (-not $sessionBody.user.id) { throw 'Managed session was not established.' }
    Write-Output 'managed_session=ok'

    if ($PairLocalDevice) {
        $pairing = vercel curl "$RouterUrl/api/pairing/create" -- --silent --cookie $cookiePath --header "Origin: $RouterUrl" --request POST
        $pairingBody = $pairing | ConvertFrom-Json
        if (-not $pairingBody.pairing_code) { throw 'Pairing code creation failed.' }
        $previousRouterUrl = $env:LATCH_ROUTER_URL
        try {
            $env:LATCH_ROUTER_URL = $RouterUrl
            & (Join-Path $PSScriptRoot '..\target\debug\latch-link.exe') pair $pairingBody.pairing_code
            if ($LASTEXITCODE) { throw "latch-link pairing failed with exit code $LASTEXITCODE." }
        } finally {
            $env:LATCH_ROUTER_URL = $previousRouterUrl
        }
        Write-Output 'local_device_pairing=ok'
    }

    if ($AuthorizationUrl) {
        $consentPage = vercel curl $AuthorizationUrl -- --silent --cookie $cookiePath --cookie-jar $cookiePath
        $fields = @{}
        foreach ($match in [regex]::Matches($consentPage, '<input type="hidden" name="([^"]+)" value="([^"]*)">')) {
            $fields[[Net.WebUtility]::HtmlDecode($match.Groups[1].Value)] = [Net.WebUtility]::HtmlDecode($match.Groups[2].Value)
        }
        if (-not $fields.csrf) { throw 'OAuth consent page did not contain CSRF state.' }
        $curlArgs = @('curl', "$RouterUrl/oauth/authorize", '--', '--silent', '--cookie', $cookiePath, '--header', "Origin: $RouterUrl", '--request', 'POST', '--dump-header', $headerPath, '--output', 'NUL')
        foreach ($entry in $fields.GetEnumerator()) { $curlArgs += @('--data-urlencode', "$($entry.Key)=$($entry.Value)") }
        $curlArgs += @('--data-urlencode', 'approve=yes')
        & vercel @curlArgs
        if ($LASTEXITCODE) { throw 'OAuth consent request failed.' }
        $location = ((Get-Content -LiteralPath $headerPath) | Where-Object { $_ -match '^location:' } | Select-Object -Last 1) -replace '^location:\s*', ''
        if ($location -notmatch '[?&]code=' -or $location -notmatch '[?&]state=') { throw 'OAuth redirect is missing required callback parameters.' }
        $callback = Invoke-WebRequest $location -SkipHttpErrorCheck
        if ($callback.StatusCode -ge 400) { throw "OAuth callback failed with HTTP $($callback.StatusCode)." }
        Write-Output 'oauth_consent_callback=ok'
    }
} finally {
    if (Test-Path -LiteralPath $cookiePath) { Remove-Item -LiteralPath $cookiePath -Force }
    if (Test-Path -LiteralPath $headerPath) { Remove-Item -LiteralPath $headerPath -Force }
}
