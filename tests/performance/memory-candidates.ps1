[CmdletBinding(DefaultParameterSetName='Measure')]
param(
    [Parameter(Mandatory=$true, ParameterSetName='Test')][switch]$SelfTest,
    [Parameter(Mandatory=$true, ParameterSetName='Measure')][string]$Exe,
    [Parameter(Mandatory=$true, ParameterSetName='Measure')][string]$OutputDir,
    [Parameter(Mandatory=$true, ParameterSetName='Measure')][ValidatePattern('^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,79}$')][string]$CandidateId,
    [string[]]$Scenarios = @('home-idle'),
    [string]$SyntheticProfile,
    [string]$P4Fixtures,
    [ValidateRange(1,120)][int]$TransitionMinutes = 30
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot '../../scripts/memory-policy.ps1')

function Get-MemoryTarget([string]$Scenario) {
    $class = Get-MemoryWorkloadClass $Scenario
    if ($MaxedMemoryPolicy.workloads.Contains($class)) { return $MaxedMemoryPolicy.workloads[$class].investigation_bytes }
    return $null
}

function Get-CandidateEvidence($Report, $Processes, $Totals, [string]$ExpectedHash, [string]$ExpectedPreference = '') {
    $valid = $true; $reason = $null
    $preference = 'unverified'
    $roles = @('Maxed','browser','renderer','GPU','utility/other','unknown')
    $breakdown = @(); $identityPeaks = @(); $sumMaxima = $null; $peak = $null
    try {
        if ($Report.executable_sha256 -cne $ExpectedHash -or $Report.build_configuration_label -ne 'release') { throw 'Candidate identity mismatch' }
        if ($Report.samples -le 0 -or $Report.incomplete_samples -ne 0 -or
            $Report.measured_seconds -lt $Report.requested_seconds -or
            $Report.memory_result -eq 'not_measured' -or
            ($Report.result -eq 'not_measured' -and
                (-not $Report.PSObject.Properties['release_policy_version'] -or $Report.release_policy_version -ne $MaxedMemoryPolicy.version)) -or
            ($Report.PSObject.Properties['driver_result'] -and $Report.driver_result -ne 'completed') -or
            ($Report.PSObject.Properties['cpu_result'] -and $Report.cpu_result -ne 'measured') -or
            $Totals.Count -ne $Report.samples -or $Processes.Count -eq 0) { throw 'Complete positive samples required' }
        $browserRows = @($Processes | Where-Object role -eq 'browser')
        $preferences = @($browserRows | ForEach-Object effective_acceleration | Sort-Object -Unique)
        if ($browserRows.Count -eq 0 -or $preferences.Count -ne 1 -or $preferences[0] -notin @('on','off')) { throw 'Effective acceleration unverified or changed' }
        $preference = $preferences[0]
        if ($ExpectedPreference -and $preference -cne $ExpectedPreference) { throw 'Acceleration preference changed between scenarios' }
        $byTime = @{}; $born = @{}; $maxima = @{}; $identityRole = @{}
        foreach ($row in $Processes) {
            if ($row.role -notin $roles -or [string]::IsNullOrWhiteSpace($row.start_utc) -or
                [long]$row.private_bytes -le 0 -or [int]$row.pid -le 0) { throw 'Missing process identity or positive counter' }
            $start = ([datetime]$row.start_utc).ToUniversalTime().ToString('o')
            if ($born.ContainsKey([int]$row.pid) -and $born[[int]$row.pid] -cne $start) { throw 'PID identity changed' }
            $born[[int]$row.pid] = $start
            $key = "$($row.pid):$start"
            if ($identityRole.ContainsKey($key) -and $identityRole[$key] -cne $row.role) { throw 'Role changed within identity' }
            $identityRole[$key] = $row.role
            if (-not $maxima.ContainsKey($key)) { $maxima[$key] = 0L }
            $maxima[$key] = [Math]::Max($maxima[$key], [long]$row.private_bytes)
            if (-not $byTime.ContainsKey($row.utc)) { $byTime[$row.utc] = @() }
            $byTime[$row.utc] += $row
        }
        if ($byTime.Count -ne $Totals.Count) { throw 'Missing sample counters' }
        $seenTimes = @{}; $roleMax = @{}; $atPeak = @{}; $peak = 0L
        foreach ($total in $Totals) {
            if ($seenTimes.ContainsKey($total.utc) -or -not $byTime.ContainsKey($total.utc)) { throw 'Missing or duplicate total' }
            $seenTimes[$total.utc] = $true
            $sample = @($byTime[$total.utc]); $ids = @{}; $roleSum = @{}; $sum = 0L
            foreach ($role in $roles) { $roleSum[$role] = 0L }
            $rootFound = $false
            foreach ($row in $sample) {
                if ($ids.ContainsKey([int]$row.pid)) { throw 'Duplicate process counter' }
                $ids[[int]$row.pid] = $true
                if ([int]$row.pid -eq $Report.root_pid -and $row.role -eq 'Maxed' -and
                    ([datetime]$row.start_utc).ToUniversalTime() -eq ([datetime]$Report.root_start_utc).ToUniversalTime()) { $rootFound = $true }
                $sum += [long]$row.private_bytes; $roleSum[$row.role] += [long]$row.private_bytes
            }
            if (-not $rootFound -or $sample.Count -ne [int]$total.processes -or $sum -ne [long]$total.private_bytes -or $sum -le 0) { throw 'Process accounting mismatch' }
            foreach ($role in $roles) {
                if (-not $roleMax.ContainsKey($role)) { $roleMax[$role] = 0L }
                $roleMax[$role] = [Math]::Max($roleMax[$role], $roleSum[$role])
            }
            if ($sum -gt $peak) { $peak = $sum; $atPeak = $roleSum }
        }
        if ($peak -ne [long]$Report.peak_bytes) { throw 'Concurrent peak mismatch' }
        $sumMaxima = 0L
        foreach ($key in ($maxima.Keys | Sort-Object)) {
            $sumMaxima += $maxima[$key]
            $identityPeaks += [pscustomobject]@{ identity=$key; role=$identityRole[$key]; peak_bytes=$maxima[$key] }
        }
        foreach ($role in $roles) {
            $breakdown += [pscustomobject]@{ role=$role; concurrent_role_peak_bytes=$roleMax[$role]; bytes_at_total_peak=$atPeak[$role]
                identities=@($identityPeaks | Where-Object role -eq $role).Count }
        }
    } catch { $valid = $false; $reason = 'Incomplete accounting, candidate identity or verified acceleration; no verdict.' }
    $target = Get-MemoryTarget $Report.scenario
    $coverageReason = Get-MemoryCoverageReason $Report.scenario
    if ($Report.scenario -in @('p4-realistic','p4-worst')) {
        $expected = if ($Report.scenario -eq 'p4-realistic') { '400,600,20,3200,5440' } else { '3000,5000,200,24770,41920' }
        $workload = $Report.PSObject.Properties['p4_workload']
        if ($null -eq $workload) { $coverageReason = 'The exact P4 fixture and completed route workload have not been verified.' }
        else {
            $value = $workload.Value
            try {
                if ($value.generator -cne 'maxed-p4-runs-workload-v1' -or $value.shape -cne $Report.scenario.Substring(3) -or
                    $value.sha256 -cnotmatch '^[a-f0-9]{64}$' -or
                    (@($value.fights,$value.timeline,$value.bags,$value.players,$value.buckets) -join ',') -cne $expected) { throw 'Fixture mismatch' }
            } catch { $coverageReason = 'P4 workload provenance is missing or does not match the required fixture.' }
        }
    }
    $class = Get-MemoryWorkloadClass $Report.scenario
    $gate = Get-MemoryReleaseGate $class $peak $preference $valid $coverageReason
    [pscustomobject]@{
        scenario=$Report.scenario; workload_class=$class; result=$gate.result
        release_policy_version=$MaxedMemoryPolicy.version; release_threshold_bytes=$gate.threshold_bytes
        release_reason=$gate.reason
        investigation_target_bytes=$target
        investigation_result=$(if (-not $valid -or $null -ne $coverageReason) {'not_measured'} elseif ($peak -le $target) {'pass'} else {'fail'})
        investigation_reason=$(if (-not $valid) {$reason} else {$coverageReason})
        effective_acceleration=$(if ($valid) {$preference} else {'unverified'})
        concurrent_peak_bytes=$(if ($valid) {$peak} else {$null})
        sum_of_identity_maxima_bytes=$(if ($valid) {$sumMaxima} else {$null})
        role_breakdown=$(if ($valid) {$breakdown} else {@()})
        process_peaks=$(if ($valid) {$identityPeaks} else {@()}); reason=$reason
    }
}

