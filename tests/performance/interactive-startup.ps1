[CmdletBinding(DefaultParameterSetName='Measure')]
param(
    [Parameter(Mandatory=$true, ParameterSetName='SelfTest')][switch]$SelfTest,
    [Parameter(Mandatory=$true, ParameterSetName='Measure')][string]$Exe,
    [Parameter(Mandatory=$true, ParameterSetName='Measure')][string]$OutputDir
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$script:LaunchedProcesses = [System.Collections.Generic.List[object]]::new()
$script:DiagnosticDirectoryReady = $false

# Raw child output is never saved. Both streams have bounded background readers,
# independent of PowerShell's pipeline and associated with the retained process.
Add-Type -TypeDefinition @"
using System;
using System.IO;
using System.Diagnostics;
using System.Threading;
public sealed class MaxedBrokenTestReader : TextReader { public override int Read() { throw new IOException("Synthetic pipe failure"); } }
public sealed class MaxedReceiptState {
    public int Count;
    public double? ReceiptMs;
    public bool Late, PipeFailure, Overflow, Eof;
}
public sealed class MaxedReceiptLedger {
    private readonly object sync = new object();
    private readonly MaxedReceiptState state = new MaxedReceiptState();
    public bool Line(string line, double elapsedMs) {
        lock (sync) {
            if (line != "MAXED_INTERACTIVE_RAIL_V1") return false;
            state.Count++;
            if (elapsedMs >= 30000) state.Late = true;
            if (!state.ReceiptMs.HasValue) state.ReceiptMs = elapsedMs;
            return state.Count == 1;
        }
    }
    public void Failed() { lock (sync) { state.PipeFailure = true; } }
    public void Exceeded() { lock (sync) { state.Overflow = true; } }
    public void End() { lock (sync) { state.Eof = true; } }
    public MaxedReceiptState Snapshot() {
        lock (sync) { return new MaxedReceiptState { Count=state.Count, ReceiptMs=state.ReceiptMs,
            Late=state.Late, PipeFailure=state.PipeFailure, Overflow=state.Overflow, Eof=state.Eof }; }
    }
}
public sealed class MaxedBoundedReceiptReader {
    public readonly MaxedReceiptLedger Ledger = new MaxedReceiptLedger();
    private readonly Thread output, error;
    public MaxedBoundedReceiptReader(Process process, Stopwatch receipt, Stopwatch deadline) : this(process.StandardOutput, process.StandardError, receipt, deadline) {}
    public MaxedBoundedReceiptReader(TextReader stdout, TextReader stderr, Stopwatch receipt, Stopwatch deadline) {
        output = new Thread(() => Read(stdout, true, receipt, deadline));
        error = new Thread(() => Read(stderr, false, receipt, deadline));
        output.IsBackground = error.IsBackground = true;
        output.Start(); error.Start();
    }
    private void Read(TextReader stream, bool stdout, Stopwatch receipt, Stopwatch deadline) {
        try {
            var line = new System.Text.StringBuilder();
            int total = 0, value;
            while ((value = stream.Read()) != -1) {
                if (++total > 65536) { Ledger.Exceeded(); return; }
                if (!stdout) continue;
                if (value == '\n') {
                    string text = line.ToString();
                    if (text.EndsWith("\r")) text = text.Substring(0, text.Length-1);
                    // Read completion is the conservative external receipt boundary.
                    if (Ledger.Line(text, receipt.IsRunning ? receipt.Elapsed.TotalMilliseconds : deadline.Elapsed.TotalMilliseconds)) receipt.Stop();
                    line.Clear();
                } else {
                    if (line.Length >= 128) { Ledger.Exceeded(); return; }
                    line.Append((char)value);
                }
            }
            // An unterminated marker is not C0's fixed newline-terminated line.
            if (stdout) Ledger.End();
        } catch { Ledger.Failed(); }
    }
    public bool Join() { return output.Join(1000) & error.Join(1000); }
}
"@
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
    if (-not $script:DiagnosticDirectoryReady) { return }
    if ([string]::IsNullOrWhiteSpace($OutputDir) -or -not [IO.Directory]::Exists($OutputDir)) { return }
    $message = Protect-DiagnosticMessage $Failure.Exception.Message
    $line = $Failure.InvocationInfo.ScriptLineNumber
    # Logging is best-effort; an unwritable directory must not prevent cleanup.
    $entry = "line ${line}: $message"
    Add-Content -LiteralPath (Join-Path $OutputDir 'diagnostics.txt') -Value $entry -Encoding utf8 -ErrorAction SilentlyContinue
}


