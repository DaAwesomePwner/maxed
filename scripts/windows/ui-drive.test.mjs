import test from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { runInNewContext } from 'node:vm';
import { mkdtemp, readFile, writeFile, unlink, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { parseArgs, validateSession, assertPortFree, completeLaunch } from './ui-drive.mjs';
import { discover, clickExpression } from './cdp.mjs';
import { cleanupLaunch } from './process-session.mjs';

test('every post-spawn launch failure kills the held child, even when its snapshot fails', async () => {
    for (const stage of ['partial_copy', 'access_denied', 'exited', 'identity timeout', 'save', 'discovery', 'verify', 'snapshot']) {
        let killed = 0, removed = 0;
        const child = { pid: 123, exitCode: null, signalCode: null,
            kill() { killed++; this.signalCode = 'SIGTERM'; return true; } };
        const ready = { alive: true, exe: 'C:\\review\\maxed.exe', started_utc_ticks: '123456789012345678' };
        const hasRecord = ['save', 'discovery', 'verify', 'snapshot'].includes(stage);
        const fail = () => { throw Error(stage); };
        await assert.rejects(completeLaunch(child, ready.exe, 9238, {
            identify: async () => stage === 'identity timeout' ? fail() : hasRecord ? ready : { reason: stage },
            save: async () => { if (stage === 'save') fail(); },
            find: async () => ['discovery', 'snapshot'].includes(stage) ? fail() : { url: 'http://tauri.localhost' },
            inspect: async () => fail(),
            cleanup: async (spawned, record) => {
                assert.equal(spawned, child);
                assert.equal(Boolean(record), hasRecord);
                return cleanupLaunch(spawned, record, async (_record, mode) => {
                    assert.equal(mode, 'snapshot');
                    assert.equal(killed, 0);
                    return stage === 'snapshot' ? fail() : { descendant_snapshot: [] };
                });
            },
            remove: async () => { removed++; },
        }), /spawned app terminated/);
        assert.equal(killed, 1, stage);
        assert.equal(removed, hasRecord && stage !== 'save' ? 1 : 0, stage);
    }
});

test('a failed exclusive save leaves a foreign session file untouched', async () => {
    const directory = await mkdtemp(join(tmpdir(), 'maxed-launch-'));
    const path = join(directory, 'session.json');
    const foreign = JSON.stringify({ pid: 456, exe: 'C:\\review\\foreign.exe',
        port: 9238, started_utc_ticks: '123456789012345679' }) + '\n';
    const ready = { alive: true, exe: 'C:\\review\\maxed.exe', started_utc_ticks: '123456789012345678' };
    try {
        await writeFile(path, foreign, { flag: 'wx' });
        for (const code of ['EEXIST', 'EACCES']) {
            let cleaned = false, removed = false;
            await assert.rejects(completeLaunch({ pid: 123 }, ready.exe, 9238, {
                identify: async () => ready,
                save: async record => {
                    if (code === 'EACCES') throw Object.assign(Error(code), { code });
                    await writeFile(path, JSON.stringify(record), { flag: 'wx' });
                },
                find: async () => assert.fail('must not discover after a save failure'),
                cleanup: async (child, record) => {
                    assert.equal(child.pid, 123);
                    assert.equal(record.started_utc_ticks, ready.started_utc_ticks);
                    cleaned = true;
                    return 'spawned app terminated';
                },
                remove: async () => { removed = true; await unlink(path); },
            }), new RegExp(code));
            assert.equal(cleaned, true);
            assert.equal(removed, false);
            assert.equal(await readFile(path, 'utf8'), foreign);
        }
    } finally { await rm(directory, { recursive: true, force: true }); }
});

test('CLI rejects missing operands, extra flags, invalid ports and unbounded observation', () => {
    assert.deepEqual(parseArgs(['launch', 'maxed.exe']), { command: 'launch', value: 'maxed.exe', port: 9238 });
    assert.deepEqual(parseArgs(['console', '--ms', '20']), { command: 'console', duration: 20 });
    for (const args of [['shot'], ['launch', 'x', '--port'], ['launch', 'x', '--port', '0'],
        ['launch', 'x', '--port', '65536'], ['launch', 'x', '--host', 'example.com'],
        ['console', '--ms', '-1'], ['console', '--ms', '60001'], ['stop', 'extra']]) {
        assert.throws(() => parseArgs(args));
    }
});

test('process records require a complete identity before any PID operation', () => {
    const record = { pid: 123, exe: 'C:\\review\\maxed.exe', port: 9238, started_utc_ticks: '123456789012345678' };
    assert.equal(validateSession(record), record);
    for (const field of Object.keys(record)) {
        const incomplete = { ...record };
        delete incomplete[field];
        assert.throws(() => validateSession(incomplete));
    }
    for (const patch of [{ pid: -1 }, { exe: 'maxed.exe' }, { port: 80 }, { started_utc_ticks: '0' }]) {
        assert.throws(() => validateSession({ ...record, ...patch }));
    }
});

test('discovery rejects foreign endpoints, redirects, ambiguous and malformed pages', async () => {
    let payload, redirect = false;
    const server = createServer((_req, res) => {
        if (redirect) { res.writeHead(302, { location: 'http://example.com/json' }); res.end(); return; }
        res.setHeader('content-type', 'application/json');
        res.end(JSON.stringify(payload));
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const port = server.address().port;
    const page = { type: 'page', url: 'http://tauri.localhost/#/runs', webSocketDebuggerUrl: `ws://127.0.0.1:${port}/devtools/page/main` };
    const find = () => discover(port, performance.now() + 150, 5);
    try {
        payload = [page];
        assert.deepEqual(await find(), page);
        await assert.rejects(assertPortFree(port), /unavailable/);
        for (const url of [`ws://localhost:${port}/devtools/page/main`, `ws://example.com:${port}/devtools/page/main`,
            `ws://127.0.0.1:${port + 1}/devtools/page/main`, `wss://127.0.0.1:${port}/devtools/page/main`,
            `ws://user@127.0.0.1:${port}/devtools/page/main`, `ws://127.0.0.1:${port}/devtools/browser/main`]) {
            payload = [{ ...page, webSocketDebuggerUrl: url }];
            await assert.rejects(find(), /unsafe/);
        }
        payload = [page, page];
        await assert.rejects(find(), /ambiguous/);
        payload = [null];
        await assert.rejects(find(), /malformed/);
        payload = [{ ...page, url: 'http://tauri.localhost.evil/' }];
        await assert.rejects(find(), /timeout/);
        redirect = true;
        await assert.rejects(find(), /timeout/);
    } finally { await new Promise(resolve => server.close(resolve)); }
    await assertPortFree(port);
});

test('click supports visible nav text, fails on ambiguity and safely quotes selectors', () => {
    let clicks = 0;
    const element = { getClientRects: () => [{}], getAttribute: () => null,
        innerText: ' Runs ', click: () => { clicks++; } };
    let elements = [element];
    const context = { document: { querySelectorAll: selector => selector.startsWith('nav ') ? elements : [] },
        getComputedStyle: () => ({ visibility: 'visible' }), location: { hash: '#/runs' } };
    const run = text => runInNewContext(clickExpression(text), context);
    assert.equal(run('Runs').hash, '#/runs');
    assert.equal(clicks, 1);
    elements = [element, element];
    assert.throws(() => run('Runs'), /found 2/);
    elements = [{ ...element, disabled: true }];
    assert.throws(() => run('Runs'), /found 0/);
    assert.throws(() => run('\"); throw Error("injected"); //'), /found 0/);
});
