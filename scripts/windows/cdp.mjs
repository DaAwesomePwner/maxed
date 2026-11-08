import { writeFile } from 'node:fs/promises';
import { setTimeout as sleep } from 'node:timers/promises';

// CDP discovery/transport follows tests/gui-smoke/cdp-smoke.mjs.
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

export async function discover(port, deadline, pollMs) {
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
                if (ws.protocol !== 'ws:' || ws.hostname !== '127.0.0.1'
                    || Number(ws.port) !== port || ws.username || ws.password
                    || !ws.pathname.startsWith('/devtools/page/') || ws.search || ws.hash) throw Error('unsafe debugger URL');
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

export function clickExpression(value) {
    return `(() => {
        const wanted = ${JSON.stringify(value)};
        const visible = e => e.getClientRects().length && getComputedStyle(e).visibility !== 'hidden'
            && !e.disabled && e.getAttribute('aria-disabled') !== 'true';
        let matches = [];
        try { matches = [...document.querySelectorAll(wanted)].filter(visible); } catch {}
        if (!matches.length) matches = [...document.querySelectorAll('nav a, nav button, [role="navigation"] a, [role="navigation"] button')]
            .filter(e => visible(e) && e.innerText.trim() === wanted);
        if (matches.length !== 1) throw Error('Expected one visible enabled match; found ' + matches.length);
        matches[0].click();
        return { clicked: wanted, hash: location.hash };
    })()`;
}

export async function drive(command, value, duration, port) {
    const target = await discover(port, now() + 10000, 200);
    const errors = [];
    const cdp = new Cdp(target.webSocketDebuggerUrl, globalThis.WebSocket, ({ method, params }) => {
        if (errors.length >= 100) return;
        if (method === 'Runtime.exceptionThrown') errors.push({ kind: method, text: brief(params.exceptionDetails?.exception?.description ?? params.exceptionDetails?.text) });
        if (method === 'Runtime.consoleAPICalled' && params.type === 'error') errors.push({ kind: method,
            text: brief(params.args.map(arg => arg.value ?? arg.description ?? '').join(' ')) });
        if (method === 'Log.entryAdded' && params.entry?.level === 'error') errors.push({ kind: method, text: brief(params.entry.text) });
    });
    const deadline = now() + (command === 'console' ? duration : 0) + 10000;
    try {
        await cdp.connect(deadline);
        if (command === 'console') {
            await cdp.call('Runtime.enable', {}, deadline);
            await cdp.call('Log.enable', {}, deadline);
            await sleep(duration);
            if (cdp.failure) throw cdp.failure;
            return { observation_ms: duration, errors };
        }
        if (command === 'shot') {
            const path = value;
            const result = await cdp.call('Page.captureScreenshot', { format: 'png', fromSurface: true }, deadline);
            const bytes = Buffer.from(result.data, 'base64');
            if (bytes.length < 24 || !bytes.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))) throw Error('Invalid PNG');
            await writeFile(path, bytes, { flag: 'wx' });
            return { path, bytes: bytes.length, width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
        }
        const result = await cdp.call('Runtime.evaluate', {
            expression: command === 'eval' ? value : clickExpression(value), awaitPromise: true, returnByValue: true, timeout: 8000,
        }, deadline);
        if (result.exceptionDetails) throw Error(brief(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text));
        return result.result;
    } finally { cdp.close(); }
}
