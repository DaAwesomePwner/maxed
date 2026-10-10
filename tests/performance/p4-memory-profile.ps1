# Dot-sourced by the driver. Only a declared temporary diagnostic profile may
# receive an invented fixture; owner settings/history are never inspected.
function Resolve-TemporaryMemoryPath([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path)) { throw 'A temporary diagnostic path is required' }
    $full = [IO.Path]::GetFullPath($Path)
    $temporary = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar
    if (-not $full.StartsWith($temporary, [StringComparison]::OrdinalIgnoreCase)) { throw 'Only temporary diagnostic paths are allowed' }
    $current = $full
    while ($current) {
        if ((Test-Path -LiteralPath $current) -and
            ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Diagnostic paths cannot contain reparse points' }
        $current = [IO.Path]::GetDirectoryName($current)
    }
    return $full
}

function Assert-MemoryProfileSettings([string]$Profile, [string]$ExeHash) {
    $profilePath = Resolve-TemporaryMemoryPath $Profile
    # Written by the diagnostic build orchestrator from its TAURI_CONFIG and
    # executable hash. This declaration must match the compiled directory override.
    $proof = Get-Content -LiteralPath (Resolve-TemporaryMemoryPath (Join-Path $profilePath 'memory-profile.json')) -Raw | ConvertFrom-Json
    if ($proof.executable_sha256 -cne $ExeHash -or $proof.custom_protocol -ne $true -or
        [IO.Path]::GetFullPath($proof.app_data_dir) -cne $profilePath) { throw 'Synthetic profile build provenance mismatch' }
    # Read only this proven temporary profile. PowerShell 5.1's UTF8 writer adds
    # a BOM which serde_json rejects; an unloaded catalog is a different workload.
    $bytes = [IO.File]::ReadAllBytes((Resolve-TemporaryMemoryPath (Join-Path $profilePath 'settings.json')))
    if ($bytes.Length -ge 3 -and $bytes[0] -eq 239 -and $bytes[1] -eq 187 -and $bytes[2] -eq 191) { throw 'Synthetic settings must be UTF8 without BOM' }
    $settings = [Text.UTF8Encoding]::new($false,$true).GetString($bytes) | ConvertFrom-Json
    if ($settings.version -ne 1 -or [string]::IsNullOrWhiteSpace($settings.assets_path) -or
        -not [IO.File]::Exists($settings.assets_path) -or $settings.hardware_acceleration -isnot [bool]) { throw 'Synthetic production catalog and acceleration settings are required' }
}

function Get-P4Fixture([string]$Profile, [string]$Fixtures, [string]$Name, [string]$ExeHash) {
    $profilePath = Resolve-TemporaryMemoryPath $Profile
    $fixtureRoot = Resolve-TemporaryMemoryPath $Fixtures
    if ($Name -notin @('p4-realistic','p4-worst')) { throw 'Unknown P4 workload' }
    Assert-MemoryProfileSettings $Profile $ExeHash
    $fixturePath = Resolve-TemporaryMemoryPath (Join-Path $fixtureRoot ($Name + '.sqlite3'))
    $manifest = Get-Content -LiteralPath (Resolve-TemporaryMemoryPath (Join-Path $fixtureRoot ($Name + '.json'))) -Raw | ConvertFrom-Json
    $expected = if ($Name -eq 'p4-realistic') { @(400,600,20,3200,5440) } else { @(3000,5000,200,24770,41920) }
    $counts = @($manifest.fights,$manifest.timeline,$manifest.bags,$manifest.players,$manifest.buckets)
    if ($manifest.generator -cne 'maxed-p4-runs-workload-v1' -or $manifest.shape -cne $Name.Substring(3) -or
        ($counts -join ',') -cne ($expected -join ',') -or
        $manifest.sha256 -cne (Get-FileHash -LiteralPath $fixturePath -Algorithm SHA256).Hash.ToLowerInvariant()) { throw 'P4 fixture provenance mismatch' }
    foreach ($suffix in @('','-wal','-shm')) { [void](Resolve-TemporaryMemoryPath (Join-Path $profilePath ('live.sqlite3' + $suffix))) }
    return [pscustomobject]@{ profile=$profilePath; database=$fixturePath; manifest=$manifest }
}

function Install-P4Fixture($Fixture) {
    Assert-NoMaxed
    foreach ($suffix in @('-wal','-shm')) {
        $sidecar = Join-Path $Fixture.profile ('live.sqlite3' + $suffix)
        if (Test-Path -LiteralPath $sidecar) { Remove-Item -LiteralPath $sidecar -Force }
    }
    Copy-Item -LiteralPath $Fixture.database -Destination (Join-Path $Fixture.profile 'live.sqlite3') -Force
}
