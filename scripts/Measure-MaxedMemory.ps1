[CmdletBinding(DefaultParameterSetName='Measure')]
param(
    [Parameter(Mandatory=$true, ParameterSetName='Test')][switch]$SelfTest,
    [Parameter(Mandatory=$true, ParameterSetName='Measure')][int]$RootProcessId,
    [int]$Seconds = 180,
    [int]$IntervalMs = 1000,
    [string]$Scenario = 'idle-visible',
    [string]$WorkloadScenario = '',
    [ValidateSet('release','debug')][string]$BuildConfiguration = 'release',
    [Parameter(Mandatory=$true, ParameterSetName='Measure')][string]$OutputPrefix
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'memory-policy.ps1')

# Raw command lines stay local to discovery. Only these bounded values leave it.
function Get-ProcessClassification([string]$Name, [string]$CommandLine) {
    $role = 'unknown'; $acceleration = 'unverified'
    if ($Name -ieq 'maxed.exe') { $role = 'Maxed' }
    elseif ($Name -ieq 'msedgewebview2.exe') {
        if ([string]::IsNullOrWhiteSpace($CommandLine)) { $role = 'unknown' }
        elseif ($CommandLine -match '(?:^|\s)--type=renderer(?:\s|$)') { $role = 'renderer' }
        elseif ($CommandLine -match '(?:^|\s)--type=gpu-process(?:\s|$)') { $role = 'GPU' }
        elseif ($CommandLine -match '(?:^|\s)--type=') { $role = 'utility/other' }
        else {
            $role = 'browser'
            $features = [regex]::Matches($CommandLine, '(?:^|\s)--disable-features=([^\s"]+)')
            $defaults = $features.Count -eq 1 -and
                @('msWebOOUI','msPdfOOUI','msSmartScreenProtection' | Where-Object {
                    $_ -notin $features[0].Groups[1].Value.Split(',') }).Count -eq 0
            $off = $CommandLine -match '(?:^|\s)--disable-gpu(?:\s|$)' -and
                $CommandLine -match '(?:^|\s)--in-process-gpu(?:\s|$)' -and
                $CommandLine -match '(?:^|\s)--enable-features=[^\s"]*\bNetworkServiceInProcess2\b'
            if ($defaults -and $off) { $acceleration = 'off' }
            elseif ($defaults -and $CommandLine -notmatch '(?:^|\s)--(?:disable-gpu|in-process-gpu|enable-features)(?:[=\s]|$)') {
                $acceleration = 'on'
            }
        }
    } elseif ($Name) { $role = 'utility/other' }
    # Keep raw command lines private; export only bounded Chromium identifiers.
    $processType = ''; $utilitySubtype = ''
    if ($Name -ieq 'msedgewebview2.exe' -and $CommandLine) {
        $processType = 'browser'
        if ($CommandLine -match '(?:^|\s)--type="?([a-z][a-z0-9_-]{0,63})(?:"?)(?:\s|$)') { $processType = $Matches[1] }
        elseif ($CommandLine -match '(?:^|\s)--type=') { $processType = 'unknown' }
        if ($CommandLine -match '(?:^|\s)--utility-sub-type="?([A-Za-z][A-Za-z0-9_.:-]{0,127})(?:"?)(?:\s|$)') { $utilitySubtype = $Matches[1] }
    }
    [pscustomobject]@{ role=$role; effective_acceleration=$acceleration; process_type=$processType; utility_sub_type=$utilitySubtype }
}

function Read-ProcessInventory {
    @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,CreationDate,Name,CommandLine | ForEach-Object {
        $classification = Get-ProcessClassification $_.Name $_.CommandLine
        [pscustomobject]@{ ProcessId=$_.ProcessId; ParentProcessId=$_.ParentProcessId
            CreationDate=$_.CreationDate; role=$classification.role
            process_type=$classification.process_type; utility_sub_type=$classification.utility_sub_type
            effective_acceleration=$classification.effective_acceleration }
    })
}

function Resolve-TreeIds($Processes, [int[]]$Seeds) {
    $ids = [System.Collections.Generic.HashSet[int]]::new()
    foreach ($seed in $Seeds) { [void]$ids.Add($seed) }
    do {
        $changed = $false
        foreach ($entry in $Processes) {
            if ($ids.Contains([int]$entry.ParentProcessId)) {
                if ($ids.Add([int]$entry.ProcessId)) { $changed = $true }
            }
        }
    } while ($changed)
    return @($ids)
}

