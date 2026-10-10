[CmdletBinding()]
param([Parameter(Mandatory)][string]$Version, [Parameter(Mandatory)][string]$Channel,
    [Parameter(Mandatory)][string]$Repository, [string]$Tag)
$ErrorActionPreference = 'Stop'
# Canonical SemVer; numeric prerelease identifiers may not have leading zeros.
$numeric = '(0|[1-9][0-9]*)'
$identifier = '(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
$semver = "\A$numeric\.$numeric\.$numeric(?:-$identifier(?:\.$identifier)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?\z"
if ($Version.Length -gt 128 -or $Version -cnotmatch $semver) { throw 'Invalid canonical version.' }
foreach ($part in ($Version.Split('-')[0].Split('+')[0].Split('.'))) {
    $number = [UInt64]0
    if (-not [UInt64]::TryParse($part, [ref]$number)) { throw 'Version component exceeds native SemVer range.' }
}
if ($Channel -cnotin @('stable', 'beta')) { throw 'Invalid channel.' }
if ($Channel -ceq 'stable' -and ($Version.Split('+')[0]).Contains('-')) { throw 'Stable cannot advertise a prerelease.' }
if ($Repository -cnotmatch '\A[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})/[A-Za-z0-9][A-Za-z0-9_.-]{0,99}\z' -or $Repository.EndsWith('.git')) { throw 'Invalid repository coordinate.' }
if ($PSBoundParameters.ContainsKey('Tag') -and $Tag -cne "v$Version") { throw 'Tag/version mismatch.' }
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$config = Get-Content -LiteralPath "$root/app/src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json
$ui = Get-Content -LiteralPath "$root/app/ui/package.json" -Raw | ConvertFrom-Json
Push-Location $root
try { $raw = & cargo metadata --offline --no-deps --format-version 1; if ($LASTEXITCODE) { throw 'Cargo metadata failed.' } }
finally { Pop-Location }
$packages = @((($raw -join "`n") | ConvertFrom-Json).packages | Where-Object { $_.name -eq 'maxed-app' })
if ($packages.Count -ne 1 -or $packages[0].version -cne $Version -or $config.version -cne $Version -or $ui.version -cne $Version) { throw 'Source versions do not match release.' }
