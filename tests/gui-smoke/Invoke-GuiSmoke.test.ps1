$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. "$PSScriptRoot/Invoke-GuiSmoke.ps1" -FunctionsOnly
$realBuild = ${function:Build-SmokeApp}
$realRemoveProfile = ${function:Remove-SmokeProfile}
$realGetProfileLockers = ${function:Get-SmokeProfileLockers}

function Assert($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

# Real child-environment construction must never modify the parent.
$before = $env:MAXED_GUI_SMOKE
$info = New-SmokeStartInfo 'synthetic.exe' @('a b', 'plain', 'ends\', 'has"quote') $PSScriptRoot @{ MAXED_GUI_SMOKE = '1' }
Assert ($info.EnvironmentVariables['MAXED_GUI_SMOKE'] -eq '1') 'Child flag missing'
Assert ($env:MAXED_GUI_SMOKE -eq $before) 'Parent environment changed'
Assert (-not $info.UseShellExecute) 'Shell execution would lose child environment isolation'
Assert ($info.Arguments -eq '"a b" "plain" "ends\\" "has\"quote"') 'Windows argument quoting failed'

# Preserve the app's disabled features in the last (environment) switch.
$policy = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../../app/src-tauri/src/window_policy.rs') -Raw
$browserArgs = Get-SmokeBrowserArguments 9238 $policy
$expectedBrowserArgs = '--remote-debugging-port=9238 --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling'
Assert ($browserArgs -ceq $expectedBrowserArgs) 'Smoke arguments lost the port, app features or display-independent flags'
foreach ($flag in @('--remote-debugging-port=', '--disable-features=', 'CalculateNativeWinOcclusion', '--disable-backgrounding-occluded-windows', '--disable-renderer-backgrounding', '--disable-background-timer-throttling')) {
    Assert ([regex]::Matches($browserArgs, [regex]::Escape($flag)).Count -eq 1) "Duplicate smoke switch: $flag"
}
$merged = Get-SmokeBrowserArguments 9345 'pub const DEFAULT_BROWSER_ARGS: &str = "--disable-features=First,CalculateNativeWinOcclusion --disable-features=Second,First --disable-gpu";'
Assert ($merged -ceq '--remote-debugging-port=9345 --disable-features=First,CalculateNativeWinOcclusion,Second --disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling') 'Feature union duplicated or discarded app features'
$noFeatures = Get-SmokeBrowserArguments 9345 'pub const DEFAULT_BROWSER_ARGS: &str = "--disable-gpu";'
Assert ($noFeatures -like '*--disable-features=CalculateNativeWinOcclusion *') 'No-feature app defaults could not add occlusion control'
$refused = $false
try { Get-SmokeBrowserArguments 9238 'pub const DEFAULT_BROWSER_ARGS: &str = OTHER_CONSTANT;' }
catch { $refused = $_.Exception.Message -like '*DEFAULT_BROWSER_ARGS*' }
Assert $refused 'Unrecognized app defaults silently discarded the app feature policy'

# Port collision uses a real local listener and must fail before any launch.
$listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$listener.Start()
try {
    $refused = $false
    try { Assert-SmokePortFree $listener.LocalEndpoint.Port } catch { $refused = $true }
    Assert $refused 'Listening port was accepted'
} finally { $listener.Stop() }

# Mock external build/Windows boundaries; retain real orchestration and finally.
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('maxed-launcher-test-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($testRoot)
$policyDir = Join-Path $testRoot 'app/src-tauri/src'
[void][IO.Directory]::CreateDirectory($policyDir)
[IO.File]::WriteAllText((Join-Path $policyDir 'window_policy.rs'), $policy)
$script:events = [Collections.Generic.List[string]]::new()
$script:failure = ''
$script:cdpExit = 0
$script:collision = $false
$script:appInfo = $null
$script:buildEnvironment = $null
$script:gracefulExit = $true
function Assert-SmokeWindows {}
function Assert-SmokePortFree([int]$Port) { if ($script:collision) { throw 'Port already listening' } }
function New-SmokeProfile { $p = Join-Path $testRoot ([guid]::NewGuid().ToString('N')); [void][IO.Directory]::CreateDirectory($p); return $p }
function Build-SmokeApp([string]$Repo, [string]$Profile) {
    $script:events.Add('build')
    $script:buildProfile = $Profile
    if ($script:failure -eq 'build') { throw 'Synthetic build failure' }
    return (Join-Path $testRoot 'maxed.exe')
}
function New-SmokeJob {
    $job = [pscustomobject]@{}
    $job | Add-Member ScriptMethod Start {
        param($Info)
        $script:events.Add('start')
        $script:appInfo = $Info
        if ($script:failure -eq 'start') { throw 'Synthetic start failure' }
        $process = [pscustomobject]@{ HasExited = $false }
        $process | Add-Member ScriptMethod CloseMainWindow {
            $script:events.Add('close-window')
            if ($script:failure -eq 'close') { throw 'Synthetic close failure' }
            return $true
        }
        $process | Add-Member ScriptMethod WaitForExit {
            param($Milliseconds)
            Assert ($Milliseconds -gt 0 -and $Milliseconds -le 3000) 'Unbounded graceful wait'
            $script:events.Add('graceful-wait')
            return $script:gracefulExit
        }
        $process | Add-Member ScriptMethod Dispose { $script:events.Add('process-dispose') }
        return $process
    }
    $job | Add-Member ScriptMethod Terminate {
        $script:events.Add('job-terminate')
        if ($script:failure -eq 'cleanup') { throw 'Synthetic job cleanup failure' }
    }
    $job | Add-Member ScriptMethod Dispose { $script:events.Add('job-dispose') }
    return $job
}
function Invoke-SmokeCdp($Job, $App, [string]$Repo, [int]$Port, [string]$Output, [int]$TimeoutSeconds) {
    $script:events.Add('cdp')
    if ($script:failure -eq 'cdp') { throw 'Synthetic CDP timeout' }
    return $script:cdpExit
}
function Remove-SmokeProfile([string]$Profile, $Job, $Report) {
    $script:events.Add('remove-profile')
    Assert ($script:failure -eq 'build' -or $script:events.Contains('job-terminate')) 'Profile removed before process cleanup'
    if ($script:failure -like 'lock-*') {
        $budget = if ($script:failure -in @('lock-clears', 'lock-outside-clears', 'lock-exited')) { 5000 } else { 40 }
        & $realRemoveProfile $Profile $Job $Report -TimeoutMilliseconds $budget -InitialDelayMilliseconds 1
        return
    }
    Remove-Item -LiteralPath $Profile -Recurse -Force
}

# Only the Windows/filesystem boundaries are mocked: exercise the real retry
# loop, classification, stdout diagnostics, report serialization and exit status.
function Remove-SmokeProfileTree([string]$Profile) {
    $script:deleteAttempts++
    if ($script:failure -in @('lock-clears', 'lock-outside-clears', 'lock-exited') -and $script:deleteAttempts -ge 3) {
        Remove-Item -LiteralPath $Profile -Recurse -Force
        return
    }
    throw [IO.IOException]::new('Synthetic sharing violation: 000003.log')
}
function Get-SmokeProfileLockers([string]$Profile, $Job) {
    Assert ($null -ne $Job) 'Diagnostics lost the job identity'
    Assert (-not $script:events.Contains('job-dispose')) 'Job handle closed before membership diagnosis'
    if ($script:failure -eq 'lock-diagnostics') { throw 'Synthetic Restart Manager access denied' }
    $processes = @()
    if ($script:failure -in @('lock-outside', 'lock-orphan', 'lock-outside-clears', 'lock-exited')) {
        $processes = @([pscustomobject]@{
            process_id = 4242; image_name = 'synthetic-webview.exe'; application_name = 'Synthetic WebView'
            started_utc = '2026-10-14T12:00:00Z'; alive = ($script:failure -ne 'lock-exited')
            in_job = ($script:failure -eq 'lock-orphan'); diagnostic_error = $null
        })
    }
    return [pscustomobject]@{ files = @((Join-Path $Profile '000003.log')); processes = $processes }
}

try {
    # Model setup-node prepending its executable ahead of a machine-wide Node.
    $firstNode = Join-Path $testRoot 'toolcache node.exe'
    $secondNode = Join-Path $testRoot 'machine node.exe'
    [IO.File]::WriteAllText($firstNode, 'synthetic')
    [IO.File]::WriteAllText($secondNode, 'synthetic')
    $resolved = @(Resolve-SmokeExecutable 'node' {
        param($Name)
        Assert ($Name -eq 'node') 'Resolver looked up the wrong command'
        [pscustomobject]@{ Source = $firstNode }
        [pscustomobject]@{ Source = $secondNode }
    })
    Assert ($resolved.Count -eq 1 -and $resolved[0] -ceq $firstNode) 'Resolver joined multiple PATH matches or ignored PATH order'
    $resolvedInfo = New-SmokeStartInfo $resolved[0] @() $testRoot
    Assert ($resolvedInfo.FileName -ceq $firstNode) 'Start info lost the single executable path'
    foreach ($invalidSource in @('relative-node.exe', (Join-Path $testRoot 'missing.exe'), $testRoot, "$firstNode $secondNode")) {
        $refused = $false
        try { Resolve-SmokeExecutable 'node' { [pscustomobject]@{ Source = $invalidSource } } }
        catch { $refused = $_.Exception.Message -like '*node*rooted path to an existing file*' }
        Assert $refused "Resolver accepted invalid executable: $invalidSource"
    }
    $refused = $false
    try { Resolve-SmokeExecutable 'node' { } }
    catch { $refused = $_.Exception.Message -like '*node*not found*PATH*' }
    Assert $refused 'Resolver did not clearly reject a missing command'

    # The always() cleanup targets one recorded owner, never a process name or a
    # recycled PID. Exercise this with a synthetic sleeper on every platform.
    $ownerOutput = Join-Path $testRoot 'owner-cleanup'
    [void][IO.Directory]::CreateDirectory($ownerOutput)
    Stop-SmokeOwner $ownerOutput # No launch record: nothing to kill.
    $sleepInfo = New-SmokeStartInfo (Get-Process -Id $PID).Path @('-NoProfile', '-Command', 'Start-Sleep -Seconds 120') $testRoot
    $sleeper = [Diagnostics.Process]::Start($sleepInfo)
    try {
        Write-SmokeOwner $ownerOutput $sleeper
        $ownerFile = Join-Path $ownerOutput 'owner.json'
        $owner = Get-Content -LiteralPath $ownerFile -Raw | ConvertFrom-Json
        $owner.started_utc_ticks = '1'
        [IO.File]::WriteAllText($ownerFile, ($owner | ConvertTo-Json))
        Stop-SmokeOwner $ownerOutput
        Assert (-not $sleeper.HasExited) 'Cleanup killed a mismatched PID identity'
        Write-SmokeOwner $ownerOutput $sleeper
        $owner = Get-Content -LiteralPath $ownerFile -Raw | ConvertFrom-Json
        $owner.executable = 'unrelated.exe'
        [IO.File]::WriteAllText($ownerFile, ($owner | ConvertTo-Json))
        Stop-SmokeOwner $ownerOutput
        Assert (-not $sleeper.HasExited) 'Cleanup killed a mismatched executable'
        Write-SmokeOwner $ownerOutput $sleeper
        Stop-SmokeOwner $ownerOutput
        Assert ($sleeper.WaitForExit(10000)) 'Cleanup left the recorded owner alive'
        Stop-SmokeOwner $ownerOutput # Already gone: idempotent.
    } finally {
        if (-not $sleeper.HasExited) { $sleeper.Kill(); $sleeper.WaitForExit() }
        $sleeper.Dispose()
    }
    foreach ($case in @('success', 'cdp-exit', 'build', 'start', 'cdp', 'port', 'cleanup', 'graceful-timeout', 'close',
        'lock-clears', 'lock-outside', 'lock-orphan', 'lock-diagnostics', 'lock-outside-clears', 'lock-exited')) {
        $script:events.Clear()
        $script:failure = $case
        $script:collision = $case -eq 'port'
        $script:cdpExit = if ($case -eq 'cdp-exit') { 17 } else { 0 }
        $script:gracefulExit = $case -ne 'graceful-timeout'
        $script:deleteAttempts = 0
        $output = Join-Path $testRoot $case
        $runOutput = @(Invoke-GuiSmoke -Repo $testRoot -Port 9238 -OutputDir $output -TimeoutSeconds 60 6>&1)
        $exitCode = $runOutput[-1]
        $log = ($runOutput | ForEach-Object { "$_" }) -join "`n"
        $expected = if ($case -in @('success', 'graceful-timeout', 'close', 'lock-clears', 'lock-exited')) { 0 } elseif ($case -eq 'cdp-exit') { 17 } else { 1 }
        Assert ($exitCode -eq $expected) "Wrong exit for $case`: $exitCode"
        if ($case -eq 'port') {
            Assert ($script:events.Count -eq 0) 'Port collision reached build/launch'
        } else {
            Assert ($script:events.Contains('remove-profile')) "Profile cleanup skipped for $case"
            if ($case -notin @('lock-outside', 'lock-orphan', 'lock-diagnostics')) {
                Assert (-not (Test-Path -LiteralPath $script:buildProfile)) 'Profile remains'
            }
            if ($case -ne 'build') { Assert ($script:events.Contains('job-dispose')) "Job cleanup skipped for $case" }
        }
        if ($case -eq 'success') {
            Assert ($script:appInfo.EnvironmentVariables['MAXED_GUI_SMOKE'] -eq '1') 'Launcher omitted smoke flag'
            Assert ($script:appInfo.EnvironmentVariables['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] -ceq $expectedBrowserArgs) 'Wrong child-only smoke browser arguments'
            Assert ($script:appInfo.EnvironmentVariables['WEBVIEW2_USER_DATA_FOLDER'].StartsWith($script:buildProfile)) 'WebView2 escaped profile'
        }
        $report = Get-Content -LiteralPath (Join-Path $output 'launcher.json') -Raw | ConvertFrom-Json
        Assert ($report.exit_code -eq $expected) 'Launcher report disagrees with exit code'
        $owner = Get-Content -LiteralPath (Join-Path $output 'owner.json') -Raw | ConvertFrom-Json
        Assert ($owner.process_id -eq $PID) 'Launch did not record its actual owner PID'
        if ($case -eq 'cleanup') { Assert ($report.cleanup_errors.Count -eq 1) 'Cleanup failure missing from report' }
        if ($case -in @('success', 'graceful-timeout', 'close')) {
            Assert ($script:events.IndexOf('close-window') -lt $script:events.IndexOf('job-terminate')) 'Graceful close not attempted before forced termination'
            Assert ($script:events.IndexOf('job-terminate') -lt $script:events.IndexOf('remove-profile')) 'Termination did not precede deletion'
            Assert ($script:events.IndexOf('remove-profile') -lt $script:events.IndexOf('job-dispose')) 'Final kill-on-close guarantee was released early'
        }
        if ($case -eq 'graceful-timeout') { Assert ($script:events.Contains('graceful-wait')) 'Graceful timeout path did not wait' }
        if ($case -eq 'lock-clears') {
            Assert ($script:deleteAttempts -eq 3) 'Transient lock did not retry to success'
            Assert ($report.cleanup_errors.Count -eq 0) 'Transient release was treated as a failure'
            Assert ($report.profile_lockers.Count -eq 2) 'Retry evidence was lost after success'
        }
        if ($case -in @('lock-outside', 'lock-orphan', 'lock-outside-clears')) {
            $locker = $report.profile_lockers[0].processes[0]
            Assert ($locker.process_id -eq 4242 -and $locker.image_name -eq 'synthetic-webview.exe') 'Locker identity missing'
            Assert (([datetime]$locker.started_utc).ToUniversalTime() -eq ([datetime]'2026-10-14T12:00:00Z').ToUniversalTime()) 'Locker start time missing'
            $kind = if ($case -eq 'lock-orphan') { 'ORPHAN' } else { 'outside smoke job' }
            Assert (($report.cleanup_errors -join ' ') -like "*$kind*synthetic-webview.exe*4242*") 'Locker classification missing from failure'
            Assert ($log -like "*$kind*synthetic-webview.exe*4242*") 'CI log omitted locker identity'
        }
        if ($case -eq 'lock-diagnostics') {
            Assert ($report.profile_lockers[0].diagnostic_error -like '*Restart Manager access denied*') 'Diagnostic failure was hidden'
            Assert (($report.cleanup_errors -join ' ') -like '*sharing violation*') 'Diagnostic error replaced deletion failure'
        }
    }
    # Add-Type compiles the native declarations on macOS without calling them.
    Add-Type -Path "$PSScriptRoot/WindowsJob.cs"
    $warmTarget = Join-Path $testRoot 'warm-target'
    $exeDir = Join-Path $warmTarget 'x86_64-pc-windows-msvc/debug'
    [void][IO.Directory]::CreateDirectory($exeDir)
    [IO.File]::WriteAllText((Join-Path $exeDir 'maxed.exe'), 'synthetic')
    $script:buildCalls = [Collections.Generic.List[object]]::new()
    function Invoke-SmokeCommand([string]$Executable, [string[]]$Arguments, [string]$Repo, [hashtable]$Environment = @{}) {
        $script:buildCalls.Add(@{ executable = $Executable; arguments = $Arguments; environment = $Environment })
        if ($Arguments[0] -eq 'metadata') { return (@{ target_directory = $warmTarget } | ConvertTo-Json) }
        return ''
    }
    $profilePath = Join-Path $testRoot 'build-profile'
    $built = & $realBuild $testRoot $profilePath
    Assert ($built -eq (Join-Path $exeDir 'maxed.exe')) 'Ignored cargo metadata target directory'
    $build = $script:buildCalls[$script:buildCalls.Count - 1]
    Assert (($build.arguments -join ' ') -eq 'build --locked -p maxed-app --bin maxed --features custom-protocol --target x86_64-pc-windows-msvc') 'Wrong debug build mode'
    $override = ($build.environment.TAURI_CONFIG | ConvertFrom-Json).app.appDirectoriesOverride
    Assert ($override.data -eq (Join-Path $profilePath 'app')) 'App data not isolated'
    Assert ($override.localData -eq (Join-Path $profilePath 'webview')) 'WebView data not isolated'
    Assert ($build.environment.CARGO_PROFILE_DEV_DEBUG_ASSERTIONS -eq 'true') 'Debug safety gate not enforced'

    if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        # Real RM marshaling/identity and exact job membership, using one file
        # holder in our job and an unrelated empty job. No WebView2 required.
        $lockProfile = Join-Path $testRoot 'native-lock-profile'
        [void][IO.Directory]::CreateDirectory($lockProfile)
        $lockFile = Join-Path $lockProfile 'held.log'
        $readyFile = Join-Path $testRoot 'lock-holder.ready'
        $holderJob = [MaxedGuiSmokeJob]::new()
        $otherJob = [MaxedGuiSmokeJob]::new()
        $holder = $null
        try {
            $holdSource = '$ErrorActionPreference = "Stop"; $f = [IO.File]::Open($env:SMOKE_LOCK_FILE, "Create", "ReadWrite", "None"); try { [IO.File]::WriteAllText($env:SMOKE_LOCK_READY, "ready"); Start-Sleep -Seconds 120 } finally { $f.Dispose() }'
            $holdInfo = New-SmokeStartInfo (Get-Process -Id $PID).Path @('-NoProfile', '-Command', $holdSource) $testRoot @{
                SMOKE_LOCK_FILE = $lockFile; SMOKE_LOCK_READY = $readyFile
            }
            $holder = $holderJob.Start($holdInfo)
            $readyClock = [Diagnostics.Stopwatch]::StartNew()
            while (-not (Test-Path -LiteralPath $readyFile)) {
                Assert (-not $holder.HasExited -and $readyClock.ElapsedMilliseconds -lt 10000) 'Native lock holder did not become ready'
                Start-Sleep -Milliseconds 25
            }
            foreach ($membership in @($true, $false)) {
                $queryJob = if ($membership) { $holderJob } else { $otherJob }
                $locks = & $realGetProfileLockers $lockProfile $queryJob
                $match = @($locks.processes | Where-Object process_id -eq $holder.Id)
                Assert ($match.Count -eq 1) 'Restart Manager did not find the native file holder'
                Assert ($match[0].alive -eq $true -and $match[0].in_job -eq $membership) 'Restart Manager misclassified exact job membership'
                Assert ($match[0].image_name -eq [IO.Path]::GetFileName($holdInfo.FileName)) 'Native locker image name missing'
                Assert (([datetime]$match[0].started_utc).ToUniversalTime() -eq $holder.StartTime.ToUniversalTime()) 'Native locker identity mismatch'
                Assert ($null -eq $match[0].diagnostic_error) 'Native locker inspection failed'
            }
            $holderJob.Terminate()
            Assert ($holder.WaitForExit(10000)) 'Terminated file holder survived'
            $locks = & $realGetProfileLockers $lockProfile $holderJob
            Assert (@($locks.processes).Count -eq 0) 'Exited file holder still reported'
        } finally {
            $holderJob.Dispose()
            $otherJob.Dispose()
            if ($null -ne $holder) { $holder.Dispose() }
        }
        # Exercise native ownership with a synthetic orphan; no app or WebView2.
        $pidFile = Join-Path $testRoot 'synthetic-child.pid'
        # detached: libuv otherwise puts children in its own kill-on-close job, so they die with the parent.
        $source = 'const {spawn}=require("node:child_process"); const fs=require("node:fs"); const c=spawn(process.execPath,["-e","setInterval(()=>{},1000)"],{stdio:"ignore",detached:true}); fs.writeFileSync(process.argv[1],String(c.pid)); c.unref();'
        $nativeJob = [MaxedGuiSmokeJob]::new()
        $parent = $null; $child = $null
        try {
            $invalidInfo = New-SmokeStartInfo 'invalid|application.exe' @() $testRoot
            $launchError = $null
            try { $nativeJob.Start($invalidInfo) } catch { $launchError = $_.Exception.InnerException }
            Assert ($null -ne $launchError) 'Invalid native launch unexpectedly succeeded'
            Assert ($launchError.Message.Contains($invalidInfo.FileName) -and $launchError.Message.Contains($testRoot)) 'Native launch error omitted application or working directory'
            Assert ($launchError.InnerException -is [ComponentModel.Win32Exception]) 'Native launch error lost the Win32 exception'
            Assert ($launchError.Message.Contains("Win32 error $($launchError.InnerException.NativeErrorCode)")) 'Native launch error omitted the Win32 code'
            $start = New-SmokeStartInfo (Resolve-SmokeExecutable 'node') @('-e', $source, $pidFile) $testRoot
            $parent = $nativeJob.Start($start)
            Assert ($parent.WaitForExit(10000)) 'Synthetic parent failed to exit'
            Assert ($parent.ExitCode -eq 0) 'Synthetic parent failed'
            $child = [Diagnostics.Process]::GetProcessById([int][IO.File]::ReadAllText($pidFile))
            [void]$child.Handle
            Assert (-not $child.HasExited) 'Synthetic child should survive parent exit'
            $nativeJob.Dispose()
            Assert ($child.WaitForExit(10000)) 'Job cleanup left an orphan'
        } finally {
            $nativeJob.Dispose()
            if ($null -ne $parent) { $parent.Dispose() }
            if ($null -ne $child) { $child.Dispose() }
        }
    } else { Write-Host 'SKIP: native Restart Manager and Job Object orphan tests require Windows' }
    Write-Host 'PASS: smoke browser switches/feature merge, cleanup retries, locker diagnostics/report/stdout, orphan/outside failures, graceful timeout fallback, first-PATH executable resolution, child environment, quoting, port refusal, exit propagation, scoped owner cleanup, isolated custom-protocol build, target directory; Windows helper compiles'
} finally {
    Remove-Item -LiteralPath $testRoot -Recurse -Force
}
