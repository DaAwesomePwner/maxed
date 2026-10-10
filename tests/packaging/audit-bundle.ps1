[CmdletBinding(DefaultParameterSetName = 'Audit')]
param(
    [Parameter(Mandatory, ParameterSetName = 'SelfTest')][switch]$SelfTest,
    [Parameter(Mandatory, ParameterSetName = 'Audit')][string]$Installer,
    [Parameter(Mandatory, ParameterSetName = 'Audit')][string]$ExpectedManifest,
    [Parameter(Mandatory, ParameterSetName = 'Audit')][string]$OutputDir,
    [Parameter(ParameterSetName = 'Audit')][string]$BundleManifest,
    [Parameter(ParameterSetName = 'Audit')][string]$Inventory
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $repoRoot 'scripts/build-release.ps1')

function New-AuditReport {
    [pscustomobject][ordered]@{
        result = 'fail'
        reasons = @()
        files = @()
        counts = [ordered]@{ files = 0; passed = 0; failed = 0; reasons = 0 }
        tools = [ordered]@{ seven_zip = 'not_run' }
    }
}

function Add-AuditFailure($Report, [string]$Reason, $File = $null) {
    if ($Reason -cnotin $Report.reasons) { $Report.reasons += $Reason }
    if ($null -ne $File) {
        $File.status = 'fail'
        if ($Reason -cnotin $File.reasons) { $File.reasons += $Reason }
    }
}

function Complete-AuditReport($Report) {
    $Report.reasons = @($Report.reasons | Sort-Object -CaseSensitive)
    $Report.files = @($Report.files | Sort-Object -Property path -CaseSensitive)
    $Report.counts.files = $Report.files.Count
    $Report.counts.failed = @($Report.files | Where-Object { $_.status -eq 'fail' }).Count
    $Report.counts.passed = $Report.counts.files - $Report.counts.failed
    $Report.counts.reasons = $Report.reasons.Count
    if ($Report.reasons.Count -eq 0) { $Report.result = 'pass' } else { $Report.result = 'fail' }
}

function Write-AuditReport($Report, [string]$Directory) {
    Complete-AuditReport $Report
    [IO.File]::WriteAllText((Join-Path $Directory 'audit-report.json'),
        ($Report | ConvertTo-Json -Depth 8) + "`n", (New-Object Text.UTF8Encoding($false)))
}

function Get-AuditFailureCode($Failure) {
    # Never serialize exception text: it can contain a path or offending bytes.
    $code = ($Failure.Exception.Message -split ':', 2)[0]
    $safe = @('audit_output_exists', 'audit_output_parent', 'audit_output_reparse',
        'audit_input_invalid', 'audit_input_reparse', 'audit_seven_zip_missing',
        'audit_seven_zip_version', 'audit_cleanup_refused', 'audit_stage_acl',
        'bundle_path_traversal', 'bundle_forbidden_pdb', 'bundle_forbidden_sqlite',
        'bundle_forbidden_pcap', 'bundle_forbidden_cli', 'bundle_unexpected_file',
        'bundle_duplicate_path', 'bundle_empty_archive', 'bundle_reparse_path',
        'bundle_unexpected_directory', 'bundle_listing_failed', 'bundle_extraction_failed',
        'bundle_extraction_mismatch')
    if ($code -cin $safe) { return $code }
    return 'audit_io_failed'
}

function New-AuditOutput([string]$Directory) {
    $full = $Directory.TrimEnd('\', '/')
    if (Test-Path -LiteralPath $full) { throw 'audit_output_exists: choose a fresh output directory.' }
    $parentPath = Split-Path -Parent $full
    if (-not (Test-Path -LiteralPath $parentPath -PathType Container)) {
        throw 'audit_output_parent: output parent must exist.'
    }
    $parent = Get-Item -LiteralPath $parentPath -Force
    while ($null -ne $parent) {
        if ($parent.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'audit_output_reparse: output ancestors must not be links.'
        }
        $parent = $parent.Parent
    }
    # New-Item has no LiteralPath parameter; no Force, and never reuse a directory.
    $null = New-Item -ItemType Directory -Path $full
    return $full
}

function Assert-AuditInput([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw 'audit_input_invalid: required input is missing.' }
    $item = Get-Item -LiteralPath $Path -Force
    while ($null -ne $item) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'audit_input_reparse: inputs must not traverse links.' }
        if ($item -is [IO.FileInfo]) { $item = $item.Directory } else { $item = $item.Parent }
    }
}

function Read-AuditJson([string]$Path) {
    Assert-AuditInput $Path
    if ((Get-Item -LiteralPath $Path).Length -gt 1048576) { throw 'audit_input_invalid: manifest exceeds bound.' }
    try { return (Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json) }
    catch { throw 'audit_input_invalid: manifest must be JSON.' }
}

