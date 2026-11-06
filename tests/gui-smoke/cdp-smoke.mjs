import { mkdir, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { setTimeout as sleep } from 'node:timers/promises';

const now = () => performance.now();
const remaining = deadline => Math.max(1, Math.ceil(deadline - now()));
const brief = value => String(value).slice(0, 1000);

function mainPage(target) {
    if (target.type !== 'page') return false;
    try {
        const url = new URL(target.url);
        return ['http://tauri.localhost', 'https://tauri.localhost'].includes(url.origin)
            && ['/', '/index.html'].includes(url.pathname);
    } catch { return false; }
}

async function discover(port, deadline, pollMs) {
    let last = 'main page not listed';
    while (now() < deadline) {
        try {
            const response = await fetch(`http://127.0.0.1:${port}/json`, {
                signal: AbortSignal.timeout(Math.min(1000, remaining(deadline))), redirect: 'error',
            });
            if (!response.ok) throw Error(`discovery HTTP ${response.status}`);
            let targets;
            try { targets = await response.json(); } catch { throw Error('malformed discovery JSON'); }
            if (!Array.isArray(targets) || targets.some(t => !t || typeof t !== 'object')) {
                throw Error('malformed discovery target list');
            }
            const pages = targets.filter(mainPage);
            if (pages.length > 1) throw Error('ambiguous main page targets');
            if (pages.length === 1) {
                const target = pages[0];
                let ws;
                try { ws = new URL(target.webSocketDebuggerUrl); } catch { throw Error('malformed debugger URL'); }
                if (ws.protocol !== 'ws:' || !['127.0.0.1', 'localhost'].includes(ws.hostname)
                    || Number(ws.port) !== port || ws.username || ws.password) throw Error('unsafe debugger URL');
                return target;
            }
        } catch (error) {
            last = error.message;
            if (/malformed|ambiguous|unsafe/.test(last)) throw error;
        }
        await sleep(Math.min(pollMs, remaining(deadline)));
    }
    throw Error(`Main page target timeout: ${last}`);
}

class Cdp {
    constructor(url, WebSocketImpl, onEvent) {
        this.pending = new Map();
        this.serial = 0;
        this.socket = new WebSocketImpl(url);
        this.opened = new Promise((resolveOpen, rejectOpen) => {
            this.rejectOpen = rejectOpen;
            this.socket.addEventListener('open', resolveOpen, { once: true });
        });
        this.socket.addEventListener('error', () => this.fail(Error('CDP WebSocket error')));
        this.socket.addEventListener('close', () => this.fail(Error('CDP WebSocket closed')));
        this.socket.addEventListener('message', event => {
            try {
                const msg = JSON.parse(event.data);
                if (!msg || typeof msg !== 'object') throw Error('invalid message');
                if ('id' in msg) {
                    const call = this.pending.get(msg.id);
                    if (!call) return;
                    this.pending.delete(msg.id);
                    clearTimeout(call.timer);
                    if (msg.error) call.reject(Error(`CDP ${call.method}: ${brief(msg.error.message)}`));
                    else if (!msg.result || typeof msg.result !== 'object') call.reject(Error('malformed CDP result'));
                    else call.resolve(msg.result);
                } else if (typeof msg.method === 'string' && msg.params && typeof msg.params === 'object') {
                    onEvent(msg);
                } else throw Error('invalid event');
            } catch (error) { this.fail(Error(`malformed CDP message: ${brief(error.message)}`)); }
        });
    }
    fail(error) {
        this.failure ??= error;
        this.rejectOpen(error);
        for (const call of this.pending.values()) { clearTimeout(call.timer); call.reject(error); }
        this.pending.clear();
    }
    async connect(deadline) {
        const timer = setTimeout(() => this.fail(Error('CDP connect timeout')), remaining(deadline));
        try { await this.opened; } finally { clearTimeout(timer); }
    }
    call(method, params, deadline) {
        if (this.failure) return Promise.reject(this.failure);
        if (now() >= deadline) return Promise.reject(Error(`CDP timeout: ${method}`));
        return new Promise((resolveCall, reject) => {
            const id = ++this.serial;
            const timer = setTimeout(() => {
                this.pending.delete(id);
                reject(Error(`CDP timeout: ${method}`));
            }, remaining(deadline));
            this.pending.set(id, { resolve: resolveCall, reject, timer, method });
            try { this.socket.send(JSON.stringify({ id, method, params })); }
            catch (error) { this.fail(error); }
        });
    }
    close() { this.fail(Error('CDP session finished')); this.socket.close(); }
}

function earlyErrors() {
    const errors = [];
    Object.defineProperty(window, '__maxedGuiSmokeErrors', { value: errors });
    addEventListener('error', event => { if (errors.length < 100) errors.push(String(event.message || 'window error').slice(0, 1000)); });
    addEventListener('unhandledrejection', () => { if (errors.length < 100) errors.push('unhandled promise rejection'); });
}

export function inspectUi() {
    const frame = document.querySelector('#app .app-frame');
    const main = document.querySelector('#app #main-content');
    const setup = document.querySelector('#app .setup-page ol[aria-label="Setup"]');
    return {
        instrumented: Array.isArray(window.__maxedGuiSmokeErrors),
        mounted: !!(frame && main && setup && setup.children.length === 3
            && frame.getBoundingClientRect().width > 0 && setup.getBoundingClientRect().height > 0),
        errors: window.__maxedGuiSmokeErrors ?? [],
    };
}

// Self-contained because CDP evaluates this function in the page's context.
// Only the isolated synthetic Setup profile is inspected, never ordinary use.
function inspectDiagnostics() {
    const text = (value, limit = 1000) => String(value ?? '').slice(0, limit);
    const number = value => Number.isFinite(value) ? value : null;
    const element = selector => {
        const node = document.querySelector(selector);
        if (!node) return { exists: false, rect: null };
        const bounds = node.getBoundingClientRect();
        const rect = {};
        for (const key of ['x', 'y', 'top', 'right', 'bottom', 'left', 'width', 'height']) rect[key] = number(bounds[key]);
        return { exists: true, rect, childElementCount: number(node.children?.length ?? 0) };
    };
    return {
        location: { href: text(location.href) },
        document: { readyState: text(document.readyState, 100), visibilityState: text(document.visibilityState, 100),
            hidden: document.hidden, hasFocus: document.hasFocus() },
        window: { innerWidth: number(window.innerWidth), innerHeight: number(window.innerHeight) },
        elements: { app_frame: element('#app .app-frame'), main_content: element('#app #main-content'),
            setup_list: element('#app .setup-page ol[aria-label="Setup"]') },
        body: { childElementCount: number(document.body?.children.length ?? 0), innerText: text(document.body?.innerText, 300) },
    };
}

async function screenshot(cdp, outputDir, deadline) {
    const result = await cdp.call('Page.captureScreenshot', { format: 'png', fromSurface: true }, deadline);
    if (typeof result.data !== 'string' || !/^[A-Za-z0-9+/]+={0,2}$/.test(result.data)) throw Error('Invalid screenshot PNG');
    const bytes = Buffer.from(result.data, 'base64');
    if (!bytes.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))) throw Error('Invalid screenshot PNG signature');
    await writeFile(join(outputDir, 'screenshot.png'), bytes);
}

