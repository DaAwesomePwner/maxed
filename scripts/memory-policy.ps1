# Owner decision, 2026-10-04: 0.1.0 default (acceleration off) release limits.
# Owner decision, 2026-10-06 (issue #40): idle raised 150 -> 200 MB and busy
# 200 -> 300 MB after the live-realm fixes; startup is unchanged.
# See docs/release/performance.md, "0.1.0 limits". Targets remain aspirational.
# This is the single table of memory policy constants used by both collectors.
$MaxedMemoryPolicy = [ordered]@{
    version = '0.1.0'
    workloads = [ordered]@{
        idle = @{ release_bytes=200000000L; investigation_bytes=130000000L }
        busy = @{ release_bytes=300000000L; investigation_bytes=140000000L }
        startup = @{ release_bytes=150000000L; investigation_bytes=150000000L }
    }
}

function Get-MemoryWorkloadClass([string]$Scenario) {
    switch ($Scenario) {
        { $_ -in @('home-idle','idle-visible') } { return 'idle' }
        { $_ -in @('p4-realistic','p4-worst','transitions','busy-visible','route-switches','dungeon-busy','demo-busy') } { return 'busy' }
        'startup' { return 'startup' }
        default { return 'unknown' }
    }
}

function Get-MemoryCoverageReason([string]$Scenario) {
    if ($Scenario -in @('dungeon-busy','demo-busy')) { return 'The tiny demo does not measure realistic busy use.' }
    if ($Scenario -eq 'startup') { return 'Collection starts after native-window readiness; full startup memory is not measured.' }
    if ((Get-MemoryWorkloadClass $Scenario) -eq 'unknown') { return 'Unknown workload class; no memory gate is defined.' }
    return $null
}

function Get-MemoryReleaseGate([string]$WorkloadClass, $Peak, [string]$Acceleration,
    [bool]$Complete = $true, [string]$CoverageReason = '') {
    $limit = if ($Acceleration -eq 'off' -and $MaxedMemoryPolicy.workloads.Contains($WorkloadClass)) {
        $MaxedMemoryPolicy.workloads[$WorkloadClass].release_bytes
    } else { $null }
    $reason = $null
    $result = if (-not $Complete -or $null -eq $Peak -or $Peak -le 0) {
        $reason='Complete positive memory samples required.'; 'not_measured'
    } elseif (-not $MaxedMemoryPolicy.workloads.Contains($WorkloadClass)) {
        $reason='Unknown workload class; no memory gate is defined.'; 'not_measured'
    } elseif ($CoverageReason) {
        $reason=$CoverageReason; 'not_measured'
    } elseif ($Acceleration -eq 'on') {
        $reason='Acceleration on is opt-in and has no 0.1.0 release memory gate.'; 'not_applicable'
    } elseif ($Acceleration -ne 'off') {
        $reason='Effective acceleration is unverified.'; 'not_measured'
    } elseif ($Peak -lt $limit) { 'pass' } else { 'fail' }
    [pscustomobject]@{ result=$result; threshold_bytes=$limit; reason=$reason }
}