function Test-AuditInstaller([string]$Path, $Manifest, $Report) {
    Assert-AuditInput $Path
    $item = Get-Item -LiteralPath $Path
    $hash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    $installerEntries = @($Manifest.files | Where-Object { $_.file -ceq $item.Name })
    if ($installerEntries.Count -ne 1) {
        Add-AuditFailure $Report 'installer_manifest_entry'
    } else {
        if (($installerEntries[0].bytes -isnot [int] -and $installerEntries[0].bytes -isnot [long]) -or
            $installerEntries[0].bytes -le 0 -or $installerEntries[0].bytes -ne $item.Length) {
            Add-AuditFailure $Report 'installer_bytes_mismatch'
        }
        if ($installerEntries[0].sha256 -cne $hash) { Add-AuditFailure $Report 'installer_hash_mismatch' }
    }
    $signatures = @($Manifest.files | Where-Object { $_.file -ceq ($item.Name + '.sig') })
    if ($signatures.Count -ne 1) {
        Add-AuditFailure $Report 'signature_manifest_entry'
    } elseif (-not (Test-Path -LiteralPath ($Path + '.sig') -PathType Leaf)) {
        Add-AuditFailure $Report 'signature_missing'
    } else {
        Assert-AuditInput ($Path + '.sig')
        if (($signatures[0].bytes -isnot [int] -and $signatures[0].bytes -isnot [long]) -or
            $signatures[0].bytes -le 0 -or $signatures[0].bytes -ne (Get-Item -LiteralPath ($Path + '.sig')).Length -or
            $signatures[0].sha256 -cne (Get-FileHash -LiteralPath ($Path + '.sig') -Algorithm SHA256).Hash.ToLowerInvariant()) {
            Add-AuditFailure $Report 'signature_identity_mismatch'
        }
    }
    return $hash
}

function Get-AuditCanaries([byte[]]$Bytes, [string]$RelativePath) {
    # Latin-1 is a one-to-one byte projection, not text decoding/normalization.
    # These conservative shapes intentionally do not claim general secret detection.
    $text = [Text.Encoding]::GetEncoding(28591).GetString($Bytes)
    # Long three-segment base64url tokens; short dotted identifiers are ignored.
    if ($text -cmatch '(?<![A-Za-z0-9_-])[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{32,}(?![A-Za-z0-9_-])') { 'canary_jwt' }
    # Explicit authorization scheme followed by at least 40 token characters.
    if ($text -match '\bBearer[ \t]+[A-Za-z0-9_./+~=-]{40,}') { 'canary_bearer' }
    if ($text -cmatch '-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----') { 'canary_private_key' }
    if ($text -match 'untrusted comment:[^\r\n]{0,160}\bsecret key\b') { 'canary_minisign_secret' }
    if ($RelativePath -cne 'demo/story.pcap') {
        # Classic pcap, both byte orders and timestamp resolutions, plus pcapng.
        foreach ($magic in @('d4c3b2a1', 'a1b2c3d4', '4d3cb2a1', 'a1b23c4d', '0a0d0d0a')) {
            $magicBytes = [byte[]]@(for ($i = 0; $i -lt 8; $i += 2) { [Convert]::ToByte($magic.Substring($i, 2), 16) })
            if ($text.Contains([Text.Encoding]::GetEncoding(28591).GetString($magicBytes))) { 'canary_capture'; break }
        }
    }
    if ($text.Contains("SQLite format 3`0")) { 'canary_sqlite' }
    if ($text -match '[A-Za-z]:\\Users\\') { 'canary_user_profile' }
}

