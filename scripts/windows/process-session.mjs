import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { setTimeout as sleep } from 'node:timers/promises';

const exec = promisify(execFile);
const brief = value => String(value).slice(0, 1000);
export const STOP_MODES = Object.freeze(['1', 'snapshot']);

export function stopGateValue(stop) {
    return stop === true ? '1' : stop === 'snapshot' ? 'snapshot' : '0';
}

// An inaccessible or reused PID is never evidence that we still own a process.
// This decision gates launch and CDP use; stop independently verifies on a held
// native handle before requesting any close or termination.
export function sessionState(state) {
    return state.alive === true && state.matches === true ? 'active' : 'stale';
}

export function stopResult(record, state) {
    return { stopped: state.matches === true, pid: record.pid,
        already_exited: state.matches !== true && state.alive === false,
        stale: state.matches !== true, reason: state.reason, close_mode: state.close_mode ?? null,
        descendants: state.descendants ?? 0, survivors: state.survivors ?? [], session: null };
}

export async function launchIdentity(pid, inspect = inspectProcess, pause = sleep) {
    // Each native inspection is bounded too; retry only the startup loader race.
    for (let attempt = 0; attempt < 5; attempt++) {
        const state = await inspect({ pid });
        if (state.reason !== 'partial_copy' || attempt === 4) return state;
        await pause(100);
    }
}

async function powershell(script, values = {}) {
    const wrapped = `$ErrorActionPreference = 'Stop'\ntry {\n${script}\n} catch { [Console]::Error.WriteLine($_.Exception.Message); exit 1 }`;
    try {
        const { stdout } = await exec('pwsh.exe', ['-NoProfile', '-NonInteractive', '-EncodedCommand',
            Buffer.from(wrapped, 'utf16le').toString('base64')], {
            windowsHide: true, timeout: 30000, env: { ...process.env, ...values },
        });
        return JSON.parse(stdout.trim());
    } catch (error) {
        // execFile's default error includes the whole encoded script. Keep the
        // actionable failure visible without dumping that implementation detail.
        throw Error(`Process inspection: ${brief(error.stderr?.trim() || error.code || error.message)}`);
    }
}

const descendantCleanup = `
$held = @()
try {
    foreach ($item in $descendants) {
        $child = $null
        try {
            $child = [Diagnostics.Process]::GetProcessById($item.pid)
            [void]$child.get_Handle()
            if ($child.get_HasExited()) { continue }
            $ticks = $child.get_StartTime().ToUniversalTime().Ticks
            # CIM CreationDate has microsecond precision (10 native ticks).
            if (($ticks - ($ticks % 10)) -ne $item.created) { continue }
            $held += $child; $child = $null
        } catch [ArgumentException] { continue }
          catch [InvalidOperationException] { continue }
          catch [ComponentModel.Win32Exception] { $survivors += $item.pid }
        finally { if ($child) { $child.Dispose() } }
    }
    foreach ($child in $held) {
        if (-not $child.get_HasExited()) {
            & "$env:SystemRoot/System32/taskkill.exe" /pid $child.Id /f 2>$null | Out-Null
        }
    }
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    foreach ($child in $held) {
        $wait = [Math]::Max(0, [int]($deadline - [DateTime]::UtcNow).TotalMilliseconds)
        if (-not $child.WaitForExit($wait)) { $survivors += $child.Id }
    }
} finally { foreach ($child in $held) { $child.Dispose() } }`;

