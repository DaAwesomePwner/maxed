$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/../../scripts/release/Publish-Release.ps1"
$realGh = ${function:Invoke-ReleaseGh}
function Assert($ok, $label) { if (-not $ok) { throw "FAIL: $label" }; Write-Host "PASS: $label" }
function Write-Json($path, $value) { [IO.File]::WriteAllText($path, ($value | ConvertTo-Json -Depth 20)) }
function Start-Sleep { param($Seconds) $script:sleeps += $Seconds }
$root = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
$null = New-Item -ItemType Directory $root
function Reset-Fixture([string]$v = '0.1.3') {
    $script:Version = $v; $script:Channel = if ($v.Contains('-')) { 'beta' } else { 'stable' }
    $script:Repository = 'example/test'
    $case = Join-Path $root ([guid]::NewGuid().ToString()); $null = New-Item -ItemType Directory $case
    $script:StageDir = Join-Path $case 'stage'; $null = New-Item -ItemType Directory $StageDir
    $script:BackupDir = Join-Path $case 'backup'; $script:remote = @{}; $script:calls = @(); $script:failPointer = ''; $script:failUpload = ''
    $script:duplicates = @(); $script:corruptDownload = $false; $script:failRestore = $false; $script:sleeps = @()
    $script:name = "Maxed_${v}_x64-setup.exe"
    foreach ($f in @($name, "$name.sig", 'LICENSE', 'NOTICE.md', 'license-inventory.json')) { [IO.File]::WriteAllText((Join-Path $StageDir $f), 'c3ludGhldGlj') }
    Write-Json "$StageDir/latest.json" @{ version = $v; notes = ''; platforms = @{ 'windows-x86_64-nsis' = @{ url = "https://github.com/$Repository/releases/download/v$v/$name"; signature = 'c3ludGhldGlj' } } }
    Save-Manifest
    Add-Pointer 'update-stable' '0.1.2'
}
function Save-Manifest {
    $files = @(Get-ChildItem $StageDir -File | Where-Object Name -NE 'release-manifest.json' | ForEach-Object { @{ file = $_.Name; bytes = $_.Length; sha256 = (Get-FileHash $_.FullName).Hash.ToLowerInvariant() } })
    Write-Json "$StageDir/release-manifest.json" @{ version = $Version; channel = $Channel; repository = $Repository; files = $files }
}
function Add-Pointer($tag, $v) {
    $script:remote[$tag] = @{ draft = $false; prerelease = ($tag -eq 'update-beta'); assets = @{ 'latest.json' = [Text.Encoding]::UTF8.GetBytes((@{version=$v} | ConvertTo-Json)) } }
}
function Invoke-ReleaseGh([string[]]$Arguments) {
    $a = $Arguments; $script:calls += ,$a
    if ($a[0] -eq 'api') {
        if ($a[1] -like '*/commits/*') { return ('a' * 40) }
        if ($a -contains 'DELETE') {
            $id = $a[-1].Split('/')[-1]
            foreach ($r in $remote.Values) { $r.assets.Remove($id) }
            return ''
        }
        $rows = @($remote.Keys | ForEach-Object { $t = $_; $r = $remote[$t]; @{ tag_name = $t; draft = $r.draft; prerelease = $r.prerelease; assets = @($r.assets.Keys | ForEach-Object { @{ name = $_; id = $_ } }) } })
        $rows += $script:duplicates
        return ConvertTo-Json -InputObject @(,$rows) -Depth 20
    }
    $tag = $a[2]; $r = $remote[$tag]
    switch ($a[1]) {
        create { Assert (-not $r) 'create only absent release'; $remote[$tag] = @{ draft = $true; prerelease = ($a -contains '--prerelease'); assets = @{}; title = $a[[array]::IndexOf($a, '--title') + 1]; notes = $a[[array]::IndexOf($a, '--notes') + 1] } }
        upload {
            Assert ($null -ne $r) 'upload existing release'
            if ($script:failRestore -and $a[3].StartsWith($BackupDir)) { throw 'Injected restore transport failure' }
            if ($tag -eq $script:failUpload) { $r.assets.Remove('latest.json'); $script:failUpload = ''; throw 'Injected upload failure after delete' }
            $r.assets[[IO.Path]::GetFileName($a[3])] = [IO.File]::ReadAllBytes($a[3])
        }
        download {
            $dir = $a[[array]::IndexOf($a, '--dir') + 1]
            foreach ($f in $r.assets.Keys) { [IO.File]::WriteAllBytes((Join-Path $dir $f), $r.assets[$f]) }
            if ($script:corruptDownload -and $tag -eq "v$Version") { [IO.File]::WriteAllText((Join-Path $dir $name), 'corrupt') }
        }
        edit {
            if ($a -contains '--draft=false') { $r.draft = $false; $r.prerelease = $a -contains '--prerelease=true'; $r.latest = $a -contains '--latest=true' }
            foreach ($field in @('title','notes')) { if ($a -contains "--$field") { $r[$field] = $a[[array]::IndexOf($a, "--$field") + 1] } }
        }
        delete { Assert ($a -contains '--cleanup-tag' -and $a -contains '--yes') 'new pointer tag cleaned up'; $remote.Remove($tag) }
        default { throw 'Unexpected CLI call' }
    }
    return ''
}
function Receive-ReleasePublic([string]$Url, [string]$Path) {
    $parts = $Url.Split('/'); $tag = $parts[-2]; $file = $parts[-1]
    Assert (-not $remote[$tag].draft) 'public download only published release'
    $bytes = $remote[$tag].assets[$file]
    if ($tag -eq $script:failPointer -and ([Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json).version -eq $Version) { throw 'Injected public verification failure' }
    [IO.File]::WriteAllBytes($Path, $bytes)
}
function Reject($label, $pattern) {
    $failed = $false
    try { Publish-Release } catch { $failed = $true; Assert ($_.Exception.Message -match $pattern) "$label reason: $_" }
    Assert $failed $label
}
try {
    # Exercise the production CLI wrapper, replacing only the native command.
    function gh { $global:LASTEXITCODE = 23; Write-Error 'synthetic gh stderr' -ErrorAction Continue }
    try {
        $message = ''; try { & $realGh @('release', 'upload', 'v0.1.3') } catch { $message = $_.Exception.Message }
        Assert ($message -match 'gh release upload' -and $message -match 'synthetic gh stderr' -and $message -match '23') 'CLI diagnostics include subcommand, stderr and exit code'
    } finally { Remove-Item Function:gh }
    foreach ($pair in @(@('0.1.3-beta.1','0.1.3'), @('0.1.3-beta.1','0.1.3-beta.2'), @('1.0.0-1','1.0.0-a'), @('1.0.0-a','1.0.0-a.1'), @('1.0.0-9','1.0.0-10'), @('1.9.0','1.10.0'))) {
        Assert ((Compare-ReleaseVersion $pair[0] $pair[1]) -eq -1) 'SemVer ascending'
        Assert ((Compare-ReleaseVersion $pair[1] $pair[0]) -eq 1) 'SemVer descending'
    }
    Assert ((Compare-ReleaseVersion '1.0.0+foo' '1.0.0+bar') -eq 0) 'build metadata ignored'
    foreach ($invalid in @('01.0.0', '1.0.0-01', '1.0', '1.0.0-', '1.0.0+')) {
        $failed = $false; try { $null = Get-SemVer $invalid } catch { $failed = $true }; Assert $failed 'invalid SemVer refused'
    }
    foreach ($mode in @('hash','bytes','extra','missing','url','signature','version','manifest')) {
        Reset-Fixture
        switch ($mode) {
            hash { [IO.File]::WriteAllText("$StageDir/LICENSE", 'xxxxxxxxxxxx') }
            bytes { [IO.File]::WriteAllText("$StageDir/LICENSE", 'x') }
            extra { [IO.File]::WriteAllText("$StageDir/extra", 'x') }
            missing { Remove-Item "$StageDir/LICENSE" }
            manifest { $m = Get-Content "$StageDir/release-manifest.json" -Raw | ConvertFrom-Json; $m.files[0].file = 'extra'; Write-Json "$StageDir/release-manifest.json" $m }
            default { $l = Get-Content "$StageDir/latest.json" -Raw | ConvertFrom-Json; if ($mode -eq 'version') { $l.version = '0.1.4' } else { $l.platforms.'windows-x86_64-nsis'.$mode = 'wrong' }; Write-Json "$StageDir/latest.json" $l; Save-Manifest }
        }
        Reject $mode 'mismatch|Unexpected|allowlist'; Assert ($calls.Count -eq 0) 'invalid stage makes no network calls'
    }
    Reset-Fixture; $remote["v$Version"] = @{draft=$false;assets=@{}}; Reject 'published refusal' 'immutable'
    foreach ($draft in @($true, $false)) {
        Reset-Fixture; $remote["v$Version"] = @{draft=$true;assets=@{}}
        $script:duplicates = @(@{tag_name="v$Version";draft=$draft;assets=@()})
        Reject 'multiple version releases refused' 'Multiple releases|immutable'
        Assert (@($calls | Where-Object { $_[0] -ne 'api' }).Count -eq 0) 'ambiguous version never mutated'
    }
    Reset-Fixture; $script:duplicates = @(@{tag_name='update-stable';draft=$true;assets=@()}); Reject 'duplicate pointer refused' 'Multiple releases'
    Reset-Fixture; $remote.Remove('update-stable'); Reject 'missing stable pointer' 'must already exist'
    foreach ($bad in @('draft','prerelease','assets')) {
        Reset-Fixture
        if ($bad -eq 'assets') { $remote['update-stable'].assets['extra'] = [byte[]]@(1) } else { $remote['update-stable'][$bad] = $true }
        Reject 'bad pointer' 'Invalid pointer release status|Unexpected pointer assets'
    }
    Reset-Fixture; $script:corruptDownload = $true; Reject 'draft download hash mismatch' 'Hash mismatch'
    Assert ($remote["v$Version"].draft) 'bad download never published'
    Reset-Fixture; Add-Pointer 'update-stable' '0.1.3'; Reject 'equal pointer refusal' 'strictly newer'
    Reset-Fixture; Add-Pointer 'update-stable' '0.2.0'; Reject 'older pointer refusal' 'strictly newer'
    Reset-Fixture; $remote["v$Version"] = @{draft=$true;assets=@{'stale'=[byte[]]@(1)}}; Add-Pointer 'update-beta' '0.1.3-beta.2'; Publish-Release
    Assert ($remote["v$Version"].latest -and -not $remote["v$Version"].prerelease) 'stable latest'
    Assert ($remote["v$Version"].title -ceq "Maxed $Version" -and $remote["v$Version"].notes -ceq "Maxed $Version") 'reused draft title and notes reset'
    Assert (-not $remote["v$Version"].assets.ContainsKey('stale') -and -not $remote["v$Version"].assets.ContainsKey('latest.json')) 'draft replaced with immutable allowlist'
    Assert (([Text.Encoding]::UTF8.GetString($remote['update-beta'].assets['latest.json']) | ConvertFrom-Json).version -eq $Version) 'stable advances beta'
    Reset-Fixture; Add-Pointer 'update-beta' '0.2.0-beta.1'; Publish-Release
    Assert (([Text.Encoding]::UTF8.GetString($remote['update-beta'].assets['latest.json']) | ConvertFrom-Json).version -eq '0.2.0-beta.1') 'stable retains newer beta'
    Reset-Fixture '0.1.3-beta.1'; Publish-Release
    Assert ($remote["v$Version"].prerelease -and -not $remote["v$Version"].latest) 'beta prerelease not latest'
    Assert ($remote['update-beta'].prerelease -and -not $remote['update-beta'].draft -and -not $remote['update-beta'].latest) 'missing beta created public prerelease'
    Assert ($remote['update-beta'].title -ceq 'Beta update channel' -and $remote["v$Version"].title -ceq "Maxed $Version") 'new release titles'
    Assert (@($calls | Where-Object { $_ -contains 'update-stable' }).Count -eq 0) 'beta never touches stable'
    Reset-Fixture; Add-Pointer 'update-beta' '0.1.3-beta.1'; $before = [Convert]::ToBase64String($remote['update-stable'].assets['latest.json']); $script:failPointer = 'update-beta'
    Reject 'post-swap rollback' 'Injected'
    Assert (($script:sleeps -join ',') -ceq '2,4,8,16,30,30') 'public verification retries for 90 seconds without real sleep'
    Assert ([Convert]::ToBase64String($remote['update-stable'].assets['latest.json']) -eq $before) 'stable restored after beta failure'
    Assert (([Text.Encoding]::UTF8.GetString($remote['update-beta'].assets['latest.json']) | ConvertFrom-Json).version -eq '0.1.3-beta.1') 'beta restored'
    Reset-Fixture; $script:failUpload = 'update-stable'; Reject 'partial upload rollback' 'Injected upload'
    Assert (([Text.Encoding]::UTF8.GetString($remote['update-stable'].assets['latest.json']) | ConvertFrom-Json).version -eq '0.1.2') 'deleted pointer restored'
    Reset-Fixture; $script:failPointer = 'update-stable'; $script:failRestore = $true
    $warnings = @(); Reject 'manual recovery required' 'MANUAL RECOVERY REQUIRED for update-stable' 3>&1 | ForEach-Object { $warnings += "$_" }
    Assert (($warnings -join ' ') -match 'Restore failed for update-stable: Injected restore transport failure') 'restore diagnostics identify pointer and error'
    Assert (Test-Path "$BackupDir/update-stable/latest.json") 'manual recovery retains backup'
    Reset-Fixture '0.1.3-beta.1'; $script:failPointer = 'update-beta'; Reject 'new beta rollback' 'Injected'; Assert (-not $remote.ContainsKey('update-beta')) 'new pointer removed on failure'
} finally { Remove-Item -LiteralPath $root -Recurse -Force }
Write-Host 'Publish release tests passed.'