# This seam is injected by SelfTest, including failures after successful inventory.
function Read-Counters([int]$ProcessId) {
    $process = Get-Process -Id $ProcessId -ErrorAction Stop
    try {
        $process.Refresh()
        $born = $process.StartTime.ToUniversalTime()
        $private = $process.PrivateMemorySize64
        $cpu = $process.TotalProcessorTime.TotalSeconds
        $name = switch ($process.ProcessName) {
            'maxed' { 'maxed' }
            'msedgewebview2' { 'msedgewebview2' }
            default { 'other-descendant' }
        }
        $version = $null
        if ($name -eq 'msedgewebview2') {
            $version = $process.MainModule.FileVersionInfo.FileVersion
            if ($version -notmatch '^\d+(\.\d+){1,3}$') { throw 'Runtime version unavailable' }
        }
        if ($process.HasExited) { throw 'Process exited during counter query' }
        [pscustomobject]@{ Born=$born; Private=$private; Cpu=$cpu; Name=$name; Version=$version }
    } finally { $process.Dispose() }
}

function Read-TreeSample($Inventory, [hashtable]$Known, [int]$RootId, [datetime]$RootStart, [scriptblock]$CounterReader) {
    $listed = @{}
    foreach ($entry in $Inventory) {
        $key = [int]$entry.ProcessId
        if ($listed.ContainsKey($key)) { throw 'Duplicate inventory identity' }
        $listed[$key] = $entry
    }
    if (-not $listed.ContainsKey($RootId)) { throw 'Root exited' }
    # Retain descendants across reparenting. Keep tombstones too: observed PID reuse
    # invalidates the measurement instead of admitting a different process.
    $seeds = @($Known.Keys | Where-Object { $listed.ContainsKey($_) })
    $cache = @{}
    foreach ($key in $seeds) {
        $counter = & $CounterReader $key
        if ($counter.Born -ne $Known[$key]) { throw 'Known identity changed' }
        $cache[$key] = $counter
    }
    $ids = @(Resolve-TreeIds $Inventory $seeds)
    $rows = [System.Collections.Generic.List[object]]::new()
    $additions = @{}
    foreach ($key in $ids) {
        if (-not $listed.ContainsKey($key)) { throw 'Inventory identity missing' }
        $counter = if ($cache.ContainsKey($key)) { $cache[$key] } else { & $CounterReader $key }
        $entry = $listed[$key]
        if ($null -eq $entry.CreationDate -or $null -eq $counter.Born -or
            [Math]::Abs(($counter.Born - $entry.CreationDate.ToUniversalTime()).TotalMilliseconds) -gt .01 -or
            $counter.Born -lt $RootStart) { throw 'Inventory identity changed' }
        if ($Known.ContainsKey($key) -and $Known[$key] -ne $counter.Born) { throw 'PID reused' }
        if ($null -eq $counter.Private -or $counter.Private -le 0 -or $null -eq $counter.Cpu -or
            $counter.Cpu -lt 0 -or [double]::IsNaN($counter.Cpu) -or [double]::IsInfinity($counter.Cpu)) {
            throw 'Counters unavailable'
        }
        $additions[$key] = $counter.Born
        $rows.Add([pscustomobject]@{
            pid=$key; parent_pid=[int]$entry.ParentProcessId; born=$counter.Born
            name=$counter.Name; private_bytes=[long]$counter.Private
            cpu_seconds=[double]$counter.Cpu; runtime_version=$counter.Version
            role=$(if ($entry.PSObject.Properties['role']) { $entry.role } else { 'unknown' })
            process_type=$(if ($entry.PSObject.Properties['process_type']) { $entry.process_type } else { '' })
            utility_sub_type=$(if ($entry.PSObject.Properties['utility_sub_type']) { $entry.utility_sub_type } else { '' })
            effective_acceleration=$(if ($entry.PSObject.Properties['effective_acceleration']) { $entry.effective_acceleration } else { 'unverified' })
        })
    }
    # Reject stale parent links: a child cannot precede its observed parent.
    foreach ($row in $rows) {
        if ($additions.ContainsKey($row.parent_pid) -and $row.born -lt $additions[$row.parent_pid]) {
            throw 'Parent identity changed'
        }
    }
    foreach ($key in $additions.Keys) { $Known[$key] = $additions[$key] }
    return @($rows.ToArray())
}

