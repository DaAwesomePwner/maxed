<#
S1 manual VM probe preparation. This first implementation prints the matrix and
creates an evidence worksheet; it never launches installers, provisions users,
changes networking, removes dependencies, or marks observations as passed.
Run inside a disposable VM before provisioning the named fresh Windows profile.
Use one reverted VM snapshot / fresh profile for each dependency-state case.
#>
[CmdletBinding()]
param(
    [string]$Installer,
    [string]$OutputDir,
    [string]$TargetProfile,
    [switch]$DisposableVm,
    [switch]$ListCases
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$cases = @(
    [ordered]@{
        case = 'webview-present-offline'; prerequisites = 'WebView2 and default Npcap present; VM offline; standard user'
        action = 'Run installer interactively, then launch installed Maxed without a checkout or Vite.'
        expected = 'No Maxed elevation or bootstrap attempt; local rail, demo and history work.'
    },
    [ordered]@{
        case = 'webview-absent-online'; prerequisites = 'VM snapshot without WebView2; online; standard user'
        action = 'Run installer interactively; observe the visible conditional Microsoft bootstrap; launch installed Maxed.'
        expected = 'Usable runtime and local rail; Maxed stays unelevated; record every bootstrap child and destination.'
    },
    [ordered]@{
        case = 'webview-absent-offline'; prerequisites = 'VM snapshot without WebView2; offline; standard user'
        action = 'Run installer; record failure; restore network and retry the installer.'
        expected = 'Explicit, repairable download failure; no claimed successful usable install before runtime exists.'
    },
    [ordered]@{
        case = 'webview-bootstrap-canceled'; prerequisites = 'VM snapshot without WebView2; online; standard user'
        action = 'Cancel the visible Microsoft bootstrap; record result; retry the installer.'
        expected = 'Cancellation is explicit and retryable; no silent success without a usable runtime.'
    },
    [ordered]@{
        case = 'webview-bootstrap-failed'; prerequisites = 'VM snapshot without WebView2; controlled bootstrap failure; standard user'
        action = 'Use a VM-only failure condition; record the condition and error; restore the snapshot and retry.'
        expected = 'Failure is explicit and repairable; no Maxed elevation or false runtime-ready state.'
    },
    [ordered]@{
        case = 'npcap-absent'; prerequisites = 'WebView2 present; Npcap absent; standard user'
        action = 'Install Maxed; review the finish-page guidance and official URL; view demo/history; use native setup Retry. Separately install official default Npcap, then Retry and observe real traffic.'
        expected = 'Guidance appears only after installation and is advisory; the official address is inline text with no browser launch. Exact Npcap option and separate repair instructions are visible. No Npcap download or execution by Maxed. Missing Npcap does not block install/demo/history; real capture works unelevated after separate setup.'
    },
    [ordered]@{
        case = 'npcap-admin-restricted'; prerequisites = 'WebView2 present; separately configured admin-restricted Npcap; standard user'
        action = 'Install and launch Maxed; use setup Retry; repair Npcap separately with the restriction unchecked and retry.'
        expected = 'Access limitation is honest; guidance never proposes elevating Maxed; demo/history remain usable.'
    },
    [ordered]@{
        case = 'npcap-incompatible'; prerequisites = 'WebView2 present; VM-only incompatible Npcap setup; standard user'
        action = 'Install and launch Maxed; use setup Retry; restore a supported separately installed Npcap and retry.'
        expected = 'Native setup distinguishes incompatibility from missing/access/traffic; advisory installer page makes no readiness claim.'
    }
)

Write-Output 'Supported S1 cases (manual observations, fresh disposable VM/profile per case):'
foreach ($case in $cases) { Write-Output ("  {0}: {1}" -f $case.case, $case.expected) }
if ($ListCases) { return }

if (-not $DisposableVm) { throw 'Refusing a non-disposable target. Run only in a disposable VM and explicitly pass -DisposableVm.' }
if (-not $Installer -or -not $OutputDir -or -not $TargetProfile) {
    throw 'Supply -Installer, a fresh -OutputDir and a not-yet-created -TargetProfile under Windows ProfilesDirectory named Maxed-P7-*.'
}

function Get-AbsolutePath([string]$Path) {
    if (-not [IO.Path]::IsPathRooted($Path)) { throw 'Matrix paths must be absolute.' }
    return [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
}

function Assert-NoReparseAncestor([string]$Path) {
    $cursor = $Path
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            if ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw 'Refusing a path with a reparse-point ancestor.'
            }
        }
        $cursor = Split-Path -Parent $cursor
    }
}

