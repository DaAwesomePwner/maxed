import { mkdir, readFile, writeFile, unlink, open, realpath } from 'node:fs/promises';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { createServer } from 'node:net';
import { discover, drive } from './cdp.mjs';
import { inspectProcess, sessionState, stopResult, launchIdentity, cleanupLaunch } from './process-session.mjs';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const output = join(repo, 'target/ui-review');
const sessionPath = join(output, 'session.json');
const exec = promisify(execFile);
const usage = `Usage: node scripts/windows/ui-drive.mjs <command>
  launch <exe> [--port N]   Launch and wait up to 30s (default port 9238)
  shot <out.png>           Capture the app via CDP
  eval '<js>'              Evaluate JavaScript, awaiting promises; print CDP JSON
  click '<selector|text>'  Click a CSS match or exact visible navigation text
  console [--ms N]         Observe errors/exceptions (default 3000ms, max 60000)
  desktop-shot <out.png>   Capture the desktop (may contain private windows)
  stop                    Stop only the recorded process tree
  status                  Report the recorded process identity
Session: target/ui-review/session.json in this script's worktree.
Screenshots must stay under this worktree's target/ directory.`;

export function validateSession(record) {
    if (!record || !Number.isInteger(record.pid) || record.pid <= 0
        || typeof record.exe !== 'string' || !/^[a-z]:[\\/]/i.test(record.exe)
        || !/^\d+$/.test(record.started_utc_ticks) || BigInt(record.started_utc_ticks) <= 0n
        || !Number.isInteger(record.port) || record.port < 1024 || record.port > 65535) {
        throw Error('Invalid session record; refusing process operations');
    }
    return record;
}

async function readSession() {
    try { return validateSession(JSON.parse(await readFile(sessionPath, 'utf8'))); }
    catch (error) { if (error.code === 'ENOENT') return null; throw error; }
}

async function locked(action) {
    await mkdir(output, { recursive: true });
    const path = join(output, 'session.lock');
    let lock;
    try { lock = await open(path, 'wx'); }
    catch (error) {
        if (error.code === 'EEXIST') throw Error('Another driver command holds session.lock; after an interrupted command, verify it exited before removing the lock');
        throw error;
    }
    try { return await action(); }
    finally { await lock.close(); await unlink(path); }
}

export async function assertPortFree(port) {
    const server = createServer();
    await new Promise((resolveListen, reject) => {
        server.once('error', error => reject(Error(`CDP port ${port} is unavailable: ${error.message}`)));
        server.listen({ host: '127.0.0.1', port, exclusive: true }, resolveListen);
    });
    await new Promise((resolveClose, reject) => server.close(error => error ? reject(error) : resolveClose()));
}

async function stopSession(record) {
    const state = await inspectProcess(record, true);
    if (state.survivors?.length) throw Error(`Descendant survivors: ${JSON.stringify(state.survivors)}; session retained`);
    await unlink(sessionPath);
    return stopResult(record, state);
}

async function launch(executable, port) {
    const old = await readSession();
    if (old && sessionState(await inspectProcess(old)) === 'active') throw Error('Recorded app is still alive; stop the session before launching');
    if (old) await unlink(sessionPath);
    await assertPortFree(port);
    const exe = await realpath(resolve(executable));
    const child = spawn(exe, [], {
        detached: true, stdio: 'ignore', windowsHide: false, cwd: dirname(exe),
        env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:
            `--remote-debugging-port=${port} --disable-backgrounding-occluded-windows --disable-renderer-backgrounding` },
    });
    try {
        await new Promise((resolveSpawn, reject) => { child.once('spawn', resolveSpawn); child.once('error', reject); });
        return await completeLaunch(child, exe, port);
    } finally { child.unref(); }
}