function New-ScenarioLaunch($Process) {
    [pscustomobject]@{ Process=$Process; Window=[IntPtr]::Zero; Minimized=$false; CloseRequested=$false; Closed=$false; Started=$false; Reader=$null; Receipt=$null; Deadline=$null; OverheadMs=$null; NativeStart=$null; LaunchUtc=$null }
}


function Close-Scenario($Launch,
    [scriptblock]$Restore = { param($Window) [void][MaxedStartupWindow]::ShowWindow($Window, 9) },
    [scriptblock]$PostClose = { param($Window) [MaxedStartupWindow]::PostMessage($Window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) },
    [scriptblock]$OwnsWindow = { param($Window, $ProcessId) [MaxedStartupWindow]::OwnsWindow($Window, $ProcessId) }) {
    if ($Launch.Closed) { return }
    if (-not $Launch.Started) { $Launch.Process.Dispose(); $Launch.Closed = $true; return }
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
            if ($null -ne $Launch.Reader -and -not $Launch.Reader.Join()) { throw 'Owned pipe reader did not finish' }
            $process.Dispose()
            $Launch.Closed = $true
        } catch { Write-Diagnostic $_; $failed = $true }
    }
    if ($failed) { throw 'Scenario shutdown failed; owned-process cleanup was required' }
}


    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class MaxedStartupWindow {
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
function Assert-NoMaxed {
    if (@(Get-Process -Name maxed -ErrorAction SilentlyContinue).Count -gt 0) { throw 'Close existing Maxed before measuring' }
}
function Assert-OutputDirectory([string]$Directory) {
    if (-not (Test-Path -LiteralPath $Directory -PathType Container)) { throw 'Output directory must already exist' }
    if (@(Get-ChildItem -LiteralPath $Directory -Force).Count -ne 0) { throw 'Output directory must be fresh and empty' }
}
function New-LaunchProcess([string]$Executable) {
    $info = New-Object Diagnostics.ProcessStartInfo
    $info.FileName = $Executable
    $info.WorkingDirectory = [IO.Path]::GetDirectoryName($Executable)
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    # EnvironmentVariables is a child-specific copy. The parent environment is
    # never mutated, including on creation failure; there is nothing to leak.
    $info.EnvironmentVariables['MAXED_MEASURE_STARTUP'] = '1'
    $info.EnvironmentVariables.Remove('MAXED_MEASURE_DEMO')
    $info.EnvironmentVariables.Remove('MAXED_MEASURE_DEMO_LOOPS')
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $info
    return $process
}
function New-ReceiptReader($Launch) {
    return [MaxedBoundedReceiptReader]::new($Launch.Process, $Launch.Receipt, $Launch.Deadline)
}
function Start-Launch([string]$Executable) {
    Assert-NoMaxed
    $process = New-LaunchProcess $Executable
    $launch = New-ScenarioLaunch $process
    $script:LaunchedProcesses.Add($launch)
    $launch.LaunchUtc = [datetime]::UtcNow
    $launch.Deadline = [Diagnostics.Stopwatch]::StartNew()
    $launch.Receipt = [Diagnostics.Stopwatch]::StartNew()
    # No wall-clock subtraction determines the gate. Stopwatch includes process
    # creation, handle retention, pipe setup, scheduling and delivery overhead.
    $launch.Started = $process.Start()
    $launch.OverheadMs = $launch.Receipt.Elapsed.TotalMilliseconds
    if (-not $launch.Started) { throw 'Process creation failed' }
    [void]$process.Handle
    $launch.NativeStart = $process.StartTime.ToUniversalTime()
    $launch.Reader = New-ReceiptReader $launch
    return $launch
}
function Get-ReadinessResult($State, [bool]$TimedOut = $false) {
    if ($State.PipeFailure) { return 'pipe_failure' }
    if ($State.Overflow) { return 'output_bound_exceeded' }
    if ($State.Count -gt 1) { return 'duplicate_marker' }
    if ($State.Late) { return 'late_marker' }
    if ($State.Count -eq 0 -or $null -eq $State.ReceiptMs) {
        if ($TimedOut) { return 'readiness_timeout' }
        return 'missing_marker'
    }
    return 'ready'
}
function Test-StartupPass($Primer, $Samples) {
    if ($Primer.readiness -ne 'ready' -or -not $Primer.closed_cleanly -or @($Samples).Count -ne 5) { return $false }
    foreach ($sample in $Samples) {
        if ($sample.readiness -ne 'ready' -or -not $sample.closed_cleanly -or $null -eq $sample.elapsed_ms -or
            [double]::IsNaN([double]$sample.elapsed_ms) -or $sample.elapsed_ms -lt 0 -or $sample.elapsed_ms -ge 1500) { return $false }
    }
    return $true
}
function Measure-Launch([string]$Executable, [string]$Kind, [int]$Trial) {
    $launch = $null; $timedOut = $false
    $sample = [pscustomobject]@{ kind=$Kind; trial=$Trial; readiness='missing_marker'; elapsed_ms=$null;
        native_process_start_utc=$null; launch_requested_utc=$null; launch_overhead_ms=$null;
        closed_cleanly=$false; marker_count=0 }
    try {
        $launch = Start-Launch $Executable
        $sample.native_process_start_utc = $launch.NativeStart.ToString('o')
        $sample.launch_requested_utc = $launch.LaunchUtc.ToString('o')
        $sample.launch_overhead_ms = $launch.OverheadMs
        while ($launch.Deadline.Elapsed.TotalMilliseconds -lt 30000) {
            $launch.Process.Refresh()
            if ($launch.Window -eq [IntPtr]::Zero) {
                $launch.Window = [MaxedStartupWindow]::FindMainWindow($launch.Process.Id)
                if ($launch.Window -ne [IntPtr]::Zero) { [void][MaxedStartupWindow]::ShowWindow($launch.Window, 5) }
            }
            $state = $launch.Reader.Ledger.Snapshot()
            if ($state.Count -gt 0 -or $state.PipeFailure -or $state.Overflow -or $state.Eof -or $launch.Process.HasExited) { break }
            Start-Sleep -Milliseconds 5
        }
        $state = $launch.Reader.Ledger.Snapshot()
        $timedOut = $state.Count -eq 0 -and $launch.Deadline.Elapsed.TotalMilliseconds -ge 30000
        if ($state.Count -gt 0) {
            # Give the primer a short cache/observation dwell. Continue reading
            # through graceful shutdown/EOF so duplicates are not silently lost.
            Start-Sleep -Milliseconds $(if ($Kind -eq 'primer') { 2000 } else { 200 })
        }
    } catch { Write-Diagnostic $_ }
    finally {
        if ($null -ne $launch) {
            try { Close-Scenario $launch; $sample.closed_cleanly = $true } catch { Write-Diagnostic $_ }
            if ($null -ne $launch.Reader) {
                $state = $launch.Reader.Ledger.Snapshot()
                $sample.marker_count = $state.Count
                $sample.elapsed_ms = $state.ReceiptMs
                $sample.readiness = Get-ReadinessResult $state $timedOut
            }
            $launch.Receipt.Stop(); $launch.Deadline.Stop()
        }
    }
    return $sample
}
function Invoke-Measurement {
    Assert-NoMaxed
    Assert-OutputDirectory $OutputDir
    $script:DiagnosticDirectoryReady = $true
    $executable = (Resolve-Path -LiteralPath $Exe).ProviderPath
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf) -or [IO.Path]::GetFileName($executable) -ine 'maxed.exe') { throw 'Select the release Maxed executable' }
    $hash = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
    $primer = Measure-Launch $executable 'primer' 0
    $samples = @(1..5 | ForEach-Object { Measure-Launch $executable 'warm' $_ })
    $passed = Test-StartupPass $primer $samples
    $report = [pscustomobject]@{ schema_version=1; signal='MAXED_INTERACTIVE_RAIL_V1'; result=$(if ($passed) {'pass'} else {'fail'});
        budget_ms=1500; readiness_deadline_ms=30000; exe_sha256=$hash; measured_at_utc=[datetime]::UtcNow.ToString('o');
        timing='Monotonic process-creation-to-stdout-receipt conservative upper bound';
        clock_correlation='No exact wall/monotonic correlation claimed; native StartTime and creation overhead are diagnostics only';
        primer=$primer; samples=$samples }
    $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $OutputDir 'interactive-startup.json') -Encoding utf8
    # Only fixed statuses, never child output, exception source lines or paths.
    foreach ($sample in @($primer) + $samples) {
        Add-Content -LiteralPath (Join-Path $OutputDir 'diagnostics.txt') -Value ("{0} {1}: {2}; markers={3}; closed_cleanly={4}" -f $sample.kind,$sample.trial,$sample.readiness,$sample.marker_count,$sample.closed_cleanly) -Encoding utf8
    }
    Write-Host ("Interactive startup: {0}; five warm samples (ms): {1}" -f $report.result, (($samples | ForEach-Object { $_.elapsed_ms }) -join ', '))
    if ($passed) { return 0 }; return 1
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