// The CLI fixes the observation period at ten seconds. Shorter intervals and an
// injected socket are available to offline tests only through this module API.
export async function runSmoke({ port = 9238, outputDir, timeoutMs = 60000,
    observationMs = 10000, pollMs = 100, WebSocketImpl = globalThis.WebSocket } = {}) {
    if (!Number.isInteger(port) || port < 1024 || port > 65535) throw Error('Port must be 1024..65535');
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0 || !Number.isFinite(observationMs) || observationMs <= 0) throw Error('Invalid deadlines');
    outputDir = resolve(outputDir);
    await mkdir(outputDir, { recursive: true });
    const started = now(), deadline = started + timeoutMs;
    const report = { ok: false, port, observation_ms: observationMs, elapsed_ms: 0, errors: [], screenshot: null };
    const evidence = { console_messages: [], network_loading_failed: [], log_entries: [], focus_errors: [] };
    const recent = (entries, entry) => { entries.push(entry); if (entries.length > 20) entries.shift(); };
    const timestamp = value => Number.isFinite(value) ? value : null;
    const consoleText = args => brief((args ?? []).slice(0, 20).map(arg => brief(arg.value ?? arg.description ?? '')).join(' '));
    let cdp;
    const record = (kind, text) => { if (report.errors.length < 100) report.errors.push({ kind, text: brief(text) }); };
    try {
        const target = await discover(port, deadline, pollMs);
        report.target = target.url;
        const appOrigin = new URL(target.url).origin;
        let navigating = false, navigationFrame, navigationFailure;
        cdp = new Cdp(target.webSocketDebuggerUrl, WebSocketImpl, ({ method, params }) => {
            if (method === 'Page.frameNavigated' && navigating && params.frame && !params.frame.parentId) {
                navigationFrame = params.frame;
                let origin;
                try { origin = new URL(navigationFrame.url).origin; } catch { /* Invalid URLs fail the origin check. */ }
                if (origin !== appOrigin) navigationFailure ??= Error(`Unexpected main-frame navigation: ${brief(navigationFrame.url)} (expected ${appOrigin})`);
            }
            if (method === 'Runtime.exceptionThrown') record(method, params.exceptionDetails?.exception?.description ?? params.exceptionDetails?.text ?? 'Uncaught exception');
            if (method === 'Runtime.consoleAPICalled') {
                const text = consoleText(params.args);
                recent(evidence.console_messages, { level: brief(params.type), text, timestamp: timestamp(params.timestamp) });
                if (params.type === 'error') record(method, text);
            }
            if (method === 'Network.loadingFailed') {
                recent(evidence.network_loading_failed, { requestId: brief(params.requestId), type: brief(params.type),
                    errorText: brief(params.errorText), canceled: params.canceled === true,
                    blockedReason: brief(params.blockedReason ?? ''), timestamp: timestamp(params.timestamp),
                    corsError: brief(params.corsErrorStatus?.corsError ?? ''),
                    failedParameter: brief(params.corsErrorStatus?.failedParameter ?? '') });
            }
            if (method === 'Log.entryAdded' && params.entry) {
                const entry = params.entry;
                recent(evidence.log_entries, { source: brief(entry.source), level: brief(entry.level), text: brief(entry.text),
                    url: brief(entry.url ?? ''), timestamp: timestamp(entry.timestamp) });
                if (entry.level === 'error') record(method, entry.text);
            }
        });
        await cdp.connect(deadline);
        for (const domain of ['Runtime', 'Log', 'Network', 'Page']) await cdp.call(`${domain}.enable`, {}, deadline);
        await cdp.call('Page.addScriptToEvaluateOnNewDocument', { source: `(${earlyErrors.toString()})()` }, deadline);
        // Discovery can advertise a pending app URL while about:blank is still
        // committed. Navigate explicitly after installing the first-script hooks;
        // reloading here would abort that pending load and reload about:blank.
        navigating = true;
        const navigation = await cdp.call('Page.navigate', { url: `${appOrigin}/` }, deadline);
        if (navigation.errorText) throw Error(`Page.navigate failed: ${brief(navigation.errorText)}`);
        if (typeof navigation.frameId !== 'string' || !navigation.frameId
            || typeof navigation.loaderId !== 'string' || !navigation.loaderId) throw Error('Malformed Page.navigate result: missing frameId or loaderId');
        // frameNavigated may precede the response. The handler retains the last
        // main-frame commit; stale loads and child frames cannot satisfy this wait.
        while (true) {
            if (navigationFailure) throw navigationFailure;
            if (cdp.failure) throw cdp.failure;
            if (report.errors.length) throw Error('UI error observed');
            if (now() >= deadline) throw Error('App navigation timeout: waiting for matching main-frame commit');
            if (navigationFrame?.id === navigation.frameId && navigationFrame.loaderId === navigation.loaderId) break;
            await sleep(Math.min(pollMs, remaining(deadline)));
        }
        // Best-effort hints; some WebView2/CDP versions do not implement them.
        for (const [method, params] of [['Page.bringToFront', {}], ['Emulation.setFocusEmulationEnabled', { enabled: true }]]) {
            try { await cdp.call(method, params, Math.min(deadline, now() + 1000)); }
            catch (error) { evidence.focus_errors.push({ method, text: brief(error.message) }); }
        }
        let mountedAt;
        while (true) {
            if (navigationFailure) throw navigationFailure;
            if (report.errors.length) throw Error('UI error observed');
            if (now() >= deadline) throw Error(mountedAt === undefined ? 'Rendered root timeout' : 'Observation timeout before ten-second check completed');
            let result;
            try {
                result = await cdp.call('Runtime.evaluate', { expression: `(${inspectUi.toString()})()`, returnByValue: true }, deadline);
            } catch (error) {
                // A committed document can still be creating its execution
                // context. Only pre-mount context transitions are transient.
                if (mountedAt === undefined && /^CDP Runtime.evaluate: (Execution context was destroyed|Cannot find context|Cannot find default execution context)/.test(error.message)) {
                    await sleep(Math.min(pollMs, remaining(deadline)));
                    continue;
                }
                throw error;
            }
            const state = result.result?.value;
            if (result.exceptionDetails || !state || typeof state.mounted !== 'boolean'
                || typeof state.instrumented !== 'boolean' || !Array.isArray(state.errors)) throw Error('Malformed UI evaluation result');
            for (const error of state.errors) record('new-document', error);
            if (report.errors.length) throw Error('UI error observed');
            if (state.instrumented && state.mounted) {
                mountedAt ??= now();
                if (now() - mountedAt >= observationMs) break;
            } else if (mountedAt !== undefined) throw Error('Rendered root disappeared during observation');
            await sleep(Math.min(pollMs, remaining(deadline)));
        }
        await screenshot(cdp, outputDir, deadline);
        report.screenshot = 'screenshot.png';
        if (navigationFailure) throw navigationFailure;
        if (report.errors.length) throw Error('UI error observed');
        report.ok = true;
    } catch (error) {
        report.reason = brief(error.message);
        if (cdp) {
            report.diagnostics = { page: null, ...evidence };
            try {
                // The run deadline may be exhausted. Keep evidence collection
                // separate and bounded so it cannot replace the original reason.
                const result = await cdp.call('Runtime.evaluate', {
                    expression: `(${inspectDiagnostics.toString()})()`, returnByValue: true, timeout: 750,
                }, now() + 1000);
                if (result.exceptionDetails || !result.result?.value?.document) throw Error('Page diagnostics evaluation unavailable');
                report.diagnostics.page = result.result.value;
            } catch (diagnosticError) { report.diagnostics.page_error = brief(diagnosticError.message); }
        }
        if (cdp && !report.screenshot) {
            try {
                await screenshot(cdp, outputDir, now() + 3000);
                report.screenshot = 'screenshot.png';
            } catch (captureError) { report.screenshot_error = brief(captureError.message); }
        }
    } finally {
        cdp?.close();
        report.elapsed_ms = Math.round(now() - started);
        await writeFile(join(outputDir, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
    }
    return report;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
    try {
        const options = { port: 9238, timeoutMs: 60000 };
        const args = process.argv.slice(2);
        while (args.length) {
            const flag = args.shift(), value = args.shift();
            if (!value) throw Error(`Missing value for ${flag}`);
            if (flag === '--port') options.port = Number(value);
            else if (flag === '--output-dir') options.outputDir = value;
            else if (flag === '--timeout-ms') options.timeoutMs = Number(value);
            else throw Error(`Unknown option: ${flag}`);
        }
        if (!options.outputDir) throw Error('--output-dir is required');
        if (options.timeoutMs < 15000) throw Error('--timeout-ms must allow at least 15 seconds');
        const report = await runSmoke(options);
        console.log(report.ok ? 'GUI smoke passed' : `GUI smoke failed: ${report.reason}`);
        process.exitCode = report.ok ? 0 : 1;
    } catch (error) {
        console.error(`GUI smoke failed: ${error.message}`);
        process.exitCode = 1;
    }
}
