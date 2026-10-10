[CmdletBinding()]
param([switch]$Check)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$utf8 = New-Object Text.UTF8Encoding($false, $true)
$texts = @{}
$review = New-Object 'System.Collections.Generic.List[string]'
$entries = New-Object 'System.Collections.Generic.List[object]'

function Get-OrdinalStrings($Values) {
    [string[]]$sorted = @($Values)
    [Array]::Sort($sorted, [StringComparer]::Ordinal)
    $sorted
}

function Read-Utf8([string]$Path) {
    # Decode explicitly: Windows PowerShell 5.1 otherwise defaults to ANSI.
    [IO.File]::ReadAllText($Path, $utf8).Replace("`r`n", "`n").Replace("`r", "`n")
}

function Add-Text([string]$Text) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $id = ([BitConverter]::ToString($sha.ComputeHash($utf8.GetBytes($Text)))).Replace('-', '').ToLowerInvariant() }
    finally { $sha.Dispose() }
    $texts[$id] = $Text
    $id
}

function Get-LicenseFiles([string]$Directory) {
    # Include nested vendored notices (e.g. ring), not just the top-level licence.
    # Reject links rather than reading outside the package source directory.
    $pending = New-Object 'System.Collections.Generic.Queue[string]'
    $pending.Enqueue($Directory)
    $found = New-Object 'System.Collections.Generic.List[string]'
    while ($pending.Count) {
        foreach ($item in Get-ChildItem -LiteralPath $pending.Dequeue() -Force) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Linked package source cannot be inventoried.' }
            if ($item.PSIsContainer) {
                # Nested npm packages are visited through the lockfile, independently.
                if ($item.Name -cne 'node_modules' -and $item.Name -cne '.git') { $pending.Enqueue($item.FullName) }
            } elseif ($item.Name -match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE)') { $found.Add($item.FullName) }
        }
    }
    Get-OrdinalStrings $found
}

function Assert-ApprovedLicense([string]$License, [string]$Label) {
    if ([string]::IsNullOrWhiteSpace($License)) {
        $review.Add("${Label}: missing license metadata.")
        return
    }
    # Legacy Cargo slash expressions are retained verbatim in the output.
    $ids = $License -split '[\s()/]+' | Where-Object { $_ -and $_ -cnotin @('AND', 'OR') }
    $approved = @('MIT', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib',
        'Unicode-3.0', 'Unlicense', 'CC0-1.0', 'MIT-0', '0BSD', 'MPL-2.0')
    foreach ($id in $ids) {
        if ($id -cnotin $approved) { throw "Coordinator review required for licence '$License' in $Label." }
    }
}

function Read-PinnedNotice([string]$RelativePath, [string]$ExpectedHash) {
    $path = Join-Path $root $RelativePath
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Pinned vendor notice is missing: $RelativePath." }
    if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedHash) {
        throw "Pinned vendor notice hash mismatch: $RelativePath."
    }
    Read-Utf8 $path
}

# Validate pinned native notices before resolving dependencies. These files were
# extracted from the official NuGet package; no download is performed here.
$webviewVendor = 'docs/release/vendor/microsoft-web-webview2-1.0.3800.47'
$webviewPins = [ordered]@{
    'LICENSE.txt' = '0af8f1b807512aae39c2ac1aa4d0cae65cabecb6fd554b8439a5162a0d6eca55'
    'NOTICE.txt' = '106423785c5b7eba0a8e61d1837f2132e9c828e20ad530f565d981c1df60dd90'
}
$webviewRefs = @{}
foreach ($file in $webviewPins.Keys) {
    $webviewRefs[$file] = Add-Text (Read-PinnedNotice "$webviewVendor/$file" $webviewPins[$file])
}
# Host-installed documentation is deliberately not consulted: minimal CI and a
# developer machine render the same reviewed, byte-pinned standard-library input.
$rustVersion = '1.99.0'
$rustVendor = "docs/release/vendor/rust-$rustVersion/COPYRIGHT-library.html"
$rustPin = '5647be074c8edf7339fd863055923a8fc80bc5610a8d4661ec3b767b9d392c27'
$rustHtml = Read-PinnedNotice $rustVendor $rustPin
$toolchain = Read-Utf8 (Join-Path $root 'rust-toolchain.toml')
$channels = [regex]::Matches($toolchain, '(?m)^channel\s*=\s*"([^"\r\n]+)"\s*$')
if ($channels.Count -ne 1 -or $channels[0].Groups[1].Value -cne $rustVersion) {
    throw 'Rust standard-library notice requires review for the changed toolchain: channel must match vendored Rust 1.99.0.'
}
. (Join-Path $PSScriptRoot 'ConvertFrom-RustNoticeHtml.ps1')
$rustText = ConvertFrom-RustNoticeHtml $rustHtml