function Test-AuditStage(
    [string]$Stage, $Bundle, [object[]]$InventoryEntries, [string]$InstallerHash,
    [string]$SourceRoot, $Report, [object[]]$PayloadFiles, [switch]$RequireUninstaller
) {
    # Production callers omit PayloadFiles: the shared collector is the sole
    # payload allowlist authority. Synthetic core tests supply explicit fixture
    # records to test notice handling independently of the collector allowlist.
    if (-not $PSBoundParameters.ContainsKey('PayloadFiles')) { $PayloadFiles = @(Get-BundlePayloadFiles $Stage) }
    $inventoryMap = @{}
    if ($InventoryEntries.Count -eq 0) { Add-AuditFailure $Report 'inventory_invalid' }
    foreach ($entry in $InventoryEntries) {
        $valid = (@($entry.PSObject.Properties.Name | Sort-Object) -join ',') -ceq 'file,license,sha256'
        if (-not ($entry.file -is [string]) -or -not ($entry.license -is [string]) -or
            $entry.license -cnotmatch '\A[A-Za-z0-9][A-Za-z0-9 .()+-]{0,127}\z' -or
            $entry.sha256 -cnotmatch '\A[a-f0-9]{64}\z') { $valid = $false }
        if ($valid) {
            try { $normalized = Assert-BundleEntryPath $entry.file }
            catch { $valid = $false }
            if ($normalized -cne $entry.file -or $entry.file -cin @('maxed.exe', 'uninstall.exe') -or $inventoryMap.ContainsKey($entry.file)) { $valid = $false }
        }
        if (-not $valid) { Add-AuditFailure $Report 'inventory_invalid'; continue }
        $inventoryMap[$entry.file] = $entry
    }
    if ($Bundle.installer.sha256 -cne $InstallerHash) { Add-AuditFailure $Report 'bundle_installer_hash_mismatch' }
    $exeEntries = @($Bundle.files | Where-Object { $_.path -ceq 'maxed.exe' })
    if ($exeEntries.Count -ne 1) { Add-AuditFailure $Report 'bundle_exe_entry' }
    $payloadMap = @{}
    foreach ($entry in $PayloadFiles) {
        # Records are from the shared collector (or explicitly constructed test
        # fixtures), never copied from an untrusted manifest into the report.
        $file = [pscustomobject][ordered]@{ path = $entry.path; bytes = $entry.bytes; sha256 = $entry.sha256; status = 'pass'; reasons = @() }
        $Report.files += $file
        $payloadMap[$entry.path] = $file
        $path = Join-Path $Stage $entry.path
        if ($entry.path -ceq 'maxed.exe') {
            if ($exeEntries.Count -ne 1 -or $entry.sha256 -cne $exeEntries[0].sha256) { Add-AuditFailure $Report 'exe_hash_mismatch' $file }
            $bytes = [IO.File]::ReadAllBytes($path)
            if ($bytes.Length -lt 2 -or $bytes[0] -ne 77 -or $bytes[1] -ne 90) { Add-AuditFailure $Report 'exe_mz_missing' $file }
            $text = [Text.Encoding]::ASCII.GetString($bytes)
            $marker = '__TAURI_BUNDLE_TYPE_VAR_NSS'
            $offset = $text.IndexOf($marker, [StringComparison]::Ordinal)
            if ($offset -lt 0) { Add-AuditFailure $Report 'exe_marker_missing' $file }
            elseif ($text.IndexOf($marker, $offset + 1, [StringComparison]::Ordinal) -ge 0) { Add-AuditFailure $Report 'exe_marker_duplicated' $file }
        } elseif ($entry.path -ceq 'uninstall.exe') {
            # Generated per build by NSIS WriteUninstaller (listed by 7-Zip 26.02+), so
            # it has no static inventory hash; pin it to the bundle manifest instead.
            $recorded = @($Bundle.files | Where-Object { $_.path -ceq 'uninstall.exe' })
            if ($recorded.Count -ne 1 -or $entry.sha256 -cne $recorded[0].sha256) { Add-AuditFailure $Report 'uninstaller_hash_mismatch' $file }
            $bytes = [IO.File]::ReadAllBytes($path)
            if (-not (Test-BundlePortableExecutable $bytes)) { Add-AuditFailure $Report 'uninstaller_pe_invalid' $file }
            foreach ($reason in @(Get-AuditCanaries $bytes $entry.path)) { Add-AuditFailure $Report $reason $file }
        } else {
            if (-not $inventoryMap.ContainsKey($entry.path)) { Add-AuditFailure $Report 'inventory_unlisted_payload' $file }
            elseif ($inventoryMap[$entry.path].file -cne $entry.path -or $inventoryMap[$entry.path].sha256 -cne $entry.sha256) { Add-AuditFailure $Report 'inventory_hash_mismatch' $file }
            # Scan even DLLs: only the main executable is exempt, and hashes still
            # pin every support binary. No extension-based canary bypass exists.
            foreach ($reason in @(Get-AuditCanaries ([IO.File]::ReadAllBytes($path)) $entry.path)) { Add-AuditFailure $Report $reason $file }
        }
    }
    if (-not $payloadMap.ContainsKey('maxed.exe')) { Add-AuditFailure $Report 'exe_missing' }
    # 7-Zip 26.02+ exposes the NSIS uninstaller, so its absence there is a broken installer.
    if ($RequireUninstaller -and -not $payloadMap.ContainsKey('uninstall.exe')) { Add-AuditFailure $Report 'uninstaller_missing' }
    foreach ($path in $inventoryMap.Keys) {
        if (-not $payloadMap.ContainsKey($path) -or $payloadMap[$path].path -cne $path) { Add-AuditFailure $Report 'inventory_missing_payload' }
    }
    $sourceFiles = @('LICENSE', 'NOTICE.md')
    if ($inventoryMap.ContainsKey('THIRD-PARTY-NOTICES.md')) { $sourceFiles += 'THIRD-PARTY-NOTICES.md' }
    foreach ($name in $sourceFiles) {
        if (-not $payloadMap.ContainsKey($name)) { Add-AuditFailure $Report 'source_notice_missing'; continue }
        $file = $payloadMap[$name]
        $source = Join-Path $SourceRoot $name
        Assert-AuditInput $source
        if ($file.bytes -ne (Get-Item -LiteralPath $source).Length -or
            $file.sha256 -cne (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()) {
            Add-AuditFailure $Report 'source_notice_mismatch' $file
        }
    }
    Complete-AuditReport $Report
}

function Remove-AuditStage([string]$Stage, [string]$Parent) {
    $resolved = [IO.Path]::GetFullPath($Stage)
    $prefix = [IO.Path]::GetFullPath($Parent).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if (-not $resolved.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or
        ((Get-Item -LiteralPath $resolved -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'audit_cleanup_refused: staging is not contained in its owned parent.'
    }
    # Do not recurse through links, including links inside a failed extraction.
    $pending = New-Object 'System.Collections.Generic.Queue[string]'
    $pending.Enqueue($resolved)
    while ($pending.Count) {
        foreach ($item in Get-ChildItem -LiteralPath $pending.Dequeue() -Force) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'audit_cleanup_refused: staging contains a link.' }
            if ($item.PSIsContainer) { $pending.Enqueue($item.FullName) }
        }
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
}

function Invoke-AuditExtraction([string]$InstallerPath, [string]$Output, $Bundle, [object[]]$Entries, [string]$Hash, $Report) {
    $sevenZip = Get-Command 7z -CommandType Application -ErrorAction SilentlyContinue
    if ($null -eq $sevenZip) { throw 'audit_seven_zip_missing: 7-Zip (7z) is required on PATH.' }
    $versionOutput = @(& $sevenZip.Source i 2>&1)
    if ($LASTEXITCODE -ne 0) { throw 'audit_seven_zip_version: 7-Zip version could not be read.' }
    # Keep only the bounded version token; arbitrary tool output is private.
    foreach ($line in $versionOutput) {
        if ([string]$line -match '^7-Zip(?: \([A-Za-z0-9 ]+\))? ([0-9]{1,3}\.[0-9]{1,3})\b') { $Report.tools.seven_zip = '7-Zip ' + $Matches[1]; break }
    }
    if ($Report.tools.seven_zip -eq 'not_run') { throw 'audit_seven_zip_version: 7-Zip version was not recognized.' }
    $requireUninstaller = Test-SevenZipExposesUninstaller ([version]$Report.tools.seven_zip.Substring(6))
    $stage = Join-Path $Output ('stage-' + [Guid]::NewGuid().ToString('N'))
    $null = New-Item -ItemType Directory -Path $stage
    try {
        try {
            $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
            $acl = New-Object Security.AccessControl.DirectorySecurity
            $acl.SetOwner($sid)
            $acl.SetAccessRuleProtection($true, $false)
            $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($sid, 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow')))
            Set-Acl -LiteralPath $stage -AclObject $acl
        } catch { throw 'audit_stage_acl: private staging permissions could not be applied.' }
        $listing = @(& $sevenZip.Source l -slt -ba -- $InstallerPath 2>&1)
        if ($LASTEXITCODE -ne 0) { throw 'bundle_listing_failed: 7-Zip could not list the payload.' }
        $archivePaths = @(Get-BundleArchivePaths $listing)
        & $sevenZip.Source x -y "-o$stage" -- $InstallerPath *> $null
        if ($LASTEXITCODE -ne 0) { throw 'bundle_extraction_failed: 7-Zip could not extract the payload.' }
        $files = @(Get-BundlePayloadFiles $stage)
        $extracted = @($files | ForEach-Object { $_.path })
        if (@(Compare-Object $archivePaths $extracted -CaseSensitive).Count) { throw 'bundle_extraction_mismatch: listed and extracted paths differ.' }
        Test-AuditStage -Stage $stage -Bundle $Bundle -InventoryEntries $Entries -InstallerHash $Hash -SourceRoot $repoRoot -Report $Report -PayloadFiles $files -RequireUninstaller:$requireUninstaller
    } finally {
        Remove-AuditStage $stage $Output
    }
}

function Invoke-BundleAudit([string]$InstallerPath, [string]$ReleasePath, [string]$OutputPath, [string]$BundlePath, [string]$InventoryPath) {
    $report = New-AuditReport
    $output = $null
    try {
        # Resolve inputs once against the caller's PowerShell location, which
        # can differ from the process working directory after Set-Location.
        $InstallerPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($InstallerPath)
        $ReleasePath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($ReleasePath)
        $OutputPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputPath)
        if ([string]::IsNullOrWhiteSpace($BundlePath)) {
            $BundlePath = Join-Path (Split-Path -Parent $InstallerPath) 'bundle-manifest.json'
        } else {
            $BundlePath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($BundlePath)
        }
        if ([string]::IsNullOrWhiteSpace($InventoryPath)) {
            $InventoryPath = Join-Path $repoRoot 'docs/release/distribution-inventory.json'
        } else {
            $InventoryPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($InventoryPath)
        }
        $output = New-AuditOutput $OutputPath
        $release = Read-AuditJson $ReleasePath
        $hash = Test-AuditInstaller $InstallerPath $release $report
        if ($report.reasons.Count -eq 0) {
            $bundle = Read-AuditJson $BundlePath
            $entries = @(Read-AuditJson $InventoryPath)
            Invoke-AuditExtraction $InstallerPath $output $bundle $entries $hash $report
            # Detect installer replacement during listing/extraction as well.
            $null = Test-AuditInstaller $InstallerPath $release $report
        }
    } catch { Add-AuditFailure $report (Get-AuditFailureCode $_) }
    Complete-AuditReport $report
    if ($null -ne $output) {
        try { Write-AuditReport $report $output }
        catch { Add-AuditFailure $report 'audit_report_write_failed'; Complete-AuditReport $report }
    }
    # A rejected output destination is never touched, even to write a failure
    # report. The caller still emits the same sanitized report to stdout.
    return $report
}

function Assert-AuditCheck([bool]$Condition, [string]$Name) {
    $script:auditChecks++
    if (-not $Condition) { throw "FAIL: $Name" }
    Write-Output "PASS: $Name"
}

# Minimal invented PE32 GUI image: DOS header, PE signature, COFF header, optional
# header, one section with its data, then an overlay. Never executed.
function New-SyntheticPortableExecutable {
    $bytes = New-Object byte[] 1024
    $bytes[0] = 0x4D; $bytes[1] = 0x5A
    [BitConverter]::GetBytes([int32]64).CopyTo($bytes, 0x3C)
    $bytes[64] = 0x50; $bytes[65] = 0x45
    [BitConverter]::GetBytes([uint16]0x14C).CopyTo($bytes, 68)      # i386
    [BitConverter]::GetBytes([uint16]1).CopyTo($bytes, 70)          # one section
    [BitConverter]::GetBytes([uint16]96).CopyTo($bytes, 84)         # PE32 optional header size
    [BitConverter]::GetBytes([uint16]0x0102).CopyTo($bytes, 86)     # executable image, 32-bit
    [BitConverter]::GetBytes([uint16]0x10B).CopyTo($bytes, 88)      # PE32 magic
    [BitConverter]::GetBytes([uint32]4096).CopyTo($bytes, 88 + 32)  # section alignment
    [BitConverter]::GetBytes([uint32]512).CopyTo($bytes, 88 + 36)   # file alignment
    [BitConverter]::GetBytes([uint32]8192).CopyTo($bytes, 88 + 56)  # image size
    [BitConverter]::GetBytes([uint32]512).CopyTo($bytes, 88 + 60)   # headers size
    [BitConverter]::GetBytes([uint16]2).CopyTo($bytes, 88 + 68)     # GUI subsystem
    [BitConverter]::GetBytes([uint32]16).CopyTo($bytes, 184 + 8)    # section virtual size
    [BitConverter]::GetBytes([uint32]4096).CopyTo($bytes, 184 + 12) # section virtual address
    [BitConverter]::GetBytes([uint32]512).CopyTo($bytes, 184 + 16)  # section raw size
    [BitConverter]::GetBytes([uint32]512).CopyTo($bytes, 184 + 20)  # section raw offset
    [BitConverter]::GetBytes([uint32]4096).CopyTo($bytes, 88 + 16)  # entry point RVA
    [BitConverter]::GetBytes([uint32]0x60000020).CopyTo($bytes, 184 + 36) # code, execute, read
    , [byte[]]($bytes + [Text.Encoding]::ASCII.GetBytes('invented uninstaller overlay; never executed'))
}

function Invoke-AuditSelfTest {
    $script:auditChecks = 0
    $tempParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/')
    $temp = Join-Path $tempParent ('maxed-bundle-audit-' + [Guid]::NewGuid().ToString('N'))
    $null = New-Item -ItemType Directory -Path $temp
    try {
        $cases = @('good', 'installer_hash', 'installer_bytes', 'installer_name', 'signature_missing',
            'signature_entry', 'signature_changed', 'extra_payload', 'inventory_missing', 'inventory_unlisted',
            'inventory_hash', 'inventory_duplicate', 'exe_hash', 'bundle_installer_hash', 'exe_marker',
            'exe_duplicate_marker', 'exe_mz', 'uninstaller_hash', 'uninstaller_mz_junk', 'uninstaller_truncated', 'uninstaller_missing', 'uninstaller_inventoried', 'changed_license', 'changed_notice', 'changed_third_notice',
            'canary_jwt', 'canary_bearer', 'canary_private_key', 'canary_minisign_secret',
            'canary_capture', 'canary_pcapng', 'canary_sqlite', 'canary_user_profile')
        foreach ($case in $cases) {
            $root = Join-Path $temp $case
            $stage = Join-Path $root 'stage'
            $source = Join-Path $root 'source'
            $output = Join-Path $root 'report'
            $null = New-Item -ItemType Directory -Path (Join-Path $stage 'demo')
            $null = New-Item -ItemType Directory -Path (Join-Path $stage '$PLUGINSDIR')
            $null = New-Item -ItemType Directory -Path $source
            $null = New-AuditOutput $output
            $texts = [ordered]@{
                'maxed.exe' = 'MZ invented __TAURI_BUNDLE_TYPE_VAR_NSS executable; never executed'
                'uninstall.exe' = 'MZ invented uninstaller; never executed'
                'LICENSE' = 'Invented license'
                'NOTICE.md' = 'Invented notice'
                'THIRD-PARTY-NOTICES.md' = 'Invented third-party notice'
                'demo/catalog.json' = '{"invented":true}'
                'demo/story.pcap' = 'Invented capture'
                '$PLUGINSDIR/System.dll' = 'Invented System plugin; never executed'
                '$PLUGINSDIR/nsDialogs.dll' = 'Invented dialogs plugin; never executed'
                '$PLUGINSDIR/NSISdl.dll' = 'Invented download plugin; never executed'
                '$PLUGINSDIR/nsis_tauri_utils.dll' = 'Invented utilities plugin; never executed'
                '$PLUGINSDIR/StartMenu.dll' = 'Invented menu plugin; never executed'
                '$PLUGINSDIR/modern-wizard.bmp' = 'Invented wizard bitmap'
            }
            foreach ($path in $texts.Keys) { [IO.File]::WriteAllText((Join-Path $stage $path), $texts[$path]) }
            [IO.File]::WriteAllBytes((Join-Path $stage 'uninstall.exe'), (New-SyntheticPortableExecutable))
            # The sole pcap exemption still permits ordinary binary capture magic.
            [IO.File]::WriteAllBytes((Join-Path $stage 'demo/story.pcap'), [byte[]]@(212, 195, 178, 161, 0))
            foreach ($name in @('LICENSE', 'NOTICE.md', 'THIRD-PARTY-NOTICES.md')) { Copy-Item -LiteralPath (Join-Path $stage $name) -Destination (Join-Path $source $name) }
            $installerPath = Join-Path $root 'Maxed_0.1.0_x64-setup.exe'
            [IO.File]::WriteAllText($installerPath, 'Invented installer; never executed')
            [IO.File]::WriteAllText(($installerPath + '.sig'), 'Invented signature; never verified')
            $release = [pscustomobject]@{ files = @(
                foreach ($path in @($installerPath, ($installerPath + '.sig'))) {
                    [pscustomobject]@{ file = [IO.Path]::GetFileName($path); bytes = (Get-Item -LiteralPath $path).Length; sha256 = (Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant() }
                }
            ) }
            $records = @(
                foreach ($path in $texts.Keys) {
                    $full = Join-Path $stage $path
                    [pscustomobject]@{ path = $path; bytes = (Get-Item -LiteralPath $full).Length; sha256 = (Get-FileHash -LiteralPath $full).Hash.ToLowerInvariant() }
                }
            )
            $entries = @($records | Where-Object { $_.path -cnotin @('maxed.exe', 'uninstall.exe') } | ForEach-Object { [pscustomobject]@{ file = $_.path; license = 'MIT'; sha256 = $_.sha256 } })
            $bundle = [pscustomobject]@{ installer = [pscustomobject]@{ sha256 = $release.files[0].sha256 }; files = @($records | ForEach-Object { [pscustomobject]@{ path = $_.path; bytes = $_.bytes; sha256 = $_.sha256 } }) }
            $expected = ''
            $canary = ''
            switch ($case) {
                'installer_hash' { $release.files[0].sha256 = '0' * 64; $expected = 'installer_hash_mismatch' }
                'installer_bytes' { $release.files[0].bytes++; $expected = 'installer_bytes_mismatch' }
                'installer_name' { $release.files[0].file = 'wrong.exe'; $expected = 'installer_manifest_entry' }
                'signature_missing' { Remove-Item -LiteralPath ($installerPath + '.sig'); $expected = 'signature_missing' }
                'signature_entry' { $release.files = @($release.files[0]); $expected = 'signature_manifest_entry' }
                'signature_changed' { [IO.File]::WriteAllText(($installerPath + '.sig'), 'Changed signature'); $expected = 'signature_identity_mismatch' }
                'extra_payload' { [IO.File]::WriteAllText((Join-Path $stage 'private-export.txt'), 'invented'); $expected = 'bundle_unexpected_file' }
                'inventory_missing' { Remove-Item -LiteralPath (Join-Path $stage 'demo/catalog.json'); $expected = 'inventory_missing_payload' }
                'inventory_unlisted' { $entries = @($entries | Where-Object { $_.file -cne 'demo/catalog.json' }); $expected = 'inventory_unlisted_payload' }
                'inventory_hash' { $entries[0].sha256 = '0' * 64; $expected = 'inventory_hash_mismatch' }
                'inventory_duplicate' { $entries += $entries[0]; $expected = 'inventory_invalid' }
                'exe_hash' { $bundle.files[0].sha256 = '0' * 64; $expected = 'exe_hash_mismatch' }
                'bundle_installer_hash' { $bundle.installer.sha256 = '0' * 64; $expected = 'bundle_installer_hash_mismatch' }
                'exe_marker' { [IO.File]::WriteAllText((Join-Path $stage 'maxed.exe'), 'MZ no marker'); $expected = 'exe_marker_missing' }
                'exe_duplicate_marker' { [IO.File]::WriteAllText((Join-Path $stage 'maxed.exe'), ($texts['maxed.exe'] * 2)); $expected = 'exe_marker_duplicated' }
                'exe_mz' { [IO.File]::WriteAllText((Join-Path $stage 'maxed.exe'), $texts['maxed.exe'].Substring(2)); $expected = 'exe_mz_missing' }
                'uninstaller_hash' { @($bundle.files | Where-Object { $_.path -ceq 'uninstall.exe' })[0].sha256 = '0' * 64; $expected = 'uninstaller_hash_mismatch' }
                'uninstaller_mz_junk' { [IO.File]::WriteAllText((Join-Path $stage 'uninstall.exe'), 'MZ invented non-PE bytes'); $expected = 'uninstaller_pe_invalid' }
                'uninstaller_truncated' { [IO.File]::WriteAllBytes((Join-Path $stage 'uninstall.exe'), [byte[]](New-SyntheticPortableExecutable)[0..99]); $expected = 'uninstaller_pe_invalid' }
                'uninstaller_missing' { Remove-Item -LiteralPath (Join-Path $stage 'uninstall.exe'); $expected = 'uninstaller_missing' }
                'uninstaller_inventoried' { $entries += [pscustomobject]@{ file = 'uninstall.exe'; license = 'Zlib'; sha256 = 'a' * 64 }; $expected = 'inventory_invalid' }
                'changed_license' { [IO.File]::WriteAllText((Join-Path $stage 'LICENSE'), 'Changed'); $expected = 'source_notice_mismatch' }
                'changed_notice' { [IO.File]::WriteAllText((Join-Path $stage 'NOTICE.md'), 'Changed'); $expected = 'source_notice_mismatch' }
                'changed_third_notice' { [IO.File]::WriteAllText((Join-Path $stage 'THIRD-PARTY-NOTICES.md'), 'Changed'); $expected = 'source_notice_mismatch' }
                'canary_jwt' { $canary = ('a' * 24) + '.' + ('b' * 32) + '.' + ('c' * 44); $expected = 'canary_jwt' }
                'canary_bearer' { $canary = 'Bearer ' + ('x' * 64); $expected = 'canary_bearer' }
                'canary_private_key' { $canary = '-----BEGIN RSA PRIVATE KEY-----'; $expected = 'canary_private_key' }
                'canary_minisign_secret' { $canary = 'untrusted comment: minisign encrypted secret key'; $expected = 'canary_minisign_secret' }
                'canary_capture' { $canary = [Text.Encoding]::GetEncoding(28591).GetString([byte[]]@(212, 195, 178, 161)); $expected = 'canary_capture' }
                'canary_pcapng' { $canary = "`n`r`r`n"; $expected = 'canary_capture' }
                'canary_sqlite' { $canary = "SQLite format 3`0"; $expected = 'canary_sqlite' }
                'canary_user_profile' { $canary = 'C:\Users\invented-canary\private'; $expected = 'canary_user_profile' }
            }
            if ($canary.Length) { [IO.File]::WriteAllBytes((Join-Path $stage 'demo/catalog.json'), [Text.Encoding]::GetEncoding(28591).GetBytes($canary)) }
            # Refresh fixture records after mutations; manifests retain originals.
            $records = @($records | Where-Object { Test-Path -LiteralPath (Join-Path $stage $_.path) } | ForEach-Object {
                $full = Join-Path $stage $_.path
                [pscustomobject]@{ path = $_.path; bytes = (Get-Item -LiteralPath $full).Length; sha256 = (Get-FileHash -LiteralPath $full).Hash.ToLowerInvariant() }
            })
            $report = New-AuditReport
            $hash = Test-AuditInstaller $installerPath $release $report
            if ($case -eq 'extra_payload') {
                # Exercise the shared collector using its admitted subset,
                # proving that the extra file itself causes rejection.
                Remove-Item -LiteralPath (Join-Path $stage 'THIRD-PARTY-NOTICES.md')
                Remove-Item -LiteralPath (Join-Path $stage 'private-export.txt')
                Assert-AuditCheck (@(Get-BundlePayloadFiles $stage).Count -eq 12) 'shared_collector_accepts_baseline'
                [IO.File]::WriteAllText((Join-Path $stage 'private-export.txt'), 'invented')
                try { $null = @(Get-BundlePayloadFiles $stage) } catch { Add-AuditFailure $report (Get-AuditFailureCode $_) }
            } else {
                Test-AuditStage -Stage $stage -Bundle $bundle -InventoryEntries $entries -InstallerHash $hash -SourceRoot $source -Report $report -PayloadFiles $records -RequireUninstaller
            }
            Write-AuditReport $report $output
            if ($case -eq 'good') {
                Assert-AuditCheck ($report.result -eq 'pass' -and $report.files.Count -eq 13) 'known_good_stage_including_third_party_notice'
            } else {
                Assert-AuditCheck ($report.result -eq 'fail' -and $expected -cin $report.reasons) $case
            }
            $json = [IO.File]::ReadAllText((Join-Path $output 'audit-report.json'))
            Assert-AuditCheck (-not $json.Contains($temp) -and $json -notmatch '[A-Za-z]:|\\\\Users\\\\') ('public_report_no_absolute_path_' + $case)
            if ($canary.Length) {
                # Check decoded values too: JSON escaping must not hide a leak.
                $roundtrip = $json | ConvertFrom-Json
                $values = @($roundtrip.result, $roundtrip.tools.seven_zip) + @($roundtrip.reasons)
                foreach ($file in $roundtrip.files) { $values += @($file.path, $file.sha256, $file.status) + @($file.reasons) }
                $leaked = @($values | Where-Object { ([string]$_).Contains($canary) }).Count -gt 0
                Assert-AuditCheck (-not $leaked -and -not $json.Contains($canary)) ('public_report_contains_no_canaries_' + $case)
            }
        }
        $existing = Join-Path $temp 'existing'
        $null = New-AuditOutput $existing
        [IO.File]::WriteAllText((Join-Path $existing 'sentinel'), 'unchanged')
        $report = Invoke-BundleAudit 'unused' 'unused' $existing 'unused' 'unused'
        Assert-AuditCheck ($report.result -eq 'fail' -and 'audit_output_exists' -cin $report.reasons) 'existing_output_directory'
        Assert-AuditCheck ((Get-ChildItem -LiteralPath $existing).Count -eq 1 -and [IO.File]::ReadAllText((Join-Path $existing 'sentinel')) -ceq 'unchanged') 'existing_output_untouched'
        $fresh = Join-Path $temp 'input-failure'
        $report = Invoke-BundleAudit 'unused' 'unused' $fresh 'unused' 'unused'
        Assert-AuditCheck ($report.result -eq 'fail' -and (Test-Path -LiteralPath (Join-Path $fresh 'audit-report.json'))) 'input_failure_writes_report'
        Push-Location -LiteralPath $temp
        try {
            $report = Invoke-BundleAudit 'unused' 'unused' 'relative-output' '' ''
            Assert-AuditCheck ($report.result -eq 'fail' -and 'audit_input_invalid' -cin $report.reasons -and
                (Test-Path -LiteralPath (Join-Path $temp 'relative-output/audit-report.json'))) 'relative_output_uses_powershell_location'
        } finally { Pop-Location }
        Write-Output "Bundle audit: $script:auditChecks checks passed. Synthetic stages only; no build, installer execution or extraction."
    } finally { Remove-AuditStage $temp $tempParent }
}

if ($SelfTest) { Invoke-AuditSelfTest; exit 0 }
$result = Invoke-BundleAudit $Installer $ExpectedManifest $OutputDir $BundleManifest $Inventory
Write-Output ($result | ConvertTo-Json -Depth 8)
if ('audit_seven_zip_missing' -cin $result.reasons) { Write-Output '7-Zip (7z) is required on PATH.' }
if ($result.result -ne 'pass') { exit 1 }
exit 0
