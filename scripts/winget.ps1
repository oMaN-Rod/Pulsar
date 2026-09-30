<#
.SYNOPSIS
Writes winget manifests for a release into dist\winget, from dist\SHA256SUMS.txt.
#>
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $root 'dist'
$setup = "pulsar-$Version-setup.exe"
$line = Get-Content (Join-Path $dist 'SHA256SUMS.txt') | Where-Object { $_ -like "*  $setup" }
if (-not $line) { throw "$setup is not in SHA256SUMS.txt" }
$sha = ($line -split '\s+')[0].ToUpper()
$id = 'oMaN-Rod.Pulsar'
$repo = 'https://github.com/oMaN-Rod/Pulsar'
$out = Join-Path $dist "winget\$id\$Version"
New-Item -ItemType Directory -Force $out | Out-Null
$schema = '1.12.0'

@"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.version.$schema.schema.json

PackageIdentifier: $id
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $schema
"@ | Set-Content (Join-Path $out "$id.yaml") -Encoding utf8

@"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.installer.$schema.schema.json

PackageIdentifier: $id
PackageVersion: $Version
Platform:
  - Windows.Desktop
MinimumOSVersion: 10.0.22000.0
InstallerType: inno
Scope: user
InstallModes:
  - interactive
  - silent
  - silentWithProgress
UpgradeBehavior: install
ProductCode: '{8F3B6C1E-3D8A-4B8E-9C61-5A2E7D4F0B19}_is1'
ReleaseDate: $(Get-Date -Format 'yyyy-MM-dd')
Installers:
  - Architecture: x64
    InstallerUrl: $repo/releases/download/v$Version/$setup
    InstallerSha256: $sha
ManifestType: installer
ManifestVersion: $schema
"@ | Set-Content (Join-Path $out "$id.installer.yaml") -Encoding utf8

@"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.defaultLocale.$schema.schema.json

PackageIdentifier: $id
PackageVersion: $Version
PackageLocale: en-US
Publisher: oMaN-Rod
PublisherUrl: https://github.com/oMaN-Rod
PackageName: Pulsar
PackageUrl: $repo
License: GPL-3.0-or-later
LicenseUrl: $repo/blob/main/LICENSE
ShortDescription: A lightweight Windows 11 taskbar monitor for CPU, memory, disk, network, GPU and ping.
Description: Pulsar shows live graphs or compact text for CPU, memory, disk, network, GPU and ping right on the Windows 11 taskbar, with hover details, GPU temperature and a floating mode. It needs no admin rights and no drivers.
Tags:
  - taskbar
  - system-monitor
  - cpu
  - gpu
  - performance
ReleaseNotesUrl: $repo/releases/tag/v$Version
ManifestType: defaultLocale
ManifestVersion: $schema
"@ | Set-Content (Join-Path $out "$id.locale.en-US.yaml") -Encoding utf8

Write-Host "winget manifests: $out"