export async function completeLaunch(child, exe, port, {
    identify = launchIdentity, find = discover, inspect = inspectProcess, cleanup = cleanupLaunch,
    save = record => writeFile(sessionPath, JSON.stringify(record, null, 2) + '\n', { flag: 'wx' }),
    remove = () => unlink(sessionPath).catch(e => { if (e.code !== 'ENOENT') throw e; }),
} = {}) {
    let record;
    let wrote = false;
    try {
        const identity = await identify(child.pid);
        if (!identity.alive || identity.exe?.toLowerCase() !== exe.toLowerCase()) {
            throw Error(`App identity unavailable (${identity.reason})`);
        }
        record = validateSession({ pid: child.pid, exe: identity.exe, port, started_utc_ticks: identity.started_utc_ticks });
        await save(record);
        wrote = true;
        const target = await find(port, performance.now() + 30000, 200);
        const state = await inspect(record);
        if (!state.alive || !state.matches) throw Error('App exited while waiting for CDP');
        return { ...record, url: target.url, session: sessionPath };
    } catch (error) {
        let cleaned;
        try {
            cleaned = await cleanup(child, record);
            if (wrote) await remove();
        } catch (failure) { throw Error(`${error.message}; ${cleaned ?? 'launch cleanup'}; cleanup failed: ${failure.message}`); }
        throw Error(`${error.message}; ${cleaned}`);
    }
}

async function screenshotPath(value) {
    const path = resolve(value);
    const target = await realpath(join(repo, 'target'));
    // Resolve the existing parent too, so a junction cannot redirect a capture
    // outside ignored output. Require the parent to exist rather than create it.
    const parent = await realpath(dirname(path));
    if (!parent.toLowerCase().startsWith(target.toLowerCase() + '\\') && parent.toLowerCase() !== target.toLowerCase()) {
        throw Error('Screenshot destination must be under this worktree\'s target/');
    }
    if (!path.toLowerCase().endsWith('.png')) throw Error('Screenshot destination must end in .png');
    // Exclusive creation also rejects existing symlinks and accidental overwrite.
    return path;
}

export function parseArgs(args) {
    const [command, value, option, extra, ...rest] = args;
    if (rest.length) throw Error(usage);
    if (command === 'launch' && value && (option === undefined || option === '--port' && extra !== undefined)) {
        const port = Number(extra ?? 9238);
        if (!Number.isInteger(port) || port < 1024 || port > 65535) throw Error('Port must be 1024..65535');
        return { command, value, port };
    }
    if (['shot', 'eval', 'click', 'desktop-shot'].includes(command) && value && option === undefined) return { command, value };
    if (command === 'console' && (value === undefined || value === '--ms' && option !== undefined) && extra === undefined) {
        const duration = Number(option ?? 3000);
        if (!Number.isInteger(duration) || duration < 1 || duration > 60000) throw Error('Console duration must be 1..60000 ms');
        return { command, duration };
    }
    if (['status', 'stop'].includes(command) && value === undefined) return { command };
    throw Error(usage);
}

async function main() {
    if (process.argv.length === 2 || ['--help', '-h'].includes(process.argv[2])) { console.log(usage); return; }
    const { command, value, port, duration } = parseArgs(process.argv.slice(2));
    if (process.platform !== 'win32') throw Error('UI driving requires the local interactive Windows session');
    const result = await locked(async () => {
        if (command === 'launch') return launch(value, port);
        if (command === 'desktop-shot') {
            const path = await screenshotPath(value);
            const { stdout } = await exec('pwsh.exe', ['-NoProfile', '-NonInteractive', '-File',
                join(repo, 'scripts/windows/desktop-shot.ps1'), '-OutputPath', path], { windowsHide: true, timeout: 20000 });
            return JSON.parse(stdout.trim());
        }
        const record = await readSession();
        if (command === 'status') {
            if (!record) return { session: null };
            const state = await inspectProcess(record);
            return { session: record, ...state, stale: sessionState(state) === 'stale' };
        }
        if (command === 'stop') return record ? stopSession(record) : { stopped: false, session: null };
        if (!record) throw Error('No recorded session; use launch first');
        if (sessionState(await inspectProcess(record)) !== 'active') throw Error('Recorded session is stale; run stop or launch');
        return drive(command, command === 'shot' ? await screenshotPath(value) : value, duration, record.port);
    });
    console.log(JSON.stringify(result));
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
    main().catch(error => { console.error(`UI drive: ${error.message}`); process.exitCode = 1; });
}
