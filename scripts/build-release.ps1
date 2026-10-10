[CmdletBinding()]
param([switch]$Offline, [switch]$Package)

# Dot-sourcing exposes only the package verification helpers for synthetic tests.
function Assert-BundleEntryPath([string]$Path) {
    $relative = $Path.Replace('\', '/')
    if ([string]::IsNullOrWhiteSpace($relative) -or $relative -match '^[\/]|:|[\x00-\x1f]' -or
        @($relative.Split('/') | Where-Object { $_ -in @('', '.', '..') -or $_ -match '[. ]$' }).Count) {
        throw 'bundle_path_traversal: invalid relative payload path.'
    }
    return $relative
}

function Assert-BundleAllowedFile([string]$Path) {
    $relative = Assert-BundleEntryPath $Path
    # Locked CLI 2.12.1 NSIS template/plugins; downloadBootstrapper embeds no runtime.
    # WriteUninstaller writes uninstall.exe at install time. 7-Zip 26.02+ also lists
    # and extracts it; it varies per build, so Write-BundleManifest checks its PE header.
    $allowed = @('maxed.exe', 'uninstall.exe', 'LICENSE', 'NOTICE.md', 'THIRD-PARTY-NOTICES.md', 'demo/story.pcap', 'demo/catalog.json',
        '$PLUGINSDIR/System.dll', '$PLUGINSDIR/nsDialogs.dll', '$PLUGINSDIR/NSISdl.dll',
        '$PLUGINSDIR/nsis_tauri_utils.dll', '$PLUGINSDIR/StartMenu.dll', '$PLUGINSDIR/modern-wizard.bmp')
    if ($relative -cnotin $allowed) {
        if ($relative -match '(?i)\.pdb$') { throw 'bundle_forbidden_pdb: debug symbols are not distributable.' }
        if ($relative -match '(?i)\.sqlite[^/]*$') { throw 'bundle_forbidden_sqlite: databases are not distributable.' }
        if ($relative -match '(?i)\.pcap$') { throw 'bundle_forbidden_pcap: only the exact synthetic demo is admitted.' }
        if ($relative -match '(?i)(^|/)maxed-cli\.exe$') { throw 'bundle_forbidden_cli: CLI is not distributable.' }
        throw 'bundle_unexpected_file: file is outside the explicit payload allowlist.'
    }
    return $relative
}

function Get-BundleArchivePaths([string[]]$Listing) {
    $seen = @{}
    foreach ($line in $Listing) {
        if ($line.StartsWith('Path = ', [StringComparison]::Ordinal)) {
            $path = Assert-BundleAllowedFile $line.Substring(7)
            if ($seen.ContainsKey($path)) { throw 'bundle_duplicate_path: archive paths must be unique.' }
            $seen[$path] = $true
            $path
        }
    }
    if ($seen.Count -eq 0) { throw 'bundle_empty_archive: no payload files were listed.' }
}

function Get-BundlePayloadFiles([string]$PayloadRoot) {
    $root = Get-Item -LiteralPath $PayloadRoot -Force
    if (-not $root.PSIsContainer -or ($root.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'bundle_reparse_path: payload root must be a real directory.'
    }
    $prefix = $root.FullName.TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    $pending = New-Object 'System.Collections.Generic.Queue[string]'
    $pending.Enqueue($root.FullName)
    while ($pending.Count) {
        foreach ($item in Get-ChildItem -LiteralPath $pending.Dequeue() -Force) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw 'bundle_reparse_path: payload links are not admitted.'
            }
            $relative = Assert-BundleEntryPath $item.FullName.Substring($prefix.Length)
            if ($item.PSIsContainer) {
                if ($relative -cnotin @('demo', '$PLUGINSDIR')) { throw 'bundle_unexpected_directory: unknown payload directory.' }
                $pending.Enqueue($item.FullName)
            } else {
                $null = Assert-BundleAllowedFile $relative
                [pscustomobject]@{ path = $relative; bytes = $item.Length
                    sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
            }
        }
    }
}

# Structural check for a generated binary that has no source hash: DOS header,
# in-bounds PE offset, "PE\0\0", the executable-image flag, a PE32/PE32+
# optional header with a GUI or console subsystem, coherent loader mapping
# fields (alignments, SizeOfHeaders, SizeOfImage) and a section table whose
# file and virtual ranges fit them. MZ junk, truncation and COFF objects fail.
# Imports, relocations and actual execution are left to Windows install acceptance.
function Test-BundlePortableExecutable([byte[]]$Bytes) {
    if ($Bytes.Length -lt 64 -or $Bytes[0] -ne 0x4D -or $Bytes[1] -ne 0x5A) { return $false }
    $pe = [BitConverter]::ToInt32($Bytes, 0x3C)
    if ($pe -lt 64 -or $pe -gt $Bytes.Length - 24) { return $false }
    if ($Bytes[$pe] -ne 0x50 -or $Bytes[$pe + 1] -ne 0x45 -or $Bytes[$pe + 2] -ne 0 -or $Bytes[$pe + 3] -ne 0) { return $false }
    if (([BitConverter]::ToUInt16($Bytes, $pe + 22) -band 0x0002) -eq 0) { return $false }
    $optionalSize = [BitConverter]::ToUInt16($Bytes, $pe + 20)
    if ($optionalSize -lt 96 -or [int64]$pe + 24 + $optionalSize -gt $Bytes.Length) { return $false }
    $magic = [BitConverter]::ToUInt16($Bytes, $pe + 24)
    if (-not (($magic -eq 0x10B -and $optionalSize -ge 96) -or ($magic -eq 0x20B -and $optionalSize -ge 112))) { return $false }
    # NumberOfRvaAndSizes directories (8 bytes each) must fit in the optional header.
    $fixedSize = if ($magic -eq 0x10B) { 96 } else { 112 }
    $directories = [int64][BitConverter]::ToUInt32($Bytes, $pe + 24 + $fixedSize - 4)
    if ($fixedSize + 8 * $directories -gt $optionalSize) { return $false }
    # The package is Windows x64 only: i386 (PE32, as NSIS emits) or AMD64 (PE32+).
    $machine = [BitConverter]::ToUInt16($Bytes, $pe + 4)
    if (-not (($machine -eq 0x14C -and $magic -eq 0x10B) -or ($machine -eq 0x8664 -and $magic -eq 0x20B))) { return $false }
    $optional = $pe + 24
    if ([BitConverter]::ToUInt16($Bytes, $optional + 68) -notin @(2, 3)) { return $false }
    # ImageBase (PE32: 4 bytes at +28; PE32+: 8 bytes at +24) must be 64 KiB aligned.
    $imageBase = if ($magic -eq 0x10B) { [uint64][BitConverter]::ToUInt32($Bytes, $optional + 28) } else { [BitConverter]::ToUInt64($Bytes, $optional + 24) }
    if ($imageBase % 65536) { return $false }
    # Mapping fields share offsets in PE32 and PE32+.
    $sectionAlign = [int64][BitConverter]::ToUInt32($Bytes, $optional + 32)
    $fileAlign = [int64][BitConverter]::ToUInt32($Bytes, $optional + 36)
    $imageSize = [int64][BitConverter]::ToUInt32($Bytes, $optional + 56)
    $headersSize = [int64][BitConverter]::ToUInt32($Bytes, $optional + 60)
    $powerOfTwo = { param($v) $v -gt 0 -and ($v -band ($v - 1)) -eq 0 }
    if (-not (& $powerOfTwo $fileAlign) -or $fileAlign -lt 512 -or $fileAlign -gt 65536 -or
        -not (& $powerOfTwo $sectionAlign) -or $sectionAlign -lt $fileAlign) { return $false }
    # Below the 4 KiB page size, Windows requires FileAlignment == SectionAlignment.
    if ($sectionAlign -lt 4096 -and $fileAlign -ne $sectionAlign) { return $false }
    $sections = [BitConverter]::ToUInt16($Bytes, $pe + 6)
    $table = [int64]$optional + $optionalSize
    # A directly launched EXE: no DLL flag, and AddressOfEntryPoint inside an
    # executable section. Sections start after the headers, so zero never matches.
    if (([BitConverter]::ToUInt16($Bytes, $pe + 22) -band 0x2000) -ne 0) { return $false }
    $entry = [int64][BitConverter]::ToUInt32($Bytes, $optional + 16)
    $entryFound = $false
    # Image sections are ascending and adjacent in RVA order, with raw data in the same order.
    $nextAddress = -1
    $rawEnd = 0
    # The Windows loader accepts at most 96 sections.
    if ($sections -eq 0 -or $sections -gt 96 -or $table + 40 * $sections -gt $Bytes.Length) { return $false }
    if ($headersSize -lt $table + 40 * $sections -or $headersSize % $fileAlign -or $headersSize -gt $Bytes.Length) { return $false }
    if ($imageSize % $sectionAlign -or $imageSize -lt $headersSize) { return $false }
    # File-backed RVA ranges: the headers, then each section's raw-data part.
    $backed = New-Object System.Collections.Generic.List[object]
    $backed.Add(@(0, $headersSize))
    for ($i = 0; $i -lt $sections; $i++) {
        $header = [int]($table + 40 * $i)
        $virtualSize = [int64][BitConverter]::ToUInt32($Bytes, $header + 8)
        $address = [int64][BitConverter]::ToUInt32($Bytes, $header + 12)
        $rawSize = [int64][BitConverter]::ToUInt32($Bytes, $header + 16)
        $rawPointer = [int64][BitConverter]::ToUInt32($Bytes, $header + 20)
        if ($rawPointer + $rawSize -gt $Bytes.Length) { return $false }
        # The spec asks for file-aligned raw sizes, but the loader accepts unaligned
        # ones (MinGW images such as git.exe use them), so only the start is checked.
        if ($rawSize -and ($rawPointer % $fileAlign -or $rawPointer -lt $headersSize)) { return $false }
        # Below page size, sections must also map identically: file offset == RVA.
        if ($sectionAlign -lt 4096 -and $rawSize -and $rawPointer -ne $address) { return $false }
        $span = if ($virtualSize) { $virtualSize } else { $rawSize }
        if ($address % $sectionAlign -or $address -lt $headersSize -or $address + $span -gt $imageSize) { return $false }
        if ($nextAddress -ge 0 -and $address -ne $nextAddress) { return $false }
        $end = $address + $span
        $nextAddress = $end + (($sectionAlign - ($end % $sectionAlign)) % $sectionAlign)
        if ($rawSize) {
            if ($rawPointer -lt $rawEnd) { return $false }
            $rawEnd = $rawPointer + $rawSize
        }
        $executable = ([BitConverter]::ToUInt32($Bytes, $header + 36) -band 0x20000000) -ne 0
        # The entry must be in file-backed bytes, not the zero-filled virtual tail.
        if ($executable -and $entry -ge $address -and $entry -lt $address + [Math]::Min($span, $rawSize)) { $entryFound = $true }
        $backed.Add(@($address, ($address + [Math]::Min($span, $rawSize))))
    }
    # SizeOfImage is exactly the last section's end rounded up to SectionAlignment.
    if ($imageSize -ne $nextAddress) { return $false }
    # Each nonempty RVA data directory must lie within one file-backed range so the
    # loader can read it; contents are not parsed. The certificate directory
    # (index 4) is a file offset the loader ignores, and NSIS uninstallers can
    # inherit a stale entry from a signed installer that points past their end.
    for ($d = 0; $d -lt $directories; $d++) {
        if ($d -eq 4) { continue }
        $slot = $pe + 24 + $fixedSize + 8 * $d
        $start = [int64][BitConverter]::ToUInt32($Bytes, $slot)
        $length = [int64][BitConverter]::ToUInt32($Bytes, $slot + 4)
        if (-not ($start -or $length)) { continue }
        if (-not @($backed | Where-Object { $start -ge $_[0] -and $start + $length -le $_[1] }).Count) { return $false }
    }
    $entryFound
}

# 7-Zip 26.02 fixed NSIS 3.10+ parsing so it lists and extracts the generated
# uninstaller; 26.00-26.01 and older can omit it (ip7z/7zip#225). Require it only from 26.02.
function Get-SevenZipVersion([string]$SevenZip) {
    foreach ($line in @(& $SevenZip i 2>&1)) {
        if ([string]$line -match '^7-Zip(?: \([A-Za-z0-9 ]+\))? ([0-9]{1,3})\.([0-9]{1,3})\b') {
            return New-Object Version ([int]$Matches[1]), ([int]$Matches[2])
        }
    }
    throw 'bundle_seven_zip_version: 7-Zip version was not recognized.'
}

function Test-SevenZipExposesUninstaller([version]$Version) { $Version -ge (New-Object Version 26, 2) }

function Get-BundleExecutableIdentity([string]$BuiltExe, [string]$PackedExe) {
    $built = [IO.File]::ReadAllBytes($BuiltExe)
    $packed = [IO.File]::ReadAllBytes($PackedExe)
    if ($built.Length -ne $packed.Length) { throw 'bundle_length_exe: built and packed executable lengths differ.' }

    # ASCII decoding preserves one character per byte, including replacement of
    # non-ASCII bytes. Ordinal searches therefore return exact byte offsets.
    $prefix = '__TAURI_BUNDLE_TYPE_VAR_'
    $text = [Text.Encoding]::ASCII.GetString($built)
    $offset = $text.IndexOf($prefix, [StringComparison]::Ordinal)
    if ($offset -lt 0) { throw 'bundle_marker_missing: built executable has no Tauri bundle-type marker.' }
    if ($text.IndexOf($prefix, $offset + 1, [StringComparison]::Ordinal) -ge 0) {
        throw 'bundle_marker_duplicated: built executable has multiple Tauri bundle-type markers.'
    }
    $valueOffset = $offset + $prefix.Length
    if ($valueOffset + 3 -gt $built.Length -or $text.Substring($valueOffset, 3) -cne 'UNK') {
        throw 'bundle_marker_built_value: built executable marker must be UNK.'
    }
    if ([Text.Encoding]::ASCII.GetString($packed, $offset, $prefix.Length + 3) -cne ($prefix + 'NSS')) {
        throw 'bundle_marker_packed_value: packed executable must have the NSS marker at the same offset.'
    }

    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $builtHash = [BitConverter]::ToString($sha.ComputeHash($built)).Replace('-', '').ToLowerInvariant()
        $packedHash = [BitConverter]::ToString($sha.ComputeHash($packed)).Replace('-', '').ToLowerInvariant()
        # Patch only this in-memory copy; the build output is never modified.
        [Array]::Copy([Text.Encoding]::ASCII.GetBytes('NSS'), 0, $built, $valueOffset, 3)
        $patchedHash = [BitConverter]::ToString($sha.ComputeHash($built)).Replace('-', '').ToLowerInvariant()
        if ($patchedHash -cne $packedHash) { throw 'bundle_hash_exe: executable differs outside the Tauri bundle-type patch.' }
    } finally {
        $sha.Dispose()
    }
    [pscustomobject]@{ built_sha256 = $builtHash; marker_offset = $offset; packed_sha256 = $packedHash }
}