function Assert([bool]$Condition, [string]$Label) {
    $script:Assertions++
    if (-not $Condition) { throw "Self-test failed: $Label" }
}
function Assert-Rejected([scriptblock]$Action, [string]$Label) {
    $rejected = $false
    try { & $Action | Out-Null } catch { $rejected = $true }
    Assert $rejected $Label
}
function Test-ExistingProcessRefusal {
    function Get-Process { return [pscustomobject]@{ Id=987 } }
    Assert-Rejected { Assert-NoMaxed } 'existing process refuses launch'
}
function Test-MockLaunch {
    function Assert-NoMaxed { }
    function New-LaunchProcess { return $fake }
    function New-ReceiptReader { return $null }
    $fake = New-FakeProcess
    $fake | Add-Member NoteProperty Handle ([IntPtr]123)
    $fake | Add-Member NoteProperty StartTime ([datetime]::UtcNow)
    $fake | Add-Member ScriptMethod Start { return $true }
    $owned = Start-Launch 'C:\synthetic\maxed.exe'
    Assert ($owned.Started -and $owned.Process -eq $fake) 'retained launch ownership'
    Assert ($owned.OverheadMs -ge 0 -and $owned.Receipt.IsRunning) 'clock starts before process creation'
    $owned.Window = [IntPtr]42
    $script:CloseCalls = 0
    Close-Scenario $owned {} { $script:CloseCalls++; return $true } { return $true }
    Close-Scenario $owned {} { $script:CloseCalls++; return $true } { return $true }
    Assert ($script:CloseCalls -eq 1 -and $fake.Kills -eq 0 -and $fake.Disposed) 'one graceful close, no second confirmation'
    $fake = New-FakeProcess $false
    $owned = New-ScenarioLaunch $fake; $owned.Started = $true; $owned.Window = [IntPtr]42
    Assert-Rejected { Close-Scenario $owned {} { return $true } { return $true } } 'shutdown failure rejects sample'
    Assert ($fake.Kills -eq 1 -and $fake.Disposed) 'only retained owned process killed'
    $fake = New-FakeProcess
    $fake | Add-Member ScriptMethod Start { throw 'Synthetic creation failure' }
    Assert-Rejected { Start-Launch 'C:\synthetic\maxed.exe' } 'creation failure rejects sample'
    $last = $script:LaunchedProcesses[$script:LaunchedProcesses.Count-1]
    Close-Scenario $last
    Assert ($fake.Disposed -and $fake.Kills -eq 0) 'unstarted process disposed without termination'
}
function Set-OptionalProcessEnvironment([string]$Name, $Value) {
    # .NET 9 distinguishes an absent variable from an empty value. Preserve
    # absence explicitly in both PowerShell editions.
    if ($null -eq $Value) { [Environment]::SetEnvironmentVariable($Name, [NullString]::Value, 'Process') }
    else { [Environment]::SetEnvironmentVariable($Name, $Value, 'Process') }
}
function Invoke-SelfTest {
    $script:Assertions = 0
    foreach ($text in @('MAXED_INTERACTIVE_RAIL_V1', 'MAXED_INTERACTIVE_RAIL_V1 ', 'prefix MAXED_INTERACTIVE_RAIL_V1', '', 'Connection Error')) {
        $ledger = [MaxedReceiptLedger]::new()
        [void]$ledger.Line($text, 123)
        Assert (($ledger.Snapshot().Count -eq 1) -eq ($text -ceq 'MAXED_INTERACTIVE_RAIL_V1')) 'exact fixed marker only'
    }
    $ledger = [MaxedReceiptLedger]::new(); [void]$ledger.Line('MAXED_INTERACTIVE_RAIL_V1', 30000)
    Assert ((Get-ReadinessResult $ledger.Snapshot()) -eq 'late_marker') '30 s boundary is late'
    $ledger = [MaxedReceiptLedger]::new(); [void]$ledger.Line('MAXED_INTERACTIVE_RAIL_V1', 10); [void]$ledger.Line('MAXED_INTERACTIVE_RAIL_V1', 20)
    Assert ((Get-ReadinessResult $ledger.Snapshot()) -eq 'duplicate_marker') 'duplicate receipt fails'
    $ledger = [MaxedReceiptLedger]::new()
    Assert ((Get-ReadinessResult $ledger.Snapshot()) -eq 'missing_marker') 'missing readiness includes connection-error page'
    Assert ((Get-ReadinessResult $ledger.Snapshot() $true) -eq 'readiness_timeout') 'stalled WebView timeout fails'
    $ledger.Failed()
    Assert ((Get-ReadinessResult $ledger.Snapshot()) -eq 'pipe_failure') 'broken pipe is missing readiness'
    foreach ($case in @(
        @{ text="MAXED_INTERACTIVE_RAIL_V1`n"; result='ready' },
        @{ text="MAXED_INTERACTIVE_RAIL_V1`r`n"; result='ready' },
        @{ text="MAXED_INTERACTIVE_RAIL_V1`nMAXED_INTERACTIVE_RAIL_V1`n"; result='duplicate_marker' },
        @{ text='MAXED_INTERACTIVE_RAIL_V1'; result='missing_marker' },
        @{ text="Connection Error`n"; result='missing_marker' },
        @{ text=('x' * 129); result='output_bound_exceeded' }
    )) {
        $receipt = [Diagnostics.Stopwatch]::StartNew(); $deadline = [Diagnostics.Stopwatch]::StartNew()
        $reader = [MaxedBoundedReceiptReader]::new([IO.StringReader]::new($case.text), [IO.StringReader]::new(''), $receipt, $deadline)
        Assert $reader.Join() 'bounded mocked reader finishes'
        Assert ((Get-ReadinessResult $reader.Ledger.Snapshot()) -eq $case.result) 'stream parser verdict'
        if ($case.result -eq 'ready') { Assert (-not $receipt.IsRunning) 'receipt stops monotonic timer' }
        $receipt.Stop(); $deadline.Stop()
    }
    $receipt = [Diagnostics.Stopwatch]::StartNew(); $deadline = [Diagnostics.Stopwatch]::StartNew()
    $reader = [MaxedBoundedReceiptReader]::new([MaxedBrokenTestReader]::new(), [IO.StringReader]::new(''), $receipt, $deadline)
    Assert $reader.Join() 'broken mocked stream completes'
    Assert ((Get-ReadinessResult $reader.Ledger.Snapshot()) -eq 'pipe_failure') 'real reader catches pipe exception'
    $receipt.Stop(); $deadline.Stop()
    $reader = [MaxedBoundedReceiptReader]::new([IO.StringReader]::new(''), [IO.StringReader]::new(('x' * 65537)), $receipt, $deadline)
    Assert $reader.Join() 'bounded stderr reader completes'
    Assert ((Get-ReadinessResult $reader.Ledger.Snapshot()) -eq 'output_bound_exceeded') 'stderr total bound'
    $primer = [pscustomobject]@{ readiness='ready'; elapsed_ms=4000; closed_cleanly=$true }
    $sample = [pscustomobject]@{ readiness='ready'; elapsed_ms=1499.99; closed_cleanly=$true }
    Assert (Test-StartupPass $primer (@($sample)*5)) 'all five below threshold pass; primer is unbudgeted'
    $sample.elapsed_ms = 1500
    Assert (-not (Test-StartupPass $primer (@($sample)*5))) '1500 exactly fails'
    $sample.elapsed_ms = $null
    Assert (-not (Test-StartupPass $primer (@($sample)*5))) 'null sample fails'
    $sample.elapsed_ms = 100; $sample.readiness = 'missing_marker'
    Assert (-not (Test-StartupPass $primer (@($sample)*5))) 'missing readiness never passes'
    $sample.readiness = 'ready'; $sample.closed_cleanly = $false
    Assert (-not (Test-StartupPass $primer (@($sample)*5))) 'cleanup failure blocks pass'
    Assert (-not (Test-StartupPass $primer (@($sample)*4))) 'five samples required'
    Test-ExistingProcessRefusal
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ('maxed-startup-selftest-' + [guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($temporary)
    try {
        Assert-OutputDirectory $temporary
        Set-Content -LiteralPath (Join-Path $temporary '.hidden') -Value 'synthetic'
        Assert-Rejected { Assert-OutputDirectory $temporary } 'non-empty directory refuses'
    } finally {
        Remove-Item -LiteralPath (Join-Path $temporary '.hidden')
        Remove-Item -LiteralPath $temporary
    }
    $saved = [Environment]::GetEnvironmentVariable('MAXED_MEASURE_STARTUP','Process')
    try {
        foreach ($value in @($null,'0','synthetic','1')) {
            Set-OptionalProcessEnvironment 'MAXED_MEASURE_STARTUP' $value
            $process = New-LaunchProcess 'C:\synthetic\maxed.exe'
            Assert ($process.StartInfo.EnvironmentVariables['MAXED_MEASURE_STARTUP'] -ceq '1') 'exact flag belongs only to child'
            Assert ($process.StartInfo.RedirectStandardOutput -and $process.StartInfo.RedirectStandardError -and -not $process.StartInfo.UseShellExecute) 'pipes bound to retained process'
            Assert ($process.StartInfo.WorkingDirectory -eq 'C:\synthetic' -and $process.StartInfo.WindowStyle -eq 'Hidden') 'driver directory and hidden launch convention'
            $process.Dispose()
            Test-MockLaunch
            Assert ([Environment]::GetEnvironmentVariable('MAXED_MEASURE_STARTUP','Process') -eq $value) 'environment unchanged after mock success and failure'
        }
    } finally { Set-OptionalProcessEnvironment 'MAXED_MEASURE_STARTUP' $saved }
    foreach ($text in @('Cannot open C:\private\secret.txt', 'Failed command: maxed.exe --secret hidden', 'Failed https://private.example/secret')) {
        Assert ((Protect-DiagnosticMessage $text) -notmatch 'private|secret|hidden|https:|maxed.exe') 'diagnostics redact paths and commands'
    }
    Write-Output "Interactive startup self-test passed ($script:Assertions assertions; no real launch)"
}

$exitCode = 2
try {
    if ($SelfTest) { Invoke-SelfTest; $exitCode = 0 }
    else { $exitCode = Invoke-Measurement }
} catch {
    Write-Diagnostic $_
    # Console also omits raw paths, commands and source lines.
    Write-Output ('Interactive startup failed: ' + (Protect-DiagnosticMessage $_.Exception.Message))
} finally {
    foreach ($launch in $script:LaunchedProcesses) {
        if (-not $launch.Closed) {
            try { Close-Scenario $launch } catch { Write-Diagnostic $_; $exitCode = 2 }
        }
    }
}
exit $exitCode
