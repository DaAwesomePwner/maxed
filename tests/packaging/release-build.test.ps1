[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $repoRoot 'scripts/build-release.ps1')
$script:checks = 0

function Assert-Check([bool]$Condition, [string]$Name) {
    $script:checks++
    if (-not $Condition) { throw "FAIL: $Name" }
    Write-Output "PASS: $Name"
}

# Minimal invented PE32 (or, with -Pe32Plus, AMD64 PE32+) GUI image: DOS header,
# PE signature, COFF header, optional header, one section with its data, then an
# overlay. Never executed.
function New-SyntheticPortableExecutable([switch]$Pe32Plus, [int]$Directories = 0) {
    $bytes = New-Object byte[] 1024
    $fixedSize = if ($Pe32Plus) { 112 } else { 96 }
    $optionalSize = $fixedSize + 8 * $Directories
    [BitConverter]::GetBytes([uint32]$Directories).CopyTo($bytes, 88 + $fixedSize - 4) # NumberOfRvaAndSizes
    $section = 88 + $optionalSize
    $bytes[0] = 0x4D; $bytes[1] = 0x5A
    [BitConverter]::GetBytes([int32]64).CopyTo($bytes, 0x3C)
    $bytes[64] = 0x50; $bytes[65] = 0x45
    [BitConverter]::GetBytes([uint16]$(if ($Pe32Plus) { 0x8664 } else { 0x14C })).CopyTo($bytes, 68) # AMD64 or i386
    [BitConverter]::GetBytes([uint16]1).CopyTo($bytes, 70)          # one section
    [BitConverter]::GetBytes([uint16]$optionalSize).CopyTo($bytes, 84) # optional header size
    [BitConverter]::GetBytes([uint16]0x0102).CopyTo($bytes, 86)     # executable image
    [BitConverter]::GetBytes([uint16]$(if ($Pe32Plus) { 0x20B } else { 0x10B })).CopyTo($bytes, 88) # magic
    [BitConverter]::GetBytes([uint32]4096).CopyTo($bytes, 88 + 32)  # section alignment
    [BitConverter]::GetBytes([uint32]512).CopyTo($bytes, 88 + 36)   # file alignment
    [BitConverter]::GetBytes([uint32]8192).CopyTo($bytes, 88 + 56)  # image size
    [BitConverter]::GetBytes([uint32]512).CopyTo($bytes, 88 + 60)   # headers size
    [BitConverter]::GetBytes([uint16]2).CopyTo($bytes, 88 + 68)     # GUI subsystem
    [BitConverter]::GetBytes([uint32]16).CopyTo($bytes, $section + 8)    # section virtual size
    [BitConverter]::GetBytes([uint32]4096).CopyTo($bytes, $section + 12) # section virtual address
    [BitConverter]::GetBytes([uint32]512).CopyTo($bytes, $section + 16)  # section raw size
    [BitConverter]::GetBytes([uint32]512).CopyTo($bytes, $section + 20)  # section raw offset
    [BitConverter]::GetBytes([uint32]4096).CopyTo($bytes, 88 + 16)  # entry point RVA
    [BitConverter]::GetBytes([uint32]0x60000020).CopyTo($bytes, $section + 36) # code, execute, read
    , [byte[]]($bytes + [Text.Encoding]::ASCII.GetBytes('invented overlay; never executed'))
}

function Assert-Rejected([scriptblock]$Action, [string]$Reason) {
    $message = ''
    try { & $Action | Out-Null } catch { $message = $_.Exception.Message }
    Assert-Check ($message.StartsWith($Reason + ':', [StringComparison]::Ordinal)) "reject $Reason"
}

$tempParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
$temp = Join-Path $tempParent ('maxed-release-build-' + [Guid]::NewGuid().ToString('N'))
try {
    $source = Join-Path $temp 'source'
    $payload = Join-Path $temp 'payload'
    $null = New-Item -ItemType Directory -Path (Join-Path $source 'tests/fixtures/scrubbed/maxed-demo')
    $null = New-Item -ItemType Directory -Path (Join-Path $payload 'demo')
    $null = New-Item -ItemType Directory -Path (Join-Path $payload '$PLUGINSDIR')
    $builtExe = Join-Path $temp 'maxed.exe'
    $installer = Join-Path $temp 'Maxed_0.1.0_x64-setup.exe'
    $marker = '__TAURI_BUNDLE_TYPE_VAR_'
    $builtText = 'MZ invented app bytes; ' + $marker + 'UNK; never executed'
    $packedText = $builtText.Replace($marker + 'UNK', $marker + 'NSS')
    $packedExe = Join-Path $payload 'maxed.exe'
    [IO.File]::WriteAllText($builtExe, $builtText)
    [IO.File]::WriteAllText($installer, 'MZ invented installer bytes; never executed')
    $fixtures = [ordered]@{
        'maxed.exe' = $builtExe
        'LICENSE' = (Join-Path $source 'LICENSE')
        'NOTICE.md' = (Join-Path $source 'NOTICE.md')
        'THIRD-PARTY-NOTICES.md' = (Join-Path $source 'THIRD-PARTY-NOTICES.md')
        'demo/story.pcap' = (Join-Path $source 'tests/fixtures/scrubbed/maxed-demo/story.pcap')
        'demo/catalog.json' = (Join-Path $source 'tests/fixtures/scrubbed/maxed-demo/catalog.json')
    }
    foreach ($path in $fixtures.Keys) {
        if ($path -ne 'maxed.exe') { [IO.File]::WriteAllText($fixtures[$path], "Invented fixture: $path") }
        Copy-Item -LiteralPath $fixtures[$path] -Destination (Join-Path $payload $path)
    }
    [IO.File]::WriteAllText($packedExe, $packedText)
    foreach ($plugin in @('System.dll', 'nsDialogs.dll', 'NSISdl.dll', 'nsis_tauri_utils.dll', 'StartMenu.dll', 'modern-wizard.bmp')) {
        [IO.File]::WriteAllText((Join-Path $payload ('$PLUGINSDIR/' + $plugin)), 'Invented plugin; never executed')
    }
    $inputs = @{ PayloadRoot = $payload; Installer = $installer; BuiltExe = $builtExe; SourceRoot = $source }
    $manifestPath = Write-BundleManifest @inputs
    $json = Get-Content -LiteralPath $manifestPath -Raw
    $manifest = $json | ConvertFrom-Json
    Assert-Check (@($manifest.files).Count -eq 12) 'good payload includes every extracted file'
    Assert-Check ($manifest.executable.path -ceq 'maxed.exe') 'executable relation identifies packed file'
    Assert-Check ($manifest.executable.built_sha256 -ceq (Get-FileHash -LiteralPath $builtExe).Hash.ToLowerInvariant()) 'manifest records original build hash'
    Assert-Check ($manifest.executable.marker_offset -eq $builtText.IndexOf($marker, [StringComparison]::Ordinal)) 'manifest records zero-based marker prefix byte offset'
    $exeEntry = @($manifest.files | Where-Object { $_.path -ceq 'maxed.exe' })[0]
    Assert-Check ($exeEntry.sha256 -cne $manifest.executable.built_sha256) 'manifest retains the distinct packed executable hash'
    Assert-Check ([IO.File]::ReadAllText($builtExe) -ceq $builtText) 'verification leaves build output unpatched'
    Assert-Check ($manifest.installer.path -ceq 'Maxed_0.1.0_x64-setup.exe') 'installer name is relative'
    Assert-Check ($manifest.installer.bytes -eq (Get-Item -LiteralPath $installer).Length) 'installer byte count'
    Assert-Check ($manifest.installer.sha256 -ceq (Get-FileHash -LiteralPath $installer).Hash.ToLowerInvariant()) 'installer hash'
    $paths = @($manifest.files | ForEach-Object { $_.path })
    Assert-Check (($paths -join '|') -ceq (($paths | Sort-Object -CaseSensitive) -join '|')) 'payload paths sorted'
    foreach ($entry in $manifest.files) {
        Assert-Check ($entry.path -notmatch '\\|:|^/' -and -not [IO.Path]::IsPathRooted($entry.path)) 'relative forward-slash payload path'
        $file = Join-Path $payload $entry.path
        Assert-Check ($entry.bytes -eq (Get-Item -LiteralPath $file).Length -and
            $entry.sha256 -ceq (Get-FileHash -LiteralPath $file).Hash.ToLowerInvariant()) 'payload size and hash identity'
    }
    Assert-Check ($json -notmatch '[A-Za-z]:|\\\\|"/' -and -not $json.Contains($temp)) 'manifest JSON contains no absolute paths'
    # 7-Zip 26.02+ extracts the NSIS-generated uninstaller: a complete PE is recorded,
    # MZ-prefixed junk, truncation or out-of-bounds headers are rejected.
    $uninstaller = Join-Path $payload 'uninstall.exe'
    $pe = New-SyntheticPortableExecutable
    [IO.File]::WriteAllBytes($uninstaller, $pe)
    $withUninstaller = Get-Content -LiteralPath (Write-BundleManifest @inputs) -Raw | ConvertFrom-Json
    $recorded = @($withUninstaller.files | Where-Object { $_.path -ceq 'uninstall.exe' })
    Assert-Check ($recorded.Count -eq 1 -and @($withUninstaller.files).Count -eq 13 -and
        $recorded[0].sha256 -ceq (Get-FileHash -LiteralPath $uninstaller).Hash.ToLowerInvariant()) 'generated uninstaller accepted with its hash'
    $badOffset = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([int32]4096).CopyTo($badOffset, 0x3C)
    $badSection = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]4096).CopyTo($badSection, 184 + 16)
    $coffObject = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint16]0).CopyTo($coffObject, 84)
    $notExecutable = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint16]0x0100).CopyTo($notExecutable, 86)
    $badMagic = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint16]0x107).CopyTo($badMagic, 88)
    $nativeSubsystem = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint16]1).CopyTo($nativeSubsystem, 88 + 68)
    $zeroFileAlign = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]0).CopyTo($zeroFileAlign, 88 + 36)
    $smallImage = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]4096).CopyTo($smallImage, 88 + 56)
    $unalignedImage = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]8191).CopyTo($unalignedImage, 88 + 56)
    $inflatedImage = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([int32]0xFFFFF000).CopyTo($inflatedImage, 88 + 56)
    $extraPageImage = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]12288).CopyTo($extraPageImage, 88 + 56)
    # The loader accepts unaligned raw sizes (MinGW images use them).
    $unalignedRaw = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]500).CopyTo($unalignedRaw, 184 + 16)
    Assert-Check (Test-BundlePortableExecutable $unalignedRaw) 'unaligned section raw size accepted'
    $lowSectionAlign = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]1024).CopyTo($lowSectionAlign, 88 + 32)
    [BitConverter]::GetBytes([uint32]1024).CopyTo($lowSectionAlign, 184 + 12); [BitConverter]::GetBytes([uint32]2048).CopyTo($lowSectionAlign, 88 + 56)
    [BitConverter]::GetBytes([uint32]1024).CopyTo($lowSectionAlign, 88 + 16)
    # Equal 512-byte alignments are valid only when the section's file offset equals its RVA.
    $lowAlignOffset = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]512).CopyTo($lowAlignOffset, 88 + 32)
    $lowAlignIdentity = [byte[]]$lowAlignOffset.Clone(); [BitConverter]::GetBytes([uint32]512).CopyTo($lowAlignIdentity, 184 + 12)
    [BitConverter]::GetBytes([uint32]1024).CopyTo($lowAlignIdentity, 88 + 56); [BitConverter]::GetBytes([uint32]512).CopyTo($lowAlignIdentity, 88 + 16)
    Assert-Check (Test-BundlePortableExecutable $lowAlignIdentity) 'sub-page alignment accepted when file offset equals RVA'
    $amd64Pe32 = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint16]0x8664).CopyTo($amd64Pe32, 68)
    $unknownMachine = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint16]0x1C0).CopyTo($unknownMachine, 68)
    $noEntry = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]0).CopyTo($noEntry, 88 + 16)
    $dataOnlyEntry = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]0x40000040).CopyTo($dataOnlyEntry, 184 + 36)
    $entryOutside = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]4112).CopyTo($entryOutside, 88 + 16)
    $dllImage = [byte[]]$noEntry.Clone(); [BitConverter]::GetBytes([uint16]0x2102).CopyTo($dllImage, 86)
    Assert-Check (Test-BundlePortableExecutable $pe) 'synthetic EXE with an entry point in its code section accepted'
    $amd64 = New-SyntheticPortableExecutable -Pe32Plus
    Assert-Check (Test-BundlePortableExecutable $amd64) 'synthetic AMD64 PE32+ EXE accepted'
    $arm64 = [byte[]]$amd64.Clone(); [BitConverter]::GetBytes([uint16]0xAA64).CopyTo($arm64, 68)
    $realBase = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]0x400000).CopyTo($realBase, 88 + 28)
    $realBase64 = [byte[]]$amd64.Clone(); [BitConverter]::GetBytes([uint64]0x140000000).CopyTo($realBase64, 88 + 24)
    Assert-Check ((Test-BundlePortableExecutable $realBase) -and (Test-BundlePortableExecutable $realBase64)) '64 KiB-aligned image bases accepted'
    $oddBase = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]1).CopyTo($oddBase, 88 + 28)
    $oddBase64 = [byte[]]$amd64.Clone(); [BitConverter]::GetBytes([uint64]0x140001000).CopyTo($oddBase64, 88 + 24)
    $withDirectories = New-SyntheticPortableExecutable -Directories 16
    Assert-Check ((Test-BundlePortableExecutable $withDirectories) -and (Test-BundlePortableExecutable (New-SyntheticPortableExecutable -Pe32Plus -Directories 16))) '16 data directories inside the optional header accepted'
    $directoriesPastHeader = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]16).CopyTo($directoriesPastHeader, 88 + 92)
    $oneTooMany = [byte[]]$withDirectories.Clone(); [BitConverter]::GetBytes([uint32]17).CopyTo($oneTooMany, 88 + 92)
    # Directory slot d of the PE32 fixture is at 88 + 96 + 8 * d.
    $importOutside = [byte[]]$withDirectories.Clone(); [BitConverter]::GetBytes([uint32]0x3000).CopyTo($importOutside, 88 + 96 + 8)
    [BitConverter]::GetBytes([uint32]0x100).CopyTo($importOutside, 88 + 96 + 12)
    $staleCertificate = [byte[]]$withDirectories.Clone(); [BitConverter]::GetBytes([uint32]0x100000).CopyTo($staleCertificate, 88 + 96 + 32)
    [BitConverter]::GetBytes([uint32]0x3000).CopyTo($staleCertificate, 88 + 96 + 36)
    Assert-Check (Test-BundlePortableExecutable $staleCertificate) 'stale certificate directory past end of file accepted (NSIS uninstallers inherit one)'
    $importUnbacked = [byte[]]$withDirectories.Clone(); [BitConverter]::GetBytes([uint32]0x1800).CopyTo($importUnbacked, 88 + 96 + 8)
    [BitConverter]::GetBytes([uint32]0x14).CopyTo($importUnbacked, 88 + 96 + 12)
    $backedDirectories = [byte[]]$withDirectories.Clone()
    [BitConverter]::GetBytes([uint32]4096).CopyTo($backedDirectories, 88 + 96 + 8); [BitConverter]::GetBytes([uint32]16).CopyTo($backedDirectories, 88 + 96 + 12)
    [BitConverter]::GetBytes([uint32]0x100).CopyTo($backedDirectories, 88 + 96 + 88); [BitConverter]::GetBytes([uint32]0x10).CopyTo($backedDirectories, 88 + 96 + 92)
    Assert-Check (Test-BundlePortableExecutable $backedDirectories) 'directories in section raw bytes and in the headers accepted'
    # N adjacent sections: code (raw 512@4096) then uninitialized data, headers sized to fit.
    $manySections = {
        param([int]$Count)
        $bytes = New-Object byte[] 4608
        [Array]::Copy($pe, $bytes, 184)
        [BitConverter]::GetBytes([uint16]$Count).CopyTo($bytes, 70)
        [BitConverter]::GetBytes([uint32]4096).CopyTo($bytes, 88 + 60)
        [BitConverter]::GetBytes([uint32](4096 * ($Count + 1))).CopyTo($bytes, 88 + 56)
        for ($i = 0; $i -lt $Count; $i++) {
            $h = 184 + 40 * $i
            [BitConverter]::GetBytes([uint32]16).CopyTo($bytes, $h + 8)
            [BitConverter]::GetBytes([uint32](4096 * ($i + 1))).CopyTo($bytes, $h + 12)
            if ($i -eq 0) {
                [BitConverter]::GetBytes([uint32]512).CopyTo($bytes, $h + 16)
                [BitConverter]::GetBytes([uint32]4096).CopyTo($bytes, $h + 20)
                [BitConverter]::GetBytes([uint32]0x60000020).CopyTo($bytes, $h + 36)
            } else { [BitConverter]::GetBytes([int32]0xC0000080).CopyTo($bytes, $h + 36) } # uninitialized data, read, write
        }
        , $bytes
    }
    Assert-Check (Test-BundlePortableExecutable (& $manySections 96)) '96 adjacent sections accepted'
    $tooManySections = & $manySections 97
    $noRawEntry = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]0).CopyTo($noRawEntry, 184 + 16)
    $tailEntry = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]1024).CopyTo($tailEntry, 184 + 8)
    [BitConverter]::GetBytes([uint32](4096 + 600)).CopyTo($tailEntry, 88 + 16)
    $rawEntry = [byte[]]$tailEntry.Clone(); [BitConverter]::GetBytes([uint32](4096 + 400)).CopyTo($rawEntry, 88 + 16)
    Assert-Check (Test-BundlePortableExecutable $rawEntry) 'entry point inside raw bytes of a larger virtual section accepted'
    # Two sections: code at RVA 4096 (raw 512@512), data at RVA 8192 (raw 512@1024).
    $twoSections = New-Object byte[] 2048
    [Array]::Copy($pe, $twoSections, 1024)
    [BitConverter]::GetBytes([uint16]2).CopyTo($twoSections, 70)
    [BitConverter]::GetBytes([uint32]12288).CopyTo($twoSections, 88 + 56)
    [BitConverter]::GetBytes([uint32]16).CopyTo($twoSections, 224 + 8)
    [BitConverter]::GetBytes([uint32]8192).CopyTo($twoSections, 224 + 12)
    [BitConverter]::GetBytes([uint32]512).CopyTo($twoSections, 224 + 16)
    [BitConverter]::GetBytes([uint32]1024).CopyTo($twoSections, 224 + 20)
    [BitConverter]::GetBytes([uint32]0x40000040).CopyTo($twoSections, 224 + 36)
    Assert-Check (Test-BundlePortableExecutable $twoSections) 'adjacent ascending sections accepted'
    $sectionGap = [byte[]]$twoSections.Clone(); [BitConverter]::GetBytes([uint32]12288).CopyTo($sectionGap, 224 + 12)
    [BitConverter]::GetBytes([uint32]16384).CopyTo($sectionGap, 88 + 56)
    $sectionOverlap = [byte[]]$twoSections.Clone(); [BitConverter]::GetBytes([uint32]4096).CopyTo($sectionOverlap, 224 + 12)
    $rawReversed = [byte[]]$twoSections.Clone(); [BitConverter]::GetBytes([uint32]1024).CopyTo($rawReversed, 184 + 20)
    [BitConverter]::GetBytes([uint32]512).CopyTo($rawReversed, 224 + 20)
    $shortHeaders = [byte[]]$pe.Clone(); [BitConverter]::GetBytes([uint32]0).CopyTo($shortHeaders, 88 + 60)
    $broken = [ordered]@{
        'MZ-prefixed junk' = [Text.Encoding]::ASCII.GetBytes('MZ invented non-PE bytes; never executed')
        'truncated image' = [byte[]]$pe[0..199]
        'out-of-bounds PE offset' = $badOffset
        'section data past end of file' = $badSection
        'COFF object without optional header' = $coffObject
        'not an executable image' = $notExecutable
        'unknown optional-header magic' = $badMagic
        'non-GUI, non-console subsystem' = $nativeSubsystem
        'zero file alignment' = $zeroFileAlign
        'image smaller than its section' = $smallImage
        'image size not section-aligned' = $unalignedImage
        'SizeOfImage inflated to 0xFFFFF000' = $inflatedImage
        'SizeOfImage one page past the last section' = $extraPageImage
        'sub-page section alignment differing from file alignment' = $lowSectionAlign
        'sub-page section whose file offset differs from its RVA' = $lowAlignOffset
        'AMD64 machine with a PE32 optional header' = $amd64Pe32
        'unsupported machine' = $unknownMachine
        'EXE without an entry point' = $noEntry
        'entry point in a non-executable section' = $dataOnlyEntry
        'entry point past its section' = $entryOutside
        'DLL image without an entry point' = $dllImage
        'ARM64 PE32+ image in the x64 package' = $arm64
        'entry point in a section without raw data' = $noRawEntry
        'entry point in the zero-filled section tail' = $tailEntry
        'gap between section RVAs' = $sectionGap
        'overlapping section RVAs' = $sectionOverlap
        'raw section data out of RVA order' = $rawReversed
        'PE32 image base not 64 KiB aligned' = $oddBase
        'PE32+ image base not 64 KiB aligned' = $oddBase64
        'more than 96 sections' = $tooManySections
        'data directories past a 96-byte optional header' = $directoriesPastHeader
        'one data directory more than the optional header holds' = $oneTooMany
        'import directory outside SizeOfImage' = $importOutside
        'import directory in unmapped space below SizeOfImage' = $importUnbacked
        'headers size not covering the section table' = $shortHeaders
    }
    foreach ($case in $broken.GetEnumerator()) {
        [IO.File]::WriteAllBytes($uninstaller, $case.Value)
        $message = ''
        try { $null = Write-BundleManifest @inputs } catch { $message = $_.Exception.Message }
        Assert-Check ($message.StartsWith('bundle_uninstaller_not_pe:', [StringComparison]::Ordinal)) "reject uninstaller: $($case.Key)"
    }
    Remove-Item -LiteralPath $uninstaller
    # Required once 7-Zip exposes it (26.02+); optional for older extractors.
    Assert-Rejected { Write-BundleManifest @inputs -RequireUninstaller } 'bundle_missing_uninstaller'
    $null = Write-BundleManifest @inputs
    Assert-Check $true 'uninstaller optional when this 7-Zip omits it'
    Assert-Check (-not (Test-SevenZipExposesUninstaller ([version]'25.1')) -and -not (Test-SevenZipExposesUninstaller ([version]'26.1')) -and (Test-SevenZipExposesUninstaller ([version]'26.2')) -and (Test-SevenZipExposesUninstaller ([version]'27.0'))) 'uninstaller required from 7-Zip 26.02'
    $stub = Join-Path $temp '7z-stub.cmd'
    [IO.File]::WriteAllText($stub, "@echo off`r`necho.`r`necho 7-Zip 26.04 (x64) : Copyright (c) 1999-2026 Igor Pavlov : 2026-10-05`r`n")
    Assert-Check ((Get-SevenZipVersion $stub) -eq [version]'26.4') '7-Zip version parsed from i output'
    [IO.File]::WriteAllText($stub, "@echo off`r`necho Invented unrelated tool`r`n")
    Assert-Rejected { Get-SevenZipVersion $stub } 'bundle_seven_zip_version'
    $listing = @($paths | ForEach-Object { 'Path = ' + $_.Replace('/', '\') })
    Assert-Check (@(Get-BundleArchivePaths $listing).Count -eq 12) '7-Zip paths normalize before extraction'
    Assert-Rejected { Get-BundleArchivePaths @('Path = ../maxed.exe') } 'bundle_path_traversal'
    Assert-Rejected { Get-BundleArchivePaths @('Path = demo/../maxed.exe') } 'bundle_path_traversal'
    Assert-Rejected { Get-BundleArchivePaths @('Path = C:\maxed.exe') } 'bundle_path_traversal'
    Assert-Rejected { Get-BundleArchivePaths @('Path = \\server\maxed.exe') } 'bundle_path_traversal'
    Assert-Rejected { Get-BundleArchivePaths @('Path = maxed.exe:stream') } 'bundle_path_traversal'
    Assert-Rejected { Get-BundleArchivePaths @('Path = demo//story.pcap') } 'bundle_path_traversal'
    Assert-Rejected { Get-BundleArchivePaths @('Path = maxed.exe.') } 'bundle_path_traversal'
    Assert-Rejected { Get-BundleArchivePaths @('Path = maxed.exe', 'Path = maxed.exe') } 'bundle_duplicate_path'
    Assert-Rejected { Get-BundleArchivePaths @('Size = 0') } 'bundle_empty_archive'

    foreach ($mutation in @(
        @{ Built = $builtText.Replace($marker, ('_' * $marker.Length)); Packed = $packedText; Reason = 'bundle_marker_missing' },
        @{ Built = $builtText + $builtText; Packed = $packedText + $packedText; Reason = 'bundle_marker_duplicated' },
        @{ Built = $builtText.Replace($marker + 'UNK', $marker + 'MSI'); Packed = $packedText; Reason = 'bundle_marker_built_value' },
        @{ Built = $marker; Packed = $marker; Reason = 'bundle_marker_built_value' },
        @{ Built = $builtText; Packed = $builtText; Reason = 'bundle_marker_packed_value' },
        @{ Built = $builtText; Packed = $packedText.Replace($marker + 'NSS', $marker + 'MSI'); Reason = 'bundle_marker_packed_value' },
        @{ Built = $builtText; Packed = $packedText.Replace($marker, ('_' * $marker.Length)); Reason = 'bundle_marker_packed_value' },
        @{ Built = $builtText; Packed = $packedText + 'x'; Reason = 'bundle_length_exe' }
    )) {
        [IO.File]::WriteAllText($builtExe, $mutation.Built)
        [IO.File]::WriteAllText($packedExe, $mutation.Packed)
        Assert-Rejected { Write-BundleManifest @inputs } $mutation.Reason
        Assert-Check (-not (Test-Path -LiteralPath $manifestPath)) 'marker failure leaves no manifest'
    }
    # Binary bytes before the marker must preserve byte offsets, and changes
    # that decode to the same ASCII replacement character must still fail hashes.
    $binaryPrefix = [byte[]]@(0, 128, 255, 0)
    $binaryBuilt = [byte[]]($binaryPrefix + [Text.Encoding]::ASCII.GetBytes($builtText))
    $binaryPacked = [byte[]]($binaryPrefix + [Text.Encoding]::ASCII.GetBytes($packedText))
    [IO.File]::WriteAllBytes($builtExe, $binaryBuilt)
    [IO.File]::WriteAllBytes($packedExe, $binaryPacked)
    $binaryManifestPath = Write-BundleManifest @inputs
    $binaryManifest = Get-Content -LiteralPath $binaryManifestPath -Raw | ConvertFrom-Json
    Assert-Check ($binaryManifest.executable.marker_offset -eq ($binaryPrefix.Length + $builtText.IndexOf($marker, [StringComparison]::Ordinal))) 'binary prefix preserves marker byte offset'
    $binaryPacked[1] = 129
    [IO.File]::WriteAllBytes($packedExe, $binaryPacked)
    Assert-Rejected { Write-BundleManifest @inputs } 'bundle_hash_exe'
    [IO.File]::WriteAllText($builtExe, $builtText)
    [IO.File]::WriteAllText($packedExe, $packedText)

    $reasons = @('exe', 'license', 'notice', 'third_party_notices', 'demo_story', 'demo_catalog')
    $index = 0
    foreach ($path in $fixtures.Keys) {
        $file = Join-Path $payload $path
        if ($path -ceq 'maxed.exe') {
            $changed = [IO.File]::ReadAllBytes($file)
            $changed[0] = $changed[0] -bxor 1
            [IO.File]::WriteAllBytes($file, $changed)
        } else {
            [IO.File]::WriteAllText($file, 'Mutated synthetic bytes')
        }
        Assert-Rejected { Write-BundleManifest @inputs } ('bundle_hash_' + $reasons[$index])
        Assert-Check (-not (Test-Path -LiteralPath $manifestPath)) 'failed verification removes stale manifest'
        Remove-Item -LiteralPath $file
        Assert-Rejected { Write-BundleManifest @inputs } ('bundle_missing_' + $reasons[$index])
        Copy-Item -LiteralPath $fixtures[$path] -Destination $file
        if ($path -ceq 'maxed.exe') { [IO.File]::WriteAllText($file, $packedText) }
        $index++
    }
    foreach ($mutation in @(
        @{ Path = 'maxed.pdb'; Reason = 'bundle_forbidden_pdb' },
        @{ Path = 'history.sqlite3'; Reason = 'bundle_forbidden_sqlite' },
        @{ Path = 'history.sqlite-wal'; Reason = 'bundle_forbidden_sqlite' },
        @{ Path = 'extra.pcap'; Reason = 'bundle_forbidden_pcap' },
        @{ Path = 'maxed-cli.exe'; Reason = 'bundle_forbidden_cli' },
        @{ Path = 'other.exe'; Reason = 'bundle_unexpected_file' },
        @{ Path = '.env'; Reason = 'bundle_unexpected_file' },
        @{ Path = 'signing.key'; Reason = 'bundle_unexpected_file' },
        @{ Path = 'extra.json'; Reason = 'bundle_unexpected_file' },
        @{ Path = '$PLUGINSDIR/unknown.dll'; Reason = 'bundle_unexpected_file' }
    )) {
        $file = Join-Path $payload $mutation.Path
        [IO.File]::WriteAllText($file, 'Invented extra file')
        Assert-Rejected { Write-BundleManifest @inputs } $mutation.Reason
        Remove-Item -LiteralPath $file
    }
    $null = Write-BundleManifest @inputs
    Assert-Check (Test-Path -LiteralPath $manifestPath -PathType Leaf) 'restored payload passes after mutations'
    $fakeTarget = Join-Path $temp 'target'
    $null = New-Item -ItemType Directory -Path $fakeTarget
    $savedPath = $env:PATH
    try {
        $env:PATH = $fakeTarget
        Assert-Check ($null -eq (Get-Command 7z -CommandType Application -ErrorAction SilentlyContinue)) 'restricted PATH has no 7-Zip'
        $message = ''
        try { Export-VerifiedBundle -Installer $installer -TargetDir $fakeTarget -BuiltExe $builtExe -SourceRoot $source | Out-Null }
        catch { $message = $_.Exception.Message }
        Assert-Check ($message -ceq '7-Zip (7z) is required on PATH to verify the NSIS payload; no extraction fallback is allowed.') 'missing 7-Zip reports required dependency'
        Assert-Check (-not (Test-Path -LiteralPath $manifestPath)) 'missing 7-Zip removes stale manifest'
        Assert-Check (@(Get-ChildItem -LiteralPath $fakeTarget -Directory -Filter 'bundle-verify-*').Count -eq 0) 'missing 7-Zip leaves no extraction staging'
    } finally { $env:PATH = $savedPath }

    # Exercise the real package entry point with synthetic notices and synthetic
    # bundler/Cargo boundaries. No signing, Cargo build or installer process can run.
    # The check follows the bundler because Tauri provisions the NSIS notice inputs.
    $gateRoot = Join-Path $temp 'notice-gate'
    $null = New-Item -ItemType Directory -Path "$gateRoot/scripts/release", "$gateRoot/app/src-tauri", "$gateRoot/app/ui/node_modules/@tauri-apps/cli"
    Copy-Item -LiteralPath "$repoRoot/scripts/build-release.ps1" -Destination "$gateRoot/scripts/build-release.ps1"
    [IO.File]::WriteAllText("$gateRoot/app/src-tauri/tauri.conf.json", '{"version":"0.1.0","bundle":{"createUpdaterArtifacts":false}}')
    [IO.File]::WriteAllText("$gateRoot/app/ui/node_modules/@tauri-apps/cli/tauri.js", 'Synthetic CLI boundary; never executed')
    @'
param([switch]$Check)
if (-not $Check) { throw 'notice_gate_mode: expected Check.' }
$fixtureRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if ([IO.File]::ReadAllText("$fixtureRoot/Cargo.lock") -cne [IO.File]::ReadAllText("$fixtureRoot/THIRD-PARTY-NOTICES.md")) {
    throw 'notice_gate_stale: synthetic dependency notices differ.'
}
[IO.File]::WriteAllText("$fixtureRoot/notice-checked", 'checked')
'@ | Set-Content -LiteralPath "$gateRoot/scripts/release/New-ThirdPartyNotices.ps1" -Encoding UTF8
    function node { $global:LASTEXITCODE = 0 }
    function cargo {
        $global:LASTEXITCODE = 0
        if ($args -contains 'metadata') { return (@{ target_directory = "$gateRoot/target" } | ConvertTo-Json) }
        throw 'cargo_gate_reached: synthetic Cargo stopped.'
    }
    $gateManifest = "$gateRoot/target/release/bundle/nsis/bundle-manifest.json"
    $null = New-Item -ItemType Directory -Force -Path (Split-Path -Parent $gateManifest)
    $runGate = {
        [IO.File]::WriteAllText($gateManifest, '{"stale":true}')
        if (Test-Path -LiteralPath "$gateRoot/notice-checked") { Remove-Item -LiteralPath "$gateRoot/notice-checked" }
        $message = ''
        try { & "$gateRoot/scripts/build-release.ps1" -Package -Offline 3>$null | Out-Null } catch { $message = $_.Exception.Message }
        $message
    }
    [IO.File]::WriteAllText("$gateRoot/Cargo.lock", 'Invented locked dependency A')
    [IO.File]::WriteAllText("$gateRoot/THIRD-PARTY-NOTICES.md", 'Invented locked dependency A')
    $message = & $runGate
    Assert-Check ((Test-Path -LiteralPath "$gateRoot/notice-checked") -and -not $message.StartsWith('notice_gate_stale:')) 'current notices pass the post-build check'
    [IO.File]::WriteAllText("$gateRoot/THIRD-PARTY-NOTICES.md", 'Mutated notices')
    $message = & $runGate
    Assert-Check ($message.StartsWith('notice_gate_stale:') -and -not (Test-Path -LiteralPath $gateManifest)) 'stale notices fail before any bundle manifest'
    [IO.File]::WriteAllText("$gateRoot/THIRD-PARTY-NOTICES.md", 'Invented locked dependency A')
    [IO.File]::WriteAllText("$gateRoot/Cargo.lock", 'Invented locked dependency B')
    $message = & $runGate
    Assert-Check ($message.StartsWith('notice_gate_stale:') -and -not (Test-Path -LiteralPath $gateManifest)) 'changed dependencies fail before any bundle manifest'
    Remove-Item -LiteralPath "$gateRoot/scripts/release/New-ThirdPartyNotices.ps1"
    $message = & $runGate
    Assert-Check ($message -and -not (Test-Path -LiteralPath "$gateRoot/notice-checked") -and -not (Test-Path -LiteralPath $gateManifest)) 'missing notices checker fails the package'
    Remove-Item -Path Function:\node, Function:\cargo

    # Real generator: pinned Microsoft and Rust notice inputs fail closed before
    # metadata or any cache lookup. Preserve the supplied originals.
    $vendorRoot = Join-Path $temp 'vendor-notice-probe'
    $vendorRelative = 'docs/release/vendor/microsoft-web-webview2-1.0.3800.47'
    $null = New-Item -ItemType Directory -Path "$vendorRoot/scripts/release", "$vendorRoot/$vendorRelative"
    Copy-Item -LiteralPath "$repoRoot/scripts/release/New-ThirdPartyNotices.ps1" -Destination "$vendorRoot/scripts/release/New-ThirdPartyNotices.ps1"
    foreach ($name in @('LICENSE.txt', 'NOTICE.txt')) {
        Copy-Item -LiteralPath "$repoRoot/$vendorRelative/$name" -Destination "$vendorRoot/$vendorRelative/$name"
    }
    foreach ($name in @('LICENSE.txt', 'NOTICE.txt')) {
        $file = "$vendorRoot/$vendorRelative/$name"
        [IO.File]::WriteAllText($file, 'Changed Microsoft notice fixture')
        Assert-Rejected { & "$vendorRoot/scripts/release/New-ThirdPartyNotices.ps1" -Check } 'Pinned vendor notice hash mismatch'
        Remove-Item -LiteralPath $file
        Assert-Rejected { & "$vendorRoot/scripts/release/New-ThirdPartyNotices.ps1" -Check } 'Pinned vendor notice is missing'
        Copy-Item -LiteralPath "$repoRoot/$vendorRelative/$name" -Destination $file
    }
    Assert-Check (-not (Test-Path -LiteralPath "$vendorRoot/THIRD-PARTY-NOTICES.md")) 'invalid vendor inputs never write generated notices'

    $rustRelative = 'docs/release/vendor/rust-1.99.0/COPYRIGHT-library.html'
    $null = New-Item -ItemType Directory -Path (Split-Path -Parent "$vendorRoot/$rustRelative")
    Copy-Item -LiteralPath "$repoRoot/scripts/release/ConvertFrom-RustNoticeHtml.ps1" -Destination "$vendorRoot/scripts/release/ConvertFrom-RustNoticeHtml.ps1"
    Copy-Item -LiteralPath "$repoRoot/rust-toolchain.toml" -Destination "$vendorRoot/rust-toolchain.toml"
    $rustFile = "$vendorRoot/$rustRelative"
    $noticeFile = "$vendorRoot/THIRD-PARTY-NOTICES.md"
    foreach ($checkMode in @($false, $true)) {
        # Missing/changed inputs must neither create output nor overwrite output,
        # in generation mode as well as the release gate's read-only Check mode.
        Remove-Item -LiteralPath $rustFile -ErrorAction SilentlyContinue
        if ($checkMode) { [IO.File]::WriteAllText($noticeFile, 'Existing notices sentinel') }
        Assert-Rejected { & "$vendorRoot/scripts/release/New-ThirdPartyNotices.ps1" -Check:$checkMode } 'Pinned vendor notice is missing'
        Assert-Check (($checkMode -and [IO.File]::ReadAllText($noticeFile) -ceq 'Existing notices sentinel') -or
            (-not $checkMode -and -not (Test-Path -LiteralPath $noticeFile))) 'missing Rust vendor notice fails before writing'
        [IO.File]::WriteAllText($rustFile, 'Changed Rust notice fixture')
        Assert-Rejected { & "$vendorRoot/scripts/release/New-ThirdPartyNotices.ps1" -Check:$checkMode } 'Pinned vendor notice hash mismatch'
        Assert-Check (($checkMode -and [IO.File]::ReadAllText($noticeFile) -ceq 'Existing notices sentinel') -or
            (-not $checkMode -and -not (Test-Path -LiteralPath $noticeFile))) 'changed Rust vendor notice fails before writing'
    }
    Copy-Item -LiteralPath "$repoRoot/$rustRelative" -Destination $rustFile
    [IO.File]::WriteAllText("$vendorRoot/rust-toolchain.toml", '[toolchain]' + "`n" + 'channel = "1.100.0"' + "`n")
    foreach ($checkMode in @($false, $true)) {
        Assert-Rejected { & "$vendorRoot/scripts/release/New-ThirdPartyNotices.ps1" -Check:$checkMode } 'Rust standard-library notice requires review for the changed toolchain'
        Assert-Check ([IO.File]::ReadAllText($noticeFile) -ceq 'Existing notices sentinel') 'toolchain channel mismatch fails before writing'
    }

    . "$repoRoot/scripts/release/ConvertFrom-RustNoticeHtml.ps1"
    $utf8 = New-Object Text.UTF8Encoding($false, $true)
    $html = [IO.File]::ReadAllText($rustFile, $utf8)
    $rustText = ConvertFrom-RustNoticeHtml $html
    $culture = [Threading.Thread]::CurrentThread.CurrentCulture
    $uiCulture = [Threading.Thread]::CurrentThread.CurrentUICulture
    try {
        foreach ($name in @('tr-TR', 'fr-FR')) {
            [Threading.Thread]::CurrentThread.CurrentCulture = New-Object Globalization.CultureInfo($name)
            [Threading.Thread]::CurrentThread.CurrentUICulture = New-Object Globalization.CultureInfo($name)
            Assert-Check ((ConvertFrom-RustNoticeHtml $html) -ceq $rustText) "Rust conversion is deterministic under $name"
        }
    } finally {
        [Threading.Thread]::CurrentThread.CurrentCulture = $culture
        [Threading.Thread]::CurrentThread.CurrentUICulture = $uiCulture
    }
    Assert-Check ((ConvertFrom-RustNoticeHtml ($html.Replace("`n", "`r`n"))) -ceq $rustText) 'Rust conversion normalizes source line endings deterministically'
    # Independent preservation oracle: every pre block must occur in order with
    # all entity-decoded characters intact, including collapsed details content.
    $cursor = 0
    $preBlocks = [regex]::Matches($html, '(?s)<pre>(.*?)</pre>')
    Assert-Check ($preBlocks.Count -gt 0) 'Rust preservation check discovers licence blocks'
    foreach ($pre in $preBlocks) {
        $expected = [Net.WebUtility]::HtmlDecode($pre.Groups[1].Value)
        $position = $rustText.IndexOf($expected, $cursor, [StringComparison]::Ordinal)
        Assert-Check ($position -ge $cursor) 'complete Rust preformatted notice preserved in order'
        $cursor = $position + $expected.Length
    }
    # All body wording must remain in order, independently of formatting and
    # added link URLs. This covers metadata, copyright lines and file exceptions.
    $body = [regex]::Match($html, '(?s)<body>(.*?)</body>').Groups[1].Value
    $flowText = [regex]::Replace($rustText, '\s+', ' ')
    $cursor = 0
    foreach ($nodeText in [regex]::Split($body, '<[^>]+>')) {
        $expected = [regex]::Replace([Net.WebUtility]::HtmlDecode($nodeText), '\s+', ' ').Trim()
        if (-not $expected) { continue }
        $position = $flowText.IndexOf($expected, $cursor, [StringComparison]::Ordinal)
        if ($position -lt $cursor) { throw 'FAIL: Rust body text omitted or reordered' }
        $cursor = $position + $expected.Length
    }
    Assert-Check ($cursor -gt 0) 'all Rust body text preserved in order'
    Assert-Check ($rustText.Contains("    File/Directory: library/core/src/unicode`n") -and
        $rustText.Contains("      File/Directory: mod.rs`n")) 'Rust in-tree exception hierarchy retained'
    foreach ($link in [regex]::Matches($html, '<a href="(https?://[^"]+)"')) {
        if (-not $rustText.Contains($link.Groups[1].Value)) { throw 'FAIL: Rust external URL omitted' }
    }
    Assert-Check ($rustText.Contains('http://opensource.org/licenses/MIT')) 'Rust external source URLs retained'
    $committed = [IO.File]::ReadAllText("$repoRoot/THIRD-PARTY-NOTICES.md", $utf8)
    Assert-Check ($committed.Contains($rustText)) 'shipped notices embed the complete deterministic Rust rendering'
    $reviewSection = $committed.Substring($committed.LastIndexOf('## Needs review', [StringComparison]::Ordinal))
    # Pin the reviewed wording independently of both Git history and the output
    # under test. Compare each position exactly so edits and reordering fail.
    $expectedReviewItems = @(
        '- Rust alloc-stdlib 0.3.0: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: BSD-3-Clause.'
        '- Rust dasp_sample 0.11.0: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: MIT OR Apache-2.0.'
        '- Rust selectors 0.38.0: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: MPL-2.0.'
        '- Rust symphonia 0.5.5: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: MPL-2.0.'
        '- Rust symphonia-codec-pcm 0.5.5: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: MPL-2.0.'
        '- Rust symphonia-core 0.5.5: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: MPL-2.0.'
        '- Rust symphonia-format-riff 0.5.5: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: MPL-2.0.'
        '- Rust symphonia-metadata 0.5.5: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: MPL-2.0.'
        '- Rust ts-rs 11.1.0: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: MIT.'
        '- Rust webview2-com 0.39.1: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: MIT.'
        '- Rust webview2-com-sys 0.39.1: crate''s own code is MIT (text missing from the package); embeds the Microsoft-licensed static loader covered in Native and installer components.'
        '- npm is-reference 3.0.3: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: MIT.'
        '- npm locate-character 3.0.0: no LICENSE*, LICENCE*, COPYING*, COPYRIGHT* or NOTICE* file; metadata fallback: MIT.'
    )
    $packageItems = @([regex]::Matches($reviewSection, '(?m)^- .*') | ForEach-Object { $_.Value.TrimEnd("`r") })
    Assert-Check ($packageItems.Count -eq $expectedReviewItems.Count) 'exactly 13 package-text review entries remain'
    for ($i = 0; $i -lt $expectedReviewItems.Count; $i++) {
        Assert-Check ([string]::Equals($packageItems[$i], $expectedReviewItems[$i], [StringComparison]::Ordinal)) "package-text review entry $($i + 1) matches the independent baseline in order"
    }
} finally {
    if (Test-Path -LiteralPath $temp) {
        $resolved = (Resolve-Path -LiteralPath $temp).ProviderPath
        if ($resolved -ine $temp -or -not $resolved.StartsWith($tempParent + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
            ((Get-Item -LiteralPath $resolved -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Refusing cleanup outside the dedicated release-build fixture directory.'
        }
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
Write-Output "Release build: $script:checks checks passed. Synthetic payloads do not prove a signed package or installation."
