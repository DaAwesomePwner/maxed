[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$configPath = Join-Path $repoRoot 'app/src-tauri/tauri.conf.json'
$buildPath = Join-Path $repoRoot 'scripts/build-release.ps1'
$script:checks = 0

function Get-ConfigFailures($Config) {
    if ($Config.build.beforeBuildCommand -cne 'npm --prefix ui run build') { 'frontend_build_directory' }
    if ($Config.build.beforeDevCommand -cne 'npm --prefix ui run dev') { 'frontend_dev_directory' }
    if ($Config.bundle.active -cne $true) { 'bundle_active' }
    if ($null -eq $Config.bundle.targets -or @($Config.bundle.targets).Count -ne 1 -or $Config.bundle.targets[0] -cne 'nsis') { 'nsis_only' }
    if ($Config.bundle.createUpdaterArtifacts -isnot [bool] -or $Config.bundle.createUpdaterArtifacts -cne $true) { 'production_updater_artifacts' }
    try {
        $key = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($Config.plugins.updater.pubkey))
        $lines = $key.TrimEnd("`r", "`n").Split("`n")
        if (-not $key.StartsWith('untrusted comment: minisign public key', [StringComparison]::Ordinal) -or
            $lines.Count -ne 2 -or [Convert]::FromBase64String($lines[1].Trim()).Length -ne 42) { 'production_updater_pubkey' }
    } catch { 'production_updater_pubkey' }
    if ($Config.bundle.windows.nsis.installMode -cne 'currentUser') { 'current_user' }
    if ($Config.bundle.windows.webviewInstallMode.type -cne 'downloadBootstrapper' -or
        $Config.bundle.windows.webviewInstallMode.silent -cne $false) { 'visible_webview_bootstrap' }
    if ($Config.bundle.windows.nsis.installerHooks -cne 'installer/npcap-guidance.nsh') { 'npcap_hook' }
    if ($Config.bundle.windows.nsis.template) { 'upstream_template' }
    foreach ($field in @('certificateThumbprint', 'signCommand', 'timestampUrl')) {
        if ($Config.bundle.windows.$field) { 'no_authenticode' }
    }
    if ($Config.bundle.externalBin) { 'no_external_binaries' }
    $expected = @{
        '../../tests/fixtures/scrubbed/maxed-demo/story.pcap' = 'demo/story.pcap'
        '../../tests/fixtures/scrubbed/maxed-demo/catalog.json' = 'demo/catalog.json'
        '../../LICENSE' = 'LICENSE'
        '../../NOTICE.md' = 'NOTICE.md'
        '../../THIRD-PARTY-NOTICES.md' = 'THIRD-PARTY-NOTICES.md'
    }
    $resources = @($Config.bundle.resources.PSObject.Properties)
    if ($resources.Count -ne 5) { 'exact_resource_mappings' }
    foreach ($resource in $resources) {
        if (-not $expected.ContainsKey($resource.Name) -or $resource.Value -cne $expected[$resource.Name]) {
            'exact_resource_mappings'
        }
        if ($resource.Name -match '[*?\[\]]' -or [string]$resource.Value -match '[*?\[\]]') { 'no_wildcard_resources' }
    }
    if ($Config.app.security.csp -cne "default-src 'self'; connect-src ipc: http://ipc.localhost; style-src 'self' 'unsafe-inline'; img-src 'self' data: http://maxed-sprite.localhost") { 'unchanged_csp' }
    if ($Config.app.security.devCsp -cne "default-src 'self'; connect-src ipc: http://ipc.localhost http://127.0.0.1:1420 ws://127.0.0.1:1420; style-src 'self' 'unsafe-inline'; img-src 'self' data: http://maxed-sprite.localhost") { 'unchanged_dev_csp' }
    if (@($Config.app.windows).Count -ne 1 -or $Config.app.windows[0].create -cne $false) { 'native_window_owner' }
}

