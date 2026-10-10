[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$Channel,
    [Parameter(Mandatory)][string]$Repository,
    [Parameter(Mandatory)][string]$Installer,
    [Parameter(Mandatory)][string]$Signature,
    [Parameter(Mandatory)][string]$OutputDir,
    [Parameter(Mandatory)][string]$LicenseInventory,
    [string]$Notes = '', [string]$PubDate
)
$ErrorActionPreference = 'Stop'
$Installer = [IO.Path]::GetFullPath($Installer)
$Signature = [IO.Path]::GetFullPath($Signature)
$LicenseInventory = [IO.Path]::GetFullPath($LicenseInventory)
& "$PSScriptRoot/Validate-Release.ps1" -Version $Version -Channel $Channel -Repository $Repository
if (Test-Path -LiteralPath $OutputDir) { throw 'Output already exists.' }
$output = [IO.Path]::GetFullPath($OutputDir)
$parent = Get-Item -LiteralPath (Split-Path -Parent $output)
while ($null -ne $parent) {
    if ($parent.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Output ancestors must not be links.' }
    $parent = $parent.Parent
}
$name = "Maxed_${Version}_x64-setup.exe"
foreach ($path in @($Installer, $Signature, $LicenseInventory)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw 'Required release input is missing.' }
    $item = Get-Item -LiteralPath $path
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Release inputs must not be links.' }
}
if ([IO.Path]::GetFileName($Installer) -cne $name -or [IO.Path]::GetFullPath($Signature) -cne ([IO.Path]::GetFullPath($Installer) + '.sig')) { throw 'Artifact/signature names do not match the immutable contract.' }
$size = (Get-Item -LiteralPath $Installer).Length
if ($size -le 0 -or $size -ge 20000000) { throw 'Installer must be nonempty and strictly below 20000000 bytes.' }
if ((Get-Item -LiteralPath $Signature).Length -gt 8192) { throw 'Signature exceeds bound.' }
$sig = [IO.File]::ReadAllText((Resolve-Path -LiteralPath $Signature)).Trim()
if ([string]::IsNullOrWhiteSpace($sig) -or $sig -cnotmatch '\A[A-Za-z0-9+/]+={0,2}\z') { throw 'Signature must contain Tauri base64 signature contents.' }
if ([Text.Encoding]::UTF8.GetByteCount($Notes) -gt 8192 -or $Notes -match '[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]') { throw 'Notes exceed plain-text bounds.' }
if ($PSBoundParameters.ContainsKey('PubDate')) {
    $date = [DateTimeOffset]::MinValue
    if ($PubDate -cnotmatch '\A\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})\z' -or -not [DateTimeOffset]::TryParse($PubDate, [ref]$date)) { throw 'Invalid RFC3339 date.' }
}
if ((Get-Item -LiteralPath $LicenseInventory).Length -gt 1048576) { throw 'License inventory exceeds bound.' }
$inventory = Get-Content -LiteralPath $LicenseInventory -Raw | ConvertFrom-Json
$inventory = @($inventory)
if ($inventory.Count -eq 0) { throw 'License inventory is empty.' }
$seen = @{}
foreach ($entry in $inventory) {
    if ((@($entry.PSObject.Properties.Name | Sort-Object) -join ',') -cne 'file,license,sha256' -or
        ($entry.file -cnotmatch '\A[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*\z' -and
         $entry.file -cnotmatch '\A\$PLUGINSDIR/[A-Za-z0-9_.-]+\z') -or
        @($entry.file.Split('/') | Where-Object { $_ -in @('.', '..') }).Count -or
        $entry.license -cnotmatch '\A[A-Za-z0-9][A-Za-z0-9 .()+-]{0,127}\z' -or
        $entry.sha256 -cnotmatch '\A[a-f0-9]{64}\z' -or $seen.ContainsKey($entry.file)) { throw 'Invalid or duplicate license inventory entry.' }
    $seen[$entry.file] = $true
}
if (-not $seen.ContainsKey('LICENSE') -or -not $seen.ContainsKey('NOTICE.md')) { throw 'License inventory must include LICENSE and NOTICE.md.' }
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
foreach ($file in @('LICENSE', 'NOTICE.md')) {
    $entry = $inventory | Where-Object { $_.file -ceq $file }
    if ($entry.sha256 -cne (Get-FileHash -LiteralPath "$root/$file" -Algorithm SHA256).Hash.ToLowerInvariant()) { throw 'License input hash mismatch.' }
}
# Verify exact bytes, configured public key and authenticated version. Never sign here.
$before = (Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash
Push-Location $root
try {
    # Windows PowerShell 5.1 turns redirected native stderr (cargo progress) into
    # terminating errors under Stop; the exit code alone decides the result.
    $ErrorActionPreference = 'Continue'
    & cargo run --offline --target-dir target -p maxed-app --features updater-signing-tool --bin maxed-update-verify -- "$root/app/src-tauri/tauri.conf.json" ([IO.Path]::GetFullPath($Installer)) ([IO.Path]::GetFullPath($Signature)) *> $null
    $verifyExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($verifyExit) { throw 'Artifact public-key verification failed.' }
} finally { $ErrorActionPreference = 'Stop'; Pop-Location }
$latest = [ordered]@{ version = $Version; notes = $Notes; platforms = @{ 'windows-x86_64-nsis' = @{ url = "https://github.com/$Repository/releases/download/v$Version/$name"; signature = $sig } } }
if ($PSBoundParameters.ContainsKey('PubDate')) { $latest.pub_date = $PubDate }
$json = $latest | ConvertTo-Json -Depth 8
if ([Text.Encoding]::UTF8.GetByteCount($json) -gt 65536) { throw 'Feed exceeds bound.' }
# Create-new directory, no force/overwrite. A failed stage is never uploaded.
$null = New-Item -ItemType Directory -Path $output -ErrorAction Stop
Copy-Item -LiteralPath $Installer -Destination (Join-Path $output $name)
Copy-Item -LiteralPath $Signature -Destination (Join-Path $output "$name.sig")
if ((Get-FileHash -LiteralPath (Join-Path $output $name)).Hash -cne $before -or [IO.File]::ReadAllText((Join-Path $output "$name.sig")).Trim() -cne $sig) { throw 'Release inputs changed during staging.' }
$utf8 = New-Object Text.UTF8Encoding($false)
[IO.File]::WriteAllText((Join-Path $output 'latest.json'), $json, $utf8)
[IO.File]::WriteAllText((Join-Path $output 'license-inventory.json'), (ConvertTo-Json -InputObject @($inventory) -Depth 8), $utf8)
foreach ($file in @('LICENSE', 'NOTICE.md')) { Copy-Item -LiteralPath "$root/$file" -Destination (Join-Path $output $file) }
$files = @($name, "$name.sig", 'latest.json', 'license-inventory.json', 'LICENSE', 'NOTICE.md')
$manifest = [ordered]@{ version = $Version; channel = $Channel; repository = $Repository; files = @($files | ForEach-Object { @{ file = $_; bytes = (Get-Item -LiteralPath (Join-Path $output $_)).Length; sha256 = (Get-FileHash -LiteralPath (Join-Path $output $_)).Hash.ToLowerInvariant() } }) }
[IO.File]::WriteAllText((Join-Path $output 'release-manifest.json'), ($manifest | ConvertTo-Json -Depth 8), $utf8)