$installerPath = Get-AbsolutePath $Installer
$outputPath = Get-AbsolutePath $OutputDir
$profilePath = Get-AbsolutePath $TargetProfile
foreach ($path in @($installerPath, $outputPath, $profilePath)) { Assert-NoReparseAncestor $path }
if (Test-Path -LiteralPath $outputPath) { throw 'OutputDir already exists; use a fresh evidence directory.' }
if (Test-Path -LiteralPath $profilePath) { throw 'TargetProfile already exists; never reuse an existing Windows profile.' }
if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf) -or [IO.Path]::GetExtension($installerPath) -ine '.exe') {
    throw 'Installer must be an existing NSIS .exe file.'
}

$profileRegistry = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList'
$profileParent = Get-AbsolutePath ([Environment]::ExpandEnvironmentVariables((Get-ItemProperty -LiteralPath $profileRegistry).ProfilesDirectory))
if ((Split-Path -Parent $profilePath) -ine $profileParent -or
    (Split-Path -Leaf $profilePath) -notmatch '^Maxed-P7-[A-Za-z0-9-]+$' -or
    $profilePath -ieq (Get-AbsolutePath $env:USERPROFILE)) {
    throw 'TargetProfile must be a new dedicated Maxed-P7-* Windows profile in ProfilesDirectory, never the current profile.'
}
foreach ($registered in Get-ChildItem -LiteralPath $profileRegistry) {
    $record = Get-ItemProperty -LiteralPath $registered.PSPath
    if ($record.PSObject.Properties['ProfileImagePath'] -and
        (Get-AbsolutePath ([Environment]::ExpandEnvironmentVariables($record.ProfileImagePath))) -ieq $profilePath) {
        throw 'TargetProfile is already registered; choose a new disposable profile.'
    }
}
if ($outputPath.StartsWith($profilePath + '\', [StringComparison]::OrdinalIgnoreCase) -or $outputPath -ieq $profilePath) {
    throw 'Keep evidence outside the not-yet-created target profile.'
}

$file = Get-Item -LiteralPath $installerPath
$matrix = [ordered]@{
    schema_version = 1
    prepared_utc = [DateTime]::UtcNow.ToString('o')
    status = 'not_run'
    installer = [ordered]@{
        name = $file.Name; sha256 = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash
        bytes = $file.Length; version = $file.VersionInfo.ProductVersion
    }
    target_profile_name = Split-Path -Leaf $profilePath
    windows_version = [Environment]::OSVersion.Version.ToString()
    disposable_vm_attested = $true
    cases = @($cases | ForEach-Object {
        [ordered]@{
            case = $_.case; prerequisites = $_.prerequisites; action = $_.action; expected = $_.expected
            result = 'not_run'; observed = ''; evidence_files = @()
            snapshot_id = ''; windows_build = ''; runtime_version_before = ''; runtime_version_after = ''
            npcap_version_and_mode = ''; user_is_standard = $null; maxed_elevated = $null
            bootstrap_elevated = $null; installer_exit_code = $null; installed_exe_sha256 = ''
            network_destinations = @(); process_id_and_start_time = @(); trace_tool_and_limits = ''
            per_user_registry_shortcuts_uninstall = ''; repair_retry_result = ''
        }
    })
}

# No cleanup, account creation, installer execution or runtime removal is performed.
$null = New-Item -ItemType Directory -Path $outputPath
$matrix | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $outputPath 'installer-matrix.json') -Encoding UTF8
@'
This is a NOT-RUN worksheet, not installation evidence.

1. Snapshot the disposable VM. Provision the reserved new standard-user profile;
   never use the owner's Windows profile. Choose the dependency state listed for
   one case. Revert to a fresh snapshot/profile between independent cases.
2. Transfer only the hash-matched installer and safe observation tools. Do not
   depend on a source checkout, Vite, development PATH or cached source assets.
3. Run the installer interactively as that standard user. Observe the case's
   action and expected results. Record installed executable identity, local UI,
   per-user registration/shortcuts/uninstall and explicit dependency Retry results.
4. Record destination metadata and process PID/start-time attribution for Maxed,
   installer and bootstrap children, plus trace-tool limitations. Do not capture
   packet bodies, credentials, private captures or unrelated profile contents.
5. Fill each observed cell as pass/fail with evidence. Unobserved cells stay
   not_run. An exit code or config parse cannot prove a usable WebView2 runtime.
6. Stop this packaging approach if Maxed requires elevation, WebView2 installation
   requires elevation in the standard-user case, or missing/canceled/failed
   bootstrap silently reports success without a usable runtime. Report a concrete
   no-go to the coordinator; never remove the owner's runtime or change security
   settings to force success. Present-runtime bootstrap traffic is also a no-go.

This S1 worksheet does not replace J1's reinstall/upgrade/uninstall preservation,
signature, update, performance, license or full installed-app acceptance matrices.
'@ | Set-Content -LiteralPath (Join-Path $outputPath 'INSTRUCTIONS.txt') -Encoding UTF8
Write-Output 'Prepared installer-matrix.json and INSTRUCTIONS.txt. Every VM case remains not_run.'