function Get-BuildFailures([string]$Source) {
    $tokens = $null; $parseErrors = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput($Source, [ref]$tokens, [ref]$parseErrors)
    if ($parseErrors.Count) { 'build_parses'; return }
    $parameters = @($ast.ParamBlock.Parameters | ForEach-Object { $_.Name.VariablePath.UserPath })
    if ('Package' -notin $parameters -or 'Offline' -notin $parameters) { 'build_switches' }
    # Inspect argument-array syntax, not a matching comment or unused text elsewhere.
    foreach ($name in @('cargoArgs', 'tauriArgs')) {
        $assignment = @($ast.FindAll({ param($node)
            $node -is [Management.Automation.Language.AssignmentStatementAst] -and
            $node.Left -is [Management.Automation.Language.VariableExpressionAst] -and
            $node.Left.VariablePath.UserPath -eq $name -and $node.Operator -eq 'Equals'
        }, $true))
        $values = @($assignment | ForEach-Object { $_.Right.FindAll({ param($node)
            $node -is [Management.Automation.Language.StringConstantExpressionAst]
        }, $true) } | ForEach-Object { $_.Value })
        $featureIndex = [Array]::IndexOf($values, '--features')
        if ($featureIndex -lt 0 -or $featureIndex + 1 -ge $values.Count -or
            $values[$featureIndex + 1] -cne 'maxed-app/custom-protocol') { "custom_protocol_$name" }
        if ($name -eq 'tauriArgs') {
            $bundleIndex = [Array]::IndexOf($values, '--bundles')
            if ('build' -notin $values -or $bundleIndex -lt 0 -or $bundleIndex + 1 -ge $values.Count -or
                $values[$bundleIndex + 1] -cne 'nsis') { 'package_nsis_command' }
        }
    }
}

function Assert-Check([bool]$Condition, [string]$Name) {
    $script:checks++
    if (-not $Condition) { throw "FAIL: $Name" }
    Write-Output "PASS: $Name"
}

function Get-HookFailures([string]$Source) {
    if ($Source -match '(?i)\bExecShell\b') { 'no_shell_launch' }
    if ($Source -match '(?i)MUI_FINISHPAGE_LINK') { 'no_browser_link' }
    if (-not $Source.Contains("Restrict Npcap driver's access to Administrators only")) { 'exact_npcap_option' }
    if (-not $Source.Contains('https://npcap.com/#download')) { 'official_npcap_address' }
    if ($Source -notmatch 'Re-run the Npcap installer and untick that option') { 'restricted_npcap_repair' }
}

$configText = Get-Content -LiteralPath $configPath -Raw
$buildText = Get-Content -LiteralPath $buildPath -Raw
$failures = @(Get-ConfigFailures ($configText | ConvertFrom-Json)) + @(Get-BuildFailures $buildText)
if ($failures.Count) { throw "Configuration contract failed: $($failures -join ', ')" }
Assert-Check $true 'release_requires_custom_protocol_and_resources / installer_config_is_current_user'