// Keep the process handle open through identity verification and taskkill so
// Windows cannot recycle its PID between the check and termination.
export async function inspectProcess(record, stop = false) {
    return powershell(`
try { $owner = [Diagnostics.Process]::GetProcessById([int]$env:MAXED_REVIEW_PID) }
catch [ArgumentException] { '{"alive":false,"matches":false,"reason":"exited"}'; exit 0 }
try {
    try {
        # Invoke getters explicitly: PowerShell property access can swallow a
        # getter exception and yield null for protected SYSTEM processes.
        [void]$owner.get_Handle()
        if ($owner.get_HasExited()) { '{"alive":false,"matches":false,"reason":"exited"}'; exit 0 }
        $exe = $owner.get_MainModule().FileName
        $started = [string]$owner.get_StartTime().ToUniversalTime().Ticks
    } catch [ComponentModel.Win32Exception] {
        $code = $_.Exception.NativeErrorCode
        if ($code -notin @(5, 299)) { throw }
        @{ alive = $null; matches = $false; reason = $(if ($code -eq 5) { 'access_denied' } else { 'partial_copy' }) } | ConvertTo-Json -Compress
        exit 0
    } catch [InvalidOperationException] {
        '{"alive":false,"matches":false,"reason":"exited"}'; exit 0
    }
    $identityMatches = $exe -ieq $env:MAXED_REVIEW_EXE -and $started -ceq $env:MAXED_REVIEW_STARTED
    $closeMode = $null
    $descendants = @(); $survivors = @()
    if ($env:MAXED_REVIEW_STOP -cin ${STOP_MODES.map(mode => `'${mode}'`).join(',')} -and $identityMatches) {
        $rows = @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,CreationDate)
        $parents = @{ $owner.Id = [long]$started }
        do {
            $added = $false
            foreach ($row in $rows) {
                $id = [int]$row.ProcessId; $parentId = [int]$row.ParentProcessId
                if ($parents.ContainsKey($id) -or -not $parents.ContainsKey($parentId) -or -not $row.CreationDate) { continue }
                $created = $row.CreationDate.ToUniversalTime().Ticks
                if ($created -le [long]$started -or $created -le $parents[$parentId]) { continue }
                $parents[$id] = $created; $added = $true
                # Preserve native tick precision across the snapshot's JSON round trip.
                $descendants += @{ pid = $id; created = [string]$created }
            }
        } while ($added)
        # Snapshot limitation: processes born after enumeration, or behind an
        # already-exited parent, cannot be proven descendants and are not killed.
        if ($env:MAXED_REVIEW_STOP -eq 'snapshot') {
            @{ descendant_snapshot = @($descendants) } | ConvertTo-Json -Compress -Depth 4; exit 0
        }
        $closeMode = 'graceful'
        try { [void]$owner.CloseMainWindow() } catch { $closeMode = 'forced' }
        if (-not $owner.WaitForExit(2000)) {
            $closeMode = 'forced'
            & "$env:SystemRoot/System32/taskkill.exe" /pid $owner.Id /f | Out-Null
            if ($LASTEXITCODE -ne 0 -and -not $owner.get_HasExited()) { throw "taskkill exited $LASTEXITCODE; session retained" }
            if (-not $owner.WaitForExit(10000)) { throw 'App did not exit; session retained' }
        }
        ${descendantCleanup}
    }
    @{ alive = -not $owner.get_HasExited(); matches = $identityMatches; exe = $exe;
       started_utc_ticks = $started; close_mode = $closeMode; descendants = $descendants.Count; survivors = @($survivors);
       reason = $(if ($identityMatches) { 'matched' } else { 'identity_mismatch' }) } | ConvertTo-Json -Compress
} finally { $owner.Dispose() }`, {
        MAXED_REVIEW_PID: String(record.pid), MAXED_REVIEW_EXE: record.exe ?? '',
        MAXED_REVIEW_STARTED: record.started_utc_ticks ?? '', MAXED_REVIEW_STOP: stopGateValue(stop),
    });
}

export async function cleanupLaunch(child, record, inspect = inspectProcess) {
    let snapshot, note = 'WebView2 descendants may survive';
    try {
        if (record) snapshot = (await inspect(record, 'snapshot')).descendant_snapshot;
    } catch (error) { note += ` (snapshot failed: ${error.message})`; }
    // The held ChildProcess handle owns the app even when identity reads fail.
    // Without a verified snapshot, WebView2 descendants may survive: never use
    // a PID/name-based tree kill to guess their ownership.
    const killed = child.kill();
    const deadline = performance.now() + 10000;
    while (child.exitCode === null && child.signalCode === null && performance.now() < deadline) await sleep(25);
    if (child.exitCode === null && child.signalCode === null) throw Error('Spawned app did not exit after child.kill()');
    const stopped = `spawned app ${killed ? 'terminated' : 'already exited'}`;
    if (snapshot) {
        try {
            const survivors = snapshot.length ? await powershell(`
$descendants = @(ConvertFrom-Json $env:MAXED_REVIEW_DESCENDANTS)
$survivors = @()
${descendantCleanup}
ConvertTo-Json -InputObject @($survivors) -Compress`, {
                MAXED_REVIEW_DESCENDANTS: JSON.stringify(snapshot),
            }) : [];
            if (survivors.length) throw Error(`Descendant survivors: ${JSON.stringify(survivors)}`);
            note = `${snapshot.length} snapshotted descendants cleaned up`;
        } catch (error) { throw Error(`${stopped}; descendant cleanup failed: ${error.message}`); }
    }
    return `${stopped}; ${note}`;
}