function Invoke-SelfTest {
    function Assert($value, $label) { if (-not $value) { throw "Self-test failed: $label" } }
    Assert ((Get-MemoryTarget 'home-idle') -eq 130000000L -and (Get-MemoryTarget 'p4-worst') -eq 140000000L -and
        (Get-MemoryTarget 'startup') -eq 150000000L) 'original aspirational targets remain unchanged'
    $start = '2026-10-04T00:00:00.0000000Z'; $hash = 'a' * 64
    $report = [pscustomobject]@{ executable_sha256=$hash; build_configuration_label='release'; samples=2; incomplete_samples=0
        measured_seconds=1; requested_seconds=1; memory_result='pass'; result='pass'; root_pid=1; root_start_utc=$start; peak_bytes=130000000L; scenario='home-idle' }
    $rows = @(); $totals = @()
    foreach ($time in @('early','late')) {
        $values = if ($time -eq 'early') { @(60000000L,10000000L,40000000L,10000000L,10000000L) } else { @(10000000L,10000000L,80000000L,10000000L,10000000L) }
        $names = @('Maxed','browser','renderer','GPU','unknown')
        for ($i=0; $i -lt 5; $i++) {
            $rows += [pscustomobject]@{ utc=$time; pid=$i+1; start_utc=$start; parent_pid=$(if ($time -eq 'late') {0} else {1})
                role=$names[$i]; private_bytes=$values[$i]; effective_acceleration=$(if ($i -eq 1) {'off'} else {'unverified'}) }
        }
        $totals += [pscustomobject]@{utc=$time; private_bytes=($values | Measure-Object -Sum).Sum; processes=5}
    }
    $e = Get-CandidateEvidence $report $rows $totals $hash
    Assert ($e.result -eq 'pass' -and $e.investigation_result -eq 'pass' -and $e.concurrent_peak_bytes -eq 130000000L -and $e.sum_of_identity_maxima_bytes -eq 170000000L) 'concurrent peak, reparenting and diagnostic maxima'
    Assert ($e.role_breakdown.Count -eq 6 -and ($e.role_breakdown | Where-Object role -eq 'unknown').bytes_at_total_peak -eq 10000000L) 'unknown counted with GPU and renderer'
    # Exercise the real serialization boundary in each PowerShell edition:
    # JSON dates can become UTC DateTimes, while CSV fields remain strings.
    $csv = [IO.Path]::GetTempFileName()
    try {
        $rows | Export-Csv -LiteralPath $csv -NoTypeInformation
        $csvRows = @(Import-Csv -LiteralPath $csv)
        $jsonReport = $report | ConvertTo-Json | ConvertFrom-Json
        Assert ((Get-CandidateEvidence $jsonReport $csvRows $totals $hash).result -eq 'pass') 'JSON/CSV root identity round-trip'
        $jsonReport.root_start_utc = ([datetime]$start).ToUniversalTime()
        Assert ((Get-CandidateEvidence $jsonReport $csvRows $totals $hash).result -eq 'pass') 'UTC DateTime and CSV timestamp identify the same root'
        $jsonReport.root_start_utc = $jsonReport.root_start_utc.AddTicks(1)
        Assert ((Get-CandidateEvidence $jsonReport $csvRows $totals $hash).result -eq 'not_measured') 'different start instant remains rejected'
    } finally { Remove-Item -LiteralPath $csv -Force }
    Assert ((Get-CandidateEvidence $report $rows $totals ('b'*64)).result -eq 'not_measured') 'candidate identity mismatch'
    Assert ((Get-CandidateEvidence $report $rows $totals $hash 'on').result -eq 'not_measured') 'accidental preference change'
    $rows[6].effective_acceleration='on'
    Assert ((Get-CandidateEvidence $report $rows $totals $hash).result -eq 'not_measured') 'within-session preference change'
    $rows[6].effective_acceleration='off'
    $rows[0].private_bytes=$null
    Assert ((Get-CandidateEvidence $report $rows $totals $hash).result -eq 'not_measured') 'missing counters'
    $rows[0].private_bytes=60000000L
    Assert ((Get-CandidateEvidence $report @($rows | Select-Object -Skip 1) $totals $hash).result -eq 'not_measured') 'incomplete process sample'
    $report.incomplete_samples=1
    Assert ((Get-CandidateEvidence $report $rows $totals $hash).result -eq 'not_measured') 'collector incomplete'
    $report.incomplete_samples=0
    $report.result='not_measured'
    Assert ((Get-CandidateEvidence $report $rows $totals $hash).result -eq 'not_measured') 'legacy incomplete result remains rejected'
    $report.result='pass'
    foreach ($field in @('driver_result','cpu_result')) {
        $report | Add-Member -NotePropertyName $field -NotePropertyValue 'not_measured'
        Assert ((Get-CandidateEvidence $report $rows $totals $hash).result -eq 'not_measured') 'driver or CPU failure cannot be turned into release evidence'
        $report.PSObject.Properties.Remove($field)
    }
    foreach ($case in @(@('transitions',299999999L,'pass'),@('transitions',300000000L,'fail'),@('home-idle',200000000L,'fail'),@('unknown-workload',130000000L,'not_measured'))) {
        $report.scenario=$case[0]; $report.peak_bytes=$case[1]
        $rows[0].private_bytes=$case[1]-70000000L; $totals[0].private_bytes=$case[1]
        $boundary=Get-CandidateEvidence $report $rows $totals $hash
        Assert ($boundary.result -eq $case[2]) "owner 0.1.0 release boundary: $($case[0]) $($case[1])"
        if ($case[0] -eq 'transitions') { Assert ($boundary.investigation_result -eq 'fail') 'aspirational busy target failure is reported independently of release' }
    }
    $report.scenario='home-idle'; $report.peak_bytes=200000000L; $rows[0].private_bytes=130000000L; $totals[0].private_bytes=200000000L
    $rows[1].effective_acceleration='on'; $rows[6].effective_acceleration='on'
    $optIn=Get-CandidateEvidence $report $rows $totals $hash
    Assert ($optIn.result -eq 'not_applicable' -and $null -eq $optIn.release_threshold_bytes -and $optIn.concurrent_peak_bytes -eq 200000000L -and $optIn.investigation_result -eq 'fail') 'acceleration on reports bytes and aspirational target without a release verdict'
    $rows[1].effective_acceleration='off'; $rows[6].effective_acceleration='off'
    foreach ($scenario in @('dungeon-busy','startup')) {
        $report.scenario = $scenario
        $coverage = Get-CandidateEvidence $report $rows $totals $hash
        Assert ($coverage.result -eq 'not_measured' -and $coverage.investigation_result -eq 'not_measured' -and
            -not [string]::IsNullOrWhiteSpace($coverage.investigation_reason)) 'demo and post-readiness samples cannot pass investigation targets'
    }
    foreach ($scenario in @('home-idle','p4-realistic','p4-worst')) {
        $target = Get-MemoryTarget $scenario; $report.scenario=$scenario; $report.peak_bytes=$target
        $rows[0].private_bytes=$target-70000000L; $totals[0].private_bytes=$target
        if ($scenario -like 'p4-*') {
            $report.PSObject.Properties.Remove('p4_workload')
            Assert ((Get-CandidateEvidence $report $rows $totals $hash).investigation_result -eq 'not_measured') 'P4 label alone is not realistic workload evidence'
            Assert ((Get-CandidateEvidence $report $rows $totals $hash).result -eq 'not_measured') 'P4 release gate also requires realistic workload evidence'
            $counts = if ($scenario -eq 'p4-realistic') { @(400,600,20,3200,5440) } else { @(3000,5000,200,24770,41920) }
            $report | Add-Member -NotePropertyName p4_workload -NotePropertyValue ([pscustomobject]@{ generator='maxed-p4-runs-workload-v1'; shape=$scenario.Substring(3); sha256=$hash
                fights=$counts[0]; timeline=$counts[1]; bags=$counts[2]; players=$counts[3]; buckets=$counts[4] })
        }
        $e = Get-CandidateEvidence $report $rows $totals $hash
        Assert ($e.investigation_result -eq 'pass') "inclusive $scenario target"
        Assert ($e.result -eq $(if ($target -eq 150000000L) {'fail'} else {'pass'})) 'strict release threshold'
        $rows[0].private_bytes++; $totals[0].private_bytes++; $report.peak_bytes++
        Assert ((Get-CandidateEvidence $report $rows $totals $hash).investigation_result -eq 'fail') 'target plus one byte'
    }
    $report.scenario='home-idle'; $report.peak_bytes=200000000L; $rows[0].private_bytes=130000000L; $totals[0].private_bytes=200000000L
    Assert ((Get-CandidateEvidence $report $rows $totals $hash).result -eq 'fail') 'exactly 200M still fails the strict release gate'
    $collector = Join-Path $PSScriptRoot '../../scripts/Measure-MaxedMemory.ps1'
    # Read only the helper definitions, never the collector's dispatch/measurement.
    $source = [IO.File]::ReadAllText($collector)
    $classification = $source.Substring($source.IndexOf('function Get-ProcessClassification'), $source.IndexOf('function Read-ProcessInventory')-$source.IndexOf('function Get-ProcessClassification'))
    . ([scriptblock]::Create($classification))
    $args = '--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-gpu --in-process-gpu --enable-features=NetworkServiceInProcess2'
    Assert ((Get-ProcessClassification 'msedgewebview2.exe' $args).effective_acceleration -eq 'off') 'off verified locally'
    Assert ((Get-ProcessClassification 'msedgewebview2.exe' '--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection').effective_acceleration -eq 'on') 'on verified locally'
    Assert ((Get-ProcessClassification 'msedgewebview2.exe' '').role -eq 'unknown') 'unavailable command line remains counted unknown'
    Assert ((Get-ProcessClassification 'msedgewebview2.exe' '--type=gpu-process private-path').role -eq 'GPU') 'bounded GPU role'
    Assert ((Get-ProcessClassification 'msedgewebview2.exe' '--type=renderer private-path').role -eq 'renderer') 'bounded renderer role'
    Write-Output 'PASS: memory_candidates_preserve_accounting_and_setting; memory_targets_require_complete_samples'
}

