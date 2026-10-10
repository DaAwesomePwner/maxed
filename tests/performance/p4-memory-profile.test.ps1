$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'p4-memory-profile.ps1')
function Assert($Value, $Label) { if (-not $Value) { throw "Self-test failed: $Label" } }
function Reject([scriptblock]$Action) {
    $rejected = $false
    try { & $Action | Out-Null } catch { $rejected = $true }
    Assert $rejected 'invalid P4 input must be rejected before app launch or profile writes'
}
$root = Join-Path ([IO.Path]::GetTempPath()) ('maxed-p4-test-' + [Guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $root
$hash = 'a' * 64
try {
    @{ executable_sha256=$hash; custom_protocol=$true; app_data_dir=$root } | ConvertTo-Json |
        Set-Content -LiteralPath (Join-Path $root 'memory-profile.json')
    $assets=Join-Path $root 'invented-assets'
    [IO.File]::WriteAllText($assets,'Invented asset presence; never launched')
    $settings=@{version=1;assets_path=$assets;hardware_acceleration=$false}|ConvertTo-Json
    $settingsPath=Join-Path $root 'settings.json'
    [IO.File]::WriteAllText($settingsPath,$settings,[Text.UTF8Encoding]::new($true))
    Reject { Assert-MemoryProfileSettings $root $hash }
    [IO.File]::WriteAllText($settingsPath,$settings,[Text.UTF8Encoding]::new($false))
    Assert-MemoryProfileSettings $root $hash
    foreach ($shape in @('realistic','worst')) {
        $name = 'p4-' + $shape
        $database = Join-Path $root ($name + '.sqlite3')
        [IO.File]::WriteAllText($database, 'Invented test fixture; never launched')
        $counts = if ($shape -eq 'realistic') { @(400,600,20,3200,5440) } else { @(3000,5000,200,24770,41920) }
        $manifest = @{ generator='maxed-p4-runs-workload-v1'; shape=$shape; sha256=(Get-FileHash -LiteralPath $database).Hash.ToLowerInvariant()
            fights=$counts[0]; timeline=$counts[1]; bags=$counts[2]; players=$counts[3]; buckets=$counts[4] }
        $manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root ($name + '.json'))
        $fixture = Get-P4Fixture $root $root $name $hash
        Assert ($fixture.manifest.fights -eq $counts[0]) 'both exact workload shapes admitted'
        Reject { Get-P4Fixture $root $root $name ('b' * 64) }
        $manifest.fights--
        $manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root ($name + '.json'))
        Reject { Get-P4Fixture $root $root $name $hash }
        $manifest.fights++
        $manifest | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $root ($name + '.json'))
        [IO.File]::AppendAllText($database, 'changed')
        Reject { Get-P4Fixture $root $root $name $hash }
    }
    Reject { Get-P4Fixture (Get-Location).Path $root 'p4-realistic' $hash }
    Reject { Get-P4Fixture $root $root 'dungeon-busy' $hash }
    Write-Output 'PASS: p4_fixture_requires_temporary_profile_exact_shape_and_build_hash'
} finally {
    # Only explicit files created by this test; no recursive cleanup.
    foreach ($file in @('settings.json','invented-assets','memory-profile.json','p4-realistic.sqlite3','p4-realistic.json','p4-worst.sqlite3','p4-worst.json')) {
        Remove-Item -LiteralPath (Join-Path $root $file) -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $root
}
