[CmdletBinding()]
param(
    [string]$Exe,
    [string]$OutputDir,
    [string[]]$Scenarios = @('home-idle','dungeon-busy','minimized','transitions','startup'),
    [int]$TransitionMinutes = 30,
    [string]$SyntheticProfile,
    [string]$P4Fixtures,
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'p4-memory-profile.ps1')
$script:LaunchedProcesses = [System.Collections.Generic.List[object]]::new()

function Protect-DiagnosticMessage([string]$Message) {
    # Preserve .NET's quoted method names and inner errors. Only remove the
    # external spans; punctuation and hyphenated prose are not commands.
    $command = '(?i)(?:\b(?:powershell|pwsh|cmd|python|node)(?:\.exe)?|\b[\w.-]+\.exe(?=[ \t]+[-/])|\b(?:Get|Set|Start|Stop|Invoke|Remove|New|Add|Test|Write|Read|Import|Export|Out|Select|ForEach|Where)-\w+)'
    $arguments = '[ \t]+(?:[^"''\r\n]|"[^"\r\n]*"|''[^''\r\n]*'')+'
    $safe = [regex]::Replace($Message, '(?i)(?:"[^"\r\n]*\.exe"|''[^''\r\n]*\.exe'')(?=[ \t]+[-/])' + $arguments, '[redacted command]')
    $safe = [regex]::Replace($safe, '(?i)(\bcommand(?: line)?:)' + $arguments, '$1 [redacted command]')
    $safe = [regex]::Replace($safe, '(?i)(?<quote>["''])[\w.-]+\.exe[ \t]+[^"''\r\n]+\k<quote>', '${quote}[redacted command]${quote}')
    $safe = [regex]::Replace($safe, $command + $arguments, '[redacted command]')
    $safe = [regex]::Replace($safe, '(?i)\b[a-z][a-z0-9+.-]*://[^\s"''<>]+', '[redacted URL]')
    # Quoted paths may contain spaces and need not have an extension.
    $safe = [regex]::Replace($safe, '(?i)(?<quote>["''])(?:[a-z]:|\\\\|\\Users\\)[^"''\r\n]*\k<quote>', '${quote}[redacted path]${quote}')
    # For unquoted paths with spaces, stop at the filename extension so the
    # following error explanation survives. Then cover extensionless tokens.
    $safe = [regex]::Replace($safe, '(?i)(?:\b[a-z]:|\\\\|\\Users\\)[^"''\r\n<>|]*?\.[a-z0-9]{1,12}(?=$|[\s"'',;:!?)])', '[redacted path]')
    $safe = [regex]::Replace($safe, '(?i)(?:\b[a-z]:|\\\\|\\Users\\)[^\s"''<>|,;!?)]*', '[redacted path]')
    $safe = [regex]::Replace($safe, '(?i)[^\s"''<>|]*[\\/][^\s"''<>|]*\.[a-z0-9]{1,12}\b', '[redacted path]')
    return [regex]::Replace($safe, '[\r\n]+', ' ')
}

function Write-Diagnostic($Failure) {
    if ([string]::IsNullOrWhiteSpace($OutputDir) -or -not [IO.Directory]::Exists($OutputDir)) { return }
    $message = Protect-DiagnosticMessage $Failure.Exception.Message
    $line = $Failure.InvocationInfo.ScriptLineNumber
    # Logging is best-effort; an unwritable directory must not prevent cleanup.
    $entry = "line ${line}: $message"
    Add-Content -LiteralPath (Join-Path $OutputDir 'diagnostics.txt') -Value $entry -Encoding utf8 -ErrorAction SilentlyContinue
}

function New-ScenarioLaunch($Process) {
    [pscustomobject]@{ Process=$Process; Window=[IntPtr]::Zero; Minimized=$false; CloseRequested=$false; Closed=$false }
}

function Resolve-Scenarios([string[]]$Names) {
    $allowed = @('home-idle','dungeon-busy','minimized','transitions','startup','p4-realistic','p4-worst')
    $result = [System.Collections.Generic.List[string]]::new()
    foreach ($name in $Names) {
        foreach ($part in $name.Split(',')) {
            $value = $part.Trim().ToLowerInvariant()
            if ($value -notin $allowed) { throw 'Unsupported scenario' }
            if (-not $result.Contains($value)) { $result.Add($value) }
        }
    }
    if ($result.Count -eq 0) { throw 'Select at least one scenario' }
    return $result.ToArray()
}

function Get-TransitionLoops([int]$Minutes) {
    if ($Minutes -lt 1 -or $Minutes -gt 120) { throw 'Transition duration must be 1 through 120 minutes' }
    # Bundled synthetic story is 12 s, plus 2 s EOF and 2 s live-namespace dwell.
    # Round up so the collector finishes while transitions are still repeating.
    return [int][Math]::Ceiling($Minutes * 60 / 16.0)
}

function Get-Percentile($Values, [double]$Fraction) {
    $sorted = @($Values | Sort-Object)
    if ($sorted.Count -eq 0) { return $null }
    return $sorted[[int][Math]::Ceiling($sorted.Count*$Fraction)-1]
}

