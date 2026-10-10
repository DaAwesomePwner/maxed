[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$temp = Join-Path ([IO.Path]::GetTempPath()) ('maxed-snapshot-' + [Guid]::NewGuid().ToString('N'))
$repo = Join-Path $temp 'repo'
$script:checks = 0
function Assert($ok, $name) { if (-not $ok) { throw "FAIL: $name" }; $script:checks++; Write-Output "PASS: $name" }
# Invented repository, identities and canaries. Literals are split so this file
# passes the snapshot rules it exercises.
function Invoke-RepoGit {
    $out = & git -C $repo -c user.name=Synthetic -c user.email=synthetic@example.invalid -c core.autocrlf=false -c commit.gpgsign=false @args
    if ($LASTEXITCODE) { throw "git $($args[0]) failed." }
    $out
}
function Write-RepoFile($path, $text) {
    $full = Join-Path $repo $path
    $null = New-Item -ItemType Directory -Force -Path (Split-Path -Parent $full)
    [IO.File]::WriteAllText($full, $text)
}
function Save-Commit($message) { $null = Invoke-RepoGit add -A; $null = Invoke-RepoGit commit -q -m $message; Invoke-RepoGit rev-parse HEAD }
function Write-List($name, [string[]]$lines) { $path = Join-Path $temp $name; [IO.File]::WriteAllLines($path, $lines); $path }
function Invoke-Export($options) {
    $p = @{ OutputDir = (Join-Path $temp ([Guid]::NewGuid().ToString('N'))); ExcludeList = $script:exclude; AllowList = $script:allow }
    foreach ($key in $options.Keys) { $p[$key] = $options[$key] }
    $null = & $script:tool @p
    $p.OutputDir
}
function Reject($options, $name, $message) {
    $out = Join-Path $temp ([Guid]::NewGuid().ToString('N'))
    if (-not $options.ContainsKey('OutputDir')) { $options.OutputDir = $out }
    $failed = $false
    try { $null = Invoke-Export $options } catch { $failed = $true; $script:lastError = $_.Exception.Message }
    Assert $failed $name
    if ($message) { Assert $script:lastError.Contains($message) "$name reports $message" }
    Assert (-not (Test-Path -LiteralPath $out)) "$name creates no output"
    Assert (@(Get-ChildItem -LiteralPath $temp -Filter '*.partial-*').Count -eq 0) "$name leaves no staging"
}
try {
    $null = New-Item -ItemType Directory -Path "$repo/scripts/release"
    $null = Invoke-RepoGit init -q
    # Operator config must not disable the unusual-path review.
    $null = Invoke-RepoGit config core.quotePath false
    Copy-Item -LiteralPath "$root/scripts/release/Export-PublicSnapshot.ps1" -Destination "$repo/scripts/release"
    $script:tool = "$repo/scripts/release/Export-PublicSnapshot.ps1"
    Write-RepoFile '.gitattributes' "* text=auto eol=lf`n*.bin binary`n*.pcap binary`n"
    Write-RepoFile 'README.md' "Synthetic Hero companion`n"
    Write-RepoFile 'src/main.rs' "fn main() {}`n"
    Write-RepoFile 'AGENTS.md' "Invented agent notes`n"
    Write-RepoFile 'internal/plan.md' "Invented plan`n"
    Write-RepoFile 'internal/deep/notes.md' "Invented notes`n"
    Write-RepoFile 'tests/canary.ps1' ("`$canary = '" + 'C:\' + 'Users\invented-canary\private' + "'`n")
    $bytes = [byte[]](0..255)
    $null = New-Item -ItemType Directory -Path "$repo/data", "$repo/tests/fixtures/scrubbed"
    [IO.File]::WriteAllBytes("$repo/data/blob.bin", $bytes)
    [IO.File]::WriteAllBytes("$repo/tests/fixtures/scrubbed/demo.pcap", [byte[]](0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0))
    $null = Invoke-RepoGit add -A
    $null = Invoke-RepoGit update-index --chmod=+x -- scripts/release/Export-PublicSnapshot.ps1
    $clean = Save-Commit 'Synthetic clean tree'
    $script:exclude = Write-List 'exclude.txt' @('# Invented review list', '', 'AGENTS.md', 'internal/')
    $captureBlob = Invoke-RepoGit rev-parse "${clean}:tests/fixtures/scrubbed/demo.pcap"
    $capture = "capture tests/fixtures/scrubbed/demo.pcap $captureBlob"
    $canary = 'user-path tests/canary.ps1 ' + (Invoke-RepoGit rev-parse "${clean}:tests/canary.ps1")
    $script:allow = Write-List 'allow.txt' @('# Invented canary and synthetic capture', $canary, $capture)

    # Uncommitted and untracked work must not reach the snapshot.
    Write-RepoFile 'README.md' "Uncommitted edit`n"
    Write-RepoFile 'notes.txt' "Untracked notes`n"
    $out = Invoke-Export @{}
    $files = @(Get-ChildItem -LiteralPath $out -Recurse -File -Force | ForEach-Object { $_.FullName.Substring($out.Length + 1).Replace('\', '/') } | Sort-Object)
    $want = @('.gitattributes', 'README.md', 'data/blob.bin', 'scripts/release/Export-PublicSnapshot.ps1', 'src/main.rs', 'tests/canary.ps1', 'tests/fixtures/scrubbed/demo.pcap')
    Assert ((@($files) -join '|') -ceq (@($want | Sort-Object) -join '|')) 'exact kept file set'
    Assert ([IO.File]::ReadAllText("$out/README.md") -ceq "Synthetic Hero companion`n") 'committed content, not working tree'
    Assert ((@([IO.File]::ReadAllBytes("$out/data/blob.bin")) -join ',') -ceq ($bytes -join ',')) 'binary bytes preserved'
    Assert (-not (Test-Path -LiteralPath "$out/internal")) 'directory entry excludes nested files'
    Assert (@(Get-ChildItem -LiteralPath $temp -Filter '*.partial-*').Count -eq 0) 'success leaves no staging'

    # The public clone is staged with source modes even when core.filemode is false.
    $public = Join-Path $temp 'public'
    $null = New-Item -ItemType Directory -Path $public
    $source = $repo; $repo = $public
    $null = Invoke-RepoGit init -q
    $null = Invoke-RepoGit config core.filemode false
    Write-RepoFile 'LICENSE' "Invented template license`n"
    $null = Save-Commit 'Invented initial commit'
    $repo = $source
    Reject @{ PublicClone = (Join-Path $temp 'absent-clone') } 'missing public clone rejected' 'root of a git checkout'
    Reject @{ PublicClone = (Join-Path $repo 'src') } 'public clone subdirectory rejected' 'root of a git checkout'
    Reject @{ PublicClone = $repo } 'source repository as public clone rejected' 'must not be the source'
    [IO.File]::WriteAllText("$public/untracked.txt", "invented`n")
    Reject @{ PublicClone = $public } 'dirty public clone rejected' 'uncommitted or untracked'
    Remove-Item -LiteralPath "$public/untracked.txt"
    $null = Invoke-Export @{ PublicClone = $public }
    $staged = @{}
    foreach ($line in @(& git -C $public ls-files -s)) { $meta, $path = $line.Split("`t", 2); $staged[$path] = $meta.Split(' ')[0..1] -join ' ' }
    Assert ((@($staged.Keys | Sort-Object) -join '|') -ceq (@($want | Sort-Object) -join '|')) 'public clone index holds exactly the kept files'
    Assert ($staged['scripts/release/Export-PublicSnapshot.ps1'] -ceq ('100755 ' + (Invoke-RepoGit rev-parse "${clean}:scripts/release/Export-PublicSnapshot.ps1"))) 'executable mode and blob staged'
    Assert ($staged['README.md'] -ceq ('100644 ' + (Invoke-RepoGit rev-parse "${clean}:README.md"))) 'regular mode and committed blob staged'
    Assert ($staged['data/blob.bin'] -ceq ('100644 ' + (Invoke-RepoGit rev-parse "${clean}:data/blob.bin"))) 'binary blob staged unchanged'
    Assert (-not (Test-Path -LiteralPath "$public/LICENSE")) 'previous public files removed'

    Reject @{ OutputDir = $out } 'existing output rejected' 'Output already exists'
    Reject @{ Commit = 'no-such-ref' } 'unknown commit rejected' 'Commit does not resolve'
    Reject @{ ExcludeList = (Join-Path $temp 'absent.txt') } 'missing exclude list rejected' 'Review list is missing'
    Reject @{ ExcludeList = (Write-List 'stale.txt' @('AGENTS.md', 'missing.md')) } 'stale exclude entry rejected' 'missing.md'
    Reject @{ ExcludeList = (Write-List 'dup.txt' @('AGENTS.md', 'AGENTS.md')) } 'duplicate exclude entry rejected' 'Duplicate exclude'
    foreach ($bad in @('../AGENTS.md', '/AGENTS.md', 'internal\plan.md', 'internal/./plan.md', 'internal/*', 'AGENTS.md extra')) {
        Reject @{ ExcludeList = (Write-List 'bad.txt' @($bad)) } "exclude entry '$bad' rejected" 'Invalid exclude entry'
    }
    Reject @{ AllowList = (Write-List 'empty.txt' @($capture)) } 'unallowed canary rejected' 'user-path tests/canary.ps1'
    Reject @{ AllowList = (Write-List 'no-capture.txt' @($canary)) } 'unapproved scrubbed capture rejected' 'capture tests/fixtures/scrubbed/demo.pcap'
    Reject @{ AllowList = (Write-List 'stale-allow.txt' @($canary, $capture, "github-token README.md $captureBlob")) } 'stale allow entry rejected' 'stale allow entry: github-token README.md'
    foreach ($bad in @("forbidden-file README.md $captureBlob", "deny-1 README.md $captureBlob", 'user-path', "user-path ../x $captureBlob", "unknown README.md $captureBlob", 'user-path tests/canary.ps1', 'capture tests/fixtures/scrubbed/demo.pcap', 'capture tests/fixtures/scrubbed/demo.pcap not-a-blob', "$capture extra")) {
        Reject @{ AllowList = (Write-List 'bad-allow.txt' @($bad)) } "allow entry '$bad' rejected" 'Invalid allow entry'
    }
    Reject @{ DenyPattern = @('synthetic\s+hero') } 'deny pattern rejected' 'deny-1 README.md'

    Remove-Item -LiteralPath "$repo/notes.txt"
    Write-RepoFile 'README.md' "Synthetic Hero companion`n"
    # Changing any allowed file, capture or canary, needs renewed review.
    [IO.File]::WriteAllBytes("$repo/tests/fixtures/scrubbed/demo.pcap", [byte[]](0xd4, 0xc3, 0xb2, 0xa1, 2, 0, 4, 0, 9))
    Write-RepoFile 'tests/canary.ps1' ([IO.File]::ReadAllText("$repo/tests/canary.ps1") + "`$other = 'invented'`n")
    $null = Save-Commit 'Synthetic changed allowed files'
    Reject @{} 'replaced capture rejected' "capture tests/fixtures/scrubbed/demo.pcap changed: blob"
    Assert $script:lastError.Contains('user-path tests/canary.ps1 changed: blob') 'changed canary file rejected'
    Assert (-not $script:lastError.Contains('stale allow entry')) 'changed allowed files are not reported as stale'
    $key = 'untrusted comment: rsign encrypted secret' + " key`nRWRTY0Iy"
    Write-RepoFile 'leak.md' (('-----BEGIN ' + 'RSA PRIVATE KEY-----') + "`n" + ('gh' + 'p_' + ('A' * 36)) + "`n" + $key + "`n" + ('/Us' + 'ers/carol/x') + "`n")
    Write-RepoFile 'encoded.txt' ([Convert]::ToBase64String([Text.Encoding]::ASCII.GetBytes($key)) + "`n")
    $null = Save-Commit 'Synthetic leaks'
    Reject @{} 'content leaks rejected'
    foreach ($finding in @('private-key leak.md', 'github-token leak.md', 'signing-key leak.md', 'user-path leak.md', 'encoded-signing-key encoded.txt')) {
        Assert $script:lastError.Contains($finding) "finding $finding"
    }

    Remove-Item -LiteralPath "$repo/leak.md", "$repo/encoded.txt"
    foreach ($path in @('capture.pcapng', 'docs/run.pcap', '.env', 'recordings/a.txt', 'store.sqlite3', 'live.sqlite3-wal', 'history.db-shm', 'store.sqlite3-journal', 'run.log')) { Write-RepoFile $path "invented`n" }
    # A renamed capture is found by its pcapng magic.
    [IO.File]::WriteAllBytes("$repo/data/trace.bin", [byte[]](0x0a, 0x0d, 0x0d, 0x0a, 0x1c, 0, 0, 0))
    $null = Save-Commit 'Synthetic forbidden files'
    Reject @{} 'forbidden files rejected'
    foreach ($path in @('capture.pcapng', '.env', 'recordings/a.txt', 'store.sqlite3', 'live.sqlite3-wal', 'history.db-shm', 'store.sqlite3-journal', 'run.log')) {
        Assert $script:lastError.Contains("forbidden-file $path") "forbidden file $path"
    }
    foreach ($path in @('docs/run.pcap', 'data/trace.bin')) { Assert $script:lastError.Contains("capture $path") "unlisted capture $path" }

    $null = Invoke-RepoGit rm -q -r -- capture.pcapng docs .env recordings store.sqlite3 live.sqlite3-wal history.db-shm store.sqlite3-journal run.log data/trace.bin
    Write-RepoFile ('caf' + [char]0xE9 + '.md') "invented`n"
    $null = Save-Commit 'Synthetic unusual name'
    Reject @{} 'unusual path rejected' 'Path needs review'

    $old = Invoke-Export @{ Commit = $clean }
    Assert (Test-Path -LiteralPath "$old/tests/canary.ps1") 'explicit earlier commit exported'
} finally {
    $resolved = [IO.Path]::GetFullPath($temp)
    if ($resolved -ne $temp -or -not $resolved.StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()), [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe fixture cleanup.' }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
Write-Output "Public snapshot: $script:checks checks passed."