Push-Location -LiteralPath $root
try {
    $metadataJson = & cargo metadata --offline --locked --format-version 1 --filter-platform x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Offline locked Cargo metadata failed.' }
    $metadata = ($metadataJson -join "`n") | ConvertFrom-Json
} finally { Pop-Location }
$packages = @{}; $nodes = @{}
foreach ($package in $metadata.packages) { $packages[$package.id] = $package }
foreach ($node in $metadata.resolve.nodes) { $nodes[$node.id] = $node }
$app = @($metadata.packages | Where-Object { $_.name -ceq 'maxed-app' -and $_.id -in $metadata.workspace_members })
if ($app.Count -ne 1) { throw 'Expected exactly one workspace maxed-app package.' }
$pending = New-Object 'System.Collections.Generic.Queue[string]'
$pending.Enqueue($app[0].id)
$seen = @{}
while ($pending.Count) {
    $id = $pending.Dequeue()
    if ($seen.ContainsKey($id)) { continue }
    $seen[$id] = $true
    $package = $packages[$id]
    if ($null -eq $package -or -not $nodes.ContainsKey($id)) { throw 'Incomplete Cargo resolve graph.' }
    if (@($package.targets | Where-Object { 'proc-macro' -in $_.kind }).Count) { continue }
    foreach ($dependency in $nodes[$id].deps) {
        if (@($dependency.dep_kinds | Where-Object { $null -eq $_.kind }).Count) { $pending.Enqueue($dependency.pkg) }
    }
    # Still traverse local dependencies, but their notices are the repository LICENSE.
    if ($id -in $metadata.workspace_members) { continue }
    if (-not $package.source) { throw "Unrecognised non-workspace local crate: $($package.name)." }
    $entries.Add([pscustomobject]@{ Kind = 'Rust'; Name = $package.name; Version = $package.version;
        License = $package.license; Directory = (Split-Path -Parent $package.manifest_path); Extra = ''; Files = @() })
}