function Measure-CpuInterval($Previous, $Current, [double]$ElapsedSeconds, [datetime]$PreviousInventoryStartUtc, [int]$Cores) {
    if ($ElapsedSeconds -le 0 -or $Cores -lt 1) { throw 'Invalid CPU interval' }
    $before = @{}; $after = @{}
    foreach ($row in $Previous) { $before[$row.pid] = $row }
    foreach ($row in $Current) { $after[$row.pid] = $row }
    # An exited process's final CPU counter is unknowable through polling.
    # Do not silently call its missing CPU time zero.
    foreach ($key in $before.Keys) { if (-not $after.ContainsKey($key)) { return $null } }
    [double]$delta = 0
    foreach ($key in $after.Keys) {
        $row = $after[$key]
        if ($before.ContainsKey($key)) {
            if ($row.born -ne $before[$key].born) { throw 'CPU identity changed' }
            $increment = $row.cpu_seconds - $before[$key].cpu_seconds
            if ($increment -lt 0) { throw 'CPU counter decreased' }
            $delta += $increment
        } elseif ($row.born -ge $PreviousInventoryStartUtc) {
            $delta += $row.cpu_seconds
        } else { return $null }
    }
    [pscustomobject]@{
        seconds=$delta; one_core_percent=100 * $delta / $ElapsedSeconds
        machine_percent=100 * $delta / $ElapsedSeconds / $Cores
    }
}

function Get-Verdict([int]$Samples, [int]$Incomplete, $Peak, [bool]$DurationComplete,
    [string]$WorkloadClass = 'unknown', [string]$Acceleration = 'unverified', [string]$CoverageReason = '') {
    return (Get-MemoryReleaseGate $WorkloadClass $Peak $Acceleration ($Samples -gt 0 -and $Incomplete -eq 0 -and $DurationComplete) $CoverageReason).result
}

# PowerShell 5.1 emits no Measure-Object result for an empty pipeline. Under
# StrictMode, reading .Sum then throws before the not_measured report is saved.
function Get-CpuStatistics($CpuRows) {
    [double]$seconds = 0; [double]$wall = 0
    $oneCorePeak = $null; $machinePeak = $null
    foreach ($row in $CpuRows) {
        $seconds += $row.cpu_seconds_delta
        $wall += $row.interval_seconds
        if ($null -eq $oneCorePeak -or $row.cpu_one_core_percent -gt $oneCorePeak) { $oneCorePeak = $row.cpu_one_core_percent }
        if ($null -eq $machinePeak -or $row.cpu_machine_percent -gt $machinePeak) { $machinePeak = $row.cpu_machine_percent }
    }
    [pscustomobject]@{ seconds=$seconds; wall=$wall; one_core_peak=$oneCorePeak; machine_peak=$machinePeak }
}

