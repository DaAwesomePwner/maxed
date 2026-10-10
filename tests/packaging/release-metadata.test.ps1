[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$temp = Join-Path ([IO.Path]::GetTempPath()) ('maxed-metadata-' + [Guid]::NewGuid().ToString('N'))
$script:checks = 0
function Assert($ok, $name) { if (-not $ok) { throw "FAIL: $name" }; $script:checks++; Write-Output "PASS: $name" }
# Invented bytes and a stubbed verifier boundary, never a production key or installer.
function cargo {
    if ($args[0] -eq 'metadata') { $global:LASTEXITCODE = 0; return ('{"packages":[{"name":"maxed-app","version":"' + $global:metadataTestState.version + '"}]}') }
    if ($args[0] -ne 'run' -or 'maxed-update-verify' -notin $args -or $args[-1] -cne ($args[-2] + '.sig')) { throw 'Unexpected verifier invocation.' }
    $global:metadataTestState.verified++
    $global:LASTEXITCODE = $global:metadataTestState.verifyExit
}
function Reject($change, $name) {
    $p = $script:inputs.Clone(); $p.OutputDir = Join-Path $temp ([Guid]::NewGuid().ToString('N'))
    & $change $p
    $failed = $false
    try { & $script:tool @p } catch { $failed = $true }
    Assert $failed $name
    Assert (-not (Test-Path -LiteralPath $p.OutputDir)) "$name creates no output"
}
try {
    $null = New-Item -ItemType Directory -Path "$temp/scripts/release", "$temp/app/src-tauri", "$temp/app/ui"
    Copy-Item -LiteralPath "$root/scripts/release/New-ReleaseMetadata.ps1", "$root/scripts/release/Validate-Release.ps1" -Destination "$temp/scripts/release"
    '{"version":"0.1.0"}' | Set-Content "$temp/app/src-tauri/tauri.conf.json"
    '{"version":"0.1.0"}' | Set-Content "$temp/app/ui/package.json"
    'Invented MIT license fixture' | Set-Content "$temp/LICENSE"
    'Invented notice fixture' | Set-Content "$temp/NOTICE.md"
    $script:tool = "$temp/scripts/release/New-ReleaseMetadata.ps1"
    $installer = "$temp/Maxed_0.1.0_x64-setup.exe"
    [IO.File]::WriteAllBytes($installer, [byte[]](1,2,3,4))
    'aW52ZW50ZWQ=' | Set-Content "$installer.sig"
    $inventory = @('LICENSE', 'NOTICE.md') | ForEach-Object { @{ file = $_; license = 'MIT'; sha256 = (Get-FileHash "$temp/$_").Hash.ToLowerInvariant() } }
    ConvertTo-Json -InputObject @($inventory) | Set-Content "$temp/inventory.json"
    $script:inputs = @{ Version = '0.1.0'; Channel = 'stable'; Repository = 'SyntheticOwner/synthetic-repo'; Installer = $installer; Signature = "$installer.sig"; OutputDir = "$temp/stage"; LicenseInventory = "$temp/inventory.json" }
    $global:metadataTestState = @{ verified = 0; verifyExit = 0; version = '0.1.0' }
    & $tool @inputs -Notes 'Invented plain text' -PubDate '2026-10-02T12:00:00Z'
    $latest = Get-Content "$temp/stage/latest.json" -Raw | ConvertFrom-Json
    $manifest = Get-Content "$temp/stage/release-manifest.json" -Raw | ConvertFrom-Json
    Assert ($global:metadataTestState.verified -eq 1) 'manifest_requires_verified_artifact positive verifier boundary'
    Assert ($latest.platforms.'windows-x86_64-nsis'.url -ceq 'https://github.com/SyntheticOwner/synthetic-repo/releases/download/v0.1.0/Maxed_0.1.0_x64-setup.exe') 'immutable URL and S2 target'
    Assert ($latest.platforms.'windows-x86_64-nsis'.signature -ceq 'aW52ZW50ZWQ=') 'signature contents'
    Assert (@($manifest.files).Count -eq 6) 'explicit six-file hash inventory'
    foreach ($f in $manifest.files) { Assert ($f.sha256 -ceq (Get-FileHash -LiteralPath "$temp/stage/$($f.file)").Hash.ToLowerInvariant()) 'staged hash identity' }
    Assert (-not (Get-Content "$temp/stage/release-manifest.json" -Raw).Contains($temp)) 'manifest has no absolute paths'
    $pluginInventory = @($inventory) + @{ file = '$PLUGINSDIR/System.dll'; license = 'Zlib'; sha256 = ('a' * 64) }
    ConvertTo-Json -InputObject $pluginInventory | Set-Content "$temp/plugin-inventory.json"
    $pluginInputs = $inputs.Clone(); $pluginInputs.OutputDir = "$temp/plugin-stage"; $pluginInputs.LicenseInventory = "$temp/plugin-inventory.json"
    & $tool @pluginInputs
    $stagedInventory = Get-Content "$temp/plugin-stage/license-inventory.json" -Raw | ConvertFrom-Json
    Assert ($stagedInventory[-1].file -ceq '$PLUGINSDIR/System.dll') 'literal plugin prefix accepted and preserved'
    foreach ($badPath in @('$OTHER/x.dll', '$PLUGINSDIR/../x', '$PLUGINSDIR/a/b.dll', '$PLUGINSDIR/', '$PLUGINSDIR/..', '$PLUGINSDIR/.', '$PLUGINSDIR/a$.dll', 'a$b.dll', 'demo/$x.dll', '$pluginsdir/System.dll')) {
        $badInventory = @($inventory) + @{ file = $badPath; license = 'Zlib'; sha256 = ('a' * 64) }
        ConvertTo-Json -InputObject $badInventory | Set-Content "$temp/plugin-inventory.json"
        Reject { param($p) $p.LicenseInventory = "$temp/plugin-inventory.json" } "reject inventory path $badPath"
    }
    # Exercise the committed inventory through the actual metadata script, with
    # only its cryptographic verifier replaced by the existing synthetic boundary.
    $realInventoryPath = "$root/docs/release/distribution-inventory.json"
    $realInventory = Get-Content -LiteralPath $realInventoryPath -Raw | ConvertFrom-Json
    $realInventory = @($realInventory)
    $expectedPaths = @('LICENSE', 'NOTICE.md', 'THIRD-PARTY-NOTICES.md', 'demo/catalog.json', 'demo/story.pcap',
        '$PLUGINSDIR/System.dll', '$PLUGINSDIR/nsDialogs.dll', '$PLUGINSDIR/NSISdl.dll',
        '$PLUGINSDIR/nsis_tauri_utils.dll', '$PLUGINSDIR/StartMenu.dll', '$PLUGINSDIR/modern-wizard.bmp')
    Assert (@(Compare-Object $expectedPaths @($realInventory.file) -CaseSensitive).Count -eq 0 -and $realInventory.Count -eq 11) 'committed inventory has exact stable payload paths, excluding changing executable'
    foreach ($file in @('LICENSE', 'NOTICE.md', 'THIRD-PARTY-NOTICES.md', 'demo/catalog.json', 'demo/story.pcap')) {
        $sourceFile = if ($file.StartsWith('demo/')) { "$root/tests/fixtures/scrubbed/maxed-demo/" + $file.Substring(5) } else { "$root/$file" }
        $entry = @($realInventory | Where-Object { $_.file -ceq $file })
        Assert ($entry.Count -eq 1 -and $entry[0].sha256 -ceq (Get-FileHash -LiteralPath $sourceFile -Algorithm SHA256).Hash.ToLowerInvariant()) "committed inventory source hash: $file"
    }
    $savedLicense = [IO.File]::ReadAllBytes("$temp/LICENSE")
    $savedNotice = [IO.File]::ReadAllBytes("$temp/NOTICE.md")
    try {
        Copy-Item -LiteralPath "$root/LICENSE", "$root/NOTICE.md" -Destination $temp -Force
        $realInputs = $inputs.Clone(); $realInputs.OutputDir = "$temp/real-inventory-stage"; $realInputs.LicenseInventory = $realInventoryPath
        & $tool @realInputs
        $staged = Get-Content "$temp/real-inventory-stage/license-inventory.json" -Raw | ConvertFrom-Json
        $staged = @($staged)
        Assert ($staged.Count -eq 11) 'committed distribution inventory passes metadata schema and stages'
        foreach ($entry in $realInventory) {
            $copy = @($staged | Where-Object { $_.file -ceq $entry.file })
            Assert ($copy.Count -eq 1 -and $copy[0].license -ceq $entry.license -and $copy[0].sha256 -ceq $entry.sha256) 'committed distribution entry preserved'
        }
    } finally {
        [IO.File]::WriteAllBytes("$temp/LICENSE", $savedLicense)
        [IO.File]::WriteAllBytes("$temp/NOTICE.md", $savedNotice)
    }
    $failed = $false; try { & $tool @inputs } catch { $failed = $true }; Assert $failed 'refuse existing output'
    foreach ($v in @('v0.1.0', '01.1.0', '0.1', '0.1.0-beta.01', "0.1.0`n", 'update-stable', '0.2.0', '18446744073709551616.0.0')) {
        Reject { param($p) $p.Version = $v } "reject invalid or mismatched version $($v.Trim())"
    }
    Reject { param($p) $p.Channel = 'Stable' } 'strict channel casing'
    Reject { param($p) $p.Version = '0.1.0-beta.1' } 'stable rejects prerelease'
    Reject { param($p) $p.Repository = 'https://github.com/a/b' } 'reject repository URL'
    Reject { param($p) $p.Signature = "$temp/missing.sig" } 'reject missing signature'
    Reject { param($p) $p.Installer = "$temp/missing.exe" } 'reject missing installer'
    Reject { param($p) $p.Notes = ('x' * 8193) } 'reject notes overflow'
    Reject { param($p) $p.PubDate = '2026-99-99T12:00:00Z' } 'reject invalid date'
    $global:metadataTestState.verifyExit = 1
    Reject { param($p) } 'reject public-key verification failure'
    $global:metadataTestState.verifyExit = 0
    '' | Set-Content "$installer.sig"
    Reject { param($p) } 'reject empty signature'
    'aW52ZW50ZWQ=' | Set-Content "$installer.sig"
    foreach ($size in @(0, 20000000, 20000001)) {
        $stream = [IO.File]::OpenWrite($installer); $stream.SetLength($size); $stream.Dispose()
        Reject { param($p) } "reject installer size $size"
    }
    [IO.File]::WriteAllBytes($installer, [byte[]](1,2,3,4))
    $bad = @(@{ file = '../private'; license = 'MIT'; sha256 = ('a' * 64) })
    ConvertTo-Json -InputObject $bad | Set-Content "$temp/inventory.json"
    Reject { param($p) } 'reject inventory traversal'
    $failed = $false
    try { & "$temp/scripts/release/Validate-Release.ps1" -Version '0.1.0' -Channel stable -Repository 'SyntheticOwner/synthetic-repo' -Tag 'update-stable' } catch { $failed = $true }
    Assert $failed 'tag_version_and_channel_match'
    $schema = Get-Content "$root/docs/release/latest.schema.json" -Raw | ConvertFrom-Json
    Assert ($schema.properties.platforms.required[0] -ceq 'windows-x86_64-nsis' -and $schema.additionalProperties -eq $false) 'schema has exact S2 target'
    $global:metadataTestState.version = '0.1.0-beta.1'
    '{"version":"0.1.0-beta.1"}' | Set-Content "$temp/app/src-tauri/tauri.conf.json"
    '{"version":"0.1.0-beta.1"}' | Set-Content "$temp/app/ui/package.json"
    & "$temp/scripts/release/Validate-Release.ps1" -Version '0.1.0-beta.1' -Channel beta -Repository 'SyntheticOwner/synthetic-repo' -Tag 'v0.1.0-beta.1'
    Assert $true 'canonical beta tag accepted when all source versions match'
} finally {
    Remove-Variable metadataTestState -Scope Global -ErrorAction SilentlyContinue
    $resolved = [IO.Path]::GetFullPath($temp)
    if ($resolved -ne $temp -or -not $resolved.StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()), [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe fixture cleanup.' }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
Write-Output "Release metadata: $script:checks checks passed; cryptographic validity is tested by native S2 tests, not the stub."
