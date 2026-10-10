$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Uninstrumented sizing experiments must not admit arbitrary browser overrides.
$priorBrowserArguments = [Environment]::GetEnvironmentVariable('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS')
try {
    foreach ($case in @(
        @{ argument='--js-flags=--max-old-space-size=64'; diagnostic=$false; mode='after' },
        @{ argument='--js-flags=--optimize-for-size --enable-features=Other'; diagnostic=$false; mode='after' },
        @{ argument='--js-flags=--optimize-for-size'; diagnostic=$true; mode='after' },
        @{ argument='--js-flags=--max-semi-space-size=1'; diagnostic=$false; mode='before' }
    )) {
        $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $case.argument
        $rejected = $false
        try {
            & (Join-Path $PSScriptRoot 'live-realm-memory.ps1') -Exe unused -Profile unused -OutputDir unused `
                -Mode $case.mode -Diagnostic:$case.diagnostic -AllowV8SizingOverride
        } catch {
            if ($_.Exception.Message -notlike 'Sizing override requires*') { throw }
            $rejected = $true
        }
        if (-not $rejected) { throw 'Unsafe or mismatched sizing override admitted' }
    }
    Write-Output 'PASS: uninstrumented_sizing_override_rejects_heap_limits_extra_switches_and_wrong_modes'
} finally {
    [Environment]::SetEnvironmentVariable('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', $priorBrowserArguments)
}
# A deliberately failed self-test must retain its assertion diagnostic. Run in
# the invoking edition so CI and local checks can repeat this in Windows PS and pwsh.
$source = [IO.File]::ReadAllText((Join-Path $PSScriptRoot '../../scripts/Measure-MaxedMemory.ps1'))
$ast = [System.Management.Automation.Language.Parser]::ParseInput($source, [ref]$null, [ref]$null)
$classification = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Get-ProcessClassification' }, $false)
Invoke-Expression $classification.Extent.Text
foreach ($case in @(
    @('--type=utility --utility-sub-type=network.mojom.NetworkService', 'utility', 'network.mojom.NetworkService'),
    @('--type="utility" --utility-sub-type="audio.mojom.AudioService"', 'utility', 'audio.mojom.AudioService'),
    @('--type=crashpad-handler --user-data-dir=C:\private', 'crashpad-handler', ''),
    @('--type=renderer --private-label=secret', 'renderer', ''),
    @('--user-data-dir=C:\private', 'browser', ''),
    @('--type=../../private --utility-sub-type=C:\private', 'unknown', '')
)) {
    $actual = Get-ProcessClassification 'msedgewebview2.exe' $case[0]
    if ($actual.process_type -cne $case[1] -or $actual.utility_sub_type -cne $case[2] -or
        ($actual | ConvertTo-Json) -match 'private|secret') { throw 'Bounded Chromium classification failed' }
}
Write-Output 'PASS: bounded_chromium_process_types_and_subtypes'
$marker = 'function Invoke-SelfTest {'
if ($source.Split(@($marker), [StringSplitOptions]::None).Count -ne 2) { throw 'Self-test injection point changed' }
$injected = $source.Replace($marker, ($marker + "`n    throw 'Self-test failed: injected assertion'"))
# The copied collector still loads the repository's shared policy, not a temp sibling.
$policy = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../scripts/memory-policy.ps1')).Replace("'", "''")
$injected = $injected.Replace(". (Join-Path `$PSScriptRoot 'memory-policy.ps1')", ". '$policy'")
$path = Join-Path ([IO.Path]::GetTempPath()) ('maxed-collector-failure-' + [Guid]::NewGuid().ToString('N') + '.ps1')
try {
    [IO.File]::WriteAllText($path, $injected)
    $shell = (Get-Process -Id $PID).Path
    $start = New-Object Diagnostics.ProcessStartInfo
    $start.FileName = $shell
    $start.Arguments = '-NoProfile -ExecutionPolicy Bypass -File "' + $path + '" -SelfTest'
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($start)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(30000)) { $process.Kill(); throw 'Injected self-test timed out' }
    $output = $stdout.Result + $stderr.Result
    if ($process.ExitCode -eq 0 -or $output -notmatch 'Self-test failed: injected assertion' -or
        $output -match 'Measurement failed before report completion') { throw 'Failed self-test hid its assertion' }
    Write-Output 'PASS: collector_self_test_preserves_assertion_diagnostic'
} finally {
    if ($null -ne (Get-Variable process -ErrorAction SilentlyContinue) -and $null -ne $process) { $process.Dispose() }
    Remove-Item -LiteralPath $path -Force
}

# Report decomposition must use the combined peak's timestamp, not each
# process's independent peak, and load-confounded evidence must not pass.
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('maxed-attribution-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
try {
    $origin = [datetimeoffset]'2026-01-01T00:00:00Z'
    @{mode='after';diagnostic=$false;machine_quiet=$true;executable_sha256='synthetic';start_utc=$origin.ToString('o')} | ConvertTo-Json | Set-Content (Join-Path $fixture 'launch.json')
    @{busy=$origin.ToString('o');idle=$origin.AddSeconds(610).ToString('o')} | ConvertTo-Json | Set-Content (Join-Path $fixture 'phases.json')
    @{busy=$true;idle=$true;subscription_failed=$false} | ConvertTo-Json | Set-Content (Join-Path $fixture 'rendered.json')
    @{memory_result='measured';cpu_result='measured'} | ConvertTo-Json | Set-Content (Join-Path $fixture 'memory-summary.json')
    @(@{heavy_count=0}) | ConvertTo-Json | Set-Content (Join-Path $fixture 'ongoing-load.json')
    $totals=@();$rows=@()
    foreach($seconds in @(0,305,609,650,700,780)) {
        $at=$origin.AddSeconds($seconds).ToString('o')
        $values=if($seconds -eq 305){@(100,300,30,10)}else{@(150,200,30,10)}
        $totals += [pscustomobject]@{utc=$at;private_bytes=($values | Measure-Object -Sum).Sum}
        for($i=0;$i -lt 4;$i++) {
            $rows += [pscustomobject]@{utc=$at;pid=$i+1;start_utc=$origin.ToString('o');role=@('Maxed','renderer','browser','utility/other')[$i];
                process_type=@('','renderer','browser','utility')[$i];utility_sub_type=$(if($i -eq 3){'audio.mojom.AudioService'}else{''});
                private_bytes=$values[$i];cpu_seconds=$seconds/10;effective_acceleration='off'}
        }
    }
    $totals | Export-Csv -NoTypeInformation (Join-Path $fixture 'memory-totals.csv')
    $rows | Export-Csv -NoTypeInformation (Join-Path $fixture 'memory-processes.csv')
    & (Join-Path $PSScriptRoot 'live-realm-report.ps1') -EvidenceDir $fixture | Out-Null
    $report=Get-Content (Join-Path $fixture 'live-realm-summary.json') -Raw | ConvertFrom-Json
    $busy=$report.phases[0]
    if($busy.combined_peak_bytes -ne 440 -or $busy.maxed_peak_bytes -ne 150 -or
        ($busy.concurrent_peak.processes | Where-Object role -eq 'Maxed').private_bytes -ne 100 -or
        ($busy.concurrent_peak.processes | Measure-Object private_bytes -Sum).Sum -ne 440 -or
        @($busy.concurrent_peak.processes | Where-Object {$_.instance -gt 0}).Count -ne 4) { throw 'Concurrent peak decomposition failed' }
    @(@{heavy_count=1}) | ConvertTo-Json | Set-Content (Join-Path $fixture 'ongoing-load.json')
    & (Join-Path $PSScriptRoot 'live-realm-report.ps1') -EvidenceDir $fixture | Out-Null
    $report=Get-Content (Join-Path $fixture 'live-realm-summary.json') -Raw | ConvertFrom-Json
    if(-not $report.load_confounded -or $report.phases[0].gate.result -ne 'not_measured') { throw 'Confounded report admitted' }
    Write-Output 'PASS: concurrent_peak_decomposition_and_load_exclusion'
} finally {
    $resolved=[IO.Path]::GetFullPath($fixture)
    $temporary=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar
    if(-not $resolved.StartsWith($temporary,[StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe fixture cleanup' }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}

& node (Join-Path $PSScriptRoot 'live-realm-attribution.mjs') --self-test
if($LASTEXITCODE -ne 0) { throw 'Attribution parser self-test failed' }
& node (Join-Path $PSScriptRoot 'live-realm-attribution-report.mjs') --self-test
if($LASTEXITCODE -ne 0) { throw 'Attribution analysis self-test failed' }

& node (Join-Path $PSScriptRoot 'live-realm-peaks.mjs') --self-test
if($LASTEXITCODE -ne 0) { throw 'Peak diagnostic self-test failed' }

& node (Join-Path $PSScriptRoot 'live-realm-peaks-report.mjs') --self-test
if($LASTEXITCODE -ne 0) { throw 'Peak timeline analysis self-test failed' }
