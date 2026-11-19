<#
.SYNOPSIS
Writes dist\SHA256SUMS.txt for the files in dist\, then the winget manifests.
#>
param([Parameter(Mandatory)][string]$Version)
$ErrorActionPreference = 'Stop'
$dist = Join-Path (Split-Path -Parent $PSScriptRoot) 'dist'
$sums = Get-ChildItem $dist -File | Where-Object Name -ne 'SHA256SUMS.txt' | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
}
Set-Content -Path (Join-Path $dist 'SHA256SUMS.txt') -Value $sums -Encoding ascii
& (Join-Path $PSScriptRoot 'winget.ps1') -Version $Version