function Write-BundleManifest([string]$PayloadRoot, [string]$Installer, [string]$BuiltExe, [string]$SourceRoot, [switch]$RequireUninstaller) {
    $manifestPath = Join-Path (Split-Path -Parent $Installer) 'bundle-manifest.json'
    # Never leave a stale successful manifest after a failed verification attempt.
    if (Test-Path -LiteralPath $manifestPath) { Remove-Item -LiteralPath $manifestPath -Force }
    $files = @(Get-BundlePayloadFiles $PayloadRoot | Sort-Object -Property path -CaseSensitive)
    $expected = [ordered]@{
        'maxed.exe' = @{ Source = $BuiltExe; Reason = 'exe' }
        'LICENSE' = @{ Source = (Join-Path $SourceRoot 'LICENSE'); Reason = 'license' }
        'NOTICE.md' = @{ Source = (Join-Path $SourceRoot 'NOTICE.md'); Reason = 'notice' }
        'THIRD-PARTY-NOTICES.md' = @{ Source = (Join-Path $SourceRoot 'THIRD-PARTY-NOTICES.md'); Reason = 'third_party_notices' }
        'demo/story.pcap' = @{ Source = (Join-Path $SourceRoot 'tests/fixtures/scrubbed/maxed-demo/story.pcap'); Reason = 'demo_story' }
        'demo/catalog.json' = @{ Source = (Join-Path $SourceRoot 'tests/fixtures/scrubbed/maxed-demo/catalog.json'); Reason = 'demo_catalog' }
    }
    foreach ($path in $expected.Keys) {
        $entry = @($files | Where-Object { $_.path -ceq $path })
        $reason = $expected[$path].Reason
        if ($entry.Count -ne 1) { throw "bundle_missing_${reason}: required payload file is missing." }
        if ($path -ceq 'maxed.exe') {
            $identity = Get-BundleExecutableIdentity -BuiltExe $BuiltExe -PackedExe (Join-Path $PayloadRoot $path)
            if ($entry[0].sha256 -cne $identity.packed_sha256) { throw 'bundle_hash_exe: packed executable changed during verification.' }
            continue
        }
        if ($entry[0].sha256 -cne (Get-FileHash -LiteralPath $expected[$path].Source -Algorithm SHA256).Hash.ToLowerInvariant()) {
            throw "bundle_hash_${reason}: payload differs from its build or repository source."
        }
    }
    # Required when this 7-Zip exposes the generated uninstaller, optional for older
    # versions that omit it; its hash is recorded below.
    $uninstaller = @($files | Where-Object { $_.path -ceq 'uninstall.exe' })
    if ($RequireUninstaller -and $uninstaller.Count -ne 1) {
        throw 'bundle_missing_uninstaller: this 7-Zip exposes the NSIS uninstaller, but the payload has none.'
    }
    if ($uninstaller.Count) {
        if (-not (Test-BundlePortableExecutable ([IO.File]::ReadAllBytes((Join-Path $PayloadRoot 'uninstall.exe'))))) {
            throw 'bundle_uninstaller_not_pe: uninstall.exe is not a complete Windows executable.'
        }
    }
    $installerFile = Get-Item -LiteralPath $Installer
    $installerName = Assert-BundleEntryPath $installerFile.Name
    $manifest = [ordered]@{
        installer = [ordered]@{ path = $installerName; bytes = $installerFile.Length
            sha256 = (Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash.ToLowerInvariant() }
        executable = [ordered]@{ path = 'maxed.exe'; built_sha256 = $identity.built_sha256; marker_offset = $identity.marker_offset }
        files = $files
    }
    [IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 5) + "`n", (New-Object Text.UTF8Encoding($false)))
    return $manifestPath
}

