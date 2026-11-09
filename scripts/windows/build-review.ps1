[CmdletBinding()]
param([switch]$RealProfile)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
. (Join-Path $repo 'tests/gui-smoke/Invoke-GuiSmoke.ps1') -FunctionsOnly
Assert-SmokeWindows
$output = Join-Path $repo 'target/ui-review'
$profilePath = Join-Path $output 'profile'
[void][IO.Directory]::CreateDirectory($output)
$clock = [Diagnostics.Stopwatch]::StartNew()
$previousTarget = $env:CARGO_TARGET_DIR
try {
    # Keep the executable/profile paired per worktree even with a shared Cargo
    # target directory in the caller's environment. Cargo build-dir may be cached.
    $env:CARGO_TARGET_DIR = Join-Path $output $(if ($RealProfile) { 'build-real' } else { 'build' })
    if ($RealProfile) {
        Write-Warning 'REAL PROFILE: this build uses your normal Maxed profile and screenshots can expose account or player data.'
        [void](Invoke-SmokeCommand 'npm.cmd' @('--prefix', 'app/ui', 'ci') $repo)
        [void](Invoke-SmokeCommand 'npm.cmd' @('--prefix', 'app/ui', 'run', 'build') $repo)
        $buildEnv = @{ TAURI_CONFIG = '{}' }
        $metadata = Invoke-SmokeCommand 'cargo' @('metadata', '--locked', '--no-deps', '--format-version', '1') $repo $buildEnv | ConvertFrom-Json
        [void](Invoke-SmokeCommand 'cargo' @('build', '--locked', '-p', 'maxed-app', '--bin', 'maxed', '--features', 'custom-protocol', '--target', 'x86_64-pc-windows-msvc') $repo $buildEnv)
        $exe = Join-Path $metadata.target_directory 'x86_64-pc-windows-msvc/debug/maxed.exe'
    } else {
        [void][IO.Directory]::CreateDirectory($profilePath)
        $exe = Build-SmokeApp $repo $profilePath
    }
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Built executable missing: $exe" }
    $report = [ordered]@{
        exe = $exe; profile = $(if ($RealProfile) { 'real' } else { $profilePath })
        elapsed_seconds = [Math]::Round($clock.Elapsed.TotalSeconds, 2)
    }
    $report | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output 'build.json') -Encoding utf8
    Write-Host "Build wall time: $($report.elapsed_seconds) seconds; profile: $($report.profile)"
    Write-Output $exe
} finally { $env:CARGO_TARGET_DIR = $previousTarget }
