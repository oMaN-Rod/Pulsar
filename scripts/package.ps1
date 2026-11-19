<#
.SYNOPSIS
Builds Pulsar and writes the installer, portable zip and checksums to dist\.
.PARAMETER ExpectVersion
A release tag such as v0.2.0; the build fails unless it matches Cargo.toml.
.PARAMETER SkipBuild
Package the existing target\release binaries.
.PARAMETER BinDir
Package the binaries in this directory instead, such as signed ones; implies -SkipBuild.
.PARAMETER NoChecksums
Leave out SHA256SUMS.txt and the winget manifests, for an installer signed afterwards.
#>
param(
    [string]$ExpectVersion,
    [switch]$SkipBuild,
    [string]$BinDir,
    [switch]$NoChecksums
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
if ($ExpectVersion -and $ExpectVersion.TrimStart('v') -ne $version) {
    throw "Tag $ExpectVersion does not match the Cargo.toml version $version"
}

if ($BinDir) {
    $BinDir = (Resolve-Path $BinDir).Path
} elseif (-not $SkipBuild) {
    cargo build --release --locked
    if ($LASTEXITCODE) { throw 'cargo build failed' }
}

function Find-Iscc {
    $command = Get-Command iscc.exe -ErrorAction SilentlyContinue
    if ($command) { return $command.Source }
    foreach ($base in @("$env:LOCALAPPDATA\Programs", ${env:ProgramFiles(x86)}, $env:ProgramFiles)) {
        $candidate = Join-Path $base 'Inno Setup 7\ISCC.exe'
        if (Test-Path $candidate) { return $candidate }
    }
    throw 'Inno Setup 7 (ISCC.exe) was not found; install it from https://jrsoftware.org/isinfo.php'
}

$dist = Join-Path $root 'dist'
if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
$stage = Join-Path $dist 'stage'
New-Item -ItemType Directory -Force $stage | Out-Null
if (-not $BinDir) { $BinDir = Join-Path $root 'target\release' }
$exes = 'pulsar.exe', 'pulsar-settings.exe', 'pulsar-details.exe', 'pulsar-sensors.exe'
Copy-Item (($exes | ForEach-Object { Join-Path $BinDir $_ }) + 'README.md') $stage
Copy-Item LICENSE (Join-Path $stage 'LICENSE.txt')

Compress-Archive -Path (Join-Path $stage '*') -DestinationPath (Join-Path $dist "pulsar-$version-portable.zip")

& (Find-Iscc) /Q "/DAppVersion=$version" "/DSourceDir=$stage" installer\pulsar.iss
if ($LASTEXITCODE) { throw 'Inno Setup failed' }

Remove-Item $stage -Recurse -Force
if (-not $NoChecksums) { & (Join-Path $PSScriptRoot 'checksums.ps1') -Version $version }
Get-ChildItem $dist | ForEach-Object { Write-Host $_.Name }
