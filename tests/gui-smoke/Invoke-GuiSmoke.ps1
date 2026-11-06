[CmdletBinding()]
param(
    [switch]$FunctionsOnly,
    [switch]$CleanupOnly,
    [ValidateRange(1024,65535)][int]$Port = 9238,
    [string]$OutputDir = 'artifacts/gui-smoke',
    [ValidateRange(15,600)][int]$TimeoutSeconds = 60
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Resolve-SmokeExecutable([string]$Name, [scriptblock]$CommandResolver = {
    param($Name)
    Get-Command $Name -CommandType Application -ErrorAction SilentlyContinue
}) {
    # setup-node prepends its toolcache entry; preserve PATH precedence even
    # when a machine-wide installation supplies another match.
    $command = & $CommandResolver $Name | Select-Object -First 1
    if ($null -eq $command) { throw "Executable '$Name' not found on PATH" }
    $path = $command.Source
    if ($path -isnot [string] -or [string]::IsNullOrWhiteSpace($path) -or
        -not [IO.Path]::IsPathRooted($path) -or -not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Executable '$Name' must resolve to a single rooted path to an existing file"
    }
    return $path
}

function New-SmokeStartInfo([string]$Executable, [string[]]$Arguments, [string]$WorkingDirectory, [hashtable]$Environment = @{}) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Executable
    $info.WorkingDirectory = $WorkingDirectory
    $info.UseShellExecute = $false
    # CommandLineToArgvW / CRT escaping, including trailing slashes and quotes.
    $quoted = foreach ($argument in $Arguments) {
        '"' + [regex]::Replace([regex]::Replace($argument, '(\\*)"', '$1$1\"'), '(\\+)$', '$1$1') + '"'
    }
    $info.Arguments = $quoted -join ' '
    foreach ($key in $Environment.Keys) {
        if ($null -eq $Environment[$key]) { $info.EnvironmentVariables.Remove($key) }
        else { $info.EnvironmentVariables[$key] = [string]$Environment[$key] }
    }
    return $info
}

function Get-SmokeBrowserArguments([int]$Port, [string]$WindowPolicySource) {
    # WebView2 appends the environment arguments. Chromium uses the last
    # --disable-features switch, so repeat the app's features in our union.
    # Fail closed if the Rust constant stops being a plain string literal.
    $policy = [regex]::Match($WindowPolicySource, '(?m)^\s*pub const DEFAULT_BROWSER_ARGS:\s*&str\s*=\s*"([^"\\]*)";')
    if (-not $policy.Success) { throw 'Cannot read DEFAULT_BROWSER_ARGS from window_policy.rs; review the smoke feature merge' }
    $features = [Collections.Generic.List[string]]::new()
    foreach ($match in [regex]::Matches($policy.Groups[1].Value, '(?:^|\s)--disable-features=([^\s]+)')) {
        foreach ($feature in $match.Groups[1].Value.Split(',')) {
            if ($feature -and -not $features.Contains($feature)) { $features.Add($feature) }
        }
    }
    if (-not $features.Contains('CalculateNativeWinOcclusion')) { $features.Add('CalculateNativeWinOcclusion') }
    return "--remote-debugging-port=$Port --disable-features=$($features -join ',') --disable-backgrounding-occluded-windows --disable-renderer-backgrounding --disable-background-timer-throttling"
}

function Assert-SmokeWindows {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'GUI smoke launch requires Windows; use the offline tests here.' }
}

function Assert-SmokePortFree([int]$Port) {
    $listeners = [Net.NetworkInformation.IPGlobalProperties]::GetIPGlobalProperties().GetActiveTcpListeners()
    if (@($listeners | Where-Object Port -eq $Port).Count -gt 0) { throw "CDP port $Port is already listening" }
    $probe = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $Port)
    $probe.Server.ExclusiveAddressUse = $true
    try { $probe.Start() } finally { $probe.Stop() }
}

function New-SmokeProfile {
    $base = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { [IO.Path]::GetTempPath() }
    $profile = Join-Path ([IO.Path]::GetFullPath($base)) ('maxed-gui-smoke-' + [guid]::NewGuid().ToString('N'))
    if (Test-Path -LiteralPath $profile) { throw 'Temporary profile already exists' }
    [void][IO.Directory]::CreateDirectory($profile)
    return $profile
}

function Remove-SmokeProfileTree([string]$Profile) {
    # Only the fresh path created by this invocation reaches this function.
    if (Test-Path -LiteralPath $Profile) { Remove-Item -LiteralPath $Profile -Recurse -Force -ErrorAction Stop }
}