function Get-MemoryGrowth($Rows, [double]$Seconds, [bool]$Complete) {
    $missing = [pscustomobject]@{ result='not_measured'; reason='First and last five-minute memory windows were incomplete or invalid.'; window_seconds=300; first_mean_bytes=$null; last_mean_bytes=$null; growth_bytes=$null; growth_percent=$null }
    if ($Seconds -lt 600) {
        $missing.reason = 'Transition duration is under 10 minutes; two disjoint five-minute windows require at least 10 minutes.'
        return $missing
    }
    if (-not $Complete -or @($Rows).Count -lt 2) { return $missing }
    $points = [System.Collections.Generic.List[object]]::new()
    try {
        foreach ($row in $Rows) {
            if ($null -eq $row.elapsed_seconds -or $null -eq $row.private_bytes -or
                [string]::IsNullOrWhiteSpace([string]$row.private_bytes)) { return $missing }
            $time = [double]::Parse([string]$row.elapsed_seconds, [Globalization.CultureInfo]::InvariantCulture)
            $bytes = [double]::Parse([string]$row.private_bytes, [Globalization.CultureInfo]::InvariantCulture)
            if ([double]::IsNaN($time) -or [double]::IsInfinity($time) -or $time -lt 0 -or
                [double]::IsNaN($bytes) -or [double]::IsInfinity($bytes) -or $bytes -le 0) { return $missing }
            if ($points.Count -gt 0) {
                $gap = $time - $points[$points.Count-1].time
                if ($gap -le 0 -or $gap -gt 10) { return $missing }
            }
            $points.Add([pscustomobject]@{ time=$time; bytes=$bytes })
        }
    } catch { Write-Diagnostic $_; return $missing }
    if ($points[0].time -gt 2 -or $points[$points.Count-1].time -lt $Seconds) { return $missing }
    # Integrate the preceding sample over each interval, clipping at window edges.
    # Hold the first sample back to t=0 (at most two seconds of discovery delay).
    $first = 0.0; $last = 0.0; $left = 0.0
    for ($i = 0; $i -lt $points.Count; $i++) {
        $right = $points[$i].time
        $bytes = if ($i -eq 0) { $points[0].bytes } else { $points[$i-1].bytes }
        # A literal integer zero selects Math.Max(Int32, Int32) in PowerShell,
        # rounding fractional sample durations and biasing both window means.
        $first += [Math]::Max(0.0, [Math]::Min($right, 300.0) - $left) * $bytes
        $last += [Math]::Max(0.0, [Math]::Min($right, $Seconds) - [Math]::Max($left, $Seconds-300.0)) * $bytes
        $left = $right
    }
    $first /= 300; $last /= 300
    [pscustomobject]@{ result='measured'; reason=$null; window_seconds=300; first_mean_bytes=$first; last_mean_bytes=$last; growth_bytes=$last-$first; growth_percent=100*($last-$first)/$first }
}

function Add-TransitionResult($Report, $Rows, [int]$Seconds, [int]$Loops) {
    $complete = $Report.memory_result -ne 'not_measured' -and $Report.incomplete_samples -eq 0 -and $Report.measured_seconds -ge $Seconds
    $growth = Get-MemoryGrowth $Rows $Seconds $complete
    $Report | Add-Member -NotePropertyName memory_growth -NotePropertyValue $growth
    $Report | Add-Member -NotePropertyName requested_demo_restarts -NotePropertyValue $Loops
    # Growth is independent of the collector/scenario verdict. A deliberately
    # short run skips growth only; invalid windows in a long run still exit 2.
    return ($Seconds -ge 600 -and $growth.result -ne 'measured')
}

function Assert([bool]$Condition, [string]$Label) { if (-not $Condition) { throw "Self-test failed: $Label" } }
function Assert-Rejected([scriptblock]$Action) {
    $rejected = $false
    try { & $Action | Out-Null } catch { Write-Diagnostic $_; $rejected = $true }
    Assert $rejected 'invalid input must fail'
}
function New-FakeProcess([bool]$Graceful = $true) {
    $fake = [pscustomobject]@{
        Id=123; MainWindowHandle=[IntPtr]999; HasExited=$false; ExitCode=0
        Graceful=$Graceful; Kills=0; Disposed=$false; Events=[System.Collections.Generic.List[string]]::new()
    }
    $fake | Add-Member ScriptMethod Refresh { }
    $fake | Add-Member ScriptMethod WaitForExit {
        param($Timeout)
        $this.Events.Add("wait:$Timeout")
        if ($this.Graceful -or $this.Kills -gt 0) { $this.HasExited = $true }
        return $this.HasExited
    }
    $fake | Add-Member ScriptMethod Kill { $this.Events.Add('kill'); $this.Kills++; $this.HasExited=$true }
    $fake | Add-Member ScriptMethod Dispose { $this.Disposed=$true }
    return $fake
}

