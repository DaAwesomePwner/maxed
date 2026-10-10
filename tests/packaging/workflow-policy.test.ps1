[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$script:checks = 0
function Assert($ok, $name) { if (-not $ok) { throw "FAIL: $name" }; $script:checks++; Write-Output "PASS: $name" }
# JSON is a strict YAML subset. Parsing with the built-in JSON parser is offline,
# structural and fail-closed: YAML-only edits must explicitly migrate this parser.
function Leaves($value, [string]$path) {
    if ($null -eq $value) { return }
    if ($value -is [Collections.IDictionary]) {
        foreach ($key in $value.Keys) { Leaves $value[$key] "$path.$key" }
    } elseif ($value -is [pscustomobject]) {
        foreach ($property in $value.PSObject.Properties) { Leaves $property.Value "$path.$($property.Name)" }
    } elseif ($value -is [array]) {
        for ($i = 0; $i -lt $value.Count; $i++) { Leaves $value[$i] "$path[$i]" }
    } else { [pscustomobject]@{ Path = $path; Value = [string]$value } }
}
function Expressions([string]$value) {
    # Collect expression bodies from each parsed scalar, respecting quoted strings
    # and GitHub's doubled-single-quote escaping. Unterminated bodies fail closed.
    $offset = 0
    while (($start = $value.IndexOf('${{', $offset, [StringComparison]::Ordinal)) -ge 0) {
        $quoted = $false; $end = $start + 3
        for (; $end -lt $value.Length; $end++) {
            if ($value[$end] -eq "'") {
                if ($quoted -and $end + 1 -lt $value.Length -and $value[$end + 1] -eq "'") { $end++; continue }
                $quoted = -not $quoted
            }
            if (-not $quoted -and $end + 1 -lt $value.Length -and $value.Substring($end, 2) -ceq '}}') { break }
        }
        $value.Substring($start + 3, $end - $start - 3)
        if ($end -ge $value.Length - 1) { break }
        $offset = $end + 2
    }
}
function HasSecretExpression([string]$value) {
    foreach ($expression in @(Expressions $value)) {
        if ($expression.IndexOf('secrets', [StringComparison]::OrdinalIgnoreCase) -ge 0) { return $true }
    }
    return $false
}
function StepSecretFailures($step, [bool]$allowSigning) {
    foreach ($field in @('env', 'run', 'with')) {
        foreach ($leaf in @(Leaves $step.$field $field)) {
            $allowed = $allowSigning -and (
                ($leaf.Path -ceq 'env.TAURI_SIGNING_PRIVATE_KEY' -and $leaf.Value -ceq '${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}') -or
                ($leaf.Path -ceq 'env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD' -and $leaf.Value -ceq '${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}'))
            if ((HasSecretExpression $leaf.Value) -and -not $allowed) { 'secret_scope' }
            if (-not $allowSigning -and ($leaf.Path + $leaf.Value).IndexOf('TAURI_SIGNING', [StringComparison]::OrdinalIgnoreCase) -ge 0) { 'secret_scope' }
        }
    }
}
function Keys($value) { return (@($value.PSObject.Properties.Name | Sort-Object) -join ',') }
function Failures($release, $manual, $codeql) {
    if ((@($release.on.PSObject.Properties.Name) -join ',') -cne 'push' -or (@($release.on.push.PSObject.Properties.Name) -join ',') -cne 'tags' -or (@($release.on.push.tags) -join ',') -cne 'v*') { 'release_events' }
    if ((Keys $manual.on) -cne 'pull_request,push,workflow_call,workflow_dispatch' -or
        (Keys $manual.on.pull_request) -cne 'branches' -or (@($manual.on.pull_request.branches) -join ',') -cne 'main' -or
        (Keys $manual.on.push) -cne 'branches' -or (@($manual.on.push.branches) -join ',') -cne 'main' -or
        (Keys $manual.on.workflow_dispatch) -cne '') { 'tests_events' }
    if ((Keys $manual.on.workflow_call) -cne 'inputs' -or (Keys $manual.on.workflow_call.inputs) -cne 'release' -or
        (Keys $manual.on.workflow_call.inputs.release) -cne 'default,description,type' -or
        $manual.on.workflow_call.inputs.release.type -cne 'boolean' -or
        $manual.on.workflow_call.inputs.release.default -cne $false) { 'reusable_inputs' }
    if ((Keys $manual.jobs) -cne 'tests' -or
        $manual.jobs.tests.if -cne "github.event_name == 'workflow_dispatch' || format('{0}', github.event.repository.private) == 'false' || inputs.release == true") { 'private_gate' }
    if ((Keys $manual.jobs.tests) -cne 'defaults,if,runs-on,steps,timeout-minutes' -or
        (Keys $manual.jobs.tests.defaults) -cne 'run' -or (Keys $manual.jobs.tests.defaults.run) -cne 'shell' -or
        $manual.jobs.tests.defaults.run.shell -cne 'pwsh') { 'tests_job_shape' }
    if ($manual.concurrency.group -cne 'tests-${{ github.workflow }}-${{ github.event_name }}-${{ github.event.pull_request.number || github.ref }}' -or
        $manual.concurrency.'cancel-in-progress' -cne '${{ github.event_name == ''pull_request'' }}') { 'tests_concurrency' }
    if ((Keys $release.jobs) -cne 'package,tests' -or
        (@($release.jobs.package.needs) -join ',') -cne 'tests' -or
        (Keys $release.jobs.tests) -cne 'if,uses,with' -or
        $release.jobs.tests.uses -cne './.github/workflows/tests.yml' -or
        $release.jobs.tests.if -cne "github.repository == 'DaAwesomePwner/maxed-rotmg' && github.ref_type == 'tag'" -or
        (Keys $release.jobs.tests.with) -cne 'release' -or $release.jobs.tests.with.release -cne $true) { 'release_tests' }
    if ((Keys $codeql.on) -cne 'pull_request,push,schedule' -or (Keys $codeql.on.pull_request) -cne 'branches' -or
        (Keys $codeql.on.push) -cne 'branches' -or (@($codeql.on.push.branches) -join ',') -cne 'main' -or
        (@($codeql.on.pull_request.branches) -join ',') -cne 'main' -or @($codeql.on.schedule).Count -ne 1 -or
        (Keys $codeql.on.schedule[0]) -cne 'cron' -or $codeql.on.schedule[0].cron -cne '23 6 * * 1') { 'codeql_events' }
    if ((Keys $codeql.jobs) -cne 'analyze' -or $codeql.jobs.analyze.if -cne "format('{0}', github.event.repository.private) == 'false'") { 'codeql_private_gate' }
    if (($codeql.jobs.analyze.strategy.matrix.language -join ',') -cne 'javascript-typescript,rust') { 'codeql_languages' }
    $init = @($codeql.jobs.analyze.steps | Where-Object { $_.uses -like 'github/codeql-action/init@*' })
    $analyze = @($codeql.jobs.analyze.steps | Where-Object { $_.uses -like 'github/codeql-action/analyze@*' })
    if ($init.Count -ne 1 -or $analyze.Count -ne 1 -or $init[0].with.languages -cne '${{ matrix.language }}' -or
        $init[0].with.'build-mode' -cne 'none' -or $analyze[0].with.category -cne '/language:${{ matrix.language }}') { 'codeql_steps' }
    foreach ($w in @($release, $manual, $codeql)) {
        if ((@($w.permissions.PSObject.Properties.Name) -join ',') -cne 'contents' -or $w.permissions.contents -cne 'read') { 'permissions' }
        if ($w.env -or $w.secrets) { 'workflow_scope' }
        foreach ($job in $w.jobs.PSObject.Properties.Value) {
            # Check presence, including false/null values, before the reusable-job exception.
            if ('continue-on-error' -in $job.PSObject.Properties.Name) { 'job_policy' }
            if ([object]::ReferenceEquals($job, $release.jobs.tests)) { continue }
            $isCodeql = [object]::ReferenceEquals($w, $codeql) -and [object]::ReferenceEquals($job, $codeql.jobs.analyze)
            $runner = if ($isCodeql) { 'ubuntu-24.04' } else { 'windows-2022' }
            if ($job.'runs-on' -cne $runner -or $job.'timeout-minutes' -ne 45 -or $job.env -or $job.secrets -or $job.uses) { 'job_policy' }
            # Only CodeQL may upload security results; all repository contents stay read-only.
            if ($isCodeql) {
                if ((Keys $job.permissions) -cne 'contents,security-events' -or $job.permissions.contents -cne 'read' -or
                    $job.permissions.'security-events' -cne 'write') { 'permissions' }
            } elseif ($job.permissions) { 'permissions' }
            if (-not [object]::ReferenceEquals($job, $release.jobs.package) -and $job.environment) { 'secret_scope' }
            foreach ($step in $job.steps) {
                $pin = if ($isCodeql) { '\A(actions/checkout|github/codeql-action/(init|analyze))@[a-f0-9]{40}\z' } else { '\Aactions/(checkout|setup-node|upload-artifact)@[a-f0-9]{40}\z' }
                if ($step.uses -and $step.uses -cnotmatch $pin) { 'action_pin' }
                if ($step.uses -like 'actions/checkout@*' -and $step.with.'persist-credentials' -cne $false) { 'checkout_credentials' }
                $allowSigning = [object]::ReferenceEquals($w, $release) -and [object]::ReferenceEquals($job, $release.jobs.package) -and $step.name -ceq 'Package and sign'
                StepSecretFailures $step $allowSigning
                if ($allowSigning) {
                    if ($step.env.TAURI_SIGNING_PRIVATE_KEY -cne '${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}' -or $step.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD -cne '${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}' -or $step.run -notlike '*./scripts/build-release.ps1 -Package -Offline*') { 'signing_step' }
                }
            }
        }
    }
    $names = @($manual.jobs.tests.steps | ForEach-Object { $_.name })
    $expectedNames = @('', '') + @($script:testCommands.Keys)
    if ($names.Count -ne $expectedNames.Count -or ($names -join '|') -cne ($expectedNames -join '|')) { 'tests_step_order' }
    for ($i = 0; $i -lt $manual.jobs.tests.steps.Count; $i++) {
        $step = $manual.jobs.tests.steps[$i]
        if ('continue-on-error' -in $step.PSObject.Properties.Name -or 'if' -in $step.PSObject.Properties.Name) { 'tests_step_policy' }
        if ($i -lt 2) {
            $action = @('checkout', 'setup-node')[$i]
            $withKey = @('persist-credentials', 'node-version')[$i]
            if ((Keys $step) -cne 'uses,with' -or $step.uses -cnotmatch "\Aactions/$action@[a-f0-9]{40}\z" -or
                (Keys $step.with) -cne $withKey) { 'tests_step_shape' }
            if ($i -eq 1 -and $step.with.'node-version' -cne '22.16.0') { 'tests_step_shape' }
        } else {
            if ((Keys $step) -cne 'name,run') { 'tests_step_shape' }
            if ($step.name -cnotin $script:testCommands.Keys -or $step.run -cne $script:testCommands[$step.name]) { 'tests_commands' }
        }
    }
    $bindings = @($manual.jobs.tests.steps | Where-Object name -CEQ 'Bindings freshness')
    if ($bindings.Count -ne 1 -or $bindings[0].run -cne $script:bindingsCommand) { 'bindings_freshness' }
    $memory = @($manual.jobs.tests.steps | Where-Object name -CEQ 'Offline memory tooling')
    if ($memory.Count -ne 1 -or $memory[0].run -cne $script:memoryCommand) { 'memory_tests' }
    if ($release.jobs.package.environment -cne 'release-signing' -or -not $release.concurrency.group -or $release.concurrency.'cancel-in-progress' -ne $false) { 'release_protection' }
    if (@($release.jobs.package.steps | Where-Object name -CEQ 'Package and sign').Count -ne 1) { 'signing_step' }
    $names = @($release.jobs.package.steps | ForEach-Object { $_.name })
    $validateIndex = [Array]::IndexOf($names, 'Validate tag and signing configuration')
    $packageIndex = [Array]::IndexOf($names, 'Package and sign')
    if ($validateIndex -lt 0 -or $packageIndex -lt 0 -or $validateIndex -ge $packageIndex) { 'validation_order' }
    # Exact guard excludes status functions (always/failure/!cancelled), preserving
    # Actions' implicit success() requirement for the tests dependency.
    if ($release.jobs.package.if -cne "github.repository == 'DaAwesomePwner/maxed-rotmg' && github.ref_type == 'tag'") { 'release_guard' }
    # Builds run in the private repository; metadata must name the public one the updater pins.
    $coordinates = @($release.jobs.package.steps | ForEach-Object { [regex]::Matches([string]$_.run, "-Repository '([^']*)'") } | ForEach-Object { $_.Groups[1].Value })
    if ($coordinates.Count -ne 2 -or @($coordinates | Where-Object { $_ -cne $script:updaterRepository }).Count) { 'distribution_repository' }
    $upload = @($release.jobs.package.steps | Where-Object { $_.uses -like 'actions/upload-artifact@*' })
    if ($upload.Count -ne 1 -or $upload[0].with.path -cne 'release-stage/' -or $upload[0].with.'retention-days' -ne 3 -or $upload[0].with.'if-no-files-found' -cne 'error') { 'upload_allowlist' }
}
$script:bindingsCommand = @'
git diff --exit-code HEAD -- app/ui/src/lib/bindings app/ui/tests/fixtures crates/maxed-types/tests/fixtures/p1-demo.json
if ($LASTEXITCODE) { throw 'Generated bindings or fixtures changed; regenerate and commit them.' }
$untracked = @(git ls-files --others -- app/ui/src/lib/bindings app/ui/tests/fixtures crates/maxed-types/tests/fixtures/p1-demo.json)
if ($LASTEXITCODE) { throw 'Could not check untracked bindings or fixtures.' }
if ($untracked.Count -gt 0) { throw ('Untracked generated bindings or fixtures: ' + ($untracked -join ', ')) }
'@ -replace "`r`n", "`n"
$script:memoryCommand = @'
pwsh -NoProfile -File tests/performance/memory-tooling.test.ps1
if ($LASTEXITCODE) { throw 'Memory tooling test failed.' }
pwsh -NoProfile -File tests/performance/p4-memory-profile.test.ps1
if ($LASTEXITCODE) { throw 'P4 memory profile test failed.' }
'@ -replace "`r`n", "`n"
# Independently pinned commands: never derive this baseline from the workflow under test.
$script:testCommands = [ordered]@{
    'Restore locked tools' = @'
rustup toolchain install 1.99.0 --profile minimal --component rustfmt --component clippy
if ($LASTEXITCODE) { throw 'Rust restore failed.' }
npm.cmd ci --prefix app/ui --no-audit --no-fund
if ($LASTEXITCODE) { throw 'UI restore failed.' }
cargo fetch --locked
if ($LASTEXITCODE) { throw 'Cargo restore failed.' }
'@ -replace "`r`n", "`n"
    'Rust' = @'
cargo fmt --all -- --check
if ($LASTEXITCODE) { throw 'fmt failed.' }
cargo clippy --offline --locked --workspace --all-targets -- -D warnings
if ($LASTEXITCODE) { throw 'clippy failed.' }
cargo test --offline --locked --workspace
if ($LASTEXITCODE) { throw 'Rust tests failed.' }
'@ -replace "`r`n", "`n"
    'Bindings freshness' = $script:bindingsCommand
    'Perf feature compile' = @'
cargo check --offline --locked -p maxed-app --all-targets --features perf-live
if ($LASTEXITCODE) { throw 'Perf feature check failed.' }
cargo clippy --offline --locked -p maxed-app --all-targets --features perf-live -- -D warnings
if ($LASTEXITCODE) { throw 'Perf feature clippy failed.' }
'@ -replace "`r`n", "`n"
    'UI' = @'
foreach ($s in @('check', 'check:tests', 'test', 'test:ui', 'build')) { npm.cmd --prefix app/ui run $s; if ($LASTEXITCODE) { throw 'UI check failed.' } }
'@ -replace "`r`n", "`n"
    'Offline memory tooling' = $script:memoryCommand
    'Packaging and collectors' = @'
foreach ($f in Get-ChildItem tests/packaging -Filter *.test.ps1 -File) { powershell -NoProfile -ExecutionPolicy Bypass -File $f.FullName; if ($LASTEXITCODE) { throw 'Packaging check failed.' } }
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/Measure-MaxedMemory.ps1 -SelfTest
if ($LASTEXITCODE) { throw 'Memory collector self-test failed.' }
powershell -NoProfile -ExecutionPolicy Bypass -File tests/performance/home-scenarios.ps1 -SelfTest
if ($LASTEXITCODE) { throw 'Scenario collector self-test failed.' }
'@ -replace "`r`n", "`n"
}
$pin = [regex]::Match((Get-Content "$root/app/src-tauri/src/updater/mod.rs" -Raw), 'pub const REPOSITORY: &str = "([^"]+)";')
if (-not $pin.Success) { throw 'Updater repository constant not found.' }
$script:updaterRepository = $pin.Groups[1].Value
$codeqlText = Get-Content "$root/.github/workflows/codeql.yml" -Raw
$c = $codeqlText | ConvertFrom-Json
$releaseText = Get-Content "$root/.github/workflows/release.yml" -Raw
$manualText = Get-Content "$root/.github/workflows/tests.yml" -Raw
$r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json
Assert (@(Failures $r $m $c).Count -eq 0) 'workflow events, billing gates, release dependency and policy positive control'
$perfStep = @($m.jobs.tests.steps | Where-Object name -CEQ 'Perf feature compile')
Assert ($perfStep.Count -eq 1 -and $perfStep[0].run.Contains('cargo check --offline --locked -p maxed-app --all-targets --features perf-live')) 'test workflow compiles the perf-only driver offline'
foreach ($case in @(
    @{ want = 'release_events'; mutate = { param($r,$m) $r.on | Add-Member schedule @() } },
    @{ want = 'tests_events'; mutate = { param($r,$m) $m.on.push.branches = @('*') } },
    @{ want = 'tests_events'; mutate = { param($r,$m) $m.on.PSObject.Properties.Remove('pull_request') } },
    @{ want = 'tests_events'; mutate = { param($r,$m) $m.on | Add-Member pull_request_target @{} } },
    @{ want = 'private_gate'; mutate = { param($r,$m) $m.jobs.tests.if = 'true' } },
    @{ want = 'private_gate'; mutate = { param($r,$m) $m.jobs.tests.if = 'github.event.repository.private == false' } },
    @{ want = 'private_gate'; mutate = { param($r,$m) $m.jobs.tests.if = "github.event_name == 'workflow_dispatch' || github.event.repository.private == false || inputs.release == true" } },
    @{ want = 'tests_job_shape'; mutate = { param($r,$m) $m.jobs.tests | Add-Member 'continue-on-error' $true } },
    @{ want = 'tests_job_shape'; mutate = { param($r,$m) $m.jobs.tests.defaults.run.shell = 'cmd' } },
    @{ want = 'tests_job_shape'; mutate = { param($r,$m) $m.jobs.tests.defaults.run | Add-Member 'working-directory' 'other' } },
    @{ want = 'tests_step_order'; mutate = { param($r,$m) $s = $m.jobs.tests.steps; $old = $s[3]; $s[3] = $s[6]; $s[6] = $old } },
    @{ want = 'tests_step_order'; mutate = { param($r,$m) $m.jobs.tests.steps = @($m.jobs.tests.steps | Where-Object name -CNE 'UI') } },
    @{ want = 'tests_step_order'; mutate = { param($r,$m) $m.jobs.tests.steps += $m.jobs.tests.steps[-1] } },
    @{ want = 'tests_step_shape'; mutate = { param($r,$m) $m.jobs.tests.steps[3] | Add-Member shell 'cmd' } },
    @{ want = 'tests_step_shape'; mutate = { param($r,$m) $m.jobs.tests.steps[0].uses = $m.jobs.tests.steps[1].uses } },
    @{ want = 'tests_step_shape'; mutate = { param($r,$m) $m.jobs.tests.steps[1].with.'node-version' = '20' } },
    @{ want = 'reusable_inputs'; mutate = { param($r,$m) $m.on.workflow_call.inputs.release.default = $true } },
    @{ want = 'tests_concurrency'; mutate = { param($r,$m) $m.concurrency.'cancel-in-progress' = $false } },
    @{ want = 'release_tests'; mutate = { param($r,$m) $r.jobs.package.PSObject.Properties.Remove('needs') } },
    @{ want = 'release_tests'; mutate = { param($r,$m) $r.jobs.tests.with.release = $false } },
    @{ want = 'release_tests'; mutate = { param($r,$m) $r.jobs.tests | Add-Member secrets 'inherit' } },
    @{ want = 'release_tests'; mutate = { param($r,$m) $r.jobs.tests.uses = './.github/workflows/other.yml' } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = 'always()' } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = 'always() && ' + $r.jobs.package.if } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = 'failure() && ' + $r.jobs.package.if } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = '!cancelled() && ' + $r.jobs.package.if } },
    @{ want = 'bindings_freshness'; mutate = { param($r,$m) ($m.jobs.tests.steps | Where-Object name -CEQ 'Bindings freshness').run = 'git diff --exit-code' } },
    @{ want = 'tests_step_policy'; mutate = { param($r,$m) ($m.jobs.tests.steps | Where-Object name -CEQ 'Bindings freshness') | Add-Member 'continue-on-error' $true } },
    @{ want = 'memory_tests'; mutate = { param($r,$m) ($m.jobs.tests.steps | Where-Object name -CEQ 'Offline memory tooling').run = 'echo skipped' } },
    @{ want = 'codeql_private_gate'; mutate = { param($r,$m,$c) $c.jobs.analyze.if = 'true' } },
    @{ want = 'codeql_private_gate'; mutate = { param($r,$m,$c) $c.jobs.analyze.if = 'github.event.repository.private == false' } },
    @{ want = 'codeql_events'; mutate = { param($r,$m,$c) $c.on | Add-Member pull_request_target @{} } },
    @{ want = 'codeql_events'; mutate = { param($r,$m,$c) $c.on.PSObject.Properties.Remove('push') } },
    @{ want = 'codeql_events'; mutate = { param($r,$m,$c) $c.on.push.branches = @('*') } },
    @{ want = 'codeql_languages'; mutate = { param($r,$m,$c) $c.jobs.analyze.strategy.matrix.language = @('javascript-typescript') } },
    @{ want = 'codeql_steps'; mutate = { param($r,$m,$c) $c.jobs.analyze.steps[1].with.'build-mode' = 'autobuild' } },
    @{ want = 'permissions'; mutate = { param($r,$m,$c) $c.jobs.analyze.permissions.contents = 'write' } },
    @{ want = 'permissions'; mutate = { param($r,$m,$c) $c.permissions | Add-Member 'security-events' 'write' } },
    @{ want = 'permissions'; mutate = { param($r,$m) $m.jobs.tests | Add-Member permissions @{ contents = 'write' } } },
    @{ want = 'action_pin'; mutate = { param($r,$m,$c) $c.jobs.analyze.steps[1].uses = 'github/codeql-action/init@v4' } },
    @{ want = 'checkout_credentials'; mutate = { param($r,$m,$c) $c.jobs.analyze.steps[0].with.'persist-credentials' = $true } },
    @{ want = 'permissions'; mutate = { param($r,$m) $m.permissions.contents = 'write' } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) $m.jobs.tests.steps[0] | Add-Member env @{ TAURI_SIGNING_PRIVATE_KEY = 'invented' } } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) $r.jobs.package.steps[0] | Add-Member env @{ KEY = '${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}' } } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) $m.jobs.tests.steps[0] | Add-Member env @{ KEY = '${{ secrets[''OTHER''] }}' } } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) $m.jobs.tests.steps[2].run += '${{ toJSON(secrets) }}' } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) $m.jobs.tests.steps[0].with | Add-Member token '${{ secrets[''OTHER''] }}' } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) ($r.jobs.package.steps | Where-Object name -EQ 'Package and sign').env | Add-Member EXTRA '${{ secrets.OTHER }}' } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) ($r.jobs.package.steps | Where-Object name -EQ 'Package and sign').run += '${{ toJSON(secrets) }}' } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) ($r.jobs.package.steps | Where-Object name -EQ 'Package and sign') | Add-Member with @{ token = '${{ secrets[''OTHER''] }}' } } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) ($r.jobs.package.steps | Where-Object name -EQ 'Package and sign').env.TAURI_SIGNING_PRIVATE_KEY += '${{ secrets.OTHER }}' } },
    @{ want = 'checkout_credentials'; mutate = { param($r,$m) $r.jobs.package.steps[0].with.'persist-credentials' = $true } },
    @{ want = 'checkout_credentials'; mutate = { param($r,$m) $m.jobs.tests.steps[0].with.PSObject.Properties.Remove('persist-credentials') } },
    @{ want = 'validation_order'; mutate = { param($r,$m) $s = $r.jobs.package.steps; $old = $s[3]; $s[3] = $s[4]; $s[4] = $old } },
    @{ want = 'validation_order'; mutate = { param($r,$m) $r.jobs.package.steps[3].name = 'Missing validation' } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = "github.ref_type == 'tag'" } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = "github.repository == 'DaAwesomePwner/maxed-rotmg'" } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = "github.repository == 'DaAwesomePwner/maxed-rotmg' || github.ref_type == 'tag'" } },
    @{ want = 'distribution_repository'; mutate = { param($r,$m) $s = $r.jobs.package.steps | Where-Object name -EQ 'Verify and stage allowlist'; $s.run = $s.run.Replace("-Repository '$script:updaterRepository'", "-Repository 'DaAwesomePwner/maxed-rotmg'") } },
    @{ want = 'distribution_repository'; mutate = { param($r,$m) $s = $r.jobs.package.steps | Where-Object name -EQ 'Validate tag and signing configuration'; $s.run = $s.run.Replace("-Repository '$script:updaterRepository'", '-Repository $env:GITHUB_REPOSITORY') } },
    @{ want = 'signing_step'; mutate = { param($r,$m) ($r.jobs.package.steps | Where-Object name -EQ 'Package and sign').env.TAURI_SIGNING_PRIVATE_KEY = '' } },
    @{ want = 'action_pin'; mutate = { param($r,$m) $r.jobs.package.steps[0].uses = 'actions/checkout@main' } },
    @{ want = 'action_pin'; mutate = { param($r,$m) $r.jobs.package.steps[0].uses = 'actions/checkout@COORDINATOR_REPLACE_WITH_REVIEWED_40_HEX_SHA' } },
    @{ want = 'upload_allowlist'; mutate = { param($r,$m) $r.jobs.package.steps[-1].with.path = '**' } }
)) {
    $r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json
    $c = $codeqlText | ConvertFrom-Json
    & $case.mutate $r $m $c
    Assert ($case.want -in @(Failures $r $m $c)) "reject mutation $($case.want)"
}
# Exercise the generic rule on every job, including the reusable release caller.
foreach ($target in @('release.tests', 'release.package', 'tests.tests', 'codeql.analyze')) {
    $r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json; $c = $codeqlText | ConvertFrom-Json
    $workflow, $jobName = $target.Split('.')
    $job = @{ release = $r; tests = $m; codeql = $c }[$workflow].jobs.$jobName
    $job | Add-Member 'continue-on-error' $true
    Assert ('job_policy' -in @(Failures $r $m $c)) "reject $target job continue-on-error"
}
$baseline = $manualText | ConvertFrom-Json
for ($i = 0; $i -lt $baseline.jobs.tests.steps.Count; $i++) {
    foreach ($field in @('continue-on-error', 'if')) {
        $r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json; $c = $codeqlText | ConvertFrom-Json
        $value = if ($field -ceq 'if') { 'false' } else { $true }
        $m.jobs.tests.steps[$i] | Add-Member $field $value
        Assert ('tests_step_policy' -in @(Failures $r $m $c)) "reject tests step $i $field"
    }
    if ($i -ge 2) {
        foreach ($suffix in @('; exit 0', ' || true')) {
            $r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json; $c = $codeqlText | ConvertFrom-Json
            $m.jobs.tests.steps[$i].run += $suffix
            Assert ('tests_commands' -in @(Failures $r $m $c)) "reject $($m.jobs.tests.steps[$i].name) appended $suffix"
        }
    }
}
# Each exporter output scope must be present in both the tracked and untracked checks.
foreach ($path in @('app/ui/src/lib/bindings', 'app/ui/tests/fixtures', 'crates/maxed-types/tests/fixtures/p1-demo.json')) {
    foreach ($line in @(0, 2)) {
        $r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json; $c = $codeqlText | ConvertFrom-Json
        $step = $m.jobs.tests.steps | Where-Object name -CEQ 'Bindings freshness'
        $lines = $step.run.Split("`n")
        $lines[$line] = $lines[$line].Replace(' ' + $path, '')
        $step.run = $lines -join "`n"
        Assert ('bindings_freshness' -in @(Failures $r $m $c)) "reject missing freshness path $path on line $line"
    }
}
foreach ($file in Get-ChildItem "$root/.github/workflows" -File) {
    Assert ($file.Name -cin @('release.yml', 'tests.yml', 'codeql.yml')) 'every workflow has an explicit policy'
    $w = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
    if ($file.Name -cne 'release.yml') {
        $forbidden = @(Leaves $w '' | Where-Object {
            (HasSecretExpression $_.Value) -or ($_.Path + $_.Value).IndexOf('TAURI_SIGNING', [StringComparison]::OrdinalIgnoreCase) -ge 0
        })
        Assert ($forbidden.Count -eq 0) 'all non-release workflows contain no secret references'
    }
}
Write-Output "Workflow policy: $script:checks checks passed. Action references require full 40-hex pins; tags and placeholders are rejected."