function Invoke-SelfTest {
    function Assert($Condition, [string]$Label) { if (-not $Condition) { throw "Self-test failed: $Label" } }
    function Assert-Throws([scriptblock]$Action, [string]$Label) {
        $threw = $false
        try { & $Action | Out-Null } catch { $threw = $true }
        Assert $threw $Label
    }
    $born = ([datetime]'2026-01-01T00:00:00Z').ToUniversalTime()
    $inventory = @(
        [pscustomobject]@{ ProcessId=1; ParentProcessId=0; CreationDate=$born },
        [pscustomobject]@{ ProcessId=2; ParentProcessId=1; CreationDate=$born.AddSeconds(1) },
        [pscustomobject]@{ ProcessId=3; ParentProcessId=2; CreationDate=$born.AddSeconds(2) },
        [pscustomobject]@{ ProcessId=9; ParentProcessId=0; CreationDate=$born }
    )
    $fakeCounters = @{}
    foreach ($entry in $inventory) {
        $fakeCounters[[int]$entry.ProcessId] = [pscustomobject]@{
            Born=$entry.CreationDate; Private=10000000; Cpu=1.0; Name='synthetic'; Version=$null
        }
    }
    $query = { param($key) $fakeCounters[$key].PSObject.Copy() }
    $known = @{ 1=$born }
    $first = @(Read-TreeSample $inventory $known 1 $born $query)
    Assert (($first.pid | Sort-Object) -join ',' -eq '1,2,3') 'grandchildren and unrelated WebView exclusion'
    $inventory[2].ParentProcessId = 0
    $inventory = @($inventory | Where-Object ProcessId -ne 2)
    $next = @(Read-TreeSample $inventory $known 1 $born $query)
    Assert (($next.pid | Sort-Object) -join ',' -eq '1,3') 'retention after parent exit and churn'
    Assert ($null -eq (Measure-CpuInterval $first $next 1 $born 24)) 'exited child CPU is unknown'
    $denied = { param($key) if ($key -eq 3) { throw 'denied' }; & $query $key }
    Assert-Throws { Read-TreeSample $inventory $known 1 $born $denied } 'denied counter seam'
    $exited = { param($key) if ($key -eq 3) { throw 'exited between inventory and query' }; & $query $key }
    Assert-Throws { Read-TreeSample $inventory $known 1 $born $exited } 'child exit seam'
    $missing = { param($key) $value = & $query $key; $value.Private = $null; $value }
    Assert-Throws { Read-TreeSample $inventory $known 1 $born $missing } 'missing private counter'
    $missingCpu = { param($key) $value = & $query $key; $value.Cpu = $null; $value }
    Assert-Throws { Read-TreeSample $inventory $known 1 $born $missingCpu } 'missing CPU counter'
    $reused = { param($key) $value = & $query $key; if ($key -eq 3) { $value.Born = $born.AddSeconds(9) }; $value }
    Assert-Throws { Read-TreeSample $inventory $known 1 $born $reused } 'descendant PID reuse'
    $rootReused = { param($key) $value = & $query $key; if ($key -eq 1) { $value.Born = $born.AddSeconds(9) }; $value }
    Assert-Throws { Read-TreeSample $inventory $known 1 $born $rootReused } 'root PID reuse'
    Assert-Throws { Read-TreeSample @($inventory | Where-Object ProcessId -ne 1) $known 1 $born $query } 'root exit'
    $badInventory = @($inventory | ForEach-Object { $_.PSObject.Copy() })
    $badInventory[1].CreationDate = $born.AddSeconds(99)
    Assert-Throws { Read-TreeSample $badInventory $known 1 $born $query } 'inventory-counter identity race'
    $newInventory = @($inventory + [pscustomobject]@{ ProcessId=5; ParentProcessId=3; CreationDate=$born.AddSeconds(5) })
    $fakeCounters[5] = [pscustomobject]@{ Born=$born.AddSeconds(5); Private=10000000; Cpu=.2; Name='synthetic'; Version=$null }
    $added = @(Read-TreeSample $newInventory $known 1 $born $query)
    Assert (($added.pid | Sort-Object) -join ',' -eq '1,3,5') 'new child of retained descendant'
    $newInventory[-1].CreationDate = $born.AddSeconds(6)
    Assert-Throws { Read-TreeSample $newInventory @{ 1=$born; 3=$born.AddSeconds(2) } 1 $born $query } 'new child identity race'
    $reappeared = @($inventory + [pscustomobject]@{ ProcessId=2; ParentProcessId=0; CreationDate=$born.AddSeconds(9) })
    $fakeCounters[2].Born = $born.AddSeconds(9)
    Assert-Throws { Read-TreeSample $reappeared $known 1 $born $query } 'exited PID tombstone reuse'
    $later = @($next | ForEach-Object { $copy=$_.PSObject.Copy(); $copy.cpu_seconds += 0.1; $copy })
    $cpu = Measure-CpuInterval $next $later 2 $born.AddSeconds(3) 4
    Assert ([Math]::Abs($cpu.one_core_percent - 10) -lt .0001) 'CPU uses deltas and elapsed wall time'
    Assert ([Math]::Abs($cpu.machine_percent - 2.5) -lt .0001) 'CPU normalization'
    $newChild = [pscustomobject]@{ pid=4; born=$born.AddSeconds(4); cpu_seconds=.2 }
    $newCpu = Measure-CpuInterval $next @($later + $newChild) 2 $born.AddSeconds(3) 4
    Assert ([Math]::Abs($newCpu.one_core_percent - 20) -lt .0001) 'new process CPU included'
    $inventoryStart = $born.AddSeconds(3)
    $sampleFinished = $born.AddSeconds(5)
    # The child born at second 4 was absent from that inventory but already alive
    # when slow counter queries completed. Its lifetime CPU is still measurable.
    $duringQueryCpu = Measure-CpuInterval $next @($later + $newChild) 2 $inventoryStart 4
    Assert ($null -ne $duringQueryCpu) 'child born during previous inventory query is counted'
    Assert ([Math]::Abs($duringQueryCpu.one_core_percent - 20) -lt .0001) 'during-query child lifetime CPU included'
    Assert ($null -eq (Measure-CpuInterval $next @($later + $newChild) 2 $sampleFinished 4)) 'pre-inventory unknown child remains incomplete'
    Assert ((Get-Verdict 1 0 299999999 $true 'busy' 'off') -eq 'pass') 'owner 0.1.0 busy below boundary'
    Assert ((Get-Verdict 1 0 300000000 $true 'busy' 'off') -eq 'fail') 'owner 0.1.0 strict busy boundary'
    Assert ((Get-Verdict 1 0 199999999 $true 'idle' 'off') -eq 'pass') 'decimal idle budget below boundary'
    Assert ((Get-Verdict 1 0 200000000 $true 'idle' 'off') -eq 'fail') 'strict idle budget boundary'
    Assert ((Get-Verdict 1 0 149999999 $true 'startup' 'off') -eq 'pass') 'startup below boundary with full coverage'
    Assert ((Get-Verdict 1 0 150000000 $true 'startup' 'off') -eq 'fail') 'strict startup boundary'
    Assert ((Get-Verdict 1 0 10 $true 'startup' 'off' (Get-MemoryCoverageReason 'startup')) -eq 'not_measured') 'post-readiness startup cannot pass'
    Assert ((Get-Verdict 1 0 10 $true 'busy' 'off' (Get-MemoryCoverageReason 'demo-busy')) -eq 'not_measured') 'tiny demo cannot pass'
    Assert ((Get-Verdict 1 0 500000000 $true 'busy' 'on') -eq 'not_applicable') 'opt-in acceleration has no release verdict'
    Assert ((Get-Verdict 1 0 10 $true 'unknown' 'off') -eq 'not_measured') 'unknown workload cannot pass'
    Assert ((Get-Verdict 1 1 10 $true) -eq 'not_measured') 'incomplete cannot pass'
    Assert ((Get-Verdict 0 0 $null $true) -eq 'not_measured') 'empty cannot pass'
    Assert ((Get-Verdict 1 0 10 $false) -eq 'not_measured') 'short run cannot pass'
    # Reproduce a failed first inventory / a single positive sample without a
    # CPU interval. Both must produce incomplete evidence, never a missing .Sum.
    Assert-Throws { (@() | Measure-Object cpu_seconds_delta -Sum).Sum } 'PowerShell empty aggregation reproduction'
    $empty = Get-CpuStatistics @()
    Assert ($empty.seconds -eq 0 -and $empty.wall -eq 0 -and $null -eq $empty.one_core_peak) 'empty CPU aggregation remains unmeasured'
    $stats = Get-CpuStatistics @(
        [pscustomobject]@{cpu_seconds_delta=.2; interval_seconds=2; cpu_one_core_percent=10; cpu_machine_percent=2.5},
        [pscustomobject]@{cpu_seconds_delta=.6; interval_seconds=3; cpu_one_core_percent=20; cpu_machine_percent=5})
    Assert ([Math]::Abs($stats.seconds-.8) -lt .0001 -and $stats.wall -eq 5 -and $stats.one_core_peak -eq 20 -and $stats.machine_peak -eq 5) 'positive elapsed-weighted CPU statistics'
    # Exercise the actual CSV/JSON reporting path without launching an app.
    # An inventory failure before/after one sample previously reached empty
    # Measure-Object aggregation and escaped without writing a summary.
    $testDirectory = Join-Path ([IO.Path]::GetTempPath()) ('maxed-memory-selftest-' + [guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($testDirectory)
    $fakeExe = Join-Path $testDirectory 'synthetic-bytes'
    [IO.File]::WriteAllText($fakeExe, 'invented executable identity, never launched')
    $fakeProcess = [pscustomobject]@{ StartTime=$born; MainModule=[pscustomobject]@{FileName=$fakeExe} }
    $fakeProcess | Add-Member -MemberType ScriptMethod -Name Dispose -Value {}
    function Get-Process { [CmdletBinding()] param([int]$Id) $fakeProcess }
    function Read-Counters([int]$ProcessId) {
        [pscustomobject]@{Born=$born; Private=10000000L; Cpu=1.0; Name='maxed'; Version=$null}
    }
    $inventoryProbe = @{ calls=0; positive=0 }
    function Read-ProcessInventory {
        if ($inventoryProbe.calls++ -ge $inventoryProbe.positive) { throw 'Synthetic inventory failure' }
        [pscustomobject]@{ProcessId=1; ParentProcessId=0; CreationDate=$born; role='Maxed'; effective_acceleration='unverified'}
    }
    try {
        $RootProcessId=1; $Seconds=1; $IntervalMs=100
        foreach ($positive in @(0,1)) {
            $inventoryProbe.calls=0; $inventoryProbe.positive=$positive
            $OutputPrefix=Join-Path $testDirectory "partial-$positive"
            $output=@(Invoke-Measurement)
            $summary=Get-Content -LiteralPath "$OutputPrefix-summary.json" -Raw | ConvertFrom-Json
            Assert ($output[-1] -eq 2 -and $summary.samples -eq $positive -and $summary.incomplete_samples -eq 1) 'partial summary is written with exit 2'
            Assert ($summary.result -eq 'not_measured' -and $summary.cpu_result -eq 'not_measured' -and $null -eq $summary.cpu_mean_machine_percent) 'missing CPU is never reported as measured zero'
            Assert ($null -ne $summary.accounting_failure_line -and $summary.accounting_failure_type -eq 'RuntimeException') 'safe accounting failure diagnostics'
        }
        function Read-ProcessInventory {
            [pscustomobject]@{ProcessId=1; ParentProcessId=0; CreationDate=$born; role='Maxed'; effective_acceleration='unverified'}
            [pscustomobject]@{ProcessId=2; ParentProcessId=1; CreationDate=$born; role='browser'; effective_acceleration=$probePreference}
        }
        foreach ($case in @(@('off','p4-realistic','off','pass',0),@('on','p4-realistic','on','not_applicable',0),
            @('startup','startup','off','not_measured',2),@('demo','dungeon-busy','off','not_measured',2),@('unknown','unknown','off','not_measured',2))) {
            $OutputPrefix=Join-Path $testDirectory ('gate-'+$case[0])
            $WorkloadScenario=$case[1]; $probePreference=$case[2]
            $output=@(Invoke-Measurement)
            $summary=Get-Content -LiteralPath "$OutputPrefix-summary.json" -Raw | ConvertFrom-Json
            Assert ($output[-1] -eq $case[4] -and $summary.release_result -eq $case[3] -and $summary.result -eq $case[3]) "serialized gate verdict: $($case[0])"
            Assert ($summary.memory_result -eq 'measured' -and $summary.cpu_result -eq 'measured' -and $summary.peak_bytes -eq 20000000L) 'raw measurement remains available independently of gate'
            if ($probePreference -eq 'on') { Assert ($null -eq $summary.budget_bytes -and $null -eq $summary.passed) 'opt-in report has no threshold or pass/fail claim' }
        }
    } finally {
        # Only exact files created by this test; no recursive deletion.
        foreach ($name in @('synthetic-bytes','partial-0-processes.csv','partial-0-totals.csv','partial-0-summary.json','partial-1-processes.csv','partial-1-totals.csv','partial-1-summary.json')) {
            $file=Join-Path $testDirectory $name
            if ([IO.File]::Exists($file)) { [IO.File]::Delete($file) }
        }
        foreach ($case in @('off','on','startup','demo','unknown')) {
            foreach ($suffix in @('-processes.csv','-totals.csv','-summary.json')) {
                $file=Join-Path $testDirectory ('gate-'+$case+$suffix)
                if ([IO.File]::Exists($file)) { [IO.File]::Delete($file) }
            }
        }
        [IO.Directory]::Delete($testDirectory)
    }
    Write-Output 'PASS: tree, retention, churn, identity, counter-failure, CPU and budget self-tests'
}

function Invoke-Measurement {
    if ($Seconds -lt 1 -or $IntervalMs -lt 100 -or $RootProcessId -le 0) { throw 'Invalid sampling parameters' }
    $prefix = [IO.Path]::GetFullPath($OutputPrefix)
    if (-not (Test-Path -LiteralPath ([IO.Path]::GetDirectoryName($prefix)) -PathType Container)) {
        throw 'Report directory missing'
    }
    foreach ($suffix in @('-processes.csv','-totals.csv','-summary.json')) {
        if (Test-Path -LiteralPath "$prefix$suffix") { throw 'Report prefix already used' }
    }
    $root = Read-Counters $RootProcessId
    if ($root.Name -ne 'maxed') { throw 'Root must be Maxed' }
    $rootStart = $root.Born
    $rootProcess = Get-Process -Id $RootProcessId -ErrorAction Stop
    try {
        if ($rootProcess.StartTime.ToUniversalTime() -ne $rootStart) { throw 'Root identity changed' }
        $exeHash = (Get-FileHash -LiteralPath $rootProcess.MainModule.FileName -Algorithm SHA256).Hash.ToLowerInvariant()
    } finally { $rootProcess.Dispose() }
    $known = @{ $RootProcessId = $rootStart }
    $rows = [System.Collections.Generic.List[object]]::new()
    $totals = [System.Collections.Generic.List[object]]::new()
    $incomplete = 0; $cpuIncomplete = 0; $previous = $null; $previousElapsed = 0.0
    $previousInventoryStartUtc = [datetime]::UtcNow
    $cores = [Environment]::ProcessorCount
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $attachedUtc = [datetime]::UtcNow
    $reason = $null
    $failureLine = $null; $failureType = $null
    # Early 100 ms discovery bounds startup polling; actual intervals include query cost.
    # Polling cannot prove the absence of unobserved, shorter-lived descendants.
    do {
        try {
            $inventoryStartUtc = [datetime]::UtcNow
            $inventory = @(Read-ProcessInventory)
            $sample = @(Read-TreeSample $inventory $known $RootProcessId $rootStart ${function:Read-Counters})
            $elapsed = $watch.Elapsed.TotalSeconds
            $utc = [datetime]::UtcNow
            $cpu = $null
            $deltaSeconds = $null
            if ($null -ne $previous) {
                $deltaSeconds = $elapsed - $previousElapsed
                $cpu = Measure-CpuInterval $previous $sample $deltaSeconds $previousInventoryStartUtc $cores
                if ($null -eq $cpu) { $cpuIncomplete++ }
            }
            [long]$sum = 0
            foreach ($row in $sample) {
                $sum += $row.private_bytes
                $rows.Add([pscustomobject]@{
                    utc=$utc.ToString('o'); elapsed_seconds=$elapsed; scenario=$Scenario
                    pid=$row.pid; start_utc=$row.born.ToString('o'); name=$row.name
                    parent_pid=$row.parent_pid; role=$row.role; effective_acceleration=$row.effective_acceleration
                    process_type=$row.process_type; utility_sub_type=$row.utility_sub_type
                    private_bytes=$row.private_bytes; cpu_seconds=$row.cpu_seconds
                    webview2_version=$row.runtime_version
                })
            }
            $totals.Add([pscustomobject]@{
                utc=$utc.ToString('o'); elapsed_seconds=$elapsed; interval_seconds=$deltaSeconds
                private_bytes=$sum; private_MB=$sum/1000000; private_MiB=$sum/1MB; processes=$sample.Count
                cpu_seconds_delta=$(if ($null -ne $cpu) { $cpu.seconds } else { $null })
                cpu_one_core_percent=$(if ($null -ne $cpu) { $cpu.one_core_percent } else { $null })
                cpu_machine_percent=$(if ($null -ne $cpu) { $cpu.machine_percent } else { $null })
            })
            $previous=$sample; $previousElapsed=$elapsed; $previousInventoryStartUtc=$inventoryStartUtc
        } catch {
            $incomplete++
            $reason = 'Process accounting incomplete; repeat with a fresh Maxed launch.'
            # Bounded diagnostics identify the failing operation without logging
            # exception text, command lines, executable paths or user content.
            $failureLine = $_.InvocationInfo.ScriptLineNumber
            $failureType = $_.Exception.GetType().Name
            # Fail closed immediately. Raw exception text can contain private paths.
            break
        }
        if ($watch.Elapsed.TotalSeconds -ge $Seconds) { break }
        $delay = if ($watch.Elapsed.TotalSeconds -lt 5) { 100 } else { $IntervalMs }
        $remaining = [Math]::Ceiling(($Seconds - $watch.Elapsed.TotalSeconds) * 1000)
        if ($remaining -gt 0) { Start-Sleep -Milliseconds ([int][Math]::Min($delay, $remaining)) }
    } while ($true)
    $durationComplete = $watch.Elapsed.TotalSeconds -ge $Seconds
    $rows.ToArray() | Export-Csv -NoTypeInformation -LiteralPath "$prefix-processes.csv"
    $totals.ToArray() | Export-Csv -NoTypeInformation -LiteralPath "$prefix-totals.csv"
    $sorted = @($totals | ForEach-Object private_bytes | Sort-Object)
    $peak = if ($sorted.Count) { [long]$sorted[-1] } else { $null }
    $memoryComplete = $sorted.Count -gt 0 -and $incomplete -eq 0 -and $durationComplete
    $memoryResult = if ($memoryComplete) { 'measured' } else { 'not_measured' }
    $workload = if ($WorkloadScenario) { $WorkloadScenario } else { $Scenario }
    $class = Get-MemoryWorkloadClass $workload
    $preferences = @($rows | Where-Object role -eq 'browser' | ForEach-Object effective_acceleration | Sort-Object -Unique)
    $acceleration = if ($preferences.Count -eq 1 -and $preferences[0] -in @('on','off')) { $preferences[0] } else { 'unverified' }
    $gate = Get-MemoryReleaseGate $class $peak $acceleration $memoryComplete (Get-MemoryCoverageReason $workload)
    $cpuRows = @($totals | Where-Object { $null -ne $_.cpu_one_core_percent })
    $cpuComplete = $cpuIncomplete -eq 0 -and $incomplete -eq 0 -and $cpuRows.Count -gt 0 -and $durationComplete
    $cpuStats = Get-CpuStatistics $cpuRows
    $cpuTime = $cpuStats.seconds
    $cpuWall = $cpuStats.wall
    $result = if (-not $memoryComplete -or -not $cpuComplete) { 'not_measured' } else { $gate.result }
    if (-not $cpuComplete -and $null -eq $reason) {
        $reason = 'CPU accounting incomplete after process churn or insufficient intervals; repeat measurement.'
    }
    $report = [pscustomobject]@{
        scenario=$Scenario; build_configuration_label=$BuildConfiguration
        windows_version=[Environment]::OSVersion.Version.ToString()
        webview2_versions=@($rows | ForEach-Object webview2_version | Where-Object { $_ } | Sort-Object -Unique)
        logical_processors=$cores; root_pid=$RootProcessId; root_start_utc=$rootStart.ToString('o')
        executable_sha256=$exeHash
        attach_delay_seconds=($attachedUtc-$rootStart).TotalSeconds
        requested_seconds=$Seconds; measured_seconds=$watch.Elapsed.TotalSeconds
        interval_ms=$IntervalMs; startup_interval_ms=100; startup_discovery_seconds=5
        samples=$sorted.Count; incomplete_samples=$incomplete; incomplete_cpu_intervals=$cpuIncomplete
        accounting_failure_line=$failureLine; accounting_failure_type=$failureType
        peak_bytes=$peak; peak_MB=$(if ($null -ne $peak) { $peak/1000000 } else { $null })
        peak_MiB=$(if ($null -ne $peak) { $peak/1MB } else { $null })
        p95_bytes=$(if ($sorted.Count) { $sorted[[int][Math]::Ceiling($sorted.Count * .95)-1] } else { $null })
        peak_process_count=$(if ($totals.Count) { ($totals | Measure-Object processes -Maximum).Maximum } else { $null })
        cpu_mean_one_core_percent=$(if ($cpuComplete) { 100*$cpuTime/$cpuWall } else { $null })
        cpu_mean_machine_percent=$(if ($cpuComplete) { 100*$cpuTime/$cpuWall/$cores } else { $null })
        cpu_peak_one_core_percent=$(if ($cpuComplete) { $cpuStats.one_core_peak } else { $null })
        cpu_peak_machine_percent=$(if ($cpuComplete) { $cpuStats.machine_peak } else { $null })
        cpu_result=$(if ($cpuComplete) { 'measured' } else { 'not_measured' })
        workload_class=$class; effective_acceleration=$acceleration; release_policy_version=$MaxedMemoryPolicy.version
        budget_bytes=$gate.threshold_bytes; release_result=$result; release_reason=$gate.reason
        memory_result=$memoryResult; passed=$(if ($result -in @('pass','fail')) { $result -eq 'pass' } else { $null }); result=$result; reason=$reason
        accounting='Maxed plus all observed descendants; retained after reparenting; private bytes, not RSS.'
        limitation='Polling can miss sub-interval processes and peaks. Validate initial tree independently; labels do not certify release provenance or demo load.'
    }
    $report | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath "$prefix-summary.json" -Encoding utf8
    Write-Output ($report | ConvertTo-Json -Depth 4 -Compress)
    if ($result -eq 'not_measured') { return 2 }
    if ($result -eq 'fail') { return 1 }
    return 0
}

if ($SelfTest) { Invoke-SelfTest; exit 0 }
try {
    # Keep the report on stdout but capture only the final numeric exit status.
    $output = @(Invoke-Measurement)
    if ($output.Count -gt 1) { $output[0..($output.Count-2)] | Write-Output }
    exit ([int]$output[-1])
} catch {
    Write-Output 'Measurement failed before report completion. Check the selected process, counters and fresh report prefix.'
    exit 2
}