function Export-VerifiedBundle([string]$Installer, [string]$TargetDir, [string]$BuiltExe, [string]$SourceRoot) {
    $manifestPath = Join-Path (Split-Path -Parent $Installer) 'bundle-manifest.json'
    if (Test-Path -LiteralPath $manifestPath) { Remove-Item -LiteralPath $manifestPath -Force }
    $sevenZip = Get-Command 7z -CommandType Application -ErrorAction SilentlyContinue
    if ($null -eq $sevenZip) { throw '7-Zip (7z) is required on PATH to verify the NSIS payload; no extraction fallback is allowed.' }
    $requireUninstaller = Test-SevenZipExposesUninstaller (Get-SevenZipVersion $sevenZip.Source)
    $parent =[IO.Path]::GetFullPath($TargetDir).TrimEnd('\', '/')
    $stage = Join-Path $parent ('bundle-verify-' + [Guid]::NewGuid().ToString('N'))
    $null = New-Item -ItemType Directory -Path $stage
    try {
        # Restrict staging to this Windows identity before writing extracted bytes.
        $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
        $acl = New-Object Security.AccessControl.DirectorySecurity
        $acl.SetOwner($sid)
        $acl.SetAccessRuleProtection($true, $false)
        $rule = New-Object Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')
        $acl.AddAccessRule($rule)
        Set-Acl -LiteralPath $stage -AclObject $acl
        $listing = @(& $sevenZip.Source l -slt -ba -- $Installer 2>&1)
        if ($LASTEXITCODE -ne 0) { throw 'bundle_listing_failed: 7-Zip could not list the NSIS payload.' }
        # Inspect names BEFORE extraction, so 7-Zip cannot normalize traversal away.
        $archivePaths = @(Get-BundleArchivePaths $listing)
        & $sevenZip.Source x -y "-o$stage" -- $Installer *> $null
        if ($LASTEXITCODE -ne 0) { throw 'bundle_extraction_failed: 7-Zip could not extract the NSIS payload.' }
        $extractedPaths = @(Get-BundlePayloadFiles $stage | ForEach-Object { $_.path })
        if (@(Compare-Object $archivePaths $extractedPaths -CaseSensitive).Count) {
            throw 'bundle_extraction_mismatch: listed and extracted payload paths differ.'
        }
        Write-BundleManifest -PayloadRoot $stage -Installer $Installer -BuiltExe $BuiltExe -SourceRoot $SourceRoot -RequireUninstaller:$requireUninstaller
    } finally {
        $resolved = [IO.Path]::GetFullPath($stage)
        if (-not $resolved.StartsWith($parent + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
            ((Get-Item -LiteralPath $resolved -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Refusing bundle cleanup outside the dedicated target staging directory.'
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}

if ($MyInvocation.InvocationName -eq '.') { return }
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $repoRoot
try {
    if ($Package) {
        $config = Get-Content -LiteralPath (Join-Path $repoRoot 'app/src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
        $updaterArtifacts = $config.bundle.PSObject.Properties['createUpdaterArtifacts']
        $signedPackage = $false
        # Only an absent field or literal JSON false disables updater artifacts.
        # In particular, Tauri's v1Compatible mode still requires signing.
        if ($null -ne $updaterArtifacts -and -not ($updaterArtifacts.Value -is [bool] -and $updaterArtifacts.Value -eq $false)) {
            $signedPackage = $true
            if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
                throw 'Updater artifact builds require TAURI_SIGNING_PRIVATE_KEY in the process environment.'
            }
            if ([string]::IsNullOrWhiteSpace($config.plugins.updater.pubkey)) {
                throw 'Signed packages require a resolved plugins.updater.pubkey.'
            }
        } else {
            Write-Warning 'Updater artifacts are disabled: building an unsigned NSIS installer. This is a packaging spike, not a signed production release.'
        }
        $tauriCli = Join-Path $repoRoot 'app/ui/node_modules/@tauri-apps/cli/tauri.js'
        if (-not (Test-Path -LiteralPath $tauriCli -PathType Leaf)) {
            throw 'Locked local Tauri CLI is missing. Restore app/ui dependencies with npm ci first.'
        }
        if ($signedPackage) {
            # Prove this process key/password can sign for the configured public key
            # BEFORE the package build. Never pass secrets on the command line or log
            # signer output. Actual installer signing still belongs to `tauri build`.
            $probeParent = [IO.Path]::GetFullPath((Join-Path $repoRoot 'target'))
            $null = New-Item -ItemType Directory -Force -Path $probeParent
            $probeDir = Join-Path $probeParent ('signing-preflight-' + [Guid]::NewGuid().ToString('N'))
            $null = New-Item -ItemType Directory -Path $probeDir
            try {
                $challenge = Join-Path $probeDir 'challenge'
                [IO.File]::WriteAllText($challenge, 'Maxed disposable package signing preflight ' + [Guid]::NewGuid().ToString('N'))
                try {
                    & node $tauriCli signer sign --app-version $config.version $challenge *> $null
                } catch {
                    throw 'Package signing key/password preflight failed.'
                }
                if ($LASTEXITCODE -ne 0) { throw 'Package signing key/password preflight failed.' }
                & cargo run --offline --target-dir target -p maxed-app --features updater-signing-tool --bin maxed-update-verify -- (Join-Path $repoRoot 'app/src-tauri/tauri.conf.json') $challenge ($challenge + '.sig')
                if ($LASTEXITCODE -ne 0) { throw 'Package signing key does not match the configured verification key/version.' }
            } finally {
                $resolvedProbe = [IO.Path]::GetFullPath($probeDir)
                if (-not $resolvedProbe.StartsWith($probeParent + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                    throw 'Signing preflight cleanup path escaped target.'
                }
                Remove-Item -LiteralPath $resolvedProbe -Recurse -Force
            }
        }
        # Tauri runs beforeBuildCommand and signs updater artifacts when enabled.
        # No npx fallback: packaging must use the locally restored, locked CLI.
        $tauriArgs = @('build', '--ci', '--bundles', 'nsis', '--features', 'maxed-app/custom-protocol')
        if ($Offline) { $tauriArgs += @('--', '--offline') }
        Push-Location -LiteralPath (Join-Path $repoRoot 'app/src-tauri')
        try {
            & node $tauriCli @tauriArgs
            if ($LASTEXITCODE -ne 0) { throw 'NSIS package build failed.' }
        } finally {
            Pop-Location
        }
    } else {
        & npm.cmd --prefix app/ui run build
        if ($LASTEXITCODE -ne 0) { throw 'Frontend release build failed.' }

        $cargoArgs = @('build', '--release', '-p', 'maxed-app', '-p', 'maxed-cli', '--features', 'maxed-app/custom-protocol')
        if ($Offline) { $cargoArgs += '--offline' }
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw 'Native release build failed.' }
    }

    # Respect Cargo target-dir configuration instead of assuming ./target.
    $metadata = & cargo metadata --offline --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Cargo output directory could not be resolved.' }
    $targetDir = ($metadata | ConvertFrom-Json).target_directory
    $releaseDir = Join-Path $targetDir 'release'
    if ($Package) {
        $installer = Join-Path $releaseDir ('bundle/nsis/Maxed_' + $config.version + '_x64-setup.exe')
        $manifestPath = Join-Path (Split-Path -Parent $installer) 'bundle-manifest.json'
        if (Test-Path -LiteralPath $manifestPath) { Remove-Item -LiteralPath $manifestPath -Force }
        # Fail closed on stale dependency/native notices before any bundle manifest exists.
        # This runs after the build because Tauri provisions the NSIS toolset the notices
        # are generated from; a clean runner has no cache before then.
        & (Join-Path $repoRoot 'scripts/release/New-ThirdPartyNotices.ps1') -Check
        if (-not $?) { throw 'Third-party notices verification failed.' }
    }
    if ($Package -and $signedPackage) {
        # No post-build signing. Verify the exact installer and signature emitted
        # by Tauri; failure must not be reported as a successful signed release.
        & cargo run --offline --target-dir target -p maxed-app --features updater-signing-tool --bin maxed-update-verify -- (Join-Path $repoRoot 'app/src-tauri/tauri.conf.json') $installer ($installer + '.sig')
        if ($LASTEXITCODE -ne 0) { throw 'Bundled installer signature verification failed.' }
    }
    if ($Package) {
        Export-VerifiedBundle -Installer $installer -TargetDir $targetDir -BuiltExe (Join-Path $releaseDir 'maxed.exe') -SourceRoot $repoRoot
    }
    $exe = Join-Path $releaseDir 'maxed.exe'
    if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Release executable is missing.' }
    foreach ($resource in @('story.pcap', 'catalog.json')) {
        $source = Join-Path $repoRoot "tests/fixtures/scrubbed/maxed-demo/$resource"
        $copied = Join-Path $releaseDir "demo/$resource"
        if (-not (Test-Path -LiteralPath $copied -PathType Leaf)) { throw "Bundled demo/$resource is missing." }
        if ((Get-FileHash -LiteralPath $source).Hash -ne (Get-FileHash -LiteralPath $copied).Hash) {
            throw "Bundled demo/$resource does not match the synthetic source."
        }
    }
    Write-Output $exe
} finally {
    Pop-Location
}
