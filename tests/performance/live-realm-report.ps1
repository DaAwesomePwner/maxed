[CmdletBinding()]
param([Parameter(Mandatory=$true)][string]$EvidenceDir)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot '../../scripts/memory-policy.ps1')
$launch=Get-Content -LiteralPath (Join-Path $EvidenceDir 'launch.json') -Raw | ConvertFrom-Json
$phases=Get-Content -LiteralPath (Join-Path $EvidenceDir 'phases.json') -Raw | ConvertFrom-Json
$rendered=Get-Content -LiteralPath (Join-Path $EvidenceDir 'rendered.json') -Raw | ConvertFrom-Json
$summary=Get-Content -LiteralPath (Join-Path $EvidenceDir 'memory-summary.json') -Raw | ConvertFrom-Json
$totals=@(Import-Csv -LiteralPath (Join-Path $EvidenceDir 'memory-totals.csv'))
$processes=@(Import-Csv -LiteralPath (Join-Path $EvidenceDir 'memory-processes.csv'))
$fresh=$launch.mode -eq 'fresh-idle'
$busy=if($fresh){[datetimeoffset]$launch.start_utc}else{[datetimeoffset]$phases.busy}
$idle=[datetimeoffset]$phases.idle
$ongoingPath=Join-Path $EvidenceDir 'ongoing-load.json'
$ongoing=@(if(Test-Path -LiteralPath $ongoingPath){Get-Content -LiteralPath $ongoingPath -Raw | ConvertFrom-Json})
$confounded=@($ongoing | Where-Object heavy_count -gt 0).Count -gt 0
$result=[ordered]@{mode=$launch.mode;diagnostic=$launch.diagnostic;machine_quiet=$launch.machine_quiet;
    executable_sha256=$launch.executable_sha256;workload=@{fights=1536;players=24;bags=320;hits_per_second=24;busy_seconds=($idle-$busy).TotalSeconds};
    collector_memory_result=$summary.memory_result;collector_cpu_result=$summary.cpu_result;
    ongoing_load_observed=$ongoing.Count -gt 0;load_confounded=$confounded;phases=@()}
if($fresh){$result.workload=@{kind='empty temporary profile';settle_seconds=30;idle_seconds=180}}
foreach ($phase in $(if($fresh){@('idle')}else{@('busy','idle')})) {
    $start=if ($phase -eq 'busy') {$busy} else {$idle.AddSeconds(30)}
    $end=if ($phase -eq 'busy') {$idle} else {$idle.AddSeconds(175)}
    if($fresh){$start=$busy.AddSeconds(30);$end=$busy.AddSeconds(210)}
    $selected=@($totals | Where-Object { $at=[datetimeoffset]::Parse($_.utc); $at -ge $start -and $at -lt $end })
    $rows=@($processes | Where-Object { $at=[datetimeoffset]::Parse($_.utc); $at -ge $start -and $at -lt $end })
    $peak=$null; $rendererPeak=0L; $nativePeak=0L
    $peakUtc=$null
    foreach ($sample in $selected) { if ($null -eq $peak -or [long]$sample.private_bytes -gt $peak) { $peak=[long]$sample.private_bytes; $peakUtc=$sample.utc } }
    foreach ($sample in ($rows | Group-Object utc)) {
        $renderer=0L; $native=0L
        foreach ($row in $sample.Group) {
            if ($row.role -eq 'renderer') { $renderer += [long]$row.private_bytes }
            if ($row.role -eq 'Maxed') { $native += [long]$row.private_bytes }
        }
        $rendererPeak=[math]::Max($rendererPeak,$renderer); $nativePeak=[math]::Max($nativePeak,$native)
    }
    $cpu=0.0; $identities=@($rows | Where-Object role -eq 'renderer' | Group-Object pid,start_utc)
    foreach ($identity in $identities) {
        $ordered=@($identity.Group | Sort-Object {[datetimeoffset]::Parse($_.utc)})
        if ($ordered.Count -gt 1) { $cpu += [double]$ordered[-1].cpu_seconds - [double]$ordered[0].cpu_seconds }
    }
    $wall=if ($selected.Count -gt 1) { ([datetimeoffset]::Parse($selected[-1].utc)-[datetimeoffset]::Parse($selected[0].utc)).TotalSeconds } else {0}
    $metricsComplete=$summary.memory_result -eq 'measured' -and $summary.cpu_result -eq 'measured' -and $identities.Count -eq 1 -and $wall -gt 0 -and $rendered.$phase -and -not $rendered.subscription_failed
    $complete=$metricsComplete -and $wall -gt 100
    if ($phase -eq 'busy') { $complete=$complete -and ($idle-$busy).TotalSeconds -ge 600 -and $wall -ge 600 }
    $acceleration=@($rows | Where-Object role -eq 'browser' | Select-Object -ExpandProperty effective_acceleration -Unique)
    $off=$acceleration.Count -eq 1 -and $acceleration[0] -eq 'off'
    $coverage=if ($launch.diagnostic) {'CDP diagnostic; not release-budget evidence.'} else {''}
    if($fresh){$coverage='Fresh-profile floor only; does not certify live idle or installed-build acceptance.'}
    if($confounded){$coverage+=' Concurrent compiler/test/active-node load observed; repeat required.'}
    $gate=Get-MemoryReleaseGate $phase $peak $(if($off){'off'}else{'unverified'}) $complete $coverage
    # Concurrent decomposition, not independent process maxima. Identity numbers
    # are local structural ordinals; raw PID/birth records stay in temporary CSVs.
    $identityLabels=@{}; $ordinal=0
    foreach($identity in @($processes | Group-Object pid,start_utc)){$identityLabels[$identity.Name]=++$ordinal}
    $breakdowns=@()
    foreach($at in @($peakUtc,$(if($selected.Count){$selected[-1].utc}else{$null}))){
        $items=@($rows | Where-Object utc -eq $at | ForEach-Object {
            $key=$_.pid + ', ' + $_.start_utc
            [pscustomobject]@{instance=$identityLabels[$key];role=$_.role;
                process_type=$(if($_.PSObject.Properties['process_type']){$_.process_type}else{'not_recorded'});
                utility_sub_type=$(if($_.PSObject.Properties['utility_sub_type']){$_.utility_sub_type}else{'not_recorded'});
                private_bytes=[long]$_.private_bytes}
        } | Sort-Object private_bytes -Descending)
        $breakdowns+=,[pscustomobject]@{utc=$at;processes=$items}
    }
    $result.phases += [pscustomobject]@{phase=$phase;samples=$selected.Count;sampled_seconds=$wall;renderer_peak_bytes=$rendererPeak;
        maxed_peak_bytes=$nativePeak;combined_peak_bytes=$peak;renderer_mean_cores=$(if($metricsComplete){$cpu/$wall}else{$null});gate=$gate;
        concurrent_peak=$breakdowns[0];final_sample=$breakdowns[1]}
}
$result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $EvidenceDir 'live-realm-summary.json') -Encoding UTF8
$result | ConvertTo-Json -Depth 8