function Get-SmokeProfileLockers([string]$Profile, $Job) {
    Initialize-SmokeNative
    # Remove-Item can report only a leaf name, or a directory-not-empty error.
    # Register remaining files together, never directories (RM rejects them).
    $files = @(if (Test-Path -LiteralPath $Profile) {
        Get-ChildItem -LiteralPath $Profile -File -Recurse -Force -ErrorAction Stop | ForEach-Object FullName
    })
    $processes = @([MaxedGuiSmokeRestartManager]::GetLockers([string[]]$files, $Job))
    return [pscustomobject]@{ files = $files; processes = $processes }
}

function Add-SmokeCleanupError($Report, [string]$Message) {
    if ($Message -notin $Report.cleanup_errors) {
        $Report.cleanup_errors += $Message
        Write-Host "GUI smoke cleanup failed: $Message"
    }
}

function Remove-SmokeProfile([string]$Profile, $Job, $Report,
    [int]$TimeoutMilliseconds = 30000, [int]$InitialDelayMilliseconds = 250) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $delay = $InitialDelayMilliseconds
    while ($true) {
        try {
            Remove-SmokeProfileTree $Profile
            Write-Host "GUI smoke profile removed after $($clock.ElapsedMilliseconds) ms"
            return
        }
        catch { $deleteError = $_.Exception.Message }
        $sample = [ordered]@{
            elapsed_ms = $clock.ElapsedMilliseconds; deletion_error = $deleteError
            files = @(); processes = @(); diagnostic_error = $null
        }
        try {
            $lockers = Get-SmokeProfileLockers $Profile $Job
            $sample.files = @($lockers.files)
            $sample.processes = @($lockers.processes)
            foreach ($locker in $sample.processes) {
                if ($locker.alive -eq $true) {
                    $identity = "$($locker.image_name) (PID $($locker.process_id), started $($locker.started_utc), application $($locker.application_name))"
                    if ($locker.in_job -eq $true) {
                        Add-SmokeCleanupError $Report "ORPHAN: live locker in terminated smoke job: $identity"
                    } elseif ($locker.in_job -eq $false) {
                        Add-SmokeCleanupError $Report "Live profile locker outside smoke job: $identity"
                    } else {
                        Add-SmokeCleanupError $Report "Live profile locker with unknown job membership: $identity"
                    }
                }
            }
        } catch { $sample.diagnostic_error = $_.Exception.Message }
        $Report.profile_lockers += [pscustomobject]$sample
        Write-Host ('GUI smoke profile lock diagnostic: ' + ($sample | ConvertTo-Json -Depth 6 -Compress))
        $remaining = $TimeoutMilliseconds - $clock.ElapsedMilliseconds
        if ($remaining -le 0) { throw "Profile cleanup failed after $($clock.ElapsedMilliseconds) ms: $deleteError" }
        Start-Sleep -Milliseconds ([int][Math]::Min($delay, $remaining))
        $delay = [Math]::Min(2000, $delay * 2)
    }
}

function Initialize-SmokeNative {
    if (-not ('MaxedGuiSmokeJob' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'WindowsJob.cs') }
}

function New-SmokeJob {
    Initialize-SmokeNative
    return [MaxedGuiSmokeJob]::new()
}

function Stop-SmokeJob($Job, $App) {
    try {
        if ($null -ne $App -and -not $App.HasExited) {
            if ($App.CloseMainWindow()) {
                if ($App.WaitForExit(3000)) { Write-Host 'GUI smoke app closed gracefully' }
                else { Write-Host 'GUI smoke graceful close timed out; terminating job' }
            } else { Write-Host 'GUI smoke has no closable main window; terminating job' }
        }
    } catch { Write-Host "GUI smoke graceful close unavailable; terminating job: $($_.Exception.Message)" }
    finally {
        # Always terminate/wait for all descendants, even after a graceful exit.
        # Retain the job handle until lock diagnostics finish; Dispose is the
        # final kill-on-close guarantee, including if termination throws.
        $Job.Terminate()
        Write-Host 'GUI smoke job reached zero active processes'
    }
}

