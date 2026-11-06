import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createServer } from 'node:http';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runInNewContext } from 'node:vm';
import { runSmoke, inspectUi } from './cdp-smoke.mjs';

const png = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aL1kAAAAASUVORK5CYII=';

function diagnosticContext() {
    const rect = { x: 1, y: 2, top: 2, right: 1101, bottom: 762, left: 1, width: 1100, height: 760 };
    return {
        location: { href: 'http://tauri.localhost/' + 'x'.repeat(2000) },
        window: { innerWidth: 1100, innerHeight: 760 },
        document: { readyState: 'complete', visibilityState: 'hidden', hidden: true, hasFocus: () => false,
            body: { children: [1, 2], innerText: 'Setup ' + 'x'.repeat(1000) },
            querySelector: selector => selector === '#app #main-content' ? null : { getBoundingClientRect: () => rect },
        },
    };
}

async function fixture(t, options = {}) {
    const outputDir = await mkdtemp(join(tmpdir(), 'maxed-cdp-test-'));
    let requests = 0;
    const server = createServer((req, res) => {
        assert.equal(req.url, '/json');
        requests++;
        res.setHeader('Content-Type', 'application/json');
        const page = { id: 'main', type: 'page', title: 'Maxed', url: 'http://tauri.localhost/',
            webSocketDebuggerUrl: `ws://127.0.0.1:${server.address().port}/devtools/page/main` };
        const decoys = [ { ...page, id: 'worker', type: 'service_worker' },
            { ...page, id: 'tools', url: 'devtools://devtools/bundled/inspector.html' } ];
        res.end(options.discovery?.(requests, page, decoys) ?? JSON.stringify([...decoys, page]));
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    const calls = [];
    let closed = false;
    class Socket extends EventTarget {
        constructor(url) {
            super();
            assert.match(url, /\/devtools\/page\/main$/);
            queueMicrotask(() => this.dispatchEvent(new Event('open')));
        }
        emit(message) {
            this.dispatchEvent(new MessageEvent('message', { data: typeof message === 'string' ? message : JSON.stringify(message) }));
        }
        send(text) {
            const { id, method, params } = JSON.parse(text);
            calls.push({ method, params });
            queueMicrotask(() => {
                if (options.respond?.(this, { id, method, params }) === false) return;
                let result = {};
                if (method === 'Runtime.evaluate') result = { result: { type: 'object', value:
                    params.expression.includes('inspectDiagnostics')
                        ? runInNewContext(params.expression, options.diagnosticContext ?? diagnosticContext())
                        : { instrumented: true, mounted: options.mounted !== false, errors: options.earlyErrors ?? [] },
                } };
                if (method === 'Page.navigate') {
                    result = { frameId: 'main', loaderId: 'app-load' };
                    this.emit({ method: 'Page.frameNavigated', params: { frame: { id: 'main', loaderId: 'app-load', url: params.url } } });
                }
                if (method === 'Page.captureScreenshot') result = { data: options.png ?? png };
                this.emit({ id, result });
            });
        }
        close() { closed = true; }
    }
    t.after(async () => {
        server.closeAllConnections();
        await new Promise(resolve => server.close(resolve));
        await rm(outputDir, { recursive: true, force: true });
    });
    return { outputDir, calls, requests: () => requests, closed: () => closed,
        run: (overrides = {}) => runSmoke({ port: server.address().port, outputDir,
            timeoutMs: 600, observationMs: 40, pollMs: 5, WebSocketImpl: Socket, ...overrides }) };
}

test('discovery retries and ignores workers/devtools; navigates to instrumented UI and saves PNG/report', async t => {
    const f = await fixture(t, { discovery: (n, page, decoys) => JSON.stringify(n < 3 ? decoys : [...decoys, page]) });
    assert.equal((await f.run()).ok, true);
    assert.equal(f.requests(), 3);
    assert.deepEqual(await readFile(join(f.outputDir, 'screenshot.png')), Buffer.from(png, 'base64'));
    assert.equal(JSON.parse(await readFile(join(f.outputDir, 'report.json'))).ok, true);
    const methods = f.calls.map(call => call.method);
    for (const method of ['Runtime.enable', 'Log.enable', 'Network.enable', 'Page.enable', 'Page.addScriptToEvaluateOnNewDocument']) {
        assert(methods.indexOf(method) < methods.indexOf('Page.navigate'));
        assert(methods.includes(method));
    }
    for (const method of ['Page.bringToFront', 'Emulation.setFocusEmulationEnabled']) {
        assert(methods.indexOf(method) > methods.indexOf('Page.navigate'));
        assert(methods.indexOf(method) < methods.indexOf('Runtime.evaluate'));
    }
    assert.equal(f.calls.find(call => call.method === 'Emulation.setFocusEmulationEnabled').params.enabled, true);
    assert(!methods.includes('Page.reload'));
    assert.deepEqual(f.calls.find(call => call.method === 'Page.navigate').params, { url: 'http://tauri.localhost/' });
    assert(f.closed());
});

test('pending app target with committed about:blank loads via navigate', async t => {
    let href = 'about:blank';
    const f = await fixture(t, { respond: (socket, { id, method, params }) => {
        if (method === 'Page.reload') return; // Reload would leave the committed blank document in place.
        if (method === 'Page.navigate') href = params.url;
        if (method === 'Runtime.evaluate' && !params.expression.includes('inspectDiagnostics')) {
            socket.emit({ id, result: { result: { value: { instrumented: href !== 'about:blank',
                mounted: href === 'http://tauri.localhost/', errors: [] } } } });
            return false;
        }
    } });
    const report = await f.run();
    assert.equal(report.ok, true, report.reason);
    assert(!f.calls.some(call => call.method === 'Page.reload'));
});

for (const origin of ['http://tauri.localhost', 'https://tauri.localhost']) {
    test(`navigation normalizes the validated ${origin} target to its root`, async t => {
        const f = await fixture(t, { discovery: (_n, page) => JSON.stringify([{ ...page,
            url: `${origin}/index.html?ignored=1#ignored` }]) });
        assert.equal((await f.run()).ok, true);
        assert.deepEqual(f.calls.find(call => call.method === 'Page.navigate').params, { url: `${origin}/` });
    });
}

test('navigate errorText fails with the original error and retains failure artifacts', async t => {
    const f = await fixture(t, { respond: (socket, { id, method }) => {
        if (method === 'Page.navigate') {
            socket.emit({ id, result: { frameId: 'main', loaderId: 'app-load', errorText: 'net::ERR_ABORTED' } });
            return false;
        }
    } });
    const report = await f.run();
    assert.equal(report.ok, false);
    assert.match(report.reason, /Page.navigate.*net::ERR_ABORTED/);
    assert.equal(report.screenshot, 'screenshot.png');
    assert(report.diagnostics.page);
});

for (const url of ['https://foreign.invalid/', 'about:blank', 'https://tauri.localhost/']) {
    test(`main-frame commit to ${url} fails even with a different loader`, async t => {
        const f = await fixture(t, { respond: (socket, { id, method }) => {
            if (method === 'Page.navigate') {
                socket.emit({ id, result: { frameId: 'main', loaderId: 'app-load' } });
                socket.emit({ method: 'Page.frameNavigated', params: { frame: { id: 'main', loaderId: 'other-load', url } } });
                return false;
            }
        } });
        const report = await f.run();
        assert.equal(report.ok, false);
        assert.match(report.reason, /Unexpected main-frame navigation/);
        assert(report.reason.includes(url), report.reason);
    });
}

test('root polling waits for the matching main-frame loader, not subframes or stale commits', async t => {
    let committed = false;
    const f = await fixture(t, { respond: (socket, { id, method, params }) => {
        if (method === 'Page.navigate') {
            socket.emit({ id, result: { frameId: 'main', loaderId: 'app-load' } });
            for (const frame of [
                { id: 'child', parentId: 'main', loaderId: 'app-load', url: 'https://foreign.invalid/' },
                { id: 'main', loaderId: 'initial-load', url: params.url },
            ]) socket.emit({ method: 'Page.frameNavigated', params: { frame } });
            setTimeout(() => {
                committed = true;
                socket.emit({ method: 'Page.frameNavigated', params: { frame: { id: 'main', loaderId: 'app-load', url: params.url } } });
            }, 25);
            return false;
        }
        if (method === 'Runtime.evaluate' && !params.expression.includes('inspectDiagnostics') && !committed) {
            socket.emit({ id, error: { code: -32000, message: 'Root polled before navigation committed' } });
            return false;
        }
    } });
    const report = await f.run();
    assert.equal(report.ok, true, report.reason);
    assert.equal(committed, true);
});

test('missing matching navigation commit is bounded by the run deadline', async t => {
    const f = await fixture(t, { respond: (socket, { id, method, params }) => {
        if (method === 'Page.navigate') {
            socket.emit({ id, result: { frameId: 'main', loaderId: 'app-load' } });
            socket.emit({ method: 'Page.frameNavigated', params: { frame: { id: 'main', loaderId: 'initial-load', url: params.url } } });
            return false;
        }
    } });
    const report = await f.run({ timeoutMs: 90 });
    assert.equal(report.ok, false);
    assert.match(report.reason, /navigation timeout/i);
    assert(!f.calls.some(call => call.method === 'Runtime.evaluate' && !call.params.expression.includes('inspectDiagnostics')));
    assert.equal(report.screenshot, 'screenshot.png');
});

for (const type of ['error', 'unhandledrejection']) {
    test(`early ${type} from the first app script after navigate is captured`, async t => {
        let source;
        let context;
        const listeners = new Map();
        const f = await fixture(t, { respond: (socket, { id, method, params }) => {
            if (method === 'Page.addScriptToEvaluateOnNewDocument') source = params.source;
            if (method === 'Page.navigate') {
                context = { window: {}, addEventListener: (name, handler) => listeners.set(name, handler) };
                runInNewContext(source, context);
                // Simulate the app's first script before either the response or commit arrives.
                try { runInNewContext('throw Error("first app script failed")', context); }
                catch (error) { listeners.get(type)({ message: error.message }); }
            }
            if (method === 'Runtime.evaluate' && !params.expression.includes('inspectDiagnostics')) {
                socket.emit({ id, result: { result: { value: { instrumented: !!context,
                    mounted: true, errors: context?.window.__maxedGuiSmokeErrors ?? [] } } } });
                return false;
            }
        } });
        const report = await f.run();
        assert.equal(report.ok, false);
        assert.match(report.reason, /UI error/);
        assert.deepEqual(report.errors, [{ kind: 'new-document',
            text: type === 'error' ? 'first app script failed' : 'unhandled promise rejection' }]);
    });
}

test('empty mount timeout saves bounded page, console, network and log diagnostics', async t => {
    const f = await fixture(t, { mounted: false, respond: (socket, { method }) => {
        if (method !== 'Page.navigate') return;
        for (let i = 0; i < 25; i++) {
            socket.emit({ method: 'Runtime.consoleAPICalled', params: { type: ['debug', 'log', 'info', 'warning'][i % 4],
                timestamp: i, args: [{ value: `${i}:` + 'x'.repeat(2000) }] } });
            socket.emit({ method: 'Network.loadingFailed', params: { requestId: `${i}`, type: 'Script',
                timestamp: i, errorText: 'net::ERR_FAILED ' + 'x'.repeat(2000), canceled: false, blockedReason: 'other' } });
            socket.emit({ method: 'Log.entryAdded', params: { entry: { source: 'network', level: 'warning',
                timestamp: i, text: `${i}:` + 'x'.repeat(2000), url: 'x'.repeat(2000) } } });
        }
    } });
    const report = await f.run({ timeoutMs: 90 });
    assert.equal(report.ok, false);
    assert.match(report.reason, /root.*timeout/i);
    const saved = JSON.parse(await readFile(join(f.outputDir, 'report.json')));
    assert.deepEqual(saved, report);
    const d = saved.diagnostics;
    assert.equal(d.page.location.href.length, 1000);
    assert.deepEqual(d.page.document, { readyState: 'complete', visibilityState: 'hidden', hidden: true, hasFocus: false });
    assert.deepEqual(d.page.window, { innerWidth: 1100, innerHeight: 760 });
    assert.equal(d.page.body.childElementCount, 2);
    assert.equal(d.page.body.innerText.length, 300);
    assert(d.page.body.innerText.startsWith('Setup '));
    assert.equal(d.page.elements.app_frame.exists, true);
    assert.deepEqual(d.page.elements.app_frame.rect, { x: 1, y: 2, top: 2, right: 1101, bottom: 762, left: 1, width: 1100, height: 760 });
    assert.deepEqual(d.page.elements.main_content, { exists: false, rect: null });
    assert.equal(d.page.elements.setup_list.exists, true);
    for (const entries of [d.console_messages, d.network_loading_failed, d.log_entries]) {
        assert.equal(entries.length, 20);
        assert.equal(entries[0].timestamp, 5);
        assert.equal(entries.at(-1).timestamp, 24);
        for (const entry of entries) for (const value of Object.values(entry)) {
            if (typeof value === 'string') assert(value.length <= 1000);
        }
    }
    assert.equal(d.console_messages[0].level, 'log');
    assert.equal(d.console_messages[0].text.length, 1000);
    assert.equal(d.network_loading_failed[0].requestId, '5');
    assert.equal(d.log_entries[0].source, 'network');
    assert.equal(saved.errors.length, 0, 'Diagnostic warnings/network failures changed error policy');
    assert.equal(saved.screenshot, 'screenshot.png');
});

test('unavailable document body and elements remain explicit in timeout diagnostics', async t => {
    const context = diagnosticContext();
    context.document.body = null;
    context.document.readyState = 'loading';
    context.document.querySelector = () => null;
    const f = await fixture(t, { mounted: false, diagnosticContext: context });
    const { diagnostics } = await f.run({ timeoutMs: 90 });
    assert.equal(diagnostics.page.document.readyState, 'loading');
    assert.deepEqual(diagnostics.page.body, { childElementCount: 0, innerText: '' });
    for (const element of Object.values(diagnostics.page.elements)) assert.deepEqual(element, { exists: false, rect: null });
});

test('unsupported foreground/focus hints do not prevent the rendered-root check', async t => {
    const f = await fixture(t, { mounted: false, respond: (socket, { id, method }) => {
        if (['Page.bringToFront', 'Emulation.setFocusEmulationEnabled'].includes(method)) {
            socket.emit({ id, error: { code: -32601, message: 'Unsupported method' } });
            return false;
        }
    } });
    const report = await f.run({ timeoutMs: 90 });
    assert.match(report.reason, /Rendered root timeout/);
    assert.equal(report.diagnostics.page.document.readyState, 'complete');
    assert.deepEqual(report.diagnostics.focus_errors.map(error => error.method), ['Page.bringToFront', 'Emulation.setFocusEmulationEnabled']);
    assert(report.diagnostics.focus_errors.every(error => /Unsupported method/.test(error.text)));
});

test('CDP failure after attach preserves the reason even when diagnostics and screenshot cannot respond', async t => {
    const f = await fixture(t, { respond: (socket, { method }) => {
        if (method === 'Page.navigate') {
            socket.emit({ method: 'Runtime.consoleAPICalled', params: { type: 'debug', args: [{ value: 'before disconnect' }] } });
            socket.dispatchEvent(new Event('close'));
            return false;
        }
    } });
    const report = await f.run();
    assert.match(report.reason, /WebSocket closed/);
    assert.equal(report.diagnostics.page, null);
    assert.match(report.diagnostics.page_error, /WebSocket closed/);
    assert.equal(report.diagnostics.console_messages[0].text, 'before disconnect');
    assert.match(report.screenshot_error, /WebSocket closed/);
});

test('diagnostics have a fresh deadline after an unanswered CDP call exhausts the run deadline', async t => {
    const f = await fixture(t, { respond: (_socket, { method, params }) => {
        if (method === 'Runtime.evaluate' && !params.expression.includes('inspectDiagnostics')) return false;
    } });
    const report = await f.run({ timeoutMs: 90 });
    assert.match(report.reason, /timeout.*Runtime.evaluate/);
    assert.equal(report.diagnostics.page.document.readyState, 'complete');
});

test('unanswered diagnostic evaluation is bounded and does not mask the root timeout or skip the screenshot', async t => {
    const f = await fixture(t, { mounted: false, respond: (_socket, { method, params }) => {
        if (method === 'Runtime.evaluate' && params.expression.includes('inspectDiagnostics')) return false;
    } });
    const report = await f.run({ timeoutMs: 90 });
    assert.match(report.reason, /Rendered root timeout/);
    assert.equal(report.diagnostics.page, null);
    assert.match(report.diagnostics.page_error, /CDP timeout: Runtime.evaluate/);
    assert.equal(report.screenshot, 'screenshot.png');
});

for (const [name, event] of [
    ['uncaught exception', { method: 'Runtime.exceptionThrown', params: { exceptionDetails: { text: 'boom' } } }],
    ['console error', { method: 'Runtime.consoleAPICalled', params: { type: 'error', args: [{ type: 'string', value: 'boom' }] } }],
    ['browser log error', { method: 'Log.entryAdded', params: { entry: { level: 'error', text: 'boom' } } }],
]) {
    test(`${name} during observation fails`, async t => {
        let sent = false;
        const f = await fixture(t, { respond: (socket, { method }) => {
            if (method === 'Runtime.evaluate' && !sent) {
                sent = true;
                setTimeout(() => socket.emit(event), 15);
            }
        } });
        const report = await f.run();
        assert.equal(report.ok, false);
        assert.match(report.reason, /UI error/);
        assert(report.errors.length > 0);
        assert.equal(report.diagnostics.page.document.readyState, 'complete');
        if (event.method === 'Runtime.consoleAPICalled') assert.equal(report.diagnostics.console_messages[0].level, 'error');
    });
}

test('console warnings do not fail the error policy', async t => {
    const f = await fixture(t, { respond: (socket, { method }) => {
        if (method === 'Page.navigate') socket.emit({ method: 'Runtime.consoleAPICalled', params: { type: 'warning', args: [] } });
    } });
    assert.equal((await f.run()).ok, true);
});

test('pre-attach exception replay and early new-document rejection both fail', async t => {
    const f = await fixture(t, { respond: (socket, { method }) => {
        if (method === 'Runtime.enable') socket.emit({ method: 'Runtime.exceptionThrown', params: { exceptionDetails: { text: 'early' } } });
    } });
    assert.equal((await f.run()).ok, false);
    const g = await fixture(t, { earlyErrors: ['unhandledrejection'] });
    assert.equal((await g.run()).ok, false);
});

test('missing main page has a bounded discovery timeout', async t => {
    const f = await fixture(t, { discovery: () => '[]' });
    assert.match((await f.run({ timeoutMs: 60 })).reason, /target.*timeout/i);
});

for (const body of ['not json', '{}', '[null]']) {
    test(`malformed discovery fails clearly: ${body}`, async t => {
        const f = await fixture(t, { discovery: () => body });
        assert.match((await f.run({ timeoutMs: 80 })).reason, /malformed/i);
    });
}

test('malformed CDP message fails rather than hanging', async t => {
    const f = await fixture(t, { respond: (socket, { method }) => {
        if (method === 'Runtime.enable') { socket.emit('{'); return false; }
    } });
    assert.match((await f.run()).reason, /malformed CDP/i);
});

test('unanswered CDP call is deadline bounded', async t => {
    const f = await fixture(t, { respond: (_socket, { method }) => method !== 'Page.enable' });
    assert.match((await f.run({ timeoutMs: 80 })).reason, /timeout.*Page.enable/i);
});

test('invalid screenshot is not accepted as success', async t => {
    const f = await fixture(t, { png: 'garbage' });
    assert.match((await f.run()).reason, /PNG/i);
});

test('context destruction after navigation is retried before mounting', async t => {
    let first = true;
    const f = await fixture(t, { respond: (socket, { id, method }) => {
        if (method === 'Runtime.evaluate' && first) {
            first = false;
            socket.emit({ id, error: { code: -32000, message: 'Execution context was destroyed.' } });
            return false;
        }
    } });
    assert.equal((await f.run()).ok, true);
});

test('CDP method errors and malformed evaluation results fail', async t => {
    const f = await fixture(t, { respond: (socket, { id, method }) => {
        if (method === 'Runtime.evaluate') { socket.emit({ id, result: { result: { type: 'undefined' } } }); return false; }
    } });
    assert.match((await f.run()).reason, /Malformed UI evaluation/);
    const g = await fixture(t, { respond: (socket, { id, method }) => {
        if (method === 'Log.enable') { socket.emit({ id, error: { code: -32601, message: 'Unsupported method' } }); return false; }
    } });
    assert.match((await g.run()).reason, /Log.enable.*Unsupported/);
});

test('an empty #app and hidden content cannot satisfy the real rendered-root predicate', () => {
    const elements = new Map();
    const context = { window: { __maxedGuiSmokeErrors: [] }, document: { querySelector: selector => elements.get(selector) } };
    const inspect = () => runInNewContext(`(${inspectUi.toString()})()`, context);
    elements.set('#app', {});
    assert.equal(inspect().mounted, false);
    elements.set('#app .app-frame', { getBoundingClientRect: () => ({ width: 100 }) });
    elements.set('#app #main-content', {});
    elements.set('#app .setup-page ol[aria-label="Setup"]', { children: [1, 2, 3], getBoundingClientRect: () => ({ height: 0 }) });
    assert.equal(inspect().mounted, false);
    elements.get('#app .setup-page ol[aria-label="Setup"]').getBoundingClientRect = () => ({ height: 100 });
    assert.equal(inspect().mounted, true);
});

test('the default observation really lasts ten seconds', async t => {
    const f = await fixture(t);
    const report = await f.run({ observationMs: undefined, timeoutMs: 12000, pollMs: 100 });
    assert.equal(report.ok, true);
    assert.equal(report.observation_ms, 10000);
    assert(report.elapsed_ms >= 10000);
});