$ui = Join-Path $root 'app/ui'
$manifest = (Read-Utf8 (Join-Path $ui 'package.json')) | ConvertFrom-Json
$lockText = Read-Utf8 (Join-Path $ui 'package-lock.json')
# ConvertFrom-Json in Windows PowerShell 5.1 cannot represent an empty property
# name. Rename only npm's root-package key for parsing, then restore it in our map.
$rootKeyPattern = '"packages"\s*:\s*\{\s*""\s*:'
if ([regex]::Matches($lockText, $rootKeyPattern).Count -ne 1 -or $lockText.Contains('__maxed_lock_root__')) { throw 'Unrecognised npm root-package structure.' }
$lock = ($lockText -replace $rootKeyPattern, '"packages": { "__maxed_lock_root__":') | ConvertFrom-Json
if ($lock.lockfileVersion -ne 3) { throw 'Expected npm lockfile version 3.' }
$locked = @{}
foreach ($property in $lock.packages.PSObject.Properties) {
    $key = if ($property.Name -ceq '__maxed_lock_root__') { '' } else { $property.Name }
    $locked[$key] = $property.Value
}
function Resolve-NpmDependency([string]$From, [string]$Name, [bool]$Optional) {
    if ($Name -cnotmatch '\A(?:@[a-zA-Z0-9_.-]+/)?[a-zA-Z0-9_.-]+\z' -or $Name -in @('.', '..')) { throw 'Invalid npm dependency name.' }
    $cursor = $From
    while ($true) {
        $candidate = if ($cursor) { "$cursor/node_modules/$Name" } else { "node_modules/$Name" }
        if ($locked.ContainsKey($candidate)) { return $candidate }
        if (-not $cursor) { break }
        $slash = $cursor.LastIndexOf('/')
        $cursor = if ($slash -lt 0) { '' } else { $cursor.Substring(0, $slash) }
    }
    if (-not $Optional) { throw "Unresolved runtime npm dependency: $Name." }
    return $null
}
$pending = New-Object 'System.Collections.Generic.Queue[string]'
foreach ($property in $manifest.dependencies.PSObject.Properties) {
    if ($locked[''].dependencies.($property.Name) -cne $property.Value) { throw 'Root npm dependencies differ from lockfile.' }
    $pending.Enqueue((Resolve-NpmDependency '' $property.Name $false))
}
if (@($manifest.dependencies.PSObject.Properties).Count -ne @($locked[''].dependencies.PSObject.Properties).Count) { throw 'Root npm dependency count differs from lockfile.' }
$seen = @{}
while ($pending.Count) {
    $path = $pending.Dequeue()
    if ($seen.ContainsKey($path)) { continue }
    $seen[$path] = $true
    $package = $locked[$path]
    if ($package.link) { throw 'Linked runtime npm dependencies are not supported.' }
    $directory = Join-Path $ui $path
    $installed = (Read-Utf8 (Join-Path $directory 'package.json')) | ConvertFrom-Json
    if ($installed.version -cne $package.version) { throw "Installed npm version differs from lockfile: $path." }
    $name = $path.Substring($path.LastIndexOf('node_modules/', [StringComparison]::Ordinal) + 13)
    if ($installed.name -cne $name) { throw "Installed npm name differs from lockfile path: $path." }
    if ($package.license -and $installed.license -cne $package.license) { throw "Installed npm licence differs from lockfile: $name." }
    $entries.Add([pscustomobject]@{ Kind = 'npm'; Name = $name; Version = $package.version;
        License = $installed.license; Directory = $directory; Extra = "Lockfile location: ``$path``."; Files = @() })
    foreach ($field in @('dependencies', 'optionalDependencies', 'peerDependencies')) {
        foreach ($property in $package.$field.PSObject.Properties) {
            $optional = $field -eq 'optionalDependencies' -or
                ($field -eq 'peerDependencies' -and $package.peerDependenciesMeta.($property.Name).optional -eq $true)
            $resolved = Resolve-NpmDependency $path $property.Name $optional
            if ($resolved) { $pending.Enqueue($resolved) }
        }
    }
}

