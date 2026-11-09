import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { inspectProcess, sessionState, stopResult, launchIdentity, STOP_MODES, stopGateValue } from './process-session.mjs';

test('only explicit stop and snapshot modes can enter the native stop gate', () => {
    assert.deepEqual(STOP_MODES, ['1', 'snapshot']);
    assert.ok(Object.isFrozen(STOP_MODES));
    assert.equal(stopGateValue(true), '1');
    assert.equal(stopGateValue('snapshot'), 'snapshot');
    for (const value of [false, undefined, null, '', '0', '1', 1, 'unknown', 'SNAPSHOT', {}, []]) {
        assert.equal(stopGateValue(value), '0');
        assert.equal(STOP_MODES.includes(stopGateValue(value)), false);
    }
    assert.equal(STOP_MODES.includes('unknown'), false);
});

test('only a live matching identity blocks launch; reused, exited and denied PIDs are stale', () => {
    assert.equal(sessionState({ alive: true, matches: true }), 'active');
    for (const state of [
        { alive: true, matches: false, reason: 'identity_mismatch' },
        { alive: null, matches: false, reason: 'access_denied' },
        { alive: null, matches: false, reason: 'partial_copy' },
        { alive: false, matches: false, reason: 'exited' },
        { alive: false, matches: true },
    ]) assert.equal(sessionState(state), 'stale');
});

test('stop distinguishes stale live/unreadable identities from already exited processes', () => {
    for (const [alive, reason] of [[true, 'identity_mismatch'], [null, 'access_denied'], [null, 'partial_copy'], [false, 'exited']]) {
        const result = stopResult({ pid: 123 }, { alive, matches: false, reason });
        assert.equal(result.stale, true);
        assert.equal(result.reason, reason);
        assert.equal(result.stopped, false);
        assert.equal(result.already_exited, alive === false);
    }
    const stopped = stopResult({ pid: 123 }, { alive: false, matches: true, close_mode: 'graceful', descendants: 2, survivors: [] });
    assert.equal(stopped.stale, false);
    assert.equal(stopped.already_exited, false);
    assert.equal(stopped.stopped, true);
    assert.equal(stopped.descendants, 2);
    assert.deepEqual(stopped.survivors, []);
});

test('launch retries partial-copy identity reads with a finite bound, but not denied/exited reads', async () => {
    const transient = { alive: null, matches: false, reason: 'partial_copy' };
    const ready = { alive: true, exe: process.execPath, started_utc_ticks: '123' };
    let calls = 0, pauses = 0;
    const pause = async ms => { assert.equal(ms, 100); pauses++; };
    assert.equal(await launchIdentity(123, async () => ++calls < 3 ? transient : ready, pause), ready);
    assert.equal(calls, 3);
    assert.equal(pauses, 2);
    calls = 0;
    assert.equal(await launchIdentity(123, async () => { calls++; return transient; }, pause), transient);
    assert.equal(calls, 5);
    for (const reason of ['access_denied', 'exited']) {
        calls = 0;
        assert.equal((await launchIdentity(123, async () => { calls++; return { reason }; }, pause)).reason, reason);
        assert.equal(calls, 1);
    }
});

test('Windows inspection leaves a reused PID alive and handles protected process getters', {
    skip: process.platform !== 'win32',
}, async () => {
    const reused = await inspectProcess({ pid: process.pid, exe: process.execPath, started_utc_ticks: '1' }, true);
    assert.equal(reused.alive, true);
    assert.equal(reused.matches, false);
    assert.equal(reused.close_mode, null);
    // SYSTEM may be readable when elevated; either outcome must remain stale.
    const protectedState = await inspectProcess({ pid: 4, exe: process.execPath, started_utc_ticks: '1' });
    assert.equal(sessionState(protectedState), 'stale');
});

test('Windows stop forces an owned process and its recorded descendant without a closable window', {
    skip: process.platform !== 'win32',
}, async () => {
    const child = spawn(process.execPath, ['-e', `
        const child = require('node:child_process').spawn(process.execPath, ['-e', 'setTimeout(() => {}, 60000)'],
            { windowsHide: true, stdio: 'ignore' });
        child.once('spawn', () => process.send(child.pid));
        setInterval(() => {}, 1000);
    `], {
        windowsHide: true, stdio: ['ignore', 'ignore', 'ignore', 'ipc'],
    });
    let descendant;
    try {
        const [pid] = await once(child, 'message');
        const grandchild = await launchIdentity(pid);
        descendant = { pid, exe: grandchild.exe, started_utc_ticks: grandchild.started_utc_ticks };
        const identity = await inspectProcess({ pid: child.pid });
        assert.equal(identity.alive, true);
        const stopped = await inspectProcess({ pid: child.pid, exe: identity.exe,
            started_utc_ticks: identity.started_utc_ticks }, true);
        assert.equal(stopped.alive, false);
        assert.equal(stopped.matches, true);
        assert.equal(stopped.close_mode, 'forced');
        assert.ok(stopped.descendants >= 1); // Windows may also create console hosts.
        assert.deepEqual(stopped.survivors, []);
        assert.equal((await inspectProcess(descendant)).alive, false);
    } finally {
        if (child.exitCode === null) child.kill();
        if (descendant) await inspectProcess(descendant, true);
    }
});
