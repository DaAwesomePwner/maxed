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
# Independent literal policy fixtures pin every step, command, cache condition and action.
# Do not load these expectations from the workflows being tested.
$script:expectedJobs = @'
{
  "rust-lint": {
    "runs-on": "windows-2022",
    "timeout-minutes": 90,
    "defaults": {
      "run": {
        "shell": "pwsh"
      }
    },
    "steps": [
      {"uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1", "with": {"persist-credentials": false}},
      {"uses": "actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1", "with": {"node-version": "22.16.0"}},
      {"name": "Restore Rust cache", "if": "runner.environment == 'github-hosted'", "uses": "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9", "with": {"path": "~/.cargo/registry/index\n~/.cargo/registry/cache\n~/.cargo/git/db\ntarget", "key": "rust-rust-lint-${{ runner.os }}-${{ hashFiles('rust-toolchain.toml', 'Cargo.lock') }}"}},
      {"name": "Restore locked tools", "run": "rustup toolchain install 1.99.0 --profile minimal --component rustfmt --component clippy\nif ($LASTEXITCODE) { throw 'Rust restore failed.' }\nnpm.cmd ci --prefix app/ui --no-audit --no-fund\nif ($LASTEXITCODE) { throw 'UI restore failed.' }\ncargo fetch --locked\nif ($LASTEXITCODE) { throw 'Cargo restore failed.' }"},
      {"name": "Rust lint", "run": "cargo fmt --all -- --check\nif ($LASTEXITCODE) { throw 'fmt failed.' }\ncargo clippy --offline --locked --workspace --all-targets -- -D warnings\nif ($LASTEXITCODE) { throw 'clippy failed.' }"},
      {"name": "Perf feature compile", "run": "cargo clippy --offline --locked -p maxed-app --all-targets --features perf-live -- -D warnings\nif ($LASTEXITCODE) { throw 'Perf feature clippy failed.' }"},
      {"name": "Prune Rust cache", "if": "runner.environment == 'github-hosted' && github.ref == 'refs/heads/main' && github.event_name != 'pull_request'", "run": "Remove-Item -LiteralPath target/debug/incremental -Recurse -Force -ErrorAction SilentlyContinue\nGet-ChildItem target/debug/deps, target/debug/.fingerprint, target/debug/build -ErrorAction SilentlyContinue | Where-Object { $_.Name -like '*maxed*' -or $_.Extension -in @('.exe', '.pdb') } | Remove-Item -Recurse -Force\nGet-ChildItem target/debug -File -ErrorAction SilentlyContinue | Remove-Item -Force"},
      {"name": "Save Rust cache", "if": "runner.environment == 'github-hosted' && github.ref == 'refs/heads/main' && github.event_name != 'pull_request'", "uses": "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9", "with": {"path": "~/.cargo/registry/index\n~/.cargo/registry/cache\n~/.cargo/git/db\ntarget", "key": "rust-rust-lint-${{ runner.os }}-${{ hashFiles('rust-toolchain.toml', 'Cargo.lock') }}"}}
    ],
    "permissions": {
      "contents": "read"
    }
  },
  "rust-test": {
    "runs-on": "windows-2022",
    "timeout-minutes": 90,
    "defaults": {
      "run": {
        "shell": "pwsh"
      }
    },
    "steps": [
      {"uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1", "with": {"persist-credentials": false}},
      {"uses": "actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1", "with": {"node-version": "22.16.0"}},
      {"name": "Restore Rust cache", "if": "runner.environment == 'github-hosted'", "uses": "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9", "with": {"path": "~/.cargo/registry/index\n~/.cargo/registry/cache\n~/.cargo/git/db\ntarget", "key": "rust-rust-test-${{ runner.os }}-${{ hashFiles('rust-toolchain.toml', 'Cargo.lock') }}"}},
      {"name": "Restore locked tools", "run": "rustup toolchain install 1.99.0 --profile minimal --component rustfmt --component clippy\nif ($LASTEXITCODE) { throw 'Rust restore failed.' }\nnpm.cmd ci --prefix app/ui --no-audit --no-fund\nif ($LASTEXITCODE) { throw 'UI restore failed.' }\ncargo fetch --locked\nif ($LASTEXITCODE) { throw 'Cargo restore failed.' }"},
      {"name": "Rust tests", "run": "cargo test --offline --locked --workspace --exclude maxed-store\nif ($LASTEXITCODE) { throw 'Rust tests failed.' }"},
      {"name": "Bindings freshness", "run": "git diff --exit-code HEAD -- app/ui/src/lib/bindings app/ui/tests/fixtures crates/maxed-types/tests/fixtures/p1-demo.json\nif ($LASTEXITCODE) { throw 'Generated bindings or fixtures changed; regenerate and commit them.' }\n$untracked = @(git ls-files --others -- app/ui/src/lib/bindings app/ui/tests/fixtures crates/maxed-types/tests/fixtures/p1-demo.json)\nif ($LASTEXITCODE) { throw 'Could not check untracked bindings or fixtures.' }\nif ($untracked.Count -gt 0) { throw ('Untracked generated bindings or fixtures: ' + ($untracked -join ', ')) }"},
      {"name": "Prune Rust cache", "if": "runner.environment == 'github-hosted' && github.ref == 'refs/heads/main' && github.event_name != 'pull_request'", "run": "Remove-Item -LiteralPath target/debug/incremental -Recurse -Force -ErrorAction SilentlyContinue\nGet-ChildItem target/debug/deps, target/debug/.fingerprint, target/debug/build -ErrorAction SilentlyContinue | Where-Object { $_.Name -like '*maxed*' -or $_.Extension -in @('.exe', '.pdb') } | Remove-Item -Recurse -Force\nGet-ChildItem target/debug -File -ErrorAction SilentlyContinue | Remove-Item -Force"},
      {"name": "Save Rust cache", "if": "runner.environment == 'github-hosted' && github.ref == 'refs/heads/main' && github.event_name != 'pull_request'", "uses": "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9", "with": {"path": "~/.cargo/registry/index\n~/.cargo/registry/cache\n~/.cargo/git/db\ntarget", "key": "rust-rust-test-${{ runner.os }}-${{ hashFiles('rust-toolchain.toml', 'Cargo.lock') }}"}}
    ],
    "permissions": {
      "contents": "read"
    }
  },
  "store-test": {
    "runs-on": "windows-2022",
    "timeout-minutes": 90,
    "defaults": {
      "run": {
        "shell": "pwsh"
      }
    },
    "steps": [
      {"uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1", "with": {"persist-credentials": false}},
      {"name": "Restore Rust cache", "if": "runner.environment == 'github-hosted'", "uses": "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9", "with": {"path": "~/.cargo/registry/index\n~/.cargo/registry/cache\n~/.cargo/git/db\ntarget", "key": "rust-store-test-${{ runner.os }}-${{ hashFiles('rust-toolchain.toml', 'Cargo.lock') }}"}},
      {"name": "Restore locked tools", "run": "rustup toolchain install 1.99.0 --profile minimal\nif ($LASTEXITCODE) { throw 'Rust restore failed.' }\ncargo fetch --locked\nif ($LASTEXITCODE) { throw 'Cargo restore failed.' }"},
      {"name": "Store tests", "run": "cargo test --offline --locked -p maxed-store\nif ($LASTEXITCODE) { throw 'Rust tests failed.' }"},
      {"name": "Prune Rust cache", "if": "runner.environment == 'github-hosted' && github.ref == 'refs/heads/main' && github.event_name != 'pull_request'", "run": "Remove-Item -LiteralPath target/debug/incremental -Recurse -Force -ErrorAction SilentlyContinue\nGet-ChildItem target/debug/deps, target/debug/.fingerprint, target/debug/build -ErrorAction SilentlyContinue | Where-Object { $_.Name -like '*maxed*' -or $_.Extension -in @('.exe', '.pdb') } | Remove-Item -Recurse -Force\nGet-ChildItem target/debug -File -ErrorAction SilentlyContinue | Remove-Item -Force"},
      {"name": "Save Rust cache", "if": "runner.environment == 'github-hosted' && github.ref == 'refs/heads/main' && github.event_name != 'pull_request'", "uses": "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9", "with": {"path": "~/.cargo/registry/index\n~/.cargo/registry/cache\n~/.cargo/git/db\ntarget", "key": "rust-store-test-${{ runner.os }}-${{ hashFiles('rust-toolchain.toml', 'Cargo.lock') }}"}}
    ],
    "permissions": {
      "contents": "read"
    }
  },
  "ui": {
    "runs-on": "windows-2022",
    "timeout-minutes": 90,
    "defaults": {
      "run": {
        "shell": "pwsh"
      }
    },
    "steps": [
      {"uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1", "with": {"persist-credentials": false}},
      {"uses": "actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1", "with": {"node-version": "22.16.0"}},
      {"name": "Restore locked tools", "run": "npm.cmd ci --prefix app/ui --no-audit --no-fund\nif ($LASTEXITCODE) { throw 'UI restore failed.' }"},
      {"name": "UI", "run": "foreach ($s in @('check', 'check:tests', 'test', 'test:ui', 'build')) { npm.cmd --prefix app/ui run $s; if ($LASTEXITCODE) { throw 'UI check failed.' } }"}
    ],
    "permissions": {
      "contents": "read"
    }
  },
  "packaging": {
    "runs-on": "windows-2022",
    "timeout-minutes": 90,
    "defaults": {
      "run": {
        "shell": "pwsh"
      }
    },
    "steps": [
      {"uses": "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1", "with": {"persist-credentials": false}},
      {"uses": "actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1", "with": {"node-version": "22.16.0"}},
      {"name": "Restore locked tools", "run": "rustup toolchain install 1.99.0 --profile minimal --component rustfmt --component clippy\nif ($LASTEXITCODE) { throw 'Rust restore failed.' }\nnpm.cmd ci --prefix app/ui --no-audit --no-fund\nif ($LASTEXITCODE) { throw 'UI restore failed.' }\ncargo fetch --locked\nif ($LASTEXITCODE) { throw 'Cargo restore failed.' }"},
      {"name": "Offline memory tooling", "run": "pwsh -NoProfile -File tests/performance/memory-tooling.test.ps1\nif ($LASTEXITCODE) { throw 'Memory tooling test failed.' }\npwsh -NoProfile -File tests/performance/p4-memory-profile.test.ps1\nif ($LASTEXITCODE) { throw 'P4 memory profile test failed.' }"},
      {"name": "Packaging and collectors", "run": "foreach ($f in Get-ChildItem tests/packaging -Filter *.test.ps1 -File) { powershell -NoProfile -ExecutionPolicy Bypass -File $f.FullName; if ($LASTEXITCODE) { throw 'Packaging check failed.' } }\npowershell -NoProfile -ExecutionPolicy Bypass -File scripts/Measure-MaxedMemory.ps1 -SelfTest\nif ($LASTEXITCODE) { throw 'Memory collector self-test failed.' }\npowershell -NoProfile -ExecutionPolicy Bypass -File tests/performance/home-scenarios.ps1 -SelfTest\nif ($LASTEXITCODE) { throw 'Scenario collector self-test failed.' }"}
    ],
    "permissions": {
      "contents": "read"
    }
  }
}
'@ | ConvertFrom-Json
$script:expectedPrior = @'
{
  "if": "github.repository == 'daplabs/maxed' && github.ref_type == 'tag'",
  "runs-on": "ubuntu-latest",
  "permissions": {
    "actions": "read"
  },
  "timeout-minutes": 5,
  "outputs": {
    "tested": "${{ steps.lookup.outputs.tested }}"
  },
  "steps": [
    {"name": "Find prior tests", "id": "lookup", "shell": "pwsh", "env": {"GH_TOKEN": "${{ github.token }}"}, "run": "$tested = 'false'\ntry {\n  for ($page = 1; $page -le 10; $page++) {\n    $response = Invoke-RestMethod -Uri \"$env:GITHUB_API_URL/repos/$env:GITHUB_REPOSITORY/actions/workflows/tests.yml/runs?head_sha=$env:GITHUB_SHA&per_page=100&page=$page\" -Headers @{ Authorization = \"Bearer $env:GH_TOKEN\"; Accept = 'application/vnd.github+json' } -ErrorAction Stop\n    $match = @($response.workflow_runs | Where-Object { $_.path -ceq '.github/workflows/tests.yml' -and $_.head_repository.full_name -ceq 'daplabs/maxed' -and $_.head_sha -ceq $env:GITHUB_SHA -and $_.head_branch -ceq 'main' -and $_.event -cin @('push', 'schedule', 'workflow_dispatch') -and $_.status -ceq 'completed' -and $_.conclusion -ceq 'success' })\n    if ($match.Count) { \"Reused tests: $($match[0].html_url)\" >> $env:GITHUB_STEP_SUMMARY; $tested = 'true'; break }\n    if (@($response.workflow_runs).Count -lt 100) { break }\n  }\n} catch { Write-Warning 'Prior tests lookup failed; running tests.'; $tested = 'false' }\n\"tested=$tested\" >> $env:GITHUB_OUTPUT"}
  ]
}
'@ | ConvertFrom-Json
$script:testsGate = "github.repository == 'daplabs/maxed' && github.ref_type == 'tag' && needs.prior-tests.result == 'success' && needs.prior-tests.outputs.tested != 'true'"
$script:packageGate = "!cancelled() && github.repository == 'daplabs/maxed' && github.ref_type == 'tag' && needs.prior-tests.result == 'success' && ((needs.prior-tests.outputs.tested == 'true' && needs.tests.result == 'skipped') || needs.tests.result == 'success')"
function Failures($release, $manual, $codeql) {
    $publish = $release.jobs.publish
    if ((Keys $publish) -cne 'concurrency,defaults,environment,if,needs,permissions,runs-on,steps,timeout-minutes') { 'publish_shape' }
    $publishCommand = @'
$v = $env:RELEASE_TAG.Substring(1)
$c = if ($v.Split('+')[0].Contains('-')) { 'beta' } else { 'stable' }
./scripts/release/Publish-Release.ps1 -Version $v -Channel $c -Repository 'daplabs/maxed' -StageDir release-stage -BackupDir release-backup
'@
    if ($publish.steps[2].run -cne $publishCommand) { 'publish_command' }
    if ($publish.environment -cne 'release-publish' -or $publish.concurrency.group -cne 'release-publish' -or
        $publish.concurrency.'cancel-in-progress' -cne $false -or (@($publish.needs) -join ',') -cne 'package' -or
        $publish.if -cne "!cancelled() && needs.package.result == 'success' && github.repository == 'daplabs/maxed' && github.ref_type == 'tag'") { 'publish_gate' }
    foreach ($leaf in @(Leaves $publish 'publish')) {
        if (HasSecretExpression $leaf.Value) { 'secret_scope' }
        foreach ($expression in @(Expressions $leaf.Value)) {
            if ($expression -match '(?i)token' -and ($leaf.Path -cne 'publish.steps[2].env.GH_TOKEN' -or $leaf.Value -cne '${{ github.token }}')) { 'secret_scope' }
        }
    }
    $download = @($publish.steps | Where-Object { $_.uses -like 'actions/download-artifact@*' })
    if ($download.Count -ne 1 -or $download[0].uses -cne 'actions/download-artifact@9000827ccba6bdab643e8b6fd33ac0654aef8333' -or
        $download[0].with.name -cne 'maxed-${{ github.ref_name }}-windows-x64' -or $download[0].with.path -cne 'release-stage/') { 'publish_artifact' }
    if ($publish.steps[2].env.GH_TOKEN -cne '${{ github.token }}') { 'secret_scope' }
    if ((@($release.on.PSObject.Properties.Name) -join ',') -cne 'push' -or (@($release.on.push.PSObject.Properties.Name) -join ',') -cne 'tags' -or (@($release.on.push.tags) -join ',') -cne 'v*') { 'release_events' }
    if ((Keys $manual.on) -cne 'pull_request,push,schedule,workflow_call,workflow_dispatch' -or
        (Keys $manual.on.pull_request) -cne 'branches,paths-ignore' -or (@($manual.on.pull_request.branches) -join ',') -cne 'main' -or
        @($manual.on.schedule).Count -ne 1 -or
        (Keys $manual.on.schedule[0]) -cne 'cron' -or $manual.on.schedule[0].cron -cne '17 9 * * *' -or
        (Keys $manual.on.workflow_dispatch) -cne '') { 'tests_events' }
    if ((Keys $manual.on.workflow_call) -cne 'inputs' -or (Keys $manual.on.workflow_call.inputs) -cne 'release' -or
        (Keys $manual.on.workflow_call.inputs.release) -cne 'default,description,type' -or
        $manual.on.workflow_call.inputs.release.type -cne 'boolean' -or
        $manual.on.workflow_call.inputs.release.default -cne $false) { 'reusable_inputs' }
    if ((Keys $manual.jobs) -cne 'packaging,rust-lint,rust-test,store-test,ui') { 'tests_job_shape' }
    foreach ($property in $manual.jobs.PSObject.Properties) {
        $job = $property.Value
        if ('if' -in $job.PSObject.Properties.Name) { 'fork_pr_guard' }
        $expected = $script:expectedJobs.($property.Name)
        if ((Keys $job) -cne (Keys $expected) -or
            ($job.defaults | ConvertTo-Json -Compress) -cne ($expected.defaults | ConvertTo-Json -Compress)) { 'tests_job_shape' }
        if ((Keys $job.permissions) -cne 'contents' -or $job.permissions.contents -cne 'read') { 'permissions' }
        if ('secrets' -in $job.PSObject.Properties.Name -or 'environment' -in $job.PSObject.Properties.Name) { 'secret_scope' }
        if ((@($job.steps | ForEach-Object { $_.name }) -join '|') -cne
            (@($expected.steps | ForEach-Object { $_.name }) -join '|')) { 'tests_step_order' }
        for ($i = 0; $i -lt $job.steps.Count; $i++) {
            $step = $job.steps[$i]; $want = $expected.steps[$i]
            if ((Keys $step) -cne (Keys $want) -or $step.uses -cne $want.uses -or
                ($step.with | ConvertTo-Json -Compress) -cne ($want.with | ConvertTo-Json -Compress)) { 'tests_step_shape' }
            if ('continue-on-error' -in $step.PSObject.Properties.Name -or $step.if -cne $want.if) { 'tests_step_policy' }
            if ($step.run -cne $want.run) { 'tests_commands' }
            if ($step.name -cin @('Restore Rust cache','Save Rust cache') -and
                ($step | ConvertTo-Json -Depth 10 -Compress) -cne ($want | ConvertTo-Json -Depth 10 -Compress)) { 'rust_cache' }
        }
    }
    $ignore = 'docs/architecture/**/*.md,docs/development/**/*.md,docs/release/**/*.md,docs/README.md,docs/known-limitations.md'
    foreach ($event in @('push', 'pull_request')) {
        if ((Keys $manual.on.$event) -cne 'branches,paths-ignore' -or
            ($manual.on.$event.branches -join ',') -cne 'main' -or
            ($manual.on.$event.'paths-ignore' -join ',') -cne $ignore) { 'tests_events' }
    }
    if (($release.jobs.'prior-tests' | ConvertTo-Json -Depth 30 -Compress) -cne
        ($script:expectedPrior | ConvertTo-Json -Depth 30 -Compress)) { 'prior_tests' }
    if ($manual.concurrency.group -cne 'tests-${{ github.workflow }}-${{ github.event_name }}-${{ github.event.pull_request.number || github.ref }}' -or
        $manual.concurrency.'cancel-in-progress' -cne '${{ github.event_name == ''pull_request'' }}') { 'tests_concurrency' }
    if ((Keys $release.jobs) -cne 'package,prior-tests,publish,tests' -or
        (@($release.jobs.package.needs) -join ',') -cne 'prior-tests,tests' -or
        (@($release.jobs.tests.needs) -join ',') -cne 'prior-tests' -or
        (Keys $release.jobs.tests) -cne 'if,needs,uses,with' -or
        $release.jobs.tests.uses -cne './.github/workflows/tests.yml' -or
        $release.jobs.tests.if -cne $script:testsGate -or
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
            $isPrior = [object]::ReferenceEquals($job, $release.jobs.'prior-tests')
            $isManual = [object]::ReferenceEquals($w, $manual)
            $isPublish = [object]::ReferenceEquals($w, $release) -and [object]::ReferenceEquals($job, $release.jobs.publish)
            $isCodeql = [object]::ReferenceEquals($w, $codeql) -and [object]::ReferenceEquals($job, $codeql.jobs.analyze)
            $runner = if ($isCodeql) { 'ubuntu-24.04' } elseif ($isPrior) { 'ubuntu-latest' } else { 'windows-2022' }
            if ($job.'runs-on' -isnot [string] -or $job.'runs-on' -cne $runner) { 'job_policy' }
            # Hosted runners need longer for the full test job; every other job keeps 45 minutes.
            $timeout = if ($isManual) { 90 } elseif ($isPrior) { 5 } else { 45 }
            if ($job.'timeout-minutes' -ne $timeout -or $job.env -or $job.secrets -or $job.uses) { 'job_policy' }
            # Only CodeQL uploads security results; only the gated publisher writes contents.
            if ($isCodeql) {
                if ((Keys $job.permissions) -cne 'contents,security-events' -or $job.permissions.contents -cne 'read' -or
                    $job.permissions.'security-events' -cne 'write') { 'permissions' }
            } elseif ($isPublish) {
                if ((Keys $job.permissions) -cne 'contents' -or $job.permissions.contents -cne 'write') { 'permissions' }
            } elseif ($isPrior) {
                if ((Keys $job.permissions) -cne 'actions' -or $job.permissions.actions -cne 'read') { 'permissions' }
            } elseif ($isManual) {
                if ((Keys $job.permissions) -cne 'contents' -or $job.permissions.contents -cne 'read') { 'permissions' }
            } elseif ($job.permissions) { 'permissions' }
            if (-not [object]::ReferenceEquals($job, $release.jobs.package) -and -not $isPublish -and $job.environment) { 'secret_scope' }
            foreach ($step in $job.steps) {
                $pin = if ($isCodeql) { '\A(actions/checkout|github/codeql-action/(init|analyze))@[a-f0-9]{40}\z' } else { '\Aactions/(checkout|setup-node|upload-artifact|download-artifact|cache/restore|cache/save)@[a-f0-9]{40}\z' }
                if ($step.uses -and $step.uses -cnotmatch $pin) { 'action_pin' }
                if ($step.uses -like 'actions/download-artifact@*' -and -not $isPublish) { 'action_pin' }
                if ($step.uses -like 'actions/checkout@*' -and $step.with.'persist-credentials' -cne $false) { 'checkout_credentials' }
                $allowSigning = [object]::ReferenceEquals($w, $release) -and [object]::ReferenceEquals($job, $release.jobs.package) -and $step.name -ceq 'Package and sign'
                StepSecretFailures $step $allowSigning
                if ($allowSigning) {
                    if ($step.env.TAURI_SIGNING_PRIVATE_KEY -cne '${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}' -or $step.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD -cne '${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}' -or $step.run -notlike '*./scripts/build-release.ps1 -Package -Offline*') { 'signing_step' }
                }
            }
        }
    }
    $bindings = @($manual.jobs.'rust-test'.steps | Where-Object name -CEQ 'Bindings freshness')
    if ($bindings.Count -ne 1 -or $bindings[0].run -cne $script:bindingsCommand) { 'bindings_freshness' }
    $memory = @($manual.jobs.packaging.steps | Where-Object name -CEQ 'Offline memory tooling')
    if ($memory.Count -ne 1 -or $memory[0].run -cne $script:memoryCommand) { 'memory_tests' }
    if ($release.jobs.package.environment -cne 'release-signing' -or -not $release.concurrency.group -or $release.concurrency.'cancel-in-progress' -ne $false) { 'release_protection' }
    if (@($release.jobs.package.steps | Where-Object name -CEQ 'Package and sign').Count -ne 1) { 'signing_step' }
    $names = @($release.jobs.package.steps | ForEach-Object { $_.name })
    $validateIndex = [Array]::IndexOf($names, 'Validate tag and signing configuration')
    $packageIndex = [Array]::IndexOf($names, 'Package and sign')
    if ($validateIndex -lt 0 -or $packageIndex -lt 0 -or $validateIndex -ge $packageIndex) { 'validation_order' }
    # Explicit result checks permit successful reuse but never failed tests or lookup jobs.
    if ($release.jobs.package.if -cne $script:packageGate) { 'release_guard' }
    # Builds and updater metadata must use the same public repository.
    $coordinates = @($release.jobs.package.steps | ForEach-Object { [regex]::Matches([string]$_.run, "-Repository '([^']*)'") } | ForEach-Object { $_.Groups[1].Value })
    if ($coordinates.Count -ne 2 -or @($coordinates | Where-Object { $_ -cne $script:updaterRepository }).Count) { 'distribution_repository' }
    $upload = @($release.jobs.package.steps | Where-Object { $_.uses -like 'actions/upload-artifact@*' })
    if ($upload.Count -ne 1 -or $upload[0].with.path -cne 'release-stage/' -or $upload[0].with.'retention-days' -ne 14 -or $upload[0].with.'if-no-files-found' -cne 'error') { 'upload_allowlist' }
    # Signed packages are always built from scratch: no build cache in the signing job.
    if (@($release.jobs.package.steps | Where-Object { $_.uses -like 'actions/cache*' }).Count) { 'signing_cache' }
}
function GuiFailures($gui) {
    if ((Keys $gui) -cne 'concurrency,jobs,name,on,permissions') { 'gui_workflow_shape' }
    if ((Keys $gui.on) -cne 'pull_request,schedule,workflow_dispatch' -or
        @($gui.on.schedule).Count -ne 1 -or
        (Keys $gui.on.schedule[0]) -cne 'cron' -or $gui.on.schedule[0].cron -cne '47 9 * * *' -or
        (Keys $gui.on.pull_request) -cne 'branches' -or (@($gui.on.pull_request.branches) -join ',') -cne 'main' -or
        (Keys $gui.on.workflow_dispatch) -cne '') { 'gui_events' }
    if ((Keys $gui.permissions) -cne 'contents' -or $gui.permissions.contents -cne 'read') { 'gui_permissions' }
    if ((Keys $gui.jobs) -cne 'gui-smoke') { 'gui_job_shape' }
    $job = $gui.jobs.'gui-smoke'
    if ($job.'runs-on' -isnot [string] -or $job.'runs-on' -cne 'windows-2022') { 'gui_runner' }
    if ($job.'timeout-minutes' -ne 45) { 'gui_timeout' }
    if ('if' -in $job.PSObject.Properties.Name) { 'gui_fork_guard' }
    if ((Keys $job) -cne 'defaults,runs-on,steps,timeout-minutes' -or
        (Keys $job.defaults) -cne 'run' -or (Keys $job.defaults.run) -cne 'shell' -or
        $job.defaults.run.shell -cne 'pwsh') { 'gui_job_shape' }
    if ('permissions' -in $job.PSObject.Properties.Name) { 'gui_permissions' }
    foreach ($scope in @($gui, $job) + @($job.steps)) {
        foreach ($field in @('env', 'secrets', 'environment')) {
            if ($field -in $scope.PSObject.Properties.Name) { 'gui_scope' }
        }
    }
    foreach ($leaf in @(Leaves $gui '')) {
        if ((HasSecretExpression $leaf.Value) -or ($leaf.Path + $leaf.Value).IndexOf('TAURI_SIGNING', [StringComparison]::OrdinalIgnoreCase) -ge 0) { 'gui_secrets' }
    }
    if ((Keys $gui.concurrency) -cne 'cancel-in-progress,group' -or
        $gui.concurrency.group -cne 'gui-smoke-${{ github.workflow }}-${{ github.event_name }}-${{ github.event.pull_request.number || github.ref }}' -or
        $gui.concurrency.'cancel-in-progress' -cne '${{ github.event_name == ''pull_request'' }}') { 'gui_concurrency' }
    $names = @($job.steps | ForEach-Object { $_.name })
    $expectedNames = @('', '') + @($script:guiCommands.Keys) + @('')
    if ($names.Count -ne $expectedNames.Count -or ($names -join '|') -cne ($expectedNames -join '|')) { 'gui_step_order' }
    foreach ($step in $job.steps) {
        $expectedIf = switch -CaseSensitive ($step.name) {
            'Ensure GUI smoke stopped' { 'always()' }
        }
        if ($step.uses -like 'actions/upload-artifact@*') { $expectedIf = 'always()' }
        if ('continue-on-error' -in $step.PSObject.Properties.Name -or
            ('if' -in $step.PSObject.Properties.Name -and -not $expectedIf) -or
            ($expectedIf -and $step.if -cne $expectedIf)) { 'gui_step_policy' }
        if ($step.uses) {
            if ($step.uses -cnotin $script:guiPins) { 'gui_action_pin' }
            if ($step.uses -like 'actions/checkout@*') {
                if ((Keys $step) -cne 'uses,with' -or (Keys $step.with) -cne 'persist-credentials' -or
                    $step.with.'persist-credentials' -cne $false) { 'gui_checkout' }
            } elseif ($step.uses -like 'actions/setup-node@*') {
                if ((Keys $step) -cne 'uses,with' -or (Keys $step.with) -cne 'node-version' -or
                    $step.with.'node-version' -cne '22.16.0') { 'gui_node' }
            } elseif ($step.uses -like 'actions/upload-artifact@*') {
                if ((Keys $step) -cne 'if,uses,with' -or (Keys $step.with) -cne 'if-no-files-found,include-hidden-files,name,path,retention-days' -or
                    $step.with.path -cne '${{ runner.temp }}/gui-smoke-${{ github.run_id }}-${{ github.run_attempt }}/' -or
                    $step.with.name -cne 'gui-smoke-${{ github.run_id }}-${{ github.run_attempt }}' -or
                    $step.with.'if-no-files-found' -cne 'warn' -or $step.with.'retention-days' -ne 3 -or
                    $step.with.'include-hidden-files' -cne $false) { 'gui_upload' }
            }
        } else {
            $shape = if ($expectedIf) { 'if,name,run' } else { 'name,run' }
            if ((Keys $step) -cne $shape) { 'gui_step_shape' }
            if ($step.name -cnotin $script:guiCommands.Keys -or $step.run -cne $script:guiCommands[$step.name]) { 'gui_commands' }
        }
    }
    # Exact action positions/counts prevent an upload or checkout replacement
    # from satisfying the unnamed entries in the name/order check.
    if ($job.steps.Count -ne 7 -or $job.steps[0].uses -cne $script:guiPins[0] -or
        $job.steps[1].uses -cne $script:guiPins[1] -or $job.steps[-1].uses -cne $script:guiPins[2]) { 'gui_action_order' }
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
$script:guiPins = @(
    'actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1',
    'actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1',
    'actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9'
)
$script:guiOutputCommand = '$output = Join-Path $env:RUNNER_TEMP ("gui-smoke-$($env:GITHUB_RUN_ID)-$($env:GITHUB_RUN_ATTEMPT)")'
$script:guiCommands = [ordered]@{
    'Restore locked tools' = @'
$channel = [regex]::Match((Get-Content rust-toolchain.toml -Raw), '(?m)^channel\s*=\s*"([^"]+)"\s*$')
if (-not $channel.Success) { throw 'Rust toolchain channel missing.' }
$toolchain = $channel.Groups[1].Value
rustup toolchain install $toolchain --profile minimal --component rustfmt --component clippy
if ($LASTEXITCODE) { throw 'Rust restore failed.' }
npm.cmd ci --prefix app/ui --no-audit --no-fund
if ($LASTEXITCODE) { throw 'UI restore failed.' }
cargo fetch --locked
if ($LASTEXITCODE) { throw 'Cargo restore failed.' }
'@ -replace "`r`n", "`n"
    'Harness unit tests' = @'
node --test tests/gui-smoke/*.test.mjs
if ($LASTEXITCODE) { throw 'CDP harness tests failed.' }
pwsh -NoProfile -File tests/gui-smoke/Invoke-GuiSmoke.test.ps1
if ($LASTEXITCODE) { throw 'GUI launcher tests failed.' }
'@ -replace "`r`n", "`n"
    'Build and run GUI smoke' = $script:guiOutputCommand + "`n" + (@'
pwsh -NoProfile -File tests/gui-smoke/Invoke-GuiSmoke.ps1 -Port 9238 -TimeoutSeconds 60 -OutputDir $output
if ($LASTEXITCODE) { throw 'GUI smoke failed.' }
'@ -replace "`r`n", "`n")
    'Ensure GUI smoke stopped' = $script:guiOutputCommand + "`n" + (@'
pwsh -NoProfile -File tests/gui-smoke/Invoke-GuiSmoke.ps1 -CleanupOnly -OutputDir $output
if ($LASTEXITCODE) { throw 'GUI smoke cleanup failed.' }
'@ -replace "`r`n", "`n")
}
$pin = [regex]::Match((Get-Content "$root/app/src-tauri/src/updater/mod.rs" -Raw), 'pub const REPOSITORY: &str = "([^"]+)";')
if (-not $pin.Success) { throw 'Updater repository constant not found.' }
$script:updaterRepository = $pin.Groups[1].Value
Assert ($script:updaterRepository -ceq 'daplabs/maxed') 'updater pins the public source repository'
$codeqlText = Get-Content "$root/.github/workflows/codeql.yml" -Raw
$c = $codeqlText | ConvertFrom-Json
$releaseText = Get-Content "$root/.github/workflows/release.yml" -Raw
$manualText = Get-Content "$root/.github/workflows/tests.yml" -Raw
$r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json
Assert (@(Failures $r $m $c).Count -eq 0) 'workflow events, runner isolation, release dependency and policy positive control'
$perfStep = @($m.jobs.'rust-lint'.steps | Where-Object name -CEQ 'Perf feature compile')
Assert ($perfStep.Count -eq 1 -and $perfStep[0].run.Contains('cargo clippy --offline --locked -p maxed-app --all-targets --features perf-live')) 'test workflow lints the perf-only driver offline'
$stepNamed = { param($m, $name) @($m.jobs.'rust-lint'.steps | Where-Object name -CEQ $name)[0] }
foreach ($case in @(
    @{ want = 'permissions'; mutate = { param($r,$m) $r.permissions.contents = 'write' } },
    @{ want = 'publish_shape'; mutate = { param($r,$m) $r.jobs.publish | Add-Member container 'image' } },
    @{ want = 'publish_shape'; mutate = { param($r,$m) $r.jobs.publish | Add-Member services @{} } },
    @{ want = 'publish_shape'; mutate = { param($r,$m) $r.jobs.publish | Add-Member outputs @{} } },
    @{ want = 'publish_command'; mutate = { param($r,$m) $r.jobs.publish.steps[2].run += '; exit 0' } },
    @{ want = 'upload_allowlist'; mutate = { param($r,$m) $r.jobs.package.steps[-1].with.'retention-days' = 3 } },
    @{ want = 'permissions'; mutate = { param($r,$m) $r.jobs.package | Add-Member permissions @{ contents = 'write' } } },
    @{ want = 'permissions'; mutate = { param($r,$m) $r.jobs.publish.permissions | Add-Member actions 'write' } },
    @{ want = 'publish_gate'; mutate = { param($r,$m) $r.jobs.publish.PSObject.Properties.Remove('environment') } },
    @{ want = 'publish_gate'; mutate = { param($r,$m) $r.jobs.publish.environment = 'release-signing' } },
    @{ want = 'publish_gate'; mutate = { param($r,$m) $r.jobs.publish.needs = @('tests') } },
    @{ want = 'publish_gate'; mutate = { param($r,$m) $r.jobs.publish.if = 'always()' } },
    @{ want = 'publish_gate'; mutate = { param($r,$m) $r.jobs.publish.if = $r.jobs.publish.if.Replace("needs.package.result == 'success' && ", '') } },
    @{ want = 'publish_gate'; mutate = { param($r,$m) $r.jobs.publish.if = $r.jobs.publish.if.Replace('!cancelled()', 'always()') } },
    @{ want = 'publish_gate'; mutate = { param($r,$m) $r.jobs.publish.concurrency.group = 'per-tag' } },
    @{ want = 'publish_artifact'; mutate = { param($r,$m) $r.jobs.publish.steps[1].with.name = 'other-artifact' } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) $r.jobs.publish.steps[2].env.GH_TOKEN = '${{ secrets.OTHER }}' } },
    @{ want = 'job_policy'; mutate = { param($r,$m) $m.jobs.'rust-lint'.'runs-on' = @('self-hosted', 'windows', 'nuc', 'build') } },
    @{ want = 'job_policy'; mutate = { param($r,$m) $m.jobs.'rust-lint'.'runs-on' = 'windows-latest' } },
    @{ want = 'job_policy'; mutate = { param($r,$m) $m.jobs.'rust-lint'.'runs-on' = @('self-hosted', 'windows', 'build') } },
    @{ want = 'job_policy'; mutate = { param($r,$m) $m.jobs.'rust-lint'.'runs-on' = @('self-hosted', 'windows', 'nuc') } },
    @{ want = 'job_policy'; mutate = { param($r,$m) $m.jobs.'rust-lint'.'runs-on' = @('self-hosted', 'windows', 'build', 'nuc') } },
    @{ want = 'job_policy'; mutate = { param($r,$m) $m.jobs.'rust-lint'.'runs-on' = 'self-hosted,windows,nuc,build' } },
    @{ want = 'job_policy'; mutate = { param($r,$m) $m.jobs.'rust-lint'.'timeout-minutes' = 60 } },
    @{ want = 'job_policy'; mutate = { param($r,$m) $m.jobs.'rust-lint'.'timeout-minutes' = 45 } },
    @{ want = 'job_policy'; mutate = { param($r,$m) $r.jobs.package.'timeout-minutes' = 90 } },
    @{ want = 'job_policy'; mutate = { param($r,$m) $r.jobs.package.'runs-on' = @('self-hosted', 'windows', 'nuc', 'build') } },
    @{ want = 'tests_step_policy'; mutate = { param($r,$m) (& $stepNamed $m 'Restore Rust cache').PSObject.Properties.Remove('if') } },
    @{ want = 'tests_step_policy'; mutate = { param($r,$m) (& $stepNamed $m 'Prune Rust cache').if = "github.ref == 'refs/heads/main'" } },
    @{ want = 'tests_step_policy'; mutate = { param($r,$m) (& $stepNamed $m 'Save Rust cache').if = "github.ref == 'refs/heads/main'" } },
    @{ want = 'rust_cache'; mutate = { param($r,$m) (& $stepNamed $m 'Restore Rust cache').with.key = 'rust-${{ runner.os }}' } },
    @{ want = 'rust_cache'; mutate = { param($r,$m) (& $stepNamed $m 'Save Rust cache').with.key = 'rust-${{ github.sha }}' } },
    @{ want = 'rust_cache'; mutate = { param($r,$m) (& $stepNamed $m 'Save Rust cache').with.path += "`n~/.cargo/bin" } },
    @{ want = 'rust_cache'; mutate = { param($r,$m) (& $stepNamed $m 'Restore Rust cache').with | Add-Member 'restore-keys' 'rust-' } },
    @{ want = 'rust_cache'; mutate = { param($r,$m) $s = & $stepNamed $m 'Restore Rust cache'; $s.uses = $s.uses.Replace('cache/restore@', 'cache/save@') } },
    @{ want = 'action_pin'; mutate = { param($r,$m) (& $stepNamed $m 'Restore Rust cache').uses = 'actions/cache/restore@v6' } },
    @{ want = 'tests_step_policy'; mutate = { param($r,$m) (& $stepNamed $m 'Save Rust cache').if = "runner.environment == 'github-hosted' && github.ref == 'refs/heads/main'" } },
    @{ want = 'tests_step_policy'; mutate = { param($r,$m) (& $stepNamed $m 'Save Rust cache').if = "runner.environment == 'github-hosted' && (github.ref == 'refs/heads/main' || github.event_name == 'pull_request')" } },
    @{ want = 'tests_step_policy'; mutate = { param($r,$m) (& $stepNamed $m 'Save Rust cache').if = 'always()' } },
    @{ want = 'tests_step_policy'; mutate = { param($r,$m) (& $stepNamed $m 'Save Rust cache').PSObject.Properties.Remove('if') } },
    @{ want = 'tests_step_policy'; mutate = { param($r,$m) (& $stepNamed $m 'Prune Rust cache').if = "startsWith(github.ref, 'refs/tags/')" } },
    @{ want = 'tests_commands'; mutate = { param($r,$m) (& $stepNamed $m 'Prune Rust cache').run += "`nRemove-Item -Recurse -Force target" } },
    @{ want = 'tests_step_order'; mutate = { param($r,$m) $m.jobs.'rust-lint'.steps = @($m.jobs.'rust-lint'.steps | Where-Object name -CNE 'Restore Rust cache') } },
    @{ want = 'tests_step_order'; mutate = { param($r,$m) $m.jobs.'rust-lint'.steps = @($m.jobs.'rust-lint'.steps | Where-Object name -CNE 'Save Rust cache') } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) (& $stepNamed $m 'Save Rust cache').with.key += '-${{ secrets.OTHER }}' } },
    @{ want = 'signing_cache'; mutate ={ param($r,$m) $r.jobs.package.steps = @($r.jobs.package.steps[0..1]) + @(& $stepNamed $m 'Restore Rust cache') + @($r.jobs.package.steps[2..($r.jobs.package.steps.Count - 1)]) } },
    @{ want = 'release_events'; mutate = { param($r,$m) $r.on | Add-Member schedule @() } },
    @{ want = 'tests_events'; mutate = { param($r,$m) $m.on.push.branches = @('*') } },
    @{ want = 'tests_events'; mutate = { param($r,$m) $m.on.PSObject.Properties.Remove('schedule') } },
    @{ want = 'tests_events'; mutate = { param($r,$m) $m.on.schedule[0].cron = '47 9 * * *' } },
    @{ want = 'tests_events'; mutate = { param($r,$m) $m.on.PSObject.Properties.Remove('pull_request') } },
    @{ want = 'tests_events'; mutate = { param($r,$m) $m.on | Add-Member pull_request_target @{} } },
    @{ want = 'fork_pr_guard'; mutate = { param($r,$m) $m.jobs.'rust-lint' | Add-Member if "github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository" } },
    @{ want = 'fork_pr_guard'; mutate = { param($r,$m) $m.jobs.'rust-lint' | Add-Member if 'true' } },
    @{ want = 'fork_pr_guard'; mutate = { param($r,$m) $m.jobs.'rust-lint' | Add-Member if "github.event_name == 'workflow_dispatch' || format('{0}', github.event.repository.private) == 'false' || inputs.release == true" } },
    @{ want = 'fork_pr_guard'; mutate = { param($r,$m) $m.jobs.'rust-lint' | Add-Member if "github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name != github.repository" } },
    @{ want = 'tests_job_shape'; mutate = { param($r,$m) $m.jobs.'rust-lint' | Add-Member 'continue-on-error' $true } },
    @{ want = 'tests_job_shape'; mutate = { param($r,$m) $m.jobs.'rust-lint'.defaults.run.shell = 'cmd' } },
    @{ want = 'tests_job_shape'; mutate = { param($r,$m) $m.jobs.'rust-lint'.defaults.run | Add-Member 'working-directory' 'other' } },
    @{ want = 'tests_step_order'; mutate = { param($r,$m) $s = $m.jobs.'rust-lint'.steps; $old = $s[3]; $s[3] = $s[6]; $s[6] = $old } },
    @{ want = 'tests_step_order'; mutate = { param($r,$m) $m.jobs.'rust-lint'.steps = @($m.jobs.'rust-lint'.steps | Where-Object name -CNE 'Rust lint') } },
    @{ want = 'tests_step_order'; mutate = { param($r,$m) $m.jobs.'rust-lint'.steps += $m.jobs.'rust-lint'.steps[-1] } },
    @{ want = 'tests_step_shape'; mutate = { param($r,$m) (& $stepNamed $m 'Restore locked tools') | Add-Member shell 'cmd' } },
    @{ want = 'tests_step_shape'; mutate = { param($r,$m) $m.jobs.'rust-lint'.steps[0].uses = $m.jobs.'rust-lint'.steps[1].uses } },
    @{ want = 'tests_step_shape'; mutate = { param($r,$m) $m.jobs.'rust-lint'.steps[1].with.'node-version' = '20' } },
    @{ want = 'reusable_inputs'; mutate = { param($r,$m) $m.on.workflow_call.inputs.release.default = $true } },
    @{ want = 'tests_concurrency'; mutate = { param($r,$m) $m.concurrency.'cancel-in-progress' = $false } },
    @{ want = 'release_tests'; mutate = { param($r,$m) $r.jobs.package.PSObject.Properties.Remove('needs') } },
    @{ want = 'release_tests'; mutate = { param($r,$m) $r.jobs.tests.with.release = $false } },
    @{ want = 'release_tests'; mutate = { param($r,$m) $r.jobs.tests | Add-Member secrets 'inherit' } },
    @{ want = 'release_tests'; mutate = { param($r,$m) $r.jobs.tests.uses = './.github/workflows/other.yml' } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = "github.repository == 'daplabs/maxed-rotmg' && github.ref_type == 'tag'" } },
    @{ want = 'release_tests'; mutate = { param($r,$m) $r.jobs.tests.if = "github.repository == 'DaAwesomePwner/maxed' && github.ref_type == 'tag'" } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = 'always()' } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = $r.jobs.package.if.Replace('!cancelled()', 'always()') } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = 'failure() && ' + $r.jobs.package.if } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = '!cancelled() && ' + $r.jobs.package.if } },
    @{ want = 'bindings_freshness'; mutate = { param($r,$m) ($m.jobs.'rust-test'.steps | Where-Object name -CEQ 'Bindings freshness').run = 'git diff --exit-code' } },
    @{ want = 'tests_step_policy'; mutate = { param($r,$m) ($m.jobs.'rust-test'.steps | Where-Object name -CEQ 'Bindings freshness') | Add-Member 'continue-on-error' $true } },
    @{ want = 'memory_tests'; mutate = { param($r,$m) ($m.jobs.packaging.steps | Where-Object name -CEQ 'Offline memory tooling').run = 'echo skipped' } },
    @{ want = 'codeql_private_gate'; mutate = { param($r,$m,$c) $c.jobs.analyze.if = 'true' } },
    @{ want = 'codeql_private_gate'; mutate = { param($r,$m,$c) $c.jobs.analyze.if = 'github.event.repository.private == false' } },
    @{ want = 'codeql_events'; mutate = { param($r,$m,$c) $c.on | Add-Member pull_request_target @{} } },
    @{ want = 'codeql_events'; mutate = { param($r,$m,$c) $c.on.PSObject.Properties.Remove('push') } },
    @{ want = 'codeql_events'; mutate = { param($r,$m,$c) $c.on.push.branches = @('*') } },
    @{ want = 'codeql_languages'; mutate = { param($r,$m,$c) $c.jobs.analyze.strategy.matrix.language = @('javascript-typescript') } },
    @{ want = 'codeql_steps'; mutate = { param($r,$m,$c) $c.jobs.analyze.steps[1].with.'build-mode' = 'autobuild' } },
    @{ want = 'permissions'; mutate = { param($r,$m,$c) $c.jobs.analyze.permissions.contents = 'write' } },
    @{ want = 'permissions'; mutate = { param($r,$m,$c) $c.permissions | Add-Member 'security-events' 'write' } },
    @{ want = 'permissions'; mutate = { param($r,$m) $m.jobs.'rust-lint'.permissions.contents = 'write' } },
    @{ want = 'action_pin'; mutate = { param($r,$m,$c) $c.jobs.analyze.steps[1].uses = 'github/codeql-action/init@v4' } },
    @{ want = 'checkout_credentials'; mutate = { param($r,$m,$c) $c.jobs.analyze.steps[0].with.'persist-credentials' = $true } },
    @{ want = 'permissions'; mutate = { param($r,$m) $m.permissions.contents = 'write' } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) $m.jobs.'rust-lint'.steps[0] | Add-Member env @{ TAURI_SIGNING_PRIVATE_KEY = 'invented' } } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) $r.jobs.package.steps[0] | Add-Member env @{ KEY = '${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}' } } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) $m.jobs.'rust-lint'.steps[0] | Add-Member env @{ KEY = '${{ secrets[''OTHER''] }}' } } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) (& $stepNamed $m 'Restore locked tools').run += '${{ toJSON(secrets) }}' } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) $m.jobs.'rust-lint'.steps[0].with | Add-Member token '${{ secrets[''OTHER''] }}' } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) ($r.jobs.package.steps | Where-Object name -EQ 'Package and sign').env | Add-Member EXTRA '${{ secrets.OTHER }}' } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) ($r.jobs.package.steps | Where-Object name -EQ 'Package and sign').run += '${{ toJSON(secrets) }}' } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) ($r.jobs.package.steps | Where-Object name -EQ 'Package and sign') | Add-Member with @{ token = '${{ secrets[''OTHER''] }}' } } },
    @{ want = 'secret_scope'; mutate = { param($r,$m) ($r.jobs.package.steps | Where-Object name -EQ 'Package and sign').env.TAURI_SIGNING_PRIVATE_KEY += '${{ secrets.OTHER }}' } },
    @{ want = 'checkout_credentials'; mutate = { param($r,$m) $r.jobs.package.steps[0].with.'persist-credentials' = $true } },
    @{ want = 'checkout_credentials'; mutate = { param($r,$m) $m.jobs.'rust-lint'.steps[0].with.PSObject.Properties.Remove('persist-credentials') } },
    @{ want = 'validation_order'; mutate = { param($r,$m) $s = $r.jobs.package.steps; $old = $s[3]; $s[3] = $s[4]; $s[4] = $old } },
    @{ want = 'validation_order'; mutate = { param($r,$m) $r.jobs.package.steps[3].name = 'Missing validation' } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = "github.ref_type == 'tag'" } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = "github.repository == 'DaAwesomePwner/maxed' && github.ref_type == 'tag'" } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = "github.repository == 'daplabs/maxed'" } },
    @{ want = 'release_guard'; mutate = { param($r,$m) $r.jobs.package.if = "github.repository == 'daplabs/maxed' || github.ref_type == 'tag'" } },
    @{ want = 'distribution_repository'; mutate = { param($r,$m) $s = $r.jobs.package.steps | Where-Object name -EQ 'Verify and stage allowlist'; $s.run = $s.run.Replace("-Repository '$script:updaterRepository'", "-Repository 'DaAwesomePwner/maxed'") } },
    @{ want = 'distribution_repository'; mutate = { param($r,$m) $s = $r.jobs.package.steps | Where-Object name -EQ 'Verify and stage allowlist'; $s.run = $s.run.Replace("-Repository '$script:updaterRepository'", "-Repository 'daplabs/maxed-rotmg'") } },
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
foreach ($name in @('rust-lint','rust-test','store-test','ui','packaging')) {
    foreach ($change in @('permission','secret','drop')) {
        $r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json; $c = $codeqlText | ConvertFrom-Json
        switch ($change) {
            permission { $m.jobs.$name.permissions | Add-Member actions 'write' }
            secret { $m.jobs.$name | Add-Member secrets 'inherit' }
            drop { $m.jobs.$name.steps = @($m.jobs.$name.steps | Where-Object { $_.name -notin @('Rust lint','Rust tests','Store tests','UI','Packaging and collectors') }) }
        }
        $want = switch ($change) { permission { 'permissions' }; secret { 'secret_scope' }; drop { 'tests_step_order' } }
        Assert ($want -in @(Failures $r $m $c)) "reject $name $change"
    }
}
foreach ($change in @('write','extra','failed')) {
    $r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json; $c = $codeqlText | ConvertFrom-Json
    switch ($change) {
        write { $r.jobs.'prior-tests'.permissions.actions = 'write' }
        extra { $r.jobs.'prior-tests'.permissions | Add-Member contents 'read' }
        failed { $r.jobs.package.if = $r.jobs.package.if.Replace("needs.tests.result == 'success'", "needs.tests.result == 'failure'") }
    }
    $want = if ($change -eq 'failed') { 'release_guard' } else { 'permissions' }
    Assert ($want -in @(Failures $r $m $c)) "reject prior tests $change"
}
# Exercise the generic rule on every job, including the reusable release caller.
foreach ($target in @('release.tests', 'release.prior-tests', 'release.package', 'release.publish', 'tests.rust-lint', 'tests.rust-test', 'tests.store-test', 'tests.ui', 'tests.packaging', 'codeql.analyze')) {
    $r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json; $c = $codeqlText | ConvertFrom-Json
    $workflow, $jobName = $target.Split('.')
    $job = @{ release = $r; tests = $m; codeql = $c }[$workflow].jobs.$jobName
    $job | Add-Member 'continue-on-error' $true
    Assert ('job_policy' -in @(Failures $r $m $c)) "reject $target job continue-on-error"
}
$baseline = $manualText | ConvertFrom-Json
foreach ($jobName in @('rust-lint','rust-test','store-test','ui','packaging')) {
for ($i = 0; $i -lt $baseline.jobs.$jobName.steps.Count; $i++) {
    foreach ($field in @('continue-on-error', 'if')) {
        $r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json; $c = $codeqlText | ConvertFrom-Json
        $value = if ($field -ceq 'if') { 'false' } else { $true }
        # Cache management already carries pinned runner/ref conditions; replace them.
        if ($field -cin $m.jobs.$jobName.steps[$i].PSObject.Properties.Name) { $m.jobs.$jobName.steps[$i].$field = $value }
        else { $m.jobs.$jobName.steps[$i] | Add-Member $field $value }
        Assert ('tests_step_policy' -in @(Failures $r $m $c)) "reject tests step $i $field"
    }
    if ($i -ge 2 -and $baseline.jobs.$jobName.steps[$i].run) {
        foreach ($suffix in @('; exit 0', ' || true')) {
            $r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json; $c = $codeqlText | ConvertFrom-Json
            $m.jobs.$jobName.steps[$i].run += $suffix
            Assert ('tests_commands' -in @(Failures $r $m $c)) "reject $($m.jobs.$jobName.steps[$i].name) appended $suffix"
        }
    }
}
}
# Each exporter output scope must be present in both the tracked and untracked checks.
foreach ($path in @('app/ui/src/lib/bindings', 'app/ui/tests/fixtures', 'crates/maxed-types/tests/fixtures/p1-demo.json')) {
    foreach ($line in @(0, 2)) {
        $r = $releaseText | ConvertFrom-Json; $m = $manualText | ConvertFrom-Json; $c = $codeqlText | ConvertFrom-Json
        $step = $m.jobs.'rust-test'.steps | Where-Object name -CEQ 'Bindings freshness'
        $lines = $step.run.Split("`n")
        $lines[$line] = $lines[$line].Replace(' ' + $path, '')
        $step.run = $lines -join "`n"
        Assert ('bindings_freshness' -in @(Failures $r $m $c)) "reject missing freshness path $path on line $line"
    }
}
$guiText = Get-Content "$root/.github/workflows/gui-smoke.yml" -Raw
$g = $guiText | ConvertFrom-Json
Assert (@(GuiFailures $g).Count -eq 0) 'GUI workflow positive control: isolated runner, unit tests before smoke, scoped cleanup/upload'
foreach ($case in @(
    @{ want = 'gui_runner'; mutate = { param($g) $g.jobs.'gui-smoke'.'runs-on' = 'windows-latest' } },
    @{ want = 'gui_runner'; mutate = { param($g) $g.jobs.'gui-smoke'.'runs-on' = @('self-hosted','windows','nuc','gui') } },
    @{ want = 'gui_runner'; mutate = { param($g) $g.jobs.'gui-smoke'.'runs-on' = @('self-hosted','windows','gui') } },
    @{ want = 'gui_runner'; mutate = { param($g) $g.jobs.'gui-smoke'.'runs-on' = @('self-hosted','windows','nuc') } },
    @{ want = 'gui_runner'; mutate = { param($g) $g.jobs.'gui-smoke'.PSObject.Properties.Remove('runs-on') } },
    @{ want = 'gui_timeout'; mutate = { param($g) $g.jobs.'gui-smoke'.'timeout-minutes' = 60 } },
    @{ want = 'gui_fork_guard'; mutate = { param($g) $g.jobs.'gui-smoke' | Add-Member if "github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository" } },
    @{ want = 'gui_fork_guard'; mutate = { param($g) $g.jobs.'gui-smoke' | Add-Member if 'true' } },
    @{ want = 'gui_permissions'; mutate = { param($g) $g.permissions.contents = 'write' } },
    @{ want = 'gui_permissions'; mutate = { param($g) $g.jobs.'gui-smoke' | Add-Member permissions @{ contents = 'write' } } },
    @{ want = 'gui_scope'; mutate = { param($g) $g | Add-Member env @{} } },
    @{ want = 'gui_scope'; mutate = { param($g) $g.jobs.'gui-smoke' | Add-Member secrets $null } },
    @{ want = 'gui_scope'; mutate = { param($g) $g.jobs.'gui-smoke' | Add-Member environment 'release-signing' } },
    @{ want = 'gui_scope'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[4] | Add-Member env @{} } },
    @{ want = 'gui_secrets'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[0].with | Add-Member token '${{ secrets.OTHER }}' } },
    @{ want = 'gui_secrets'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[2].run += '${{ toJSON(secrets) }}' } },
    @{ want = 'gui_events'; mutate = { param($g) $g.on | Add-Member pull_request_target @{} } },
    @{ want = 'gui_events'; mutate = { param($g) $g.on | Add-Member workflow_call @{} } },
    @{ want = 'gui_events'; mutate = { param($g) $g.on | Add-Member push @{ branches = @('main') } } },
    @{ want = 'gui_events'; mutate = { param($g) $g.on.PSObject.Properties.Remove('schedule') } },
    @{ want = 'gui_events'; mutate = { param($g) $g.on.schedule[0].cron = '17 9 * * *' } },
    @{ want = 'gui_events'; mutate = { param($g) $g.on.PSObject.Properties.Remove('pull_request') } },
    @{ want = 'gui_concurrency'; mutate = { param($g) $g.concurrency.'cancel-in-progress' = $false } },
    @{ want = 'gui_action_pin'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[0].uses = 'actions/checkout@v4' } },
    @{ want = 'gui_action_pin'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[0].uses = 'actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' } },
    @{ want = 'gui_action_pin'; mutate = { param($g) $g.jobs.'gui-smoke'.steps += @{ uses = 'actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9' } } },
    @{ want = 'gui_checkout'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[0].with.'persist-credentials' = $true } },
    @{ want = 'gui_node'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[1].with.'node-version' = '20' } },
    @{ want = 'gui_upload'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[-1].with.path = '${{ github.workspace }}/' } },
    @{ want = 'gui_upload'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[-1].with.path = '${{ runner.temp }}/**' } },
    @{ want = 'gui_upload'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[-1].with.'include-hidden-files' = $true } },
    @{ want = 'gui_upload'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[-1].with.'retention-days' = 30 } },
    @{ want = 'gui_step_order'; mutate = { param($g) $s = $g.jobs.'gui-smoke'.steps; $old = $s[4]; $s[4] = $s[5]; $s[5] = $old } },
    @{ want = 'gui_step_order'; mutate = { param($g) $g.jobs.'gui-smoke'.steps = @($g.jobs.'gui-smoke'.steps | Where-Object name -CNE 'Build and run GUI smoke') } },
    @{ want = 'gui_step_order'; mutate = { param($g) $g.jobs.'gui-smoke'.steps = @($g.jobs.'gui-smoke'.steps | Where-Object name -CNE 'Harness unit tests') } },
    @{ want = 'gui_commands'; mutate = { param($g) $s = $g.jobs.'gui-smoke'.steps[2]; $s.run = $s.run.Replace('cargo fetch --locked', 'cargo fetch') } },
    @{ want = 'gui_commands'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[3].run = 'echo skipped' } },
    @{ want = 'gui_commands'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[4].run += '; exit 0' } },
    @{ want = 'gui_commands'; mutate = { param($g) $g.jobs.'gui-smoke'.steps[5].run = 'taskkill /IM maxed.exe /F' } },
    @{ want = 'gui_job_shape'; mutate = { param($g) $g.jobs.'gui-smoke' | Add-Member 'continue-on-error' $true } }
)) {
    $g = $guiText | ConvertFrom-Json
    & $case.mutate $g
    Assert ($case.want -in @(GuiFailures $g)) "reject GUI mutation $($case.want)"
}
for ($i = 0; $i -lt 7; $i++) {
    foreach ($field in @('continue-on-error', 'if')) {
        $g = $guiText | ConvertFrom-Json
        $value = if ($field -ceq 'if') { 'false' } else { $true }
        if ($field -in $g.jobs.'gui-smoke'.steps[$i].PSObject.Properties.Name) { $g.jobs.'gui-smoke'.steps[$i].$field = $value }
        else { $g.jobs.'gui-smoke'.steps[$i] | Add-Member $field $value }
        Assert ('gui_step_policy' -in @(GuiFailures $g)) "reject GUI step $i $field"
    }
}
foreach ($file in Get-ChildItem "$root/.github/workflows" -File) {
    Assert ($file.Name -cin @('release.yml', 'tests.yml', 'codeql.yml', 'gui-smoke.yml')) 'every workflow has an explicit policy'
    $w = Get-Content -LiteralPath $file.FullName -Raw | ConvertFrom-Json
    if ($file.Name -cne 'release.yml') {
        $forbidden = @(Leaves $w '' | Where-Object {
            (HasSecretExpression $_.Value) -or ($_.Path + $_.Value).IndexOf('TAURI_SIGNING', [StringComparison]::OrdinalIgnoreCase) -ge 0
        })
        Assert ($forbidden.Count -eq 0) 'all non-release workflows contain no secret references'
    }
}
Write-Output "Workflow policy: $script:checks checks passed. Action references require full 40-hex pins; tags and placeholders are rejected."