if ($SelfTest) { Invoke-SelfTest; exit 0 }
$canReportFailure = $false
$hash = $null
try {
    $executable = (Resolve-Path -LiteralPath $Exe).ProviderPath
    if ([IO.Path]::GetFileName($executable) -ine 'maxed.exe' -or -not [IO.File]::Exists($executable)) { throw 'Select Maxed release executable' }
    if (-not [IO.Directory]::Exists($OutputDir) -or @(Get-ChildItem -LiteralPath $OutputDir -Force).Count) { throw 'Output directory must exist and be empty' }
    $canReportFailure = $true
    if (@(Get-Process -Name maxed -ErrorAction SilentlyContinue).Count) { throw 'Close Maxed before measuring' }
    foreach ($variable in @('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS','WEBVIEW2_BROWSER_EXECUTABLE_FOLDER','WEBVIEW2_USER_DATA_FOLDER')) {
        if ([Environment]::GetEnvironmentVariable($variable,'Process')) { throw 'Remove diagnostic WebView2 environment overrides before budget runs' }
    }
    $hash = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
    $destination = (Resolve-Path -LiteralPath $OutputDir).ProviderPath
    $driverArgs = @('-NoProfile','-ExecutionPolicy','Bypass','-File',(Join-Path $PSScriptRoot 'home-scenarios.ps1'),
        '-Exe',$executable,'-OutputDir',$destination,'-Scenarios',($Scenarios -join ','),'-TransitionMinutes',$TransitionMinutes)
    if ($SyntheticProfile) { $driverArgs += @('-SyntheticProfile',$SyntheticProfile) }
    if ($P4Fixtures) { $driverArgs += @('-P4Fixtures',$P4Fixtures) }
    & powershell.exe @driverArgs | Out-Null
    $driverExit = $LASTEXITCODE
    $afterHash = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
    $results = @(); $preference = ''
    foreach ($file in @(Get-ChildItem -LiteralPath $destination -Filter '*-summary.json' | Sort-Object Name)) {
        $prefix = $file.FullName.Substring(0,$file.FullName.Length-13)
        $report = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
        $rows = @(Import-Csv -LiteralPath "$prefix-processes.csv")
        $totals = @(Import-Csv -LiteralPath "$prefix-totals.csv")
        $expected = if ($hash -ceq $afterHash) {$hash} else {''}
        $result = Get-CandidateEvidence $report $rows $totals $expected $preference
        if ($result.effective_acceleration -in @('on','off')) { $preference=$result.effective_acceleration }
        $results += $result
    }
    $expectedScenarios = @($Scenarios | ForEach-Object { $_.Split(',') } | ForEach-Object { $_.Trim().ToLowerInvariant() } | Sort-Object -Unique)
    $actualScenarios = @($results | ForEach-Object scenario | Sort-Object -Unique)
    $verdict = if ($driverExit -notin @(0,1) -or $results.Count -eq 0 -or
        @(Compare-Object $expectedScenarios $actualScenarios).Count -or @($results | Where-Object result -eq 'not_measured').Count) {'not_measured'}
        elseif ($driverExit -eq 1 -or @($results | Where-Object result -eq 'fail').Count) {'fail'}
        elseif (@($results | Where-Object result -eq 'not_applicable').Count -eq $results.Count) {'not_applicable'} else {'pass'}
    $candidate = [pscustomobject]@{ candidate_id=$CandidateId; executable_sha256=$hash; driver_exit_code=$driverExit
        effective_acceleration=$(if ($verdict -eq 'not_measured') {'unverified'} else {$preference}); result=$verdict
        release_policy=$MaxedMemoryPolicy; release_comparison='strictly less than'; scenarios=$results
        limitation='Driver attaches after native-window readiness, including startup. Demo is not full busy acceptance. No settings file is read; preference derives from effective browser switches.' }
    $candidate | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $destination 'candidate.json') -Encoding utf8
    Write-Output ($candidate | ConvertTo-Json -Depth 8 -Compress)
    if ($verdict -eq 'not_measured') { exit 2 }; if ($verdict -eq 'fail') { exit 1 }; exit 0
} catch {
    if ($canReportFailure) {
        [pscustomobject]@{ candidate_id=$CandidateId; executable_sha256=$hash; result='not_measured'
            effective_acceleration='unverified'; scenarios=@(); release_policy=$MaxedMemoryPolicy
            reason='Launch, collection or report assembly failed; inspect sanitized driver diagnostics. No candidate verdict.' } |
            ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $OutputDir 'candidate.json') -Encoding utf8
    }
    Write-Output 'Candidate not measured: verify executable, fresh directory, complete reports and controlled environment. No raw paths or command lines are logged.'
    exit 2
}
