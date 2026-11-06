[CmdletBinding()]
param([string]$Version, [string]$Channel, [string]$Repository, [string]$StageDir, [string]$BackupDir)
$ErrorActionPreference = 'Stop'
# Only these transport functions access the network; tests replace both.
function Invoke-ReleaseGh([string[]]$Arguments) {
    $err = [IO.Path]::GetTempFileName()
    try {
        $ErrorActionPreference = 'Continue'
        $result = & gh @Arguments 2> $err
        $code = $LASTEXITCODE
        $ErrorActionPreference = 'Stop'
        if ($code) { throw "GitHub CLI failed (exit $code)." }
        return ($result -join "`n")
    } finally { Remove-Item -LiteralPath $err -Force }
}
function Receive-ReleasePublic([string]$Url, [string]$Path) {
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $Path -TimeoutSec 120 | Out-Null
}
function Get-SemVer([string]$Value) {
    $n = '(?:0|[1-9][0-9]*)'; $id = '(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)'
    if ($Value -cnotmatch "\A($n)\.($n)\.($n)(?:-($id(?:\.$id)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?\z") { throw 'Invalid SemVer.' }
    return @{ Core = @($Matches[1], $Matches[2], $Matches[3]); Pre = $Matches[4] }
}
function Compare-ReleaseVersion([string]$Left, [string]$Right) {
    $a = Get-SemVer $Left; $b = Get-SemVer $Right
    for ($i = 0; $i -lt 3; $i++) {
        $x = $a.Core[$i]; $y = $b.Core[$i]
        if ($x.Length -ne $y.Length) { return [Math]::Sign($x.Length - $y.Length) }
        $c = [string]::CompareOrdinal($x, $y); if ($c) { return [Math]::Sign($c) }
    }
    if (-not $a.Pre -and -not $b.Pre) { return 0 }
    if (-not $a.Pre) { return 1 }; if (-not $b.Pre) { return -1 }
    $aa = $a.Pre.Split('.'); $bb = $b.Pre.Split('.')
    for ($i = 0; $i -lt [Math]::Min($aa.Count, $bb.Count); $i++) {
        $x = $aa[$i]; $y = $bb[$i]; $xn = $x -cmatch '^\d+$'; $yn = $y -cmatch '^\d+$'
        if ($xn -and -not $yn) { return -1 }; if ($yn -and -not $xn) { return 1 }
        if ($xn -and $x.Length -ne $y.Length) { return [Math]::Sign($x.Length - $y.Length) }
        $c = [string]::CompareOrdinal($x, $y); if ($c) { return [Math]::Sign($c) }
    }
    return [Math]::Sign($aa.Count - $bb.Count)
}
function Get-Release([string]$Tag) {
    # Successful listing distinguishes absence from authentication/network failure.
    $pages = Invoke-ReleaseGh @('api', "repos/$Repository/releases?per_page=100", '--paginate', '--slurp') | ConvertFrom-Json
    foreach ($page in $pages) { foreach ($r in $page) { if ($r.tag_name -ceq $Tag) { return $r } } }
    return $null
}
function Assert-ReleaseHash([string]$Path, [string]$Hash) {
    if ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ine $Hash) { throw "Hash mismatch: $([IO.Path]::GetFileName($Path))" }
}
function Confirm-PublicFile([string]$Url, [string]$Hash, [string]$Path) {
    for ($attempt = 0; $attempt -lt 5; $attempt++) {
        try { Receive-ReleasePublic $Url $Path; Assert-ReleaseHash $Path $Hash; return }
        catch { if ($attempt -eq 4) { throw }; Start-Sleep -Seconds 2 }
    }
}
function Publish-Release {
    $null = Get-SemVer $Version
    if ($Repository -cnotmatch '\A[A-Za-z0-9][A-Za-z0-9-]*/[A-Za-z0-9][A-Za-z0-9_.-]*\z') { throw 'Invalid repository.' }
    $expectedChannel = if ($Version.Split('+')[0].Contains('-')) { 'beta' } else { 'stable' }
    if ($Channel -cne $expectedChannel) { throw 'Channel/version mismatch.' }
    $name = "Maxed_${Version}_x64-setup.exe"
    $files = @($name, "$name.sig", 'LICENSE', 'NOTICE.md', 'latest.json', 'license-inventory.json')
    $all = @($files) + @('release-manifest.json')
    $items = @(Get-ChildItem -LiteralPath $StageDir -Force)
    if ($items.Count -ne $all.Count) { throw 'Unexpected stage file count.' }
    foreach ($item in $items) {
        if ($item.Name -cnotin $all -or $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unexpected stage entry.' }
    }
    $manifest = Get-Content -LiteralPath "$StageDir/release-manifest.json" -Raw | ConvertFrom-Json
    if ($manifest.version -cne $Version -or $manifest.channel -cne $Channel -or $manifest.repository -cne $Repository -or @($manifest.files).Count -ne $files.Count) { throw 'Manifest identity mismatch.' }
    $seen = @{}
    foreach ($entry in $manifest.files) {
        if ($entry.file -cnotin $files -or $seen.ContainsKey($entry.file) -or $entry.sha256 -cnotmatch '\A[a-f0-9]{64}\z') { throw 'Invalid manifest allowlist.' }
        $seen[$entry.file] = $entry.sha256
        $path = Join-Path $StageDir $entry.file
        if ((Get-Item -LiteralPath $path).Length -ne $entry.bytes) { throw 'Byte count mismatch.' }
        Assert-ReleaseHash $path $entry.sha256
    }
    $latest = Get-Content -LiteralPath "$StageDir/latest.json" -Raw | ConvertFrom-Json
    $platform = $latest.platforms.'windows-x86_64-nsis'
    $url = "https://github.com/$Repository/releases/download/v$Version/$name"
    if ($latest.version -cne $Version -or $platform.url -cne $url -or $platform.signature -cne ([IO.File]::ReadAllText((Join-Path $StageDir "$name.sig"))).Trim()) { throw 'Feed identity/signature mismatch.' }
    $tag = "v$Version"; $release = Get-Release $tag
    if ($release -and -not $release.draft) { throw 'Published version is immutable; use manual recovery.' }
    if (Test-Path -LiteralPath $BackupDir) { throw 'Backup directory must be new.' }
    $null = New-Item -ItemType Directory -Path $BackupDir
    $work = Join-Path $BackupDir 'verification'
    $null = New-Item -ItemType Directory -Path $work
    $pointers = @(); $tags = @("update-$Channel")
    if ($Channel -ceq 'stable') { $tags += 'update-beta' }
    foreach ($pointer in $tags) {
        $old = Get-Release $pointer
        if (-not $old) {
            if ($pointer -ceq 'update-stable') { throw 'Stable pointer must already exist.' }
            if ($Channel -ceq 'stable') { continue }
        }
        $backup = Join-Path $BackupDir "$pointer/latest.json"; $hash = $null
        if ($old) {
            if ($old.draft -or [bool]$old.prerelease -ne ($pointer -ceq 'update-beta')) { throw 'Invalid pointer release status.' }
            if (@($old.assets).Count -ne 1 -or $old.assets[0].name -cne 'latest.json') { throw 'Unexpected pointer assets.' }
            $null = New-Item -ItemType Directory -Path (Split-Path -Parent $backup)
            $null = Invoke-ReleaseGh @('release', 'download', $pointer, '--repo', $Repository, '--pattern', 'latest.json', '--dir', (Split-Path -Parent $backup))
            $prior = Get-Content -LiteralPath $backup -Raw | ConvertFrom-Json
            $hash = (Get-FileHash -LiteralPath $backup).Hash
            Write-Host "$pointer backup version=$($prior.version) sha256=$hash"
            if ((Compare-ReleaseVersion $Version $prior.version) -le 0) {
                if ($pointer -ceq "update-$Channel") { throw 'Promotion must be strictly newer.' }
                continue
            }
        }
        $pointers += [pscustomobject]@{ Tag = $pointer; Old = $old; Backup = $backup; Hash = $hash }
    }
    if (-not $release) {
        $null = Invoke-ReleaseGh @('release', 'create', $tag, '--repo', $Repository, '--verify-tag', '--draft', '--title', $tag, '--notes', "Maxed $Version")
    } else {
        foreach ($asset in $release.assets) { $null = Invoke-ReleaseGh @('api', '--method', 'DELETE', "repos/$Repository/releases/assets/$($asset.id)") }
    }
    $immutable = @($all | Where-Object { $_ -cne 'latest.json' })
    foreach ($file in $immutable) { $null = Invoke-ReleaseGh @('release', 'upload', $tag, (Join-Path $StageDir $file), '--repo', $Repository) }
    $null = Invoke-ReleaseGh @('release', 'download', $tag, '--repo', $Repository, '--dir', $work)
    if (@(Get-ChildItem -LiteralPath $work -Force).Count -ne $immutable.Count) { throw 'Remote asset count mismatch.' }
    foreach ($file in $immutable) { Assert-ReleaseHash (Join-Path $work $file) (Get-FileHash -LiteralPath (Join-Path $StageDir $file)).Hash }
    $pre = if ($Channel -ceq 'beta') { '--prerelease=true' } else { '--prerelease=false' }
    $latestFlag = if ($Channel -ceq 'stable') { '--latest=true' } else { '--latest=false' }
    $null = Invoke-ReleaseGh @('release', 'edit', $tag, '--repo', $Repository, '--draft=false', $pre, $latestFlag)
    Confirm-PublicFile $url $seen[$name] (Join-Path $work 'public-installer')
    $changed = @()
    try {
        foreach ($p in $pointers) {
            $changed += $p # Before any potentially partially successful mutation.
            if (-not $p.Old) {
                $commit = Invoke-ReleaseGh @('api', "repos/$Repository/commits/$tag", '--jq', '.sha')
                if ($commit -cnotmatch '\A[a-f0-9]{40}\z') { throw 'Invalid release target commit.' }
                $null = Invoke-ReleaseGh @('release', 'create', $p.Tag, '--repo', $Repository, '--draft', '--prerelease', '--latest=false', '--target', $commit, '--title', $p.Tag, '--notes', 'Beta update pointer')
            }
            $null = Invoke-ReleaseGh @('release', 'upload', $p.Tag, (Join-Path $StageDir 'latest.json'), '--repo', $Repository, '--clobber')
            if (-not $p.Old) { $null = Invoke-ReleaseGh @('release', 'edit', $p.Tag, '--repo', $Repository, '--draft=false', '--prerelease=true', '--latest=false') }
            Confirm-PublicFile "https://github.com/$Repository/releases/download/$($p.Tag)/latest.json" $seen['latest.json'] (Join-Path $work 'public-feed')
            Confirm-PublicFile $url $seen[$name] (Join-Path $work 'public-installer')
        }
    } catch {
        $failure = $_; $restoreErrors = @(); [array]::Reverse($changed)
        foreach ($p in $changed) {
            try {
                if ($p.Old) {
                    $null = Invoke-ReleaseGh @('release', 'upload', $p.Tag, $p.Backup, '--repo', $Repository, '--clobber')
                    Confirm-PublicFile "https://github.com/$Repository/releases/download/$($p.Tag)/latest.json" $p.Hash (Join-Path $work 'restored-feed')
                } else {
                    if (Get-Release $p.Tag) { $null = Invoke-ReleaseGh @('release', 'delete', $p.Tag, '--repo', $Repository, '--yes') }
                    if (Get-Release $p.Tag) { throw 'New pointer removal failed.' }
                }
            } catch { $restoreErrors += $p.Tag }
        }
        if ($restoreErrors.Count) { throw "Promotion failed; MANUAL RECOVERY REQUIRED for $($restoreErrors -join ', '). Backups: $BackupDir" }
        throw $failure
    }
}
if ($MyInvocation.InvocationName -ne '.') { Publish-Release }
