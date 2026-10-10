[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$OutputDir,
    [string]$Commit = 'HEAD',
    [string]$ExcludeList,
    [string]$AllowList,
    [string[]]$DenyPattern = @(),
    [string]$PublicClone
)
$ErrorActionPreference = 'Stop'
# Exports one commit's tracked files, minus the reviewed exclude list, as the
# public source snapshot. Untracked, ignored and uncommitted files never reach it.
# With -PublicClone it also stages the snapshot there with the source file modes.
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $ExcludeList) { $ExcludeList = Join-Path $PSScriptRoot 'public-exclude.txt' }
if (-not $AllowList) { $AllowList = Join-Path $PSScriptRoot 'public-allow.txt' }
foreach ($list in @($ExcludeList, $AllowList)) {
    if (-not (Test-Path -LiteralPath $list -PathType Leaf)) { throw "Review list is missing: $list" }
}
if (Test-Path -LiteralPath $OutputDir) { throw 'Output already exists.' }
$output = [IO.Path]::GetFullPath($OutputDir)
$parent = Get-Item -LiteralPath (Split-Path -Parent $output)
while ($null -ne $parent) {
    if ($parent.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Output ancestors must not be links.' }
    $parent = $parent.Parent
}

function Invoke-Git {
    $out = & git -C $root @args
    if ($LASTEXITCODE) { throw "git $($args[0]) failed." }
    $out
}

if ($PublicClone) {
    $clone = [IO.Path]::GetFullPath($PublicClone).TrimEnd('\', '/')
    if (-not (Test-Path -LiteralPath $clone -PathType Container)) { throw 'Public clone must be the root of a git checkout.' }
    $top = & git -C $clone rev-parse --show-toplevel
    if ($LASTEXITCODE -or -not $top -or [IO.Path]::GetFullPath($top).TrimEnd('\', '/') -ne $clone) { throw 'Public clone must be the root of a git checkout.' }
    if ($clone -eq [IO.Path]::GetFullPath($root).TrimEnd('\', '/')) { throw 'Public clone must not be the source repository.' }
    if (@(& git -C $clone status --porcelain --untracked-files=all).Count -or $LASTEXITCODE) { throw 'Public clone has uncommitted or untracked files.' }
}
function Invoke-CloneGit {
    $out = & git --literal-pathspecs -c core.quotePath=true -C $clone @args
    if ($LASTEXITCODE) { throw "git $($args[0]) failed in the public clone; reset it with: git -C `"$clone`" reset --hard HEAD" }
    $out
}

function Read-ReviewList($path) {
    foreach ($line in [IO.File]::ReadAllLines([IO.Path]::GetFullPath($path))) {
        $entry = $line.Trim()
        if ($entry -and -not $entry.StartsWith('#')) { $entry }
    }
}

$pathShape = '[A-Za-z0-9._-]+(?:/[A-Za-z0-9._-]+)*'
function Assert-RepoPath($path, $what) {
    if ($path -cnotmatch "\A$pathShape/?\z" -or @($path.TrimEnd('/').Split('/') | Where-Object { $_ -in @('.', '..') }).Count) {
        throw "Invalid $what entry: $path"
    }
}

$sha = & git -C $root rev-parse --verify --quiet "$Commit^{commit}"
if ($LASTEXITCODE -or $sha -cnotmatch '\A[0-9a-f]{40,64}\z') { throw 'Commit does not resolve.' }
# Forced quoting marks unusual names with a leading quote whatever the user's
# config; only printable ASCII paths pass. Links and submodules need review too.
# Modes are kept for the public clone.
$modes = @{}
$all = @(foreach ($line in @(Invoke-Git -c core.quotePath=true ls-tree -r --full-tree $sha)) {
    $meta, $path = $line.Split("`t", 2)
    $mode, $type, $object = $meta.Split(' ')
    if ($path -cnotmatch '\A[\x20-\x7E]+\z' -or $path.StartsWith('"') -or $type -cne 'blob' -or $mode -cnotin @('100644', '100755')) { throw "Path needs review before export: $path" }
    $modes[$path] = "$mode $object"
    $path
})

$entries = @(Read-ReviewList $ExcludeList)
foreach ($entry in $entries) { Assert-RepoPath $entry 'exclude' }
if (@($entries | Select-Object -Unique).Count -ne $entries.Count) { throw 'Duplicate exclude entry.' }
$used = @{}
$kept = New-Object System.Collections.Generic.List[string]
foreach ($path in $all) {
    $match = $null
    foreach ($entry in $entries) {
        if ($path -ceq $entry -or ($entry.EndsWith('/') -and $path.StartsWith($entry, [StringComparison]::Ordinal))) { $match = $entry; break }
    }
    if ($match) { $used[$match] = $true } else { $kept.Add($path) }
}
foreach ($entry in $entries) { if (-not $used.ContainsKey($entry)) { throw "Exclude entry matches nothing at ${sha}: $entry" } }

# Content rules run over every exported byte, read as Latin-1 so strings inside
# binary fixtures are checked too. Base64 prefixes catch encoded signing keys.
$encoded = foreach ($comment in @('rsign', 'minisign')) {
    # Split so this script does not match its own signing-key rule.
    $text = "untrusted comment: $comment encrypted secret" + ' key'
    [regex]::Escape([Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes($text.Substring(0, $text.Length - ($text.Length % 3)))))
}
$rules = [ordered]@{
    'private-key' = '-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----'
    'signing-key' = '(?i)untrusted comment:[^\r\n]{0,160}\bsecret key\b'
    'encoded-signing-key' = ($encoded -join '|')
    'github-token' = '\b(?:gh[pousr]_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22,})'
    # Both branches start at the folder name so the scan can skip ahead.
    'user-path' = '(?i)(?:(?<=\b[A-Z]:\\+)Users\\+(?!Public\b|Default\b)[^\\\s"''<>%]+|(?<=(?<![A-Za-z0-9_.-])/)(?:Users|home)/(?!runner\b)[a-z][^/\s"''<>]*/)'
}
# Captures are denied by default, by name or by classic pcap/pcapng magic, so a
# new scrubbed-real fixture stays private until its file is explicitly allowed.
$captureMagic = @('d4c3b2a1', 'a1b2c3d4', '4d3cb2a1', 'a1b23c4d', '0a0d0d0a')
$allowable = @($rules.Keys) + 'capture'
$index = 0
foreach ($pattern in $DenyPattern) { $index++; $rules["deny-$index"] = "(?i)$pattern" }
$compiled = [ordered]@{}
foreach ($rule in $rules.GetEnumerator()) { $compiled[$rule.Key] = New-Object regex $rule.Value, ([Text.RegularExpressions.RegexOptions]::Compiled) }
# Mirrors .gitignore: SQLite sidecars (-wal, -shm, -journal) and logs carry runtime data too.
$forbiddenName = '(?i)\.(?:pcapng|mxrec|pem|pfx|p12|key|log)$|\.(?:sqlite3?|db)(?:-[a-z]+)?$|(?:^|/)recordings/|(?:^|/)\.env(?:\.|$)'

# Each approval names the reviewed git blob, so any later change to an allowed
# file, such as a real key next to a canary, needs renewed review.
$allowed = @{}
$approvedBlob = @{}
foreach ($line in @(Read-ReviewList $AllowList)) {
    $parts = $line -split '\s+'
    if ($parts.Count -ne 3 -or $parts[0] -cnotin $allowable -or $parts[2] -cnotmatch '\A[0-9a-f]{40}(?:[0-9a-f]{24})?\z') { throw "Invalid allow entry: $line" }
    Assert-RepoPath $parts[1] 'allow'
    $pair = "$($parts[0]) $($parts[1])"
    if ($allowed.ContainsKey($pair)) { throw 'Duplicate allow entry.' }
    $allowed[$pair] = $false
    $approvedBlob[$pair] = $parts[2]
}

$zip = Join-Path ([IO.Path]::GetTempPath()) ('maxed-snapshot-' + [Guid]::NewGuid().ToString('N') + '.zip')
$staging = "$output.partial-" + [Guid]::NewGuid().ToString('N')
try {
    $pathspecs = @('.') + @($entries | ForEach-Object { ':(exclude,literal)' + $_.TrimEnd('/') })
    $null = Invoke-Git archive --format=zip "--output=$zip" $sha -- @pathspecs
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::ExtractToDirectory($zip, $staging)
    $prefix = $staging.Length + 1
    $expected = New-Object 'System.Collections.Generic.HashSet[string]' ([string[]]$kept.ToArray()), ([StringComparer]::Ordinal)
    $written = 0
    foreach ($file in Get-ChildItem -LiteralPath $staging -Recurse -File -Force) {
        $written++
        if (-not $expected.Contains($file.FullName.Substring($prefix).Replace('\', '/'))) { throw 'Archive contents differ from the reviewed file list.' }
    }
    if ($written -ne $expected.Count) { throw 'Archive contents differ from the reviewed file list.' }

    $latin1 = [Text.Encoding]::GetEncoding(28591)
    $findings = New-Object System.Collections.Generic.List[string]
    foreach ($path in $kept) {
        if ($path -match $forbiddenName) { $findings.Add("forbidden-file $path") }
        $bytes = [IO.File]::ReadAllBytes((Join-Path $staging $path))
        $content = $latin1.GetString($bytes)
        $hits = @(foreach ($rule in $compiled.GetEnumerator()) { if ($rule.Value.IsMatch($path) -or $rule.Value.IsMatch($content)) { $rule.Key } })
        if ($path -match '(?i)\.pcap$' -or ($bytes.Length -ge 4 -and [BitConverter]::ToString($bytes, 0, 4).Replace('-', '').ToLowerInvariant() -in $captureMagic)) { $hits += 'capture' }
        foreach ($key in $hits) {
            $finding = "$key $path"
            if (-not $allowed.ContainsKey($finding)) { $findings.Add($finding); continue }
            $allowed[$finding] = $true
            $blob = $modes[$path].Split(' ')[1]
            if ($approvedBlob[$finding] -cne $blob) { $findings.Add("$finding changed: blob $blob is not the approved $($approvedBlob[$finding])") }
        }
    }
    foreach ($pair in $allowed.GetEnumerator()) { if (-not $pair.Value) { $findings.Add("stale allow entry: $($pair.Key)") } }
    if ($findings.Count) { throw ("Snapshot failed review:`n" + ($findings -join "`n")) }
    Move-Item -LiteralPath $staging -Destination $output
} finally {
    Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue
    if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
}
Write-Output "Exported $sha to ${output}: $($kept.Count) files kept, $($all.Count - $kept.Count) excluded by $($entries.Count) entries."
if (-not $PublicClone) { return }

# Copying through the file system loses executable bits where core.filemode is
# false, so stage source modes explicitly, then require an exact blob/mode match.
$null = Invoke-CloneGit rm -r -q --ignore-unmatch -- .
foreach ($path in $kept) {
    $target = Join-Path $clone $path
    $null = New-Item -ItemType Directory -Force -Path (Split-Path -Parent $target)
    [IO.File]::Copy((Join-Path $output $path), $target, $true)
}
$list = Join-Path ([IO.Path]::GetTempPath()) ('maxed-snapshot-' + [Guid]::NewGuid().ToString('N') + '.txt')
try {
    [IO.File]::WriteAllText($list, (($kept -join "`n") + "`n"))
    $null = Invoke-CloneGit add -f "--pathspec-from-file=$list"
} finally { Remove-Item -LiteralPath $list -Force -ErrorAction SilentlyContinue }
$executable = @($kept | Where-Object { $modes[$_].StartsWith('100755 ') })
if ($executable.Count) { $null = Invoke-CloneGit update-index --chmod=+x -- @executable }
$staged = @{}
foreach ($line in @(Invoke-CloneGit ls-files -s)) {
    $meta, $path = $line.Split("`t", 2)
    $mode, $object, $null = $meta.Split(' ')
    $staged[$path] = "$mode $object"
}
if ($staged.Count -ne $kept.Count -or @($kept | Where-Object { $staged[$_] -cne $modes[$_] }).Count) {
    throw "Public clone index differs from the export; reset it with: git -C `"$clone`" reset --hard HEAD"
}
Write-Output "Staged the snapshot in $clone with source file modes ($($executable.Count) executable). Review it with git diff --cached, then commit."
