# Test-only accessibility workload; does not change settings or require CDP.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][int]$RootProcessId,
    [Parameter(Mandatory=$true)][datetime]$RootStartUtc,
    [Parameter(Mandatory=$true)][long]$WindowHandle,
    [ValidateRange(10,7200)][int]$Seconds = 1800,
    [Parameter(Mandatory=$true)][string]$OutputFile
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices;
public static class MemoryRouteWindow {
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    public static bool Owns(IntPtr window, int pid) {
        uint owner; return GetWindowThreadProcessId(window, out owner) != 0 && owner == (uint)pid;
    }
}
'@
$process = Get-Process -Id $RootProcessId
$born = $process.StartTime.ToUniversalTime()
$watch = [Diagnostics.Stopwatch]::StartNew()
$visits = [System.Collections.Generic.List[object]]::new()
$routes = @('Home','Runs','Settings','Runs')
$cycle = 0
$admitted = $false
try {
    if ($process.ProcessName -ne 'maxed' -or $born -ne $RootStartUtc.ToUniversalTime() -or [IO.File]::Exists($OutputFile)) { throw 'Select owned Maxed identity and a fresh output file' }
    $admitted = $true
    while ($watch.Elapsed.TotalSeconds -lt $Seconds) {
        $process.Refresh()
        if ($process.HasExited -or $process.StartTime.ToUniversalTime() -ne $born) { throw 'Workload process identity changed' }
        # Process.MainWindowHandle may point at WebView2's untitled helper HWND.
        # The launcher supplies the original titled, shown window (like C4).
        $window = [IntPtr]$WindowHandle
        if (-not [MemoryRouteWindow]::Owns($window,$RootProcessId) -or -not [MemoryRouteWindow]::IsWindowVisible($window)) { throw 'Owned visible Maxed window unavailable' }
        $root = [System.Windows.Automation.AutomationElement]::FromHandle($window)
        $route = $routes[$cycle % 4]
        $condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty, $route)
        $matches = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)
        $invoked = $false
        foreach ($element in $matches) {
            $pattern = $null
            if ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)) {
                $pattern.Invoke(); $invoked=$true; break
            }
        }
        $visits.Add([pscustomobject]@{elapsed_seconds=$watch.Elapsed.TotalSeconds;route=$route;invoked=$invoked})
        Write-Output ("route={0} invoked={1} elapsed={2:F1}" -f $route,$invoked,$watch.Elapsed.TotalSeconds)
        Start-Sleep -Seconds 2
        if ($cycle % 4 -eq 3) {
            # Select a row in the actual Runs list, independent of dungeon text.
            # Navigation links with the same name cannot satisfy this condition.
            $list = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
                [System.Windows.Automation.AndCondition]::new(
                    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Runs'),
                    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::List)))
            $links = @()
            if ($null -ne $list) {
                $links = $list.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Hyperlink))
            }
            $story = $false
            foreach ($element in $links) {
                $pattern = $null
                if ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)) {
                    $pattern.Invoke(); $story=$true; break
                }
            }
            $visits.Add([pscustomobject]@{elapsed_seconds=$watch.Elapsed.TotalSeconds;route='Run Story';invoked=$story})
            Write-Output ("route=Run Story invoked={0} elapsed={1:F1}" -f $story,$watch.Elapsed.TotalSeconds)
        }
        Start-Sleep -Seconds 8; $cycle++
    }
} finally {
    $process.Dispose()
    if ($admitted) { [pscustomobject]@{ root_pid=$RootProcessId;root_start_utc=$born.ToString('o');requested_seconds=$Seconds;measured_seconds=$watch.Elapsed.TotalSeconds
        visits=$visits.ToArray();successful_visits=@($visits | Where-Object invoked).Count
        limitation='Accessibility Invoke success is a route-action control, not rendered content validation. Only synthetic isolated profiles are admitted by the orchestration.' } |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $OutputFile -Encoding utf8 }
}