function Write-SmokeOwner([string]$Output, [Diagnostics.Process]$Owner) {
    $record = [ordered]@{
        process_id = $Owner.Id
        started_utc_ticks = [string]$Owner.StartTime.ToUniversalTime().Ticks
        executable = $Owner.MainModule.FileName
        output_directory = [IO.Path]::GetFullPath($Output)
    }
    [IO.File]::WriteAllText((Join-Path $Output 'owner.json'), ($record | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
}

function Stop-SmokeOwner([string]$Output) {
    $outputPath = [IO.Path]::GetFullPath($Output)
    $recordPath = Join-Path $outputPath 'owner.json'
    if (-not (Test-Path -LiteralPath $recordPath -PathType Leaf)) { return }
    $record = Get-Content -LiteralPath $recordPath -Raw | ConvertFrom-Json
    if ($record.output_directory -cne $outputPath -or [int]$record.process_id -le 0 -or
        [string]::IsNullOrWhiteSpace($record.executable) -or [long]$record.started_utc_ticks -le 0) { throw 'Invalid smoke owner record' }
    if ([int]$record.process_id -eq $PID) { throw 'Cleanup must run in a separate process from the smoke owner' }
    try { $owner = [Diagnostics.Process]::GetProcessById([int]$record.process_id) }
    catch [ArgumentException] { return } # The launcher already exited.
    try {
        # Retain the handle before comparing identity; a recycled PID is not ours.
        try { [void]$owner.Handle } catch [InvalidOperationException] { return }
        if ($owner.HasExited) { return }
        if ([string]$owner.StartTime.ToUniversalTime().Ticks -cne [string]$record.started_utc_ticks -or
            $owner.MainModule.FileName -cne $record.executable) { return }
        # The owner holds the sole, non-inheritable Windows Job Object handle.
        # Its exit kills Maxed/WebView2/Node through KILL_ON_JOB_CLOSE, including
        # descendants whose original parent exited. No PID/name-wide tree scan.
        $owner.Kill()
        if (-not $owner.WaitForExit(10000)) { throw 'Smoke owner did not exit during cleanup' }
    } finally { $owner.Dispose() }
}

function Invoke-SmokeCommand([string]$Executable, [string[]]$Arguments, [string]$Repo, [hashtable]$Environment = @{}) {
    $info = New-SmokeStartInfo $Executable $Arguments $Repo $Environment
    # npm.cmd needs cmd.exe; no user strings are interpolated in its command.
    if ($Executable -eq 'npm.cmd') {
        if (($Arguments -join ' ') -notin @('--prefix app/ui ci', '--prefix app/ui run build')) { throw 'Unexpected npm command' }
        $info.FileName = $env:ComSpec
        $info.Arguments = '/d /s /c "npm.cmd ' + ($Arguments -join ' ') + '"'
    }
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $info
    try {
        if (-not $process.Start()) { throw "Could not start $Executable" }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $out = $stdout.GetAwaiter().GetResult()
        $err = $stderr.GetAwaiter().GetResult()
        if ($err) { Write-Host $err }
        if ($process.ExitCode -ne 0) { throw "$Executable exited $($process.ExitCode): $out" }
        return $out
    } finally { $process.Dispose() }
}

function Build-SmokeApp([string]$Repo, [string]$Profile) {
    # Absolute per-run directories are compiled in; ordinary Tauri Known Folder
    # resolution ignores APPDATA/LOCALAPPDATA environment overrides on Windows.
    $config = @{ app = @{ appDirectoriesOverride = @{
        config = (Join-Path $Profile 'app'); data = (Join-Path $Profile 'app')
        localData = (Join-Path $Profile 'webview'); cache = (Join-Path $Profile 'cache'); log = (Join-Path $Profile 'logs')
    } } } | ConvertTo-Json -Depth 5 -Compress
    $buildEnv = @{ TAURI_CONFIG = $config; CARGO_PROFILE_DEV_DEBUG_ASSERTIONS = 'true' }
    # An inherited compiler override could disable the debug-only safety gate.
    foreach ($flags in @($env:RUSTFLAGS, $env:CARGO_ENCODED_RUSTFLAGS, $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS)) {
        if ($flags -match 'debug.assertions') { throw 'Remove the inherited debug-assertions compiler override before GUI smoke' }
    }
    [void](Invoke-SmokeCommand 'npm.cmd' @('--prefix', 'app/ui', 'ci') $Repo)
    [void](Invoke-SmokeCommand 'npm.cmd' @('--prefix', 'app/ui', 'run', 'build') $Repo)
    $metadata = Invoke-SmokeCommand 'cargo' @('metadata', '--locked', '--no-deps', '--format-version', '1') $Repo $buildEnv | ConvertFrom-Json
    if (-not [IO.Path]::IsPathRooted($metadata.target_directory)) { throw 'Cargo metadata returned an invalid target_directory' }
    [void](Invoke-SmokeCommand 'cargo' @('build', '--locked', '-p', 'maxed-app', '--bin', 'maxed', '--features', 'custom-protocol', '--target', 'x86_64-pc-windows-msvc') $Repo $buildEnv)
    $exe = Join-Path $metadata.target_directory 'x86_64-pc-windows-msvc/debug/maxed.exe'
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Built executable missing: $exe" }
    return $exe
}

function Invoke-SmokeCdp($Job, $App, [string]$Repo, [int]$Port, [string]$Output, [int]$TimeoutSeconds) {
    $node = Resolve-SmokeExecutable 'node'
    $info = New-SmokeStartInfo $node @((Join-Path $PSScriptRoot 'cdp-smoke.mjs'), '--port', "$Port", '--output-dir', $Output, '--timeout-ms', "$($TimeoutSeconds * 1000)") $Repo
    $process = $Job.Start($info)
    try {
        $clock = [Diagnostics.Stopwatch]::StartNew()
        while (-not $process.WaitForExit(100)) {
            if ($App.HasExited) { throw 'Maxed exited before the CDP smoke completed' }
            if ($clock.Elapsed.TotalSeconds -gt ($TimeoutSeconds + 10)) { throw 'CDP script exceeded its launcher deadline' }
        }
        if ($App.HasExited) { throw 'Maxed exited before the CDP smoke completed' }
        return $process.ExitCode
    } finally { $process.Dispose() }
}

function Invoke-GuiSmoke([string]$Repo, [int]$Port, [string]$OutputDir, [int]$TimeoutSeconds) {
    $profile = $null; $job = $null; $app = $null; $exitCode = 1
    $report = [ordered]@{ ok = $false; exit_code = 1; reason = $null; cleanup_errors = @(); profile_lockers = @() }
    $output = [IO.Path]::GetFullPath($OutputDir)
    [void][IO.Directory]::CreateDirectory($output)
    foreach ($name in @('report.json', 'screenshot.png', 'launcher.json', 'owner.json')) {
        if (Test-Path -LiteralPath (Join-Path $output $name)) { throw 'Use a fresh output directory; previous smoke artifacts exist' }
    }
    try {
        $owner = [Diagnostics.Process]::GetCurrentProcess()
        try { Write-SmokeOwner $output $owner } finally { $owner.Dispose() }
        Assert-SmokeWindows
        Assert-SmokePortFree $Port
        $profile = New-SmokeProfile
        $exe = Build-SmokeApp $Repo $profile
        Assert-SmokePortFree $Port
        $browserArgs = Get-SmokeBrowserArguments $Port (Get-Content -LiteralPath (Join-Path $Repo 'app/src-tauri/src/window_policy.rs') -Raw)
        $job = New-SmokeJob
        $info = New-SmokeStartInfo $exe @() $Repo @{
            MAXED_GUI_SMOKE = '1'
            WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $browserArgs
            WEBVIEW2_USER_DATA_FOLDER = (Join-Path $profile 'webview')
            WEBVIEW2_BROWSER_EXECUTABLE_FOLDER = $null
            MAXED_MEASURE_DEMO = $null
            MAXED_MEASURE_DEMO_LOOPS = $null
        }
        $app = $job.Start($info)
        $exitCode = Invoke-SmokeCdp $job $app $Repo $Port $output $TimeoutSeconds
    } catch {
        $report.reason = $_.Exception.Message
        Write-Host "GUI smoke failed: $($report.reason)"
    } finally {
        # A job owns the suspended process before its first instruction executes.
        # Keep its handle for live locker membership checks after termination.
        try {
            if ($null -ne $job) {
                try { Stop-SmokeJob $job $app } catch { Add-SmokeCleanupError $report $_.Exception.Message }
            }
            if ($null -ne $app) {
                try { $app.Dispose() } catch { Add-SmokeCleanupError $report $_.Exception.Message }
            }
            if ($null -ne $profile) {
                try { Remove-SmokeProfile $profile $job $report } catch { Add-SmokeCleanupError $report $_.Exception.Message }
            }
        } finally {
            if ($null -ne $job) {
                try { $job.Dispose() } catch { Add-SmokeCleanupError $report $_.Exception.Message }
            }
        }
        if ($report.cleanup_errors.Count -gt 0) { $exitCode = 1 }
        $report.exit_code = $exitCode
        $report.ok = $exitCode -eq 0
        [IO.File]::WriteAllText((Join-Path $output 'launcher.json'), ($report | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    }
    return $exitCode
}

if (-not $FunctionsOnly) {
    $repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
    try {
        if ($CleanupOnly) { Stop-SmokeOwner $OutputDir; exit 0 }
        exit (Invoke-GuiSmoke $repo $Port $OutputDir $TimeoutSeconds)
    }
    catch { Write-Error $_; exit 1 }
}
