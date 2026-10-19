# Publish the release for the version in Cargo.toml, using the zip that
# tools\package.ps1 produced.
#
# Usage:  powershell -NoProfile -ExecutionPolicy Bypass -File tools\release.ps1
#
# The GitHub token is read from %USERPROFILE%\.git-credentials (the same store
# `git push` uses) and is never printed or written anywhere.
#
# Notes on the encoding traps this avoids, all of which bit by hand:
#   * the release body is written to a UTF-8 file and sent with
#     `--data-binary`, so PowerShell's default ANSI encoding cannot mangle it
#   * the body is one JSON string with \n escapes, not a here-string, so
#     backticks and quotes in it are not interpreted
#   * it uses curl.exe rather than Invoke-RestMethod, whose error output is
#     unusable here
param(
    [string]$Repo = "AntarcticBearHKL/sifa-cpa-exam-app",
    [string]$BodyFile
)
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$version = (Select-String -Path (Join-Path $root "Cargo.toml") -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$tag = "v" + (($version -split '\.')[0..1] -join '.')
$zip = Join-Path $root "dist\sfzklearning-$tag-windows-x64.zip"
if (-not (Test-Path $zip)) { throw "no zip for $tag -- run tools\package.ps1 first" }
if (-not $BodyFile) { throw "pass -BodyFile <path> with the release notes (UTF-8 markdown)" }
if (-not (Test-Path $BodyFile)) { throw "no such body file: $BodyFile" }

$cred = Get-Content "$env:USERPROFILE\.git-credentials" -ErrorAction SilentlyContinue | Select-Object -First 1
$tok = $null
if ($cred -match '://([^:]+):([^@]+)@') { $tok = $Matches[2] }
if (-not $tok) { throw "no token in .git-credentials" }

$tmp = Join-Path $env:TEMP "sfzklearning-release-$tag"
New-Item -ItemType Directory -Path $tmp -Force | Out-Null

# The body as one JSON string with escaped newlines: no here-string, so no
# PowerShell expansion, and written as UTF-8 so the Chinese survives.
$body = [System.IO.File]::ReadAllText($BodyFile, [System.Text.Encoding]::UTF8)
$escaped = $body -replace '\\', '\\' -replace '"', '\"' -replace "`r`n", '\n' -replace "`n", '\n'
$json = '{"tag_name":"' + $tag + '","target_commitish":"main","name":"' + $tag + '","draft":false,"prerelease":false,"body":"' + $escaped + '"}'
$payload = Join-Path $tmp "payload.json"
[System.IO.File]::WriteAllText($payload, $json, (New-Object System.Text.UTF8Encoding($false)))

$h = @(
    "-H", "Authorization: token $tok",
    "-H", "Accept: application/vnd.github+json",
    "-H", "User-Agent: sfzklearning-release"
)

$resp = Join-Path $tmp "create.json"
$code = & curl.exe -s -o $resp -w "%{http_code}" -X POST @h -H "Content-Type: application/json" --data-binary "@$payload" "https://api.github.com/repos/$Repo/releases"
if ($code -ne "201") {
    Write-Output ("create failed http=$code")
    Write-Output ([System.IO.File]::ReadAllText($resp))
    exit 1
}
$rel = [System.IO.File]::ReadAllText($resp) | ConvertFrom-Json
Write-Output ("release created: $($rel.tag_name) (id $($rel.id))")

$resp2 = Join-Path $tmp "asset.json"
$code2 = & curl.exe -s -o $resp2 -w "%{http_code}" -X POST @h -H "Content-Type: application/zip" --data-binary "@$zip" "https://uploads.github.com/repos/$Repo/releases/$($rel.id)/assets?name=$(Split-Path $zip -Leaf)"
if ($code2 -ne "201") {
    Write-Output ("upload failed http=$code2")
    Write-Output ([System.IO.File]::ReadAllText($resp2))
    exit 1
}
$asset = [System.IO.File]::ReadAllText($resp2) | ConvertFrom-Json
Write-Output ("asset: {0} ({1:N1} MB)" -f $asset.name, ($asset.size / 1MB))

$check = & curl.exe -s -o NUL -w "%{http_code}" -L -r 0-1048575 $asset.browser_download_url
Write-Output ("download check: http=$check")
Write-Output $asset.browser_download_url
