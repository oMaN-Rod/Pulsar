<#
.SYNOPSIS
Builds Pulsar and writes the installer, portable zip and checksums to dist\.
.PARAMETER ExpectVersion
A release tag such as v0.2.0; the build fails unless it matches Cargo.toml.
.PARAMETER SkipBuild
Package the existing target\release binaries.
#>
param(
    [string]$ExpectVersion,
    [switch]$SkipBuild
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
if ($ExpectVersion -and $ExpectVersion.TrimStart('v') -ne $version) {
    throw "Tag $ExpectVersion does not match the Cargo.toml version $version"
}

if (-not $SkipBuild) {
    cargo build --release --locked
    if ($LASTEXITCODE) { throw 'cargo build failed' }
}

function Find-Iscc {
    $command = Get-Command iscc.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    foreach ($base in @("$env:LOCALAPPDATA\Programs", ${env:ProgramFiles(x86)}, $env:ProgramFiles)) {
        foreach ($name in @('Inno Setup 7', 'Inno Setup 6')) {
            $candidate = Join-Path $base "$name\ISCC.exe"
            if (Test-Path $candidate) { return $candidate }
        }
    }
    throw 'Inno Setup (ISCC.exe) was not found; install it from https://jrsoftware.org/isinfo.php'
}

$dist = Join-Path $root 'dist'
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
$stage = Join-Path $dist 'stage'
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target\release\pulsar.exe, target\release\pulsar-settings.exe, README.md $stage
Copy-Item LICENSE (Join-Path $stage 'LICENSE.txt')

Compress-Archive -Path (Join-Path $stage '*') -DestinationPath (Join-Path $dist "pulsar-$version-portable.zip")

& (Find-Iscc) /Q "/DAppVersion=$version" "/DSourceDir=$stage" installer\pulsar.iss
if ($LASTEXITCODE) { throw 'Inno Setup failed' }

Remove-Item $stage -Recurse -Force
$sums = Get-ChildItem $dist -File | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
}
Set-Content -Path (Join-Path $dist 'SHA256SUMS.txt') -Value $sums -Encoding ascii
Get-ChildItem $dist | ForEach-Object { Write-Host $_.Name }