function Test-Diagnostics {
    $OutputDir = Join-Path ([IO.Path]::GetTempPath()) ('maxed-diagnostics-' + [guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($OutputDir)
    $diagnostics = Join-Path $OutputDir 'diagnostics.txt'
    try {
        try { throw 'Synthetic diagnostic failure' } catch { Write-Diagnostic $_ }
        try { throw 'Second diagnostic failure' } catch { Write-Diagnostic $_ }
        $entries = @(Get-Content -LiteralPath $diagnostics)
        Assert ($entries.Count -eq 2 -and $entries[0] -match '^line [1-9][0-9]*: Synthetic diagnostic failure$' -and
            $entries[1] -match 'Second diagnostic failure$') 'diagnostics append exception message and line number'
        foreach ($message in @(
            'Cannot find path ''C:\private folder\secret.txt'' because it does not exist.',
            'Cannot open C:\private\secret.txt',
            'Cannot open C:private',
            'Cannot open \\private-server\share\secret.txt',
            'Cannot open ../private/secret.txt',
            'Cannot open \Users\private\secret.txt',
            'Cannot open relative\private\secret.txt',
            'Failed https://private.example/secret?token=hidden',
            'Failed command: powershell -File private.ps1',
            'Failed command: Get-Content private',
            'Failed command: custom.exe private',
            'Failed "custom.exe private"',
            'Failed "maxed.exe --secret hidden"',
            'Failed command: "C:\private folder\maxed.exe" --secret hidden',
            "Failed`ncmd /c echo hidden"
        )) {
            try { throw $message } catch { Write-Diagnostic $_ }
        }
        $text = Get-Content -Raw -LiteralPath $diagnostics
        Assert ($text -notmatch 'private|secret|hidden|https:|maxed\.exe|Get-Content|powershell|cmd /c') 'diagnostics omit paths URLs and command lines'
        $wrapped = 'Exception calling "WaitForExit" with "1" argument(s): "Access denied to C:\private folder\secret.log while waiting for exit."'
        $expected = 'Exception calling "WaitForExit" with "1" argument(s): "Access denied to [redacted path] while waiting for exit."'
        Assert ((Protect-DiagnosticMessage $wrapped) -ceq $expected) 'method name and inner error survive path redaction'
        Assert ((Protect-DiagnosticMessage $wrapped.Replace('secret.log', 'maxed.exe')) -ceq $expected) 'executable path followed by error prose is not a command'
        try { throw $wrapped } catch { Write-Diagnostic $_ }
        $lastEntry = @(Get-Content -LiteralPath $diagnostics)[-1]
        Assert ($lastEntry.EndsWith($expected)) 'wrapped exception remains useful in diagnostics file'
        foreach ($fixed in @(
            'Scenario shutdown failed; owned-process cleanup was required',
            'Original window unavailable',
            'Window close timed out',
            'Maxed reported a measurement or shutdown failure'
        )) {
            Assert ((Protect-DiagnosticMessage $fixed) -ceq $fixed) 'fixed script messages remain intact'
        }
        $before = @(Get-Content -LiteralPath $diagnostics).Count
        $invalid = @([pscustomobject]@{ elapsed_seconds='invalid'; private_bytes=1 }) * 2
        Assert ((Get-MemoryGrowth $invalid 600 $true).result -eq 'not_measured') 'parse failure preserves growth status'
        Assert (@(Get-Content -LiteralPath $diagnostics).Count -eq $before + 1) 'growth parse catch appends diagnostics'
        $fake = New-FakeProcess $false
        $owned = New-ScenarioLaunch $fake
        Assert-Rejected { Close-Scenario $owned }
        $text = Get-Content -Raw -LiteralPath $diagnostics
        Assert ($text -match 'Original window unavailable' -and @(Get-Content -LiteralPath $diagnostics).Count -gt $before) 'caught growth and close failures are recorded'
        $fake = New-FakeProcess $false
        $fake | Add-Member ScriptMethod Kill { throw 'Synthetic termination failure' } -Force
        $owned = New-ScenarioLaunch $fake
        $before = @(Get-Content -LiteralPath $diagnostics).Count
        Assert-Rejected { Close-Scenario $owned }
        Assert (@(Get-Content -LiteralPath $diagnostics).Count -eq $before + 3) 'both close and termination catches append diagnostics'
    } finally {
        if (Test-Path -LiteralPath $diagnostics) { Remove-Item -LiteralPath $diagnostics }
        Remove-Item -LiteralPath $OutputDir
    }
}

function Test-ScenarioLaunch {
    function Assert-NoMaxed { }
    function Start-Process($FilePath, $WorkingDirectory, [switch]$PassThru, $WindowStyle) {
        $fake | Add-Member NoteProperty LaunchArguments @{
            file=$FilePath; directory=$WorkingDirectory; pass_thru=$PassThru; style=$WindowStyle
            demo=[Environment]::GetEnvironmentVariable('MAXED_MEASURE_DEMO','Process')
            loops=[Environment]::GetEnvironmentVariable('MAXED_MEASURE_DEMO_LOOPS','Process')
        }
        return $fake
    }
    $script:Executable = Join-Path ([IO.Path]::GetTempPath()) 'maxed.exe'
    $savedDemo = [Environment]::GetEnvironmentVariable('MAXED_MEASURE_DEMO','Process')
    $savedLoops = [Environment]::GetEnvironmentVariable('MAXED_MEASURE_DEMO_LOOPS','Process')
    foreach ($name in @('home-idle', 'dungeon-busy', 'transitions', 'p4-realistic', 'p4-worst')) {
        $fake = New-FakeProcess
        $fake | Add-Member NoteProperty Handle ([IntPtr]321)
        $owned = Start-Scenario $name 4
        $launchArguments = $fake.LaunchArguments
        Assert ($launchArguments.file -eq $script:Executable -and $launchArguments.directory -eq [IO.Path]::GetDirectoryName($script:Executable)) 'launch uses executable directory'
        Assert ($launchArguments.pass_thru -and $launchArguments.style -eq 'Hidden') 'launch retains process and starts hidden'
        Assert (($launchArguments.demo -eq '1') -eq ($name -in @('dungeon-busy','transitions'))) 'demo flag matches scenario; P4 uses native history'
        Assert (($launchArguments.loops -eq '4') -eq ($name -eq 'transitions')) 'loop flag only applies to transitions'
        Assert ([Environment]::GetEnvironmentVariable('MAXED_MEASURE_DEMO','Process') -eq $savedDemo -and
            [Environment]::GetEnvironmentVariable('MAXED_MEASURE_DEMO_LOOPS','Process') -eq $savedLoops) 'launch restores environment'
        $owned.Window = [IntPtr]42
        Close-Scenario $owned {} { return $true } { return $true }
    }
}

function Invoke-SelfTest {
    Test-Diagnostics
    Test-ScenarioLaunch
    Assert (((Resolve-Scenarios @(' HOME-IDLE,transitions','startup','home-idle')) -join ',') -eq 'home-idle,transitions,startup') 'scenario subset and deduplication'
    Assert (((Resolve-Scenarios @('p4-realistic,p4-worst')) -join ',') -eq 'p4-realistic,p4-worst') 'both exact P4 history workloads are selectable'
    Assert-Rejected { Resolve-Scenarios @('unknown') }
    Assert-Rejected { Resolve-Scenarios @() }
    Assert-Rejected { Resolve-Scenarios @('startup,') }
    Assert ((Get-TransitionLoops 30) -eq 113) '30-minute loop sizing'
    Assert ((Get-TransitionLoops 120) -le 500) 'loop bound'
    Assert ((Get-TransitionLoops 1) -eq 4) 'one-minute loop sizing'
    Assert ((Get-TransitionLoops 9) -eq 34) 'short transition sizing'
    Assert ((Get-TransitionLoops 10) -eq 38) 'growth minimum loop sizing'
    Assert-Rejected { Get-TransitionLoops 0 }
    Assert-Rejected { Get-TransitionLoops -1 }
    Assert-Rejected { Get-TransitionLoops 121 }
    Assert ((Get-Percentile @(3,1,2) .5) -eq 2) 'nearest-rank median'
    Assert ($null -eq (Get-Percentile @() .95)) 'missing percentile is null'
    $rows = @(0..1200 | ForEach-Object { [pscustomobject]@{ elapsed_seconds=$_; private_bytes=1000000+1000*$_ } })
    $growth = Get-MemoryGrowth $rows 1200 $true
    Assert ($growth.result -eq 'measured' -and $growth.growth_bytes -eq 900000) 'first and last five-minute means'
    $flat = @($rows | ForEach-Object { [pscustomobject]@{ elapsed_seconds=$_.elapsed_seconds; private_bytes=1000000 } })
    Assert ((Get-MemoryGrowth $flat 1200 $true).growth_bytes -eq 0) 'known zero growth'
    $fractional = @(0..500 | ForEach-Object { [pscustomobject]@{elapsed_seconds=($_ * 1.2);private_bytes=100000000L} })
    $fractionalGrowth = Get-MemoryGrowth $fractional 600 $true
    Assert ([Math]::Abs($fractionalGrowth.first_mean_bytes-100000000) -lt 1 -and
        [Math]::Abs($fractionalGrowth.last_mean_bytes-100000000) -lt 1) 'fractional sample intervals preserve a constant byte level'
    Assert ((Get-MemoryGrowth $rows 1200 $false).result -eq 'not_measured') 'incomplete collector'
    Assert ((Get-MemoryGrowth $rows 599 $true).result -eq 'not_measured') 'overlapping windows refused'
    foreach ($seconds in @(60, 540, 599, 600, 1200)) {
        $report = [pscustomobject]@{ result='pass'; passed=$true; memory_result='measured'; incomplete_samples=0; measured_seconds=$seconds }
        $growthFailed = Add-TransitionResult $report $rows $seconds (Get-TransitionLoops ([int][Math]::Ceiling($seconds / 60.0)))
        Assert (-not $growthFailed -and $report.result -eq 'pass' -and $report.passed) 'growth does not overwrite scenario success'
        if ($seconds -lt 600) {
            Assert ($report.memory_growth.result -eq 'not_measured' -and $report.memory_growth.reason -match 'under 10 minutes' -and $null -eq $report.memory_growth.growth_bytes) 'short runs explicitly skip growth'
        } else {
            Assert ($report.memory_growth.result -eq 'measured') 'ten minutes and longer measure growth'
        }
    }
    foreach ($seconds in @(60, 1200)) {
        $report = [pscustomobject]@{ result='fail'; passed=$false; memory_result='measured'; incomplete_samples=0; measured_seconds=$seconds }
        [void](Add-TransitionResult $report $rows $seconds 4)
        Assert ($report.result -eq 'fail' -and -not $report.passed) 'growth cannot hide collector budget failure'
    }
    $report = [pscustomobject]@{ result='pass'; passed=$true; memory_result='measured'; incomplete_samples=0; measured_seconds=1200 }
    Assert (Add-TransitionResult $report $rows[0..1199] 1200 75) 'incomplete long-run growth requires exit 2'
    Assert ($report.result -eq 'pass' -and $report.memory_growth.result -eq 'not_measured') 'scenario and incomplete growth are separate'
    Assert ((Get-MemoryGrowth $rows[0..1199] 1200 $true).result -eq 'not_measured') 'short run'
    Assert ((Get-MemoryGrowth $rows[3..1200] 1200 $true).result -eq 'not_measured') 'late first sample'
    Assert ((Get-MemoryGrowth @($rows[0..50] + $rows[62..1200]) 1200 $true).result -eq 'not_measured') 'sample gap'
    $flat[100].private_bytes = $null
    Assert ((Get-MemoryGrowth $flat 1200 $true).result -eq 'not_measured') 'missing counter is not zero'
    $rows[100].elapsed_seconds = 99
    Assert ((Get-MemoryGrowth $rows 1200 $true).result -eq 'not_measured') 'nonmonotonic sample'
    $fake = New-FakeProcess
    $owned = New-ScenarioLaunch $fake
    $owned.Window = [IntPtr]42; $owned.Minimized = $true
    $restore = { param($Window) $fake.Events.Add("restore:$Window") }
    $post = { param($Window) $fake.Events.Add("close:$Window"); return $true }
    $owner = { param($Window, $ProcessId) return $Window -eq [IntPtr]42 -and $ProcessId -eq 123 }
    Close-Scenario $owned $restore $post $owner
    Assert (($fake.Events -join ',') -eq 'restore:42,close:42,wait:15000') 'restore then close original HWND despite changed MainWindowHandle'
    Assert ($owned.Closed -and $fake.Disposed -and $fake.Kills -eq 0) 'normal close avoids forced termination'
    Close-Scenario $owned $restore $post $owner
    Assert ($fake.Events.Count -eq 3) 'closed launch is not closed twice'
    foreach ($failure in @('timeout','post-failed','no-window','wrong-owner','restore-failed')) {
        $fake = New-FakeProcess $false
        $owned = New-ScenarioLaunch $fake
        $owned.Window = [IntPtr]42; $owned.Minimized = $true
        $restore = { param($Window) $fake.Events.Add("restore:$Window") }
        $post = { param($Window) $fake.Events.Add("close:$Window"); return $true }
        $owner = { return $true }
        switch ($failure) {
            'post-failed' { $post = { return $false } }
            'no-window' { $owned.Window = [IntPtr]::Zero }
            'wrong-owner' { $owner = { return $false } }
            'restore-failed' { $restore = { throw 'Synthetic window failure' } }
        }
        Assert-Rejected { Close-Scenario $owned $restore $post $owner }
        Assert ($fake.HasExited -and $owned.Closed -and $fake.Kills -eq 1 -and $fake.Disposed) "owned failure cleanup: $failure"
    }
    $fake = New-FakeProcess $false
    $owned = New-ScenarioLaunch $fake
    $owned.Window = [IntPtr]42; $owned.CloseRequested = $true
    $unrelated = New-FakeProcess $false
    Assert-Rejected { Close-Scenario $owned { throw 'No restore expected' } { throw 'No second close expected' } { return $true } }
    Assert (($fake.Events -join ',') -eq 'wait:15000,kill,wait:15000') 'timed-out close is not sent twice'
    Assert (-not $unrelated.HasExited -and $unrelated.Kills -eq 0) 'cleanup only touches retained launch object'
    Write-Output 'PASS: scenario parsing, loop bounds, separate growth results, sanitized diagnostics, launch directory and owned window cleanup self-tests'
}

function Assert-NoMaxed {
    if (@(Get-Process -Name maxed -ErrorAction SilentlyContinue).Count -gt 0) { throw 'Close existing Maxed before measuring' }
}

function Set-OptionalProcessEnvironment([string]$Name, $Value) {
    # .NET 9 preserves empty environment values. NullString requests actual
    # deletion rather than PowerShell's null-to-empty string conversion.
    if ($null -eq $Value) { [Environment]::SetEnvironmentVariable($Name,[NullString]::Value,'Process') }
    else { [Environment]::SetEnvironmentVariable($Name,$Value,'Process') }
}

function Start-Scenario([string]$Name, [int]$Loops) {
    Assert-NoMaxed
    $savedDemo = [Environment]::GetEnvironmentVariable('MAXED_MEASURE_DEMO','Process')
    $savedLoops = [Environment]::GetEnvironmentVariable('MAXED_MEASURE_DEMO_LOOPS','Process')
    try {
        $demo = if ($Name -in @('dungeon-busy','transitions')) { '1' } else { $null }
        $repeat = if ($Name -eq 'transitions') { [string]$Loops } else { $null }
        Set-OptionalProcessEnvironment 'MAXED_MEASURE_DEMO' $demo
        Set-OptionalProcessEnvironment 'MAXED_MEASURE_DEMO_LOOPS' $repeat
        $process = Start-Process -FilePath $script:Executable -WorkingDirectory ([IO.Path]::GetDirectoryName($script:Executable)) -PassThru -WindowStyle Hidden
        $launch = New-ScenarioLaunch $process
        $script:LaunchedProcesses.Add($launch)
        # Retain the launched process handle, never reopen a PID for cleanup.
        [void]$process.Handle
        return $launch
    } finally {
        Set-OptionalProcessEnvironment 'MAXED_MEASURE_DEMO' $savedDemo
        Set-OptionalProcessEnvironment 'MAXED_MEASURE_DEMO_LOOPS' $savedLoops
    }
}

function Wait-Window($Launch) {
    $process = $Launch.Process
    $deadline = [datetime]::UtcNow.AddSeconds(30)
    do {
        $Process.Refresh()
        if ($Process.HasExited) { throw 'Maxed exited before window readiness' }
        if ($Launch.Window -eq [IntPtr]::Zero) {
            $Launch.Window = [MaxedHomeWindow]::FindMainWindow($Process.Id)
        }
        $window = $Launch.Window
        if ($window -ne [IntPtr]::Zero) {
            # Start hidden to avoid launcher flashes, then show the requested app.
            [void][MaxedHomeWindow]::ShowWindow($window, 5)
            $Process.Refresh()
            $idle = $false
            try { $idle = $Process.WaitForInputIdle(50) } catch { Write-Diagnostic $_; $idle = $false }
            if ($idle -and [MaxedHomeWindow]::IsWindowVisible($window) -and $Process.Responding) {
                return ([datetime]::UtcNow - $Process.StartTime.ToUniversalTime()).TotalMilliseconds
            }
        }
        Start-Sleep -Milliseconds 20
    } while ([datetime]::UtcNow -lt $deadline)
    throw 'Main window readiness timed out'
}

function Close-Scenario($Launch,
    [scriptblock]$Restore = { param($Window) [void][MaxedHomeWindow]::ShowWindow($Window, 9) },
    [scriptblock]$PostClose = { param($Window) [MaxedHomeWindow]::PostMessage($Window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) },
    [scriptblock]$OwnsWindow = { param($Window, $ProcessId) [MaxedHomeWindow]::OwnsWindow($Window, $ProcessId) }) {
    if ($Launch.Closed) { return }
    $process = $Launch.Process
    $failed = $false
    try {
        $process.Refresh()
        if (-not $process.HasExited) {
            if ($Launch.Window -eq [IntPtr]::Zero -or -not (& $OwnsWindow $Launch.Window $process.Id)) { throw 'Original window unavailable' }
            if (-not $Launch.CloseRequested) {
                if ($Launch.Minimized) { & $Restore $Launch.Window }
                # Never send a second close that could confirm an unsaved-data discard.
                $Launch.CloseRequested = $true
                if (-not (& $PostClose $Launch.Window)) { throw 'Window close request failed' }
            }
            if (-not $process.WaitForExit(15000)) { throw 'Window close timed out' }
        }
        if ($process.ExitCode -ne 0) { throw 'Maxed reported a measurement or shutdown failure' }
    } catch {
        Write-Diagnostic $_
        $failed = $true
    } finally {
        try {
            $process.Refresh()
            if (-not $process.HasExited) {
                # Approved failure cleanup: only this retained launch object, never
                # process-name enumeration or a freshly looked-up/reused PID.
                $failed = $true
                $process.Kill()
                if (-not $process.WaitForExit(15000)) { throw 'Owned process termination timed out' }
            }
            $process.Dispose()
            $Launch.Closed = $true
        } catch { Write-Diagnostic $_; $failed = $true }
    }
    if ($failed) { throw 'Scenario shutdown failed; owned-process cleanup was required' }
}

function Write-Summary($Report, [string]$Prefix) {
    $Report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath "$Prefix-summary.json" -Encoding utf8
}

function Invoke-Scenarios {
    $selected = @(Resolve-Scenarios $Scenarios)
    $loops = Get-TransitionLoops $TransitionMinutes
    Assert-NoMaxed
    if (-not $Exe -or -not $OutputDir) { throw 'Exe and OutputDir are required' }
    $script:Executable = (Resolve-Path -LiteralPath $Exe).ProviderPath
    if (-not (Test-Path -LiteralPath $script:Executable -PathType Leaf) -or [IO.Path]::GetFileName($script:Executable) -ine 'maxed.exe') { throw 'Select the release Maxed executable' }
    if (-not (Test-Path -LiteralPath $OutputDir -PathType Container)) { throw 'Output directory must already exist' }
    $destination = (Resolve-Path -LiteralPath $OutputDir).ProviderPath
    $collector = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../scripts/Measure-MaxedMemory.ps1'))
    if (-not (Test-Path -LiteralPath $collector -PathType Leaf)) { throw 'M1 collector missing' }
    $p4 = @{}
    if ($SyntheticProfile) { Assert-MemoryProfileSettings $SyntheticProfile (Get-FileHash -LiteralPath $script:Executable -Algorithm SHA256).Hash.ToLowerInvariant() }
    foreach ($name in $selected) {
        if ($name -in @('p4-realistic','p4-worst')) {
            $p4[$name] = Get-P4Fixture $SyntheticProfile $P4Fixtures $name (Get-FileHash -LiteralPath $script:Executable -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    }
    foreach ($name in $selected) {
        foreach ($suffix in @('-summary.json','-processes.csv','-totals.csv')) {
            if (Test-Path -LiteralPath (Join-Path $destination "$name$suffix")) { throw 'Scenario output already exists' }
        }
    }
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class MaxedHomeWindow {
    private delegate bool EnumWindow(IntPtr window, IntPtr context);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumWindow callback, IntPtr context);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr window, StringBuilder title, int maximum);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr window);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    public static bool OwnsWindow(IntPtr window, int processId) {
        uint owner;
        return GetWindowThreadProcessId(window, out owner) != 0 && owner == (uint)processId;
    }
    public static IntPtr FindMainWindow(int processId) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr window, IntPtr context) {
            uint owner;
            GetWindowThreadProcessId(window, out owner);
            if (owner == (uint)processId) {
                var title = new StringBuilder(64);
                GetWindowText(window, title, title.Capacity);
                if (title.ToString() == "Maxed") { found = window; return false; }
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@
    $exitCode = 0
    foreach ($name in $selected) {
        $prefix = Join-Path $destination $name
        $process = $null; $report = $null; $scenarioFailed = $false; $routeJob = $null
        try {
            if ($p4.ContainsKey($name)) { Install-P4Fixture $p4[$name] }
            $startup = [System.Collections.Generic.List[double]]::new()
            if ($name -eq 'startup') {
                # Unmeasured primer, then five consecutive warm launches; fifth
                # remains alive for the M1 process-tree accounting sample.
                $process = Start-Scenario $name 0
                [void](Wait-Window $process)
                Start-Sleep -Seconds 2
                Close-Scenario $process; $process = $null
                for ($trial = 0; $trial -lt 5; $trial++) {
                    $process = Start-Scenario $name 0
                    $startup.Add((Wait-Window $process))
                    if ($trial -lt 4) { Close-Scenario $process; $process = $null }
                }
            } else {
                $process = Start-Scenario $name $loops
                [void](Wait-Window $process)
            }
            if ($name -eq 'minimized') {
                $process.Minimized = $true
                [void][MaxedHomeWindow]::ShowWindow($process.Window, 6)
                Start-Sleep -Milliseconds 100
                if (-not [MaxedHomeWindow]::IsIconic($process.Window)) { throw 'Window did not minimize' }
            }
            $seconds = switch ($name) { 'home-idle' {180} 'dungeon-busy' {60} 'minimized' {120} 'transitions' {$TransitionMinutes*60} 'startup' {5} default {120} }
            $label = switch ($name) { 'home-idle' {'idle-visible'} 'dungeon-busy' {'demo-busy'} 'minimized' {'hidden'} 'transitions' {'route-switches'} 'startup' {'idle-visible'} default {'busy-visible'} }
            if ($p4.ContainsKey($name) -or $name -eq 'transitions') {
                $routeJob = Start-Job -ScriptBlock {
                    param($Directory,$Helper,$RootId,$StartUtc,$Window,$Output,$Duration)
                    Set-Location -LiteralPath $Directory
                    & $Helper -RootProcessId $RootId -RootStartUtc $StartUtc -WindowHandle $Window -Seconds $Duration -OutputFile $Output
                } -ArgumentList (Get-Location).Path,(Join-Path $PSScriptRoot 'memory-route-workload.ps1'),$process.Process.Id,
                    $process.Process.StartTime.ToUniversalTime().ToString('o'),$process.Window.ToInt64(),($prefix + '-routes.json'),($seconds-10)
            }
            # Preserve the workload as well as its window label: a post-readiness
            # startup sample must not inherit the idle-visible release gate.
            & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $collector -RootProcessId $process.Process.Id -Seconds $seconds -Scenario $label -WorkloadScenario $name -BuildConfiguration release -OutputPrefix $prefix | Out-Null
            $collectorExit = $LASTEXITCODE
            if (-not (Test-Path -LiteralPath "$prefix-summary.json")) { throw 'Collector did not write a summary' }
            $report = Get-Content -Raw -LiteralPath "$prefix-summary.json" | ConvertFrom-Json
            $report | Add-Member -NotePropertyName collector_scenario -NotePropertyValue $report.scenario
            $report.scenario = $name
            $report | Add-Member -NotePropertyName collector_exit_code -NotePropertyValue $collectorExit
            $report | Add-Member -NotePropertyName driver_result -NotePropertyValue 'completed'
            if ($null -ne $routeJob) {
                if (-not (Wait-Job $routeJob -Timeout 30)) { throw 'P4 route workload timed out' }
                Receive-Job $routeJob -ErrorAction Stop | Out-Null
                $routes = Get-Content -LiteralPath ($prefix + '-routes.json') -Raw | ConvertFrom-Json
                foreach ($route in @('Home','Runs','Settings','Run Story')) {
                    if (@($routes.visits | Where-Object { $_.route -eq $route -and $_.invoked }).Count -lt 2) { throw 'P4 route workload did not visit all required routes twice' }
                }
                if ($p4.ContainsKey($name)) { $report | Add-Member -NotePropertyName p4_workload -NotePropertyValue $p4[$name].manifest }
            }
            $rows = @(Import-Csv -LiteralPath "$prefix-totals.csv")
            $memory = @($rows | ForEach-Object { [double]$_.private_bytes })
            $report | Add-Member -NotePropertyName median_bytes -NotePropertyValue (Get-Percentile $memory .5)
            $cpu = @()
            if ($report.cpu_result -eq 'measured') {
                $cpu = @($rows | Where-Object { -not [string]::IsNullOrWhiteSpace($_.cpu_machine_percent) } | ForEach-Object { [double]$_.cpu_machine_percent })
            }
            $report | Add-Member -NotePropertyName cpu_median_machine_percent -NotePropertyValue (Get-Percentile $cpu .5)
            $report | Add-Member -NotePropertyName cpu_p95_machine_percent -NotePropertyValue (Get-Percentile $cpu .95)
            if ($name -eq 'transitions') {
                if (Add-TransitionResult $report $rows $seconds $loops) { $exitCode = 2 }
            }
            if ($name -eq 'startup') {
                $timing = [pscustomobject]@{ samples_ms=$startup.ToArray(); maximum_ms=($startup | Measure-Object -Maximum).Maximum; budget_ms=1500; result='pass'; signal='Main HWND visible, WaitForInputIdle successful, Process.Responding true; native window proxy, not interactive rail'; resource_trial=5 }
                if ($timing.maximum_ms -ge 1500) {
                    $timing.result = 'fail'; $exitCode = [Math]::Max($exitCode,1)
                    if ($report.result -ne 'not_measured') { $report.result = 'fail' }
                    $report.passed = $false
                }
                $report | Add-Member -NotePropertyName startup -NotePropertyValue $timing
            }
            if ($collectorExit -notin @(0,1,2)) { throw 'Collector failed' }
            $exitCode = [Math]::Max($exitCode,$collectorExit)
        } catch {
            Write-Diagnostic $_
            $scenarioFailed = $true; $exitCode = 2
        } finally {
            if ($null -ne $routeJob) { Stop-Job $routeJob; Remove-Job $routeJob }
            if ($null -ne $process) {
                try { Close-Scenario $process } catch { Write-Diagnostic $_; $scenarioFailed = $true; $exitCode = 2 }
            }
        }
        if ($scenarioFailed) {
            if ($null -eq $report) {
                $report = [pscustomobject]@{ scenario=$name; build_configuration_label='release'; result='not_measured'; memory_result='not_measured'; cpu_result='not_measured'; passed=$false; reason=$null }
            }
            $report | Add-Member -NotePropertyName driver_result -NotePropertyValue 'not_measured' -Force
            $report.result = 'not_measured'; $report.passed = $false
            $report.reason = 'Scenario, window readiness, collection or clean shutdown failed. Inspect Maxed before repeating with fresh output files.'
        }
        $report | Add-Member -NotePropertyName scenario_result -NotePropertyValue $report.result
        Write-Summary $report $prefix
        Write-Output ("Scenario {0}: {1}" -f $name,$report.scenario_result)
        if ($name -eq 'transitions' -and $null -ne $report.PSObject.Properties['memory_growth']) {
            Write-Output ("Growth {0}: {1} {2}" -f $name,$report.memory_growth.result,$report.memory_growth.reason)
        }
        if ($scenarioFailed) { break }
    }
    return $exitCode
}

if ($SelfTest) { Invoke-SelfTest; exit 0 }
try {
    $output = @(Invoke-Scenarios)
    if ($output.Count -gt 1) { $output[0..($output.Count-2)] | Write-Output }
    exit ([int]$output[-1])
} catch {
    Write-Diagnostic $_
    # Never print raw exceptions: launch/CIM/filesystem errors can contain paths.
    Write-Output 'Home measurement failed. Check scenario names, release executable, existing output directory, fresh filenames and that Maxed is closed.'
    exit 2
} finally {
    # Covers startup/readiness failures, environment restoration and report errors,
    # including launches that could not be returned to the scenario's local owner.
    $cleanupFailed = $false
    foreach ($launch in $script:LaunchedProcesses) {
        if (-not $launch.Closed) {
            try { Close-Scenario $launch } catch { Write-Diagnostic $_; $cleanupFailed = $true }
        }
    }
    if ($cleanupFailed) {
        Write-Output 'Owned Maxed process cleanup encountered a failure.'
        exit 2
    }
}