# Each mutation starts from a fresh serialized copy of the actual configuration.
$mutations = @(
    @{ Name = 'frontend_build_directory'; Change = { param($c) $c.build.beforeBuildCommand = 'npm --prefix ../ui run build' } },
    @{ Name = 'frontend_dev_directory'; Change = { param($c) $c.build.beforeDevCommand = 'npm --prefix ../ui run dev' } },
    @{ Name = 'bundle_active'; Change = { param($c) $c.bundle.active = $false } },
    @{ Name = 'nsis_only'; Change = { param($c) $c.bundle.targets = @('nsis', 'msi') } },
    @{ Name = 'nsis_only'; Change = { param($c) $c.bundle.targets = @() } },
    @{ Name = 'production_updater_artifacts'; Change = { param($c) $c.bundle.createUpdaterArtifacts = $false } },
    @{ Name = 'production_updater_artifacts'; Change = { param($c) $c.bundle.PSObject.Properties.Remove('createUpdaterArtifacts') } },
    @{ Name = 'production_updater_artifacts'; Change = { param($c) $c.bundle.createUpdaterArtifacts = 'true' } },
    @{ Name = 'production_updater_pubkey'; Change = { param($c) $c.plugins.updater.pubkey = '' } },
    @{ Name = 'production_updater_pubkey'; Change = { param($c) $c.plugins.updater.pubkey = 'garbage' } },
    @{ Name = 'production_updater_pubkey'; Change = { param($c) $c.plugins.updater.pubkey = 'dGVzdA==' } },
    @{ Name = 'production_updater_pubkey'; Change = { param($c) $c.plugins.updater.pubkey = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes('untrusted comment: minisign public key')) } },
    @{ Name = 'current_user'; Change = { param($c) $c.bundle.windows.nsis.installMode = 'perMachine' } },
    @{ Name = 'current_user'; Change = { param($c) $c.bundle.windows.nsis.installMode = 'both' } },
    @{ Name = 'visible_webview_bootstrap'; Change = { param($c) $c.bundle.windows.webviewInstallMode.type = 'skip' } },
    @{ Name = 'visible_webview_bootstrap'; Change = { param($c) $c.bundle.windows.webviewInstallMode.silent = $true } },
    @{ Name = 'npcap_hook'; Change = { param($c) $c.bundle.windows.nsis.installerHooks = 'other.nsh' } },
    @{ Name = 'upstream_template'; Change = { param($c) $c.bundle.windows.nsis | Add-Member template 'custom.nsi' } },
    @{ Name = 'no_authenticode'; Change = { param($c) $c.bundle.windows | Add-Member signCommand 'sign.exe' } },
    @{ Name = 'no_external_binaries'; Change = { param($c) $c.bundle | Add-Member externalBin @('extra.exe') } },
    @{ Name = 'exact_resource_mappings'; Change = { param($c) $c.bundle.resources.PSObject.Properties.Remove('../../tests/fixtures/scrubbed/maxed-demo/story.pcap') } },
    @{ Name = 'exact_resource_mappings'; Change = { param($c) $c.bundle.resources.'../../tests/fixtures/scrubbed/maxed-demo/catalog.json' = 'catalog.json' } },
    @{ Name = 'exact_resource_mappings'; Change = { param($c) $c.bundle.resources.PSObject.Properties.Remove('../../LICENSE') } },
    @{ Name = 'exact_resource_mappings'; Change = { param($c) $c.bundle.resources.PSObject.Properties.Remove('../../NOTICE.md') } },
    @{ Name = 'exact_resource_mappings'; Change = { param($c) $c.bundle.resources.PSObject.Properties.Remove('../../THIRD-PARTY-NOTICES.md') } },
    @{ Name = 'exact_resource_mappings'; Change = { param($c) $c.bundle.resources.'../../THIRD-PARTY-NOTICES.md' = 'notices.md' } },
    @{ Name = 'exact_resource_mappings'; Change = { param($c) $c.bundle.resources | Add-Member 'extra.json' 'demo/extra.json' } },
    @{ Name = 'no_wildcard_resources'; Change = { param($c) $c.bundle.resources | Add-Member '../../**' 'demo/' } },
    @{ Name = 'no_wildcard_resources'; Change = { param($c) $c.bundle.resources.'../../tests/fixtures/scrubbed/maxed-demo/story.pcap' = 'demo/*' } },
    @{ Name = 'unchanged_csp'; Change = { param($c) $c.app.security.csp += '; connect-src https:' } },
    @{ Name = 'unchanged_dev_csp'; Change = { param($c) $c.app.security.devCsp = "default-src *" } },
    @{ Name = 'native_window_owner'; Change = { param($c) $c.app.windows[0].create = $true } }
)
foreach ($mutation in $mutations) {
    $copy = $configText | ConvertFrom-Json
    & $mutation.Change $copy
    $roundTrip = $copy | ConvertTo-Json -Depth 30 | ConvertFrom-Json
    Assert-Check ($mutation.Name -in @(Get-ConfigFailures $roundTrip)) "reject mutation: $($mutation.Name)"
}
$withoutFeature = $buildText.Replace('maxed-app/custom-protocol', 'maxed-app/absent-feature')
foreach ($name in @('cargoArgs', 'tauriArgs')) {
    Assert-Check ("custom_protocol_$name" -in @(Get-BuildFailures $withoutFeature)) "reject missing custom-protocol: $name"
}
Assert-Check ('package_nsis_command' -in @(Get-BuildFailures ($buildText.Replace("'--bundles', 'nsis'", "'--bundles', 'msi'")))) 'reject non-NSIS package command'
Assert-Check (Test-Path -LiteralPath (Join-Path $repoRoot 'app/src-tauri/installer/npcap-guidance.nsh') -PathType Leaf) 'guidance hook exists'
$hookText = Get-Content -LiteralPath (Join-Path $repoRoot 'app/src-tauri/installer/npcap-guidance.nsh') -Raw
$hookFailures = @(Get-HookFailures $hookText)
Assert-Check ($hookFailures.Count -eq 0) "finish guidance contract: $($hookFailures -join ', ')"
foreach ($mutation in @(
    @{ Name = 'no_shell_launch'; Source = $hookText + "`nExecShell open url" },
    @{ Name = 'no_browser_link'; Source = $hookText + "`n!define MUI_FINISHPAGE_LINK url" },
    @{ Name = 'exact_npcap_option'; Source = $hookText.Replace("Restrict Npcap driver's access to Administrators only", 'admin-only access') },
    @{ Name = 'official_npcap_address'; Source = $hookText.Replace('https://npcap.com/#download', 'missing address') },
    @{ Name = 'restricted_npcap_repair'; Source = $hookText.Replace('Re-run the Npcap installer and untick that option', 'Restart Maxed') }
)) {
    Assert-Check ($mutation.Name -in @(Get-HookFailures $mutation.Source)) "reject guidance mutation: $($mutation.Name)"
}

