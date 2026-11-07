[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
# Execute the actual checkout-free workflow lookup against a stubbed API.
$workflow = Get-Content "$root/.github/workflows/release.yml" -Raw | ConvertFrom-Json
$source = (Get-Content "$root/scripts/release/Find-PriorTests.ps1" | Select-Object -Skip 1) -join "`n"
if ($source -cne $workflow.jobs.'prior-tests'.steps[0].run) { throw 'Workflow lookup differs from tested script' }
$lookup = [scriptblock]::Create($source)
$temp = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
New-Item -ItemType Directory $temp | Out-Null
$names = @('GITHUB_SHA','GITHUB_REPOSITORY','GITHUB_API_URL','GH_TOKEN','GITHUB_OUTPUT','GITHUB_STEP_SUMMARY')
$saved = @{}
foreach ($name in $names) { $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
function Invoke-RestMethod {
    param($Uri, $Headers, $ErrorAction)
    if ($Uri -notlike 'https://api.github.com/repos/synthetic/repo/actions/workflows/tests.yml/runs?head_sha=abc&per_page=100&page=*') { throw 'Wrong API scope' }
    if ($Headers.Authorization -cne 'Bearer synthetic') { throw 'Wrong token' }
    $script:calls++
    if ($script:apiError) { throw 'Synthetic API error' }
    if ($script:calls -gt $script:pages.Count) { throw 'Synthetic later-page error' }
    return $script:pages[$script:calls - 1]
}
try {
    $env:GITHUB_SHA = 'abc'; $env:GITHUB_REPOSITORY = 'synthetic/repo'
    $env:GITHUB_API_URL = 'https://api.github.com'; $env:GH_TOKEN = 'synthetic'
    $env:GITHUB_OUTPUT = Join-Path $temp 'output'; $env:GITHUB_STEP_SUMMARY = Join-Path $temp 'summary'
    foreach ($case in @('success','path','missing-path','repository','missing-repository','missing-full-name','sha','pr','failed','cancelled','branch','running','error','empty','pagination','later-error','schedule','dispatch')) {
        $run = @{ path = '.github/workflows/tests.yml'; head_repository = @{ full_name = 'daplabs/maxed' }; head_sha = 'abc'; head_branch = 'main'; event = 'push'; status = 'completed'; conclusion = 'success'; html_url = 'https://example.invalid/run/1' }
        switch ($case) {
            path { $run.path = '.github/workflows/other.yml' }
            missing-path { $run.Remove('path') }
            repository { $run.head_repository.full_name = 'synthetic/fork' }
            missing-repository { $run.Remove('head_repository') }
            missing-full-name { $run.head_repository.Remove('full_name') }
            sha { $run.head_sha = 'other' }
            pr { $run.event = 'pull_request' }
            failed { $run.conclusion = 'failure' }
            cancelled { $run.conclusion = 'cancelled' }
            branch { $run.head_branch = 'topic' }
            running { $run.status = 'in_progress' }
            schedule { $run.event = 'schedule' }
            dispatch { $run.event = 'workflow_dispatch' }
        }
        $script:pages = @(@{ workflow_runs = @($run) })
        if ($case -eq 'empty') { $script:pages = @(@{ workflow_runs = @() }) }
        if ($case -in @('pagination','later-error')) {
            $wrong = @{ head_sha = 'other' }
            $script:pages = @(@{ workflow_runs = @(1..100 | ForEach-Object { $wrong }) })
            if ($case -eq 'pagination') { $script:pages += @{ workflow_runs = @($wrong, $run) } }
        }
        $script:calls = 0; $script:apiError = $case -eq 'error'
        Remove-Item $env:GITHUB_OUTPUT,$env:GITHUB_STEP_SUMMARY -ErrorAction SilentlyContinue
        & $lookup
        $expected = if ($case -in @('success','pagination','schedule','dispatch')) { 'true' } else { 'false' }
        if ((Get-Content $env:GITHUB_OUTPUT -Raw).Trim() -cne "tested=$expected") { throw "FAIL: $case" }
        if ($expected -eq 'true' -and (Get-Content $env:GITHUB_STEP_SUMMARY -Raw) -notmatch 'https://example.invalid/run/1') { throw 'Missing summary URL' }
        if ($case -eq 'pagination' -and $script:calls -ne 2) { throw 'Missing pagination' }
        Write-Output "PASS: $case"
    }
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $saved[$name]) }
    Remove-Item $temp -Recurse -Force
}
