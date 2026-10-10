# Explicit perf-live/custom-protocol release build; profile is compiled via TAURI_CONFIG.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Exe,
    [Parameter(Mandatory=$true)][string]$Profile,
    [Parameter(Mandatory=$true)][string]$OutputDir,
    [ValidateSet('before','after')][string]$Mode = 'after',
    [switch]$Diagnostic,
    [switch]$DiagnosticRetry,
    [switch]$DiagnosticShort,
    [switch]$Attribution,
    [switch]$FreshIdle,
    [switch]$AllowV8SizingOverride,
    [ValidateSet('none','natural','seed-gc','optimize-size','seed-no-atlas')][string]$PeaksProbe = 'none',
    [int]$DebugPort = 9238
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$sizingOverride = [Environment]::GetEnvironmentVariable('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS')
if ($AllowV8SizingOverride -and ($Diagnostic -or $FreshIdle -or $Mode -ne 'after' -or
    $sizingOverride -notin @('--js-flags=--optimize-for-size','--js-flags=--max-semi-space-size=1'))) {
    throw 'Sizing override requires an uninstrumented after run and one supported sizing-only argument'
}
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$output = (Resolve-Path -LiteralPath $OutputDir).Path
if (@(Get-ChildItem -LiteralPath $output -Force).Count) { throw 'Use a fresh output directory' }
$exePath = (Resolve-Path -LiteralPath $Exe).Path
$profilePath = [IO.Path]::GetFullPath($Profile)
. (Join-Path $PSScriptRoot 'p4-memory-profile.ps1')
[void](Resolve-TemporaryMemoryPath $output)
[void](Resolve-TemporaryMemoryPath $profilePath)
if ($Attribution -and (-not $Diagnostic -or $DiagnosticShort -or $FreshIdle)) { throw 'Attribution requires a full diagnostic live run' }
if ($PeaksProbe -ne 'none' -and (-not $Attribution -or $Mode -ne 'after')) { throw 'Peak probes require after attribution diagnostics' }
if ($FreshIdle -and ($Diagnostic -or @(Get-ChildItem -LiteralPath $profilePath -Force).Count)) { throw 'Fresh idle requires an empty profile without CDP' }
$temporary = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
if (-not $profilePath.StartsWith($temporary,[StringComparison]::OrdinalIgnoreCase)) { throw 'Use a compiled temporary profile' }
# The build's configured literal and feature marker must be in these exact bytes.
$binaryText = [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($exePath))
if (-not $binaryText.Contains($profilePath) -or -not $binaryText.Contains('PERF_LIVE busy')) { throw 'Executable does not attest the compiled perf profile' }
$binaryText = $null
$load = [Collections.Generic.List[object]]::new()
$quiet = $false
if ($DiagnosticRetry -and -not $Diagnostic) { throw 'Only diagnostic retries may skip the full load wait' }
if ($DiagnosticShort -and (-not $Diagnostic -or $Mode -ne 'before')) { throw 'Only before diagnostics may shorten the schedule' }
$lastAttempt = if ($DiagnosticRetry) {0} else {15}
for ($attempt = 0; $attempt -le $lastAttempt; $attempt++) {
    $first = @(Get-Process cargo,rustc,node,vitest -ErrorAction SilentlyContinue)
    $cpu = @{}; foreach ($p in $first) { $cpu[$p.Id] = $p.CPU }
    Start-Sleep -Seconds 5
    $active = @(Get-Process cargo,rustc,node,vitest -ErrorAction SilentlyContinue | Where-Object {
        $_.ProcessName -in @('cargo','rustc','vitest') -or
        ($cpu.ContainsKey($_.Id) -and ($_.CPU - $cpu[$_.Id]) -gt 0.25)
    })
    $os = Get-CimInstance Win32_OperatingSystem
    $load.Add([pscustomobject]@{utc=[datetime]::UtcNow.ToString('o');attempt=$attempt;
        heavy_count=$active.Count; names=@($active | Select-Object -ExpandProperty ProcessName);
        free_physical_bytes=[long]$os.FreePhysicalMemory*1024})
    Write-Output "load attempt=$attempt heavy=$($active.Count)"
    if (-not $active.Count) { $quiet=$true; break }
    if ($attempt -lt $lastAttempt) { Start-Sleep -Seconds 115 }
}
$load.ToArray() | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $output 'machine-load.json') -Encoding UTF8
if (-not $quiet) { throw 'Machine did not become quiet; launch blocked' }
$lockPath = Join-Path (Split-Path -Parent $repo) '.maxed-launch.lock'
$token = [guid]::NewGuid().ToString('N')
$lock = $null; $process = $null; $collector = $null; $profiler = $null
try {
    try { $lock = [IO.File]::Open($lockPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::ReadWrite,[IO.FileShare]::Read) }
    catch [IO.IOException] { throw 'Launch lock unavailable; another owner may be measuring. No launch attempted.' }
    $bytes = [Text.Encoding]::UTF8.GetBytes("wave3-lane-h $token $([datetime]::UtcNow.ToString('o'))")
    $lock.Write($bytes,0,$bytes.Length); $lock.Flush()
    # Recheck compiler/test activity immediately before launch.
    $launchLoad = @(Get-Process cargo,rustc,vitest -ErrorAction SilentlyContinue)
    [pscustomobject]@{utc=[datetime]::UtcNow.ToString('o');compiler_test_count=$launchLoad.Count;
        names=@($launchLoad | Select-Object -ExpandProperty ProcessName);
        free_physical_bytes=[long](Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory*1024} |
        ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $output 'launch-load.json') -Encoding UTF8
    if ($launchLoad.Count) { throw 'Compiler/test load blocked launch' }
    $env:MAXED_PERF_LIVE=$Mode
    if ($FreshIdle) { Remove-Item Env:MAXED_PERF_LIVE -ErrorAction SilentlyContinue }
    Remove-Item Env:MAXED_MEASURE_DEMO,Env:MAXED_MEASURE_DEMO_LOOPS -ErrorAction SilentlyContinue
    if ($DiagnosticShort) { $env:MAXED_PERF_SHORT='1' }
    else { Remove-Item Env:MAXED_PERF_SHORT -ErrorAction SilentlyContinue }
    Remove-Item Env:WEBVIEW2_USER_DATA_FOLDER -ErrorAction SilentlyContinue
    if ($Diagnostic -and @(Get-NetTCPConnection -LocalPort $DebugPort -State Listen -ErrorAction SilentlyContinue).Count) { throw 'Diagnostic port is already in use' }
    if ($Diagnostic) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=$DebugPort" }
    elseif ($AllowV8SizingOverride) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=$sizingOverride }
    else { Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
    # Diagnostic only; heap-sizing flags are not production policy.
    if ($PeaksProbe -eq 'optimize-size') { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS += ' --js-flags=--optimize-for-size' }
    $stderr = Join-Path $output 'driver.log'
    $process = Start-Process -FilePath $exePath -PassThru -WindowStyle Hidden -RedirectStandardError $stderr -RedirectStandardOutput (Join-Path $output 'stdout.log')
    $born = $process.StartTime.ToUniversalTime()
    [pscustomobject]@{mode=$(if($FreshIdle){'fresh-idle'}else{$Mode});diagnostic=[bool]$Diagnostic;machine_quiet=$quiet;
        executable_sha256=(Get-FileHash -LiteralPath $exePath -Algorithm SHA256).Hash.ToLowerInvariant();
        start_utc=$born.ToString('o');root_pid=$process.Id;peaks_probe=$PeaksProbe;
        sizing_override=$(if($AllowV8SizingOverride){$sizingOverride}else{$null})} |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output 'launch.json') -Encoding UTF8
    $seconds = if ($PeaksProbe -eq 'seed-no-atlas') {120} elseif ($PeaksProbe -eq 'optimize-size') {700} elseif ($PeaksProbe -eq 'natural') {1400} elseif ($PeaksProbe -ne 'none') {930} elseif ($FreshIdle) {210} elseif ($Attribution) {930} elseif ($DiagnosticShort) {300} else {840}
    $collector = Start-Job -ArgumentList $repo,$process.Id,$output,$seconds -ScriptBlock {
        param($repo,$rootId,$output,$seconds)
        & (Join-Path $repo 'scripts/Measure-MaxedMemory.ps1') -RootProcessId $rootId -Seconds $seconds -IntervalMs 1000 -Scenario 'live-realm' -OutputPrefix (Join-Path $output 'memory')
    }
    if ($Attribution) {
        $profilerScript = if ($PeaksProbe -eq 'none') {'live-realm-attribution.mjs'} else {'live-realm-peaks.mjs'}
        $profiler = Start-Process -FilePath 'node.exe' -ArgumentList @(('"' + (Join-Path $PSScriptRoot $profilerScript) + '"'),$DebugPort,('"' + $output + '"'),$PeaksProbe) -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $output 'attribution-error.log') -RedirectStandardOutput (Join-Path $output 'attribution.log')
    }
    $deadlineSeconds = if ($PeaksProbe -eq 'none') {1020} else {$seconds + 120}
    $deadline = [datetime]::UtcNow.AddSeconds($deadlineSeconds)
    $phases = @{}; $rendered = @{busy=$false;idle=$false;subscription_failed=$false}
    $ongoingLoad = [Collections.Generic.List[object]]::new(); $previousLoad = @{}; $loadAt = [datetime]::MinValue
    while ([datetime]::UtcNow -lt $deadline) {
        $process.Refresh(); if ($process.HasExited) { throw 'Perf app exited early' }
        if (([datetime]::UtcNow - $loadAt).TotalSeconds -ge 10) {
            $activeNames = @(); $nextLoad = @{}
            foreach ($candidate in @(Get-Process cargo,rustc,node,vitest -ErrorAction SilentlyContinue)) {
                if ($profiler -and $candidate.Id -eq $profiler.Id) { continue }
                $nextLoad[$candidate.Id] = $candidate.CPU
                if ($candidate.ProcessName -ne 'node' -or ($previousLoad.ContainsKey($candidate.Id) -and ($candidate.CPU-$previousLoad[$candidate.Id]) -gt 0.5)) { $activeNames += $candidate.ProcessName }
            }
            $loadAt = [datetime]::UtcNow; $previousLoad = $nextLoad
            $ongoingLoad.Add([pscustomobject]@{utc=$loadAt.ToString('o');heavy_count=$activeNames.Count;names=$activeNames})
        }
        $log = if (Test-Path -LiteralPath $stderr) { Get-Content -LiteralPath $stderr -Raw } else { '' }
        foreach ($phase in @('busy','idle','complete')) {
            if (-not $phases.ContainsKey($phase) -and $log -match "PERF_LIVE $phase") {
                $phases[$phase]=[datetime]::UtcNow.ToString('o')
                if ($log -match "PERF_LIVE $phase[^\r\n]*utc=([^\s]+)") { $phases[$phase]=$Matches[1] }
                Write-Output "phase=$phase"
            }
        }
        if ($log -match 'PERF_LIVE failed') { throw 'Synthetic driver failed' }
        if ($FreshIdle -and -not $phases.ContainsKey('idle') -and ([datetime]::UtcNow-$born).TotalSeconds -ge 30) { $phases.idle=[datetime]::UtcNow.ToString('o') }
        $phaseNow = if ($phases.ContainsKey('idle')) {'idle'} elseif ($phases.ContainsKey('busy')) {'busy'} else {$null}
        # Observe each rendered phase once. Repeated accessibility-tree walks
        # can perturb the renderer whose allocation rate we are measuring.
        # A failed live subscription persists and is checked again at Nexus.
        if ($phaseNow -and -not $rendered[$phaseNow]) {
            $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$process.Id)
            $window = [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Children,$condition)
            if ($window) {
                $location = if ($FreshIdle) {'Home'} elseif ($phaseNow -eq 'busy') {'Realm'} else {'Nexus'}
                $label = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$location)
                $failureLabel = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Disconnected. Reconnect.')
                if ($window.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$failureLabel)) { $rendered.subscription_failed=$true }
                $seen = $null -ne $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$label)
                if ($seen -and -not $rendered.subscription_failed -and ($FreshIdle -or $phaseNow -eq 'busy' -or $rendered.busy)) { $rendered[$phaseNow]=$true }
            }
        }
        if ($Diagnostic -and -not $Attribution -and $phases.ContainsKey('idle') -and -not $profiler) {
            $heap = Join-Path $output 'heap'; New-Item -ItemType Directory -Path $heap | Out-Null
            $profiler = Start-Process -FilePath 'node.exe' -ArgumentList @((Join-Path $repo 'tests/performance/live-realm-heap.mjs'),$DebugPort,$heap) -PassThru -WindowStyle Hidden -RedirectStandardError (Join-Path $output 'heap-error.log') -RedirectStandardOutput (Join-Path $output 'heap.log')
        }
        if ($collector.State -in @('Completed','Failed','Stopped')) { break }
        Start-Sleep -Seconds 2
    }
    $phases | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output 'phases.json') -Encoding UTF8
    $rendered | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output 'rendered.json') -Encoding UTF8
    $ongoingLoad.ToArray() | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath (Join-Path $output 'ongoing-load.json') -Encoding UTF8
    Receive-Job $collector -Wait
    if ($Diagnostic -and $profiler) {
        if (-not $profiler.WaitForExit(30000)) { throw 'Diagnostic profiler did not finish' }
        if ($profiler.ExitCode -ne 0) { throw 'Diagnostic profiler failed; inspect temporary error log' }
    }
} finally {
    if ($AllowV8SizingOverride) { Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
    if ($profiler -and -not $profiler.HasExited) { $profiler.Kill(); [void]$profiler.WaitForExit(5000) }
    if ($collector) { Stop-Job $collector -ErrorAction SilentlyContinue; Remove-Job $collector -Force -ErrorAction SilentlyContinue }
    if ($process) {
        $process.Refresh()
        if (-not $process.HasExited -and $process.StartTime.ToUniversalTime() -eq $born) {
            [void]$process.CloseMainWindow(); if (-not $process.WaitForExit(15000)) { Stop-Process -Id $process.Id }
        }
        Start-Sleep -Seconds 3
        # The collector's PID+birth records also cover reparented WebView children.
        $recorded = Join-Path $output 'memory-processes.csv'
        if (Test-Path -LiteralPath $recorded) {
            foreach ($row in (Import-Csv -LiteralPath $recorded | Sort-Object pid,start_utc -Unique)) {
                $owned = Get-Process -Id ([int]$row.pid) -ErrorAction SilentlyContinue
                if ($owned -and $owned.StartTime.ToUniversalTime() -eq ([datetimeoffset]::Parse($row.start_utc)).UtcDateTime) {
                    Stop-Process -InputObject $owned -ErrorAction SilentlyContinue
                    [void]$owned.WaitForExit(5000)
                }
            }
        }
    }
    if ($lock) { $lock.Dispose(); if ((Get-Content -LiteralPath $lockPath -Raw).Contains($token)) { Remove-Item -LiteralPath $lockPath } }
}
