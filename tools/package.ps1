# Build the Windows release zip for the version in Cargo.toml.
#
# Usage:  powershell -NoProfile -ExecutionPolicy Bypass -File tools\package.ps1
#
# What it guarantees, because each of these has gone wrong by hand before:
#   * the version comes from Cargo.toml, so the zip name cannot drift from it
#   * entries use forward slashes (the ZIP spec; .NET writes '\' on Windows)
#   * `data\corpus` is never shipped (it is the dev-only classifier input)
#   * the packaged data is hash-compared against `data\` before shipping
#   * the zip is extracted to a temp dir and the app is actually launched,
#     so "extract and run" is verified rather than assumed
#   * only the newest zip is kept in dist\ (older ones stay on GitHub)
param(
    [switch]$KeepOldZips
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.IO.Compression

$root = Split-Path -Parent $PSScriptRoot
$cargo = Join-Path $root "Cargo.toml"
$version = (Select-String -Path $cargo -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$tag = "v" + (($version -split '\.')[0..1] -join '.')   # 0.12.0 -> v0.12, matching the release tags
$name = "sfzklearning-$tag-windows-x64.zip"
$dist = Join-Path $root "dist"
$zip = Join-Path $dist $name
$stage = Join-Path $env:TEMP "sfzklearning-pkg-$tag"

Write-Output "version $version -> tag $tag"
New-Item -ItemType Directory -Path $dist -Force | Out-Null
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $stage -Force | Out-Null

# --- build ---------------------------------------------------------------
Push-Location $root
try {
    # No `--locked`: a version bump changes Cargo.toml, and the lock records the
    # package version, so `--locked` would refuse until the lock is refreshed.
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }

    # ...but the refreshed lock must be *committed*, so that the tree this zip
    # was built from is the tree that is actually recorded. A version bump that
    # ships without its lock leaves the next `cargo build --locked` failing.
    $dirty = & git status --porcelain -- Cargo.lock Cargo.toml
    if ($dirty) {
        throw ("Cargo.lock/Cargo.toml changed during the build and are not committed:`n" +
               ($dirty -join "`n") +
               "`nCommit them, then re-run this script.")
    }
} finally { Pop-Location }

# --- stage ---------------------------------------------------------------
foreach ($exe in @("sfzklearning.exe", "classify.exe")) {
    Copy-Item (Join-Path $root "target\release\$exe") (Join-Path $stage $exe)
}
Copy-Item (Join-Path $root "README.md") (Join-Path $stage "README.md")
Copy-Item (Join-Path $root "DESIGN.md") (Join-Path $stage "DESIGN.md")
foreach ($d in @("law", "cpa")) {
    $dst = Join-Path $stage "data\$d"
    New-Item -ItemType Directory -Path $dst -Force | Out-Null
    Copy-Item (Join-Path $root "data\$d\*") $dst -Recurse -Force
}

# --- zip with forward-slash entries --------------------------------------
if (Test-Path $zip) { Remove-Item $zip -Force }
$stream = [System.IO.File]::Create($zip)
$archive = New-Object System.IO.Compression.ZipArchive($stream, [System.IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($f in (Get-ChildItem $stage -Recurse -File)) {
        $rel = $f.FullName.Substring($stage.Length + 1) -replace '\\', '/'
        $entry = $archive.CreateEntry($rel, [System.IO.Compression.CompressionLevel]::Optimal)
        $entry.LastWriteTime = $f.LastWriteTime
        $out = $entry.Open()
        try {
            $in = [System.IO.File]::OpenRead($f.FullName)
            try { $in.CopyTo($out) } finally { $in.Dispose() }
        } finally { $out.Dispose() }
    }
} finally { $archive.Dispose(); $stream.Dispose() }

# --- verify --------------------------------------------------------------
$z = [System.IO.Compression.ZipFile]::OpenRead($zip)
$backslash = ($z.Entries | Where-Object { $_.FullName -like '*\*' }).Count
$corpus = ($z.Entries | Where-Object { $_.FullName -like '*corpus*' }).Count
$bad = ($z.Entries | Where-Object { $_.FullName.StartsWith('/') -or $_.FullName.Contains('..') }).Count
$count = $z.Entries.Count
$z.Dispose()
if ($backslash -or $corpus -or $bad) {
    throw "zip is wrong: backslash=$backslash corpus=$corpus bad=$bad"
}
Write-Output ("zip: {0} entries, {1:N1} MB" -f $count, ((Get-Item $zip).Length / 1MB))

$ex = Join-Path $env:TEMP "sfzklearning-verify-$tag"
Remove-Item $ex -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $ex -Force | Out-Null
[System.IO.Compression.ZipFile]::ExtractToDirectory($zip, $ex)

$same = 0; $diff = 0
foreach ($d in @("law", "cpa")) {
    foreach ($f in Get-ChildItem (Join-Path $root "data\$d\subjects") -File -Filter *.json) {
        $a = Join-Path "$ex\data\$d\subjects" $f.Name
        if ((Test-Path $a) -and ((Get-FileHash $f.FullName).Hash -eq (Get-FileHash $a).Hash)) { $same++ } else { $diff++ }
    }
}
if ($diff) { throw "packaged data differs from data\: $diff file(s)" }
Write-Output "data files identical to data\: $same"

$st = Join-Path $ex "_smoke"
$env:CMF_START_SESSION = "1"
$p = Start-Process -FilePath (Join-Path $ex "sfzklearning.exe") -ArgumentList @("--state-dir", $st) -WorkingDirectory $ex -PassThru
Start-Sleep -Seconds 12
$alive = -not $p.HasExited
if ($alive) { Stop-Process -Id $p.Id -Force }
Remove-Item Env:\CMF_START_SESSION
if (-not $alive) { throw "the packaged app exited during the smoke run" }
$u = [System.IO.File]::ReadAllText((Join-Path $st "user.json"), [System.Text.Encoding]::UTF8) | ConvertFrom-Json
Write-Output ("smoke run ok; fresh defaults {0}/{1}" -f $u.glass_transparency, $u.glass_blur)

# --- tidy ----------------------------------------------------------------
if (-not $KeepOldZips) {
    $old = Get-ChildItem $dist -File -Filter "sfzklearning-*.zip" | Where-Object { $_.Name -ne $name }
    if ($old) {
        $mb = [Math]::Round((($old | Measure-Object Length -Sum).Sum) / 1MB, 1)
        $old | Remove-Item -Force
        Write-Output ("removed {0} older zip(s), {1} MB (they stay on GitHub)" -f $old.Count, $mb)
    }
}
Write-Output ("sha256 " + (Get-FileHash $zip -Algorithm SHA256).Hash)
Write-Output ("zip: " + $zip)
Write-Output ("next: tools\release.ps1   (publishes $tag)")