# Approval covers these exact unmodified MPL packages and source routes.
$mplApproved = @{
    'cssparser' = '0.37.0'; 'dtoa-short' = '0.3.5'; 'option-ext' = '0.2.0'; 'selectors' = '0.38.0'
    'symphonia' = '0.5.5'; 'symphonia-codec-pcm' = '0.5.5'; 'symphonia-core' = '0.5.5'
    'symphonia-format-riff' = '0.5.5'; 'symphonia-metadata' = '0.5.5'
}
$mplPackage = @($entries | Where-Object { $_.Kind -ceq 'Rust' -and $_.Name -ceq 'cssparser' })
if ($mplPackage.Count -ne 1) { throw 'Approved MPL licence source cssparser is missing.' }
$mplText = Read-Utf8 (Join-Path $mplPackage[0].Directory 'LICENSE')
$mplId = Add-Text $mplText
function Get-MplComparison([string]$Text) {
    # Source copies vary only in whitespace and http/https in the Exhibit A URL.
    ($Text.Replace('https://mozilla.org/MPL/2.0/', 'http://mozilla.org/MPL/2.0/') -replace '\s', '')
}
$mplComparison = Get-MplComparison $mplText
foreach ($entry in $entries) {
    $label = "$($entry.Kind) $($entry.Name) $($entry.Version)"
    Assert-ApprovedLicense $entry.License $label
    if ($entry.License -match 'MPL' -and ($entry.License -cne 'MPL-2.0' -or $entry.Kind -cne 'Rust' -or $mplApproved[$entry.Name] -cne $entry.Version)) {
        throw "Coordinator approval required for MPL package $label."
    }
    $files = @(Get-LicenseFiles $entry.Directory)
    foreach ($file in $files) {
        $text = Read-Utf8 $file
        if ([string]::IsNullOrWhiteSpace($text)) { $review.Add("${label}: empty licence file $([IO.Path]::GetFileName($file))."); continue }
        $textId = if ((Get-MplComparison $text) -ceq $mplComparison) { $mplId } else { Add-Text $text }
        $relative = $file.Substring($entry.Directory.TrimEnd('\', '/').Length + 1).Replace('\', '/')
        $entry.Files += [pscustomobject]@{ Path = $relative; Id = $textId }
    }
    if ($entry.Files.Count -eq 0) {
        if ($entry.Kind -ceq 'Rust' -and $entry.Name -ceq 'webview2-com-sys') {
            $review.Add("${label}: crate's own code is MIT (text missing from the package); embeds the Microsoft-licensed static loader covered in Native and installer components.")
        } else {
            $review.Add("${label}: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: $($entry.License).")
        }
    }
}
$webviewCrate = @($entries | Where-Object { $_.Kind -ceq 'Rust' -and $_.Name -ceq 'webview2-com-sys' })
if ($webviewCrate.Count -ne 1 -or $webviewCrate[0].Version -cne '0.39.1') { throw 'WebView2 loader notice association requires webview2-com-sys 0.39.1.' }

$nsis = Join-Path $env:LOCALAPPDATA 'tauri/NSIS'
$cli = Join-Path $ui 'node_modules/@tauri-apps/cli'
$cliManifest = (Read-Utf8 (Join-Path $cli 'package.json')) | ConvertFrom-Json
if ($cliManifest.version -cne '2.12.1' -or $locked['node_modules/@tauri-apps/cli'].version -cne '2.12.1') { throw 'Cached installer licence association requires locked Tauri CLI 2.12.1.' }
$plugin = Join-Path $nsis 'Plugins/x86-unicode/additional/nsis_tauri_utils.dll'
$pluginHash = (Get-FileHash -LiteralPath $plugin -Algorithm SHA256).Hash.ToLowerInvariant()
if ($pluginHash -cne '5ba143b5db4a87d32d6e7802e033330aae56cbceabe0d1e3ba41948385ad4709') { throw 'Cached nsis_tauri_utils DLL differs from the approved payload.' }
$installerTexts = [ordered]@{
    'NSIS COPYING (includes LZMA exception)' = (Join-Path $nsis 'COPYING')
    'NSISdl License.txt' = (Join-Path $nsis 'Docs/NSISdl/License.txt')
    'Modern UI 2 License.txt' = (Join-Path $nsis 'Docs/Modern UI 2/License.txt')
    'Tauri CLI LICENSE-MIT' = (Join-Path $cli 'LICENSE-MIT')
    'Tauri CLI LICENSE-APACHE-2.0' = (Join-Path $cli 'LICENSE-APACHE-2.0')
}
$installerRefs = @{}
foreach ($name in $installerTexts.Keys) {
    # NSIS ships this one copyright notice in Windows-1252, not UTF-8.
    $text = if ($name -ceq 'Modern UI 2 License.txt') {
        [IO.File]::ReadAllText($installerTexts[$name], [Text.Encoding]::GetEncoding(1252)).Replace("`r`n", "`n").Replace("`r", "`n")
    } else { Read-Utf8 $installerTexts[$name] }
    $installerRefs[$name] = Add-Text $text
}

$output = New-Object Text.StringBuilder
function Line([string]$Text = '') { $null = $output.Append($Text).Append("`n") }
Line '# Third-party notices'
Line
Line 'Generated offline by `scripts/release/New-ThirdPartyNotices.ps1`. UTF-8 without BOM; LF newlines. Do not edit by hand.'
Line
Line 'Rust scope: Cargo.lock and the Windows x86_64 MSVC resolve graph rooted at maxed-app, following normal dependency edges only. Build dependencies, dev dependencies, proc-macro crates and their exclusively reached dependencies are excluded; they do not ship in the binary. Workspace-local crates are covered by LICENSE and NOTICE.md.'
Line
Line 'npm scope: the locked dependency closure rooted at app/ui/package.json dependencies, including resolved optional/peer dependencies. Dev-dependency roots are excluded. This is a conservative notice set: tree-shaking can omit code from these packages.'
Line
Line 'Each package links its complete cached licence/notice/copyright files below, including nested vendored notices. Identical text is stored once. MPL-2.0 is reproduced once; whitespace and the http/https spelling of its Exhibit A URL are treated as equivalent. Original copyright identities in upstream texts are retained. No local filesystem paths are emitted.'
Line
Line 'Maxed, the original project icon and synthetic demo use LICENSE. RealmShark, UnityPy and alert-sound attribution remain in NOTICE.md. Game assets are extracted locally at runtime and are never shipped. See docs/release/LICENSE-INVENTORY.md in the source repository for distribution coverage and provenance.'
Line
foreach ($section in @('Rust', 'MPL-2.0 (file-level copyleft)', 'npm')) {
    Line "## $section"
    Line
    $sectionEntries = @($entries | Where-Object {
        if ($section -eq 'Rust') { $_.Kind -eq 'Rust' -and $_.License -cne 'MPL-2.0' }
        elseif ($section -eq 'npm') { $_.Kind -eq 'npm' }
        else { $_.Kind -eq 'Rust' -and $_.License -ceq 'MPL-2.0' }
    })
    $byKey = @{}
    foreach ($entry in $sectionEntries) { $byKey["$($entry.Name) $($entry.Version) $($entry.Extra)"] = $entry }
    foreach ($key in Get-OrdinalStrings $byKey.Keys) {
        $entry = $byKey[$key]
        Line "### $($entry.Name) $($entry.Version)"
        Line
        Line "License metadata: ``$($entry.License)``."
        if ($entry.Kind -ceq 'Rust' -and $entry.Name -ceq 'webview2-com-sys') {
            Line 'MIT applies to the crate code. The embedded Microsoft WebView2 SDK static loader has separate Microsoft terms; see [Native and installer components](#native-and-installer-components).'
        }
        if ($entry.Extra) { Line $entry.Extra }
        if ($entry.License -ceq 'MPL-2.0') {
            Line "Unmodified. Source Code Form is available at https://crates.io/crates/$($entry.Name)/$($entry.Version)"
            Line "Full MPL-2.0 text: [licence](#text-$mplId)."
        }
        Line
        foreach ($file in $entry.Files) { Line "- ``$($file.Path)``: [full text](#text-$($file.Id))." }
        if ($entry.Files.Count -eq 0) { Line 'No licence file in cached package; metadata fallback above. See Needs review.' }
        Line
    }
}
Line '## Native and installer components'
Line
Line '### Microsoft WebView2 SDK loader (WebView2LoaderStatic.lib) 1.0.3800.47, statically linked via webview2-com-sys 0.39.1'
Line
Line 'Microsoft Corporation. Licence: Microsoft WebView2 SDK BSD-3-Clause-style terms reproduced below; these are separate from the Rust binding crate MIT licence. The x64 loader is statically linked on MSVC. Version/provenance verified by the coordinator from the companion DLL version resource and official NuGet package.'
Line
Line 'Provenance: official NuGet package Microsoft.Web.WebView2 1.0.3800.47, downloaded by the coordinator from api.nuget.org. Nupkg SHA-256: `56c9f26bdd07916a2d1949fb58a5c7e434dfa1173577dca879206050c4e718db`.'
Line
foreach ($file in $webviewPins.Keys) {
    Line "- ``$webviewVendor/$file``; SHA-256 ``$($webviewPins[$file])``; [full text](#text-$($webviewRefs[$file]))."
}
Line
Line '### NSIS and Tauri installer'
Line
Line 'NSIS toolset: cached toolset used by Tauri CLI 2.12.1. NSIS code/plugins/graphics use zlib/libpng except as stated in COPYING. LZMA is the installer decompression stub, not a separate extracted payload file; its CPL-1.0 module has the NSIS linking exception reproduced in COPYING. The cached toolset also includes the bzip2 licence in COPYING.'
Line
Line 'The modern-wizard.bmp payload matches NSIS Contrib/Graphics/Wizard/win.bmp (SHA-256 3ad2dc318056d0a2024af1804ea741146cfc18cc404649a44610cbf8b2056cf2). Modern UI 2 License.txt is transcoded from its original Windows-1252 encoding; all notice wording is retained.'
Line
Line 'nsis_tauri_utils: upstream project `tauri-apps/nsis-tauri-utils`, fetched by tauri-cli 2.12.1. MIT OR Apache-2.0. The coordinator approved association with the cached Tauri CLI licence texts.'
Line "DLL SHA-256: ``$pluginHash``."
Line
foreach ($name in Get-OrdinalStrings $installerRefs.Keys) { Line "- ${name}: [full text](#text-$($installerRefs[$name]))." }
Line
Line '## Rust standard library'
Line
Line "rustc $rustVersion, pinned by rust-toolchain.toml. Statically linked std/core/alloc and toolchain dependencies are outside Cargo.lock. The complete toolchain attribution follows, including all in-tree exceptions and out-of-tree dependency notices; no target-specific filtering is applied."
Line
Line "Source: ``$rustVendor``; copied unchanged from share/doc/rust/COPYRIGHT-library.html in the $rustVersion-x86_64-pc-windows-msvc toolchain; SHA-256 ``$rustPin``."
Line
Line 'Rendered offline from the pinned HTML. HTML flow whitespace becomes plain-text lines; nested file/directory exceptions are indented, external link URLs are retained, and preformatted licence texts preserve all decoded characters. Host-installed documentation is never read.'
Line
$rustFenceLength = 3
foreach ($match in [regex]::Matches($rustText, '~+')) { $rustFenceLength = [Math]::Max($rustFenceLength, $match.Length + 1) }
$rustFence = '~' * $rustFenceLength
Line ($rustFence + 'text')
$null = $output.Append($rustText)
Line $rustFence
Line
Line '## Licence texts'
Line
foreach ($id in Get-OrdinalStrings $texts.Keys) {
    Line "<a id=`"text-$id`"></a>"
    Line "### Text $id"
    Line
    $text = $texts[$id]
    $maxFence = 2
    foreach ($match in [regex]::Matches($text, '~+')) { $maxFence = [Math]::Max($maxFence, $match.Length) }
    $fence = '~' * ($maxFence + 1)
    Line ($fence + 'text')
    $null = $output.Append($text)
    if (-not $text.EndsWith("`n", [StringComparison]::Ordinal)) { Line }
    Line $fence
    Line
}
Line '## Needs review'
Line
if ($review.Count) { foreach ($item in Get-OrdinalStrings $review) { Line "- $item" } }
else { Line 'None. Every dependency has cached licence/notice text.' }
Line
Line 'This generated inventory does not approve future dependency or asset changes. Missing-text entries remain explicit review items even when a shared licence text is available elsewhere in this file.'
$content = $output.ToString()
foreach ($privatePrefix in @($root, $env:USERPROFILE, $env:LOCALAPPDATA)) {
    if ($privatePrefix -and ($content.IndexOf($privatePrefix, [StringComparison]::OrdinalIgnoreCase) -ge 0 -or
        $content.IndexOf($privatePrefix.Replace('\', '/'), [StringComparison]::OrdinalIgnoreCase) -ge 0)) { throw 'Local path detected in generated notices.' }
}
$bytes = $utf8.GetBytes($content)
Write-Output "Third-party notices: $($bytes.Length) bytes; $(@($entries | Where-Object Kind -eq Rust).Count) Rust crates; $(@($entries | Where-Object Kind -eq npm).Count) npm packages; $($review.Count) review items."
# The approved embedded Rust attribution exceeds the former 2 MB ceiling.
# Keep a bounded document budget; the separate installer limit is 20,000,000 bytes.
if ($bytes.Length -gt 3000000) { throw 'Notices exceed the 3 MB budget. Coordinator approval required; output not written.' }
$destination = Join-Path $root 'THIRD-PARTY-NOTICES.md'
if ($Check) {
    if (-not (Test-Path -LiteralPath $destination -PathType Leaf) -or
        [Convert]::ToBase64String([IO.File]::ReadAllBytes($destination)) -cne [Convert]::ToBase64String($bytes)) {
        throw 'THIRD-PARTY-NOTICES.md differs from offline generation. Regenerate and review it.'
    }
    Write-Output 'THIRD-PARTY-NOTICES.md is current.'
} else { [IO.File]::WriteAllBytes($destination, $bytes) }