# Run the real build entry point in an isolated fixture, without a CLI. Reaching
# the missing-CLI guard proves deliberately disabled artifacts need no signing key.
# A fresh child process removes the key without reading/changing the caller's key.
$tempParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$probeRoot = Join-Path $tempParent ('maxed-production-signing-' + [Guid]::NewGuid().ToString('N'))
try {
    $null = New-Item -ItemType Directory -Path (Join-Path $probeRoot 'scripts')
    $null = New-Item -ItemType Directory -Path (Join-Path $probeRoot 'app/src-tauri')
    $probeBuild = Join-Path $probeRoot 'scripts/build-release.ps1'
    Copy-Item -LiteralPath $buildPath -Destination $probeBuild
    $probeConfig = Join-Path $probeRoot 'app/src-tauri/tauri.conf.json'
    $runner = Join-Path $probeRoot 'run.ps1'
    @'
param([string]$BuildScript)
$env:TAURI_SIGNING_PRIVATE_KEY = $null
try { & $BuildScript -Package; exit 0 }
catch { Write-Output $_.Exception.Message; exit 1 }
'@ | Set-Content -LiteralPath $runner -Encoding UTF8
    $signingCases = @(
        @{ Name = 'boolean false'; Value = $false; Enabled = $false },
        @{ Name = 'absent'; Omit = $true; Enabled = $false },
        @{ Name = 'boolean true'; Value = $true; Enabled = $true },
        @{ Name = 'v1Compatible'; Value = 'v1Compatible'; Enabled = $true },
        @{ Name = 'string false'; Value = 'false'; Enabled = $true },
        @{ Name = 'zero'; Value = 0; Enabled = $true },
        @{ Name = 'empty string'; Value = ''; Enabled = $true },
        @{ Name = 'explicit null'; Value = $null; Enabled = $true }
    )
    foreach ($signingCase in $signingCases) {
        $copy = $configText | ConvertFrom-Json
        if ($signingCase.Omit) { $copy.bundle.PSObject.Properties.Remove('createUpdaterArtifacts') }
        else { $copy.bundle.createUpdaterArtifacts = $signingCase.Value }
        $copy | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $probeConfig -Encoding UTF8
        $output = @(& powershell -NoProfile -ExecutionPolicy Bypass -File $runner -BuildScript $probeBuild)
        $exitCode = $LASTEXITCODE
        if ($signingCase.Enabled) {
            Assert-Check ($exitCode -eq 1 -and ($output -join "`n") -match 'require TAURI_SIGNING_PRIVATE_KEY' -and
                ($output -join "`n") -notmatch 'CLI is missing|unsigned NSIS installer') "updater-artifacts $($signingCase.Name) mutation requires key before build"
        } else {
            Assert-Check ($exitCode -eq 1 -and ($output -join "`n") -match 'Locked local Tauri CLI is missing' -and
                ($output -join "`n") -notmatch 'require TAURI_SIGNING_PRIVATE_KEY') "explicitly disabled artifacts ($($signingCase.Name)) do not require key"
            Assert-Check (($output -join "`n") -match 'unsigned NSIS installer') "disabled artifacts ($($signingCase.Name)) emit clear note"
        }
    }
} finally {
    if (Test-Path -LiteralPath $probeRoot) {
        $resolvedProbe = (Resolve-Path -LiteralPath $probeRoot).ProviderPath
        if ($resolvedProbe -ine $probeRoot -or -not $resolvedProbe.StartsWith($tempParent + '\', [StringComparison]::OrdinalIgnoreCase) -or
            ((Get-Item -LiteralPath $resolvedProbe -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Refusing cleanup outside the dedicated signing probe directory.'
        }
        Remove-Item -LiteralPath $resolvedProbe -Recurse -Force
    }
}
Write-Output "Packaging configuration: $script:checks checks passed. Static contracts do not prove VM installation."
