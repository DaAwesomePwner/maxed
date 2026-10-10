// Real WebView regression; diagnostic CDP memory must never count as budget evidence.
// Caller owns the launch lock and launches an isolated P4 profile with a child-only CDP port.
// node sprite-route-cache.mjs <port> <fresh temporary report filename>
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
const [portText, reportPath] = process.argv.slice(2);
const port = Number(portText);
assert(Number.isInteger(port) && port >= 1024 && port <= 65535);
const destination = path.resolve(reportPath);
const relative = path.relative(fs.realpathSync(os.tmpdir()), fs.realpathSync(path.dirname(destination)));
assert(!relative.startsWith('..') && !path.isAbsolute(relative) && !fs.existsSync(destination));
const deadline = setTimeout(() => { console.error('Sprite regression deadline exceeded'); process.exit(2); }, 150_000);
const tabs = await (await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(10_000) })).json();
const target = tabs.find(tab => tab.type === 'page' && /^https?:\/\/tauri\.localhost\//.test(tab.url));
assert(target && /^ws:\/\/(127\.0\.0\.1|localhost):\d+\//.test(target.webSocketDebuggerUrl));
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
let serial = 0, phase = 0;
const pending = new Map(), requests = new Map(), routes = [];
socket.onmessage = event => {
  const message = JSON.parse(event.data);
  if (message.id) {
    const request = pending.get(message.id);
    if (!request) return;
    clearTimeout(request.timer); pending.delete(message.id);
    if (message.error) request.reject(Error('CDP command failed')); else request.resolve(message.result);
  }
  const p = message.params;
  if (message.method === 'Network.requestWillBeSent' && /^http:\/\/maxed-sprite\.localhost\/[a-f0-9]{64}\/[A-Za-z_]+\.png\/\d+-\d+-\d+-\d+\.png$/.test(p.request.url)) {
    requests.set(p.requestId, { url: p.request.url, phase, cached: false, bytes: 0, cacheControl: null });
  }
  const row = requests.get(p?.requestId);
  if (!row) return;
  if (message.method === 'Network.requestServedFromCache') row.cached = true;
  if (message.method === 'Network.responseReceived') {
    row.cached ||= p.response.fromDiskCache || p.response.fromPrefetchCache || false;
    row.cacheControl = Object.entries(p.response.headers).find(([key]) => key.toLowerCase() === 'cache-control')?.[1] ?? null;
  }
  if (message.method === 'Network.loadingFinished') row.bytes = p.encodedDataLength;
};
function call(method, params = {}) {
  const id = ++serial;
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject, timer: setTimeout(() => { pending.delete(id); reject(Error('CDP command deadline')); }, 15_000) });
    socket.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expression) {
  const result = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  assert(!result.exceptionDetails, 'Route evaluation failed');
  return result.result.value;
}
async function visit(hash, label) {
  phase++;
  await evaluate(`(async()=>{
    const hash=${JSON.stringify(hash)},find=()=>[...document.querySelectorAll('a[href]')].find(a=>a.getAttribute('href')===hash);
    if(!find()&&hash.startsWith('#/runs/')){document.querySelector('a[href="#/runs"]').click();await new Promise(resolve=>setTimeout(resolve,1200));}
    const link=find();if(!link)throw Error('Route link missing');link.click();
  })()`);
  if (label === 'Home') {
    await new Promise(resolve => setTimeout(resolve, 500));
    await evaluate(`(async()=>{
      const button=name=>[...document.querySelectorAll('button')].find(b=>b.textContent===name);
      button('Session')?.click();await new Promise(resolve=>setTimeout(resolve,200));button('Today')?.click();
    })()`);
  }
  await new Promise(resolve => setTimeout(resolve, 1800));
  const state = await evaluate(`(async()=>{
    const images=[...document.querySelectorAll('main img[src^="http://maxed-sprite.localhost/"]')];
    await Promise.all(images.map(img=>img.decode().catch(()=>{})));
    return {hash:location.hash,images:images.length,decoded:images.filter(img=>img.naturalWidth>0).length,
      rail:!!document.querySelector('a[href="#/runs"]'),heading:!!document.querySelector('main h1')};
  })()`);
  assert.equal(state.hash, hash);
  assert(state.rail && state.heading, 'Real rail and route content required');
  routes.push({ route: label, ...state });
  if (label !== 'Settings' && label !== 'Runs') assert(state.images > 0 && state.images === state.decoded, `${label} requires decoded sprites`);
}
let passed = false;
try {
  await call('Network.enable');
  await call('Network.setCacheDisabled', { cacheDisabled: false });
  // P4's invented history is dated October 1. Set only this diagnostic page's
  // wall clock so Home/Today reads it; no persisted setting or owner data.
  await evaluate(`(()=>{const OriginalDate=Date;window.Date=class extends OriginalDate {
    constructor(...args){super(...(args.length?args:['2026-10-01T18:00:00Z']));}
    static now(){return new OriginalDate('2026-10-01T18:00:00Z').getTime();}
  };})()`);
  for (let i = 0; i < 3; i++) { await visit('#/', 'Home'); await visit('#/settings', 'Settings'); }
  await visit('#/runs', 'Runs');
  const story = await evaluate(`document.querySelector('main a[href^="#/runs/"]')?.getAttribute('href')`);
  assert(story && /^#\/runs\/[^/]+$/.test(story), 'P4 Story link required');
  for (let i = 0; i < 3; i++) { await visit(story, 'Story'); await visit('#/settings', 'Settings'); }
  await visit('#/', 'Home');
  const byUrl = new Map();
  for (const row of requests.values()) {
    assert.match(row.cacheControl ?? '', /max-age=31536000.*immutable/, 'Sprite responses must be immutable');
    const rows = byUrl.get(row.url) ?? []; rows.push(row); byUrl.set(row.url, rows);
  }
  assert(byUrl.size > 0, 'Real tile responses required');
  // Initial DOM consumers can race for the same tile.
  // The behavioural contract is no new transfer on a later route mount.
  for (const rows of byUrl.values()) assert(new Set(rows.filter(row => !row.cached && row.bytes > 0).map(row => row.phase)).size <= 1,
    'A released Home or Story tile was transferred again in a later route phase');
  passed = true;
} finally {
  fs.writeFileSync(destination, JSON.stringify({ diagnostic_only: true, passed, routes, responses: [...requests.values()] }, null, 2), { flag: 'wx' });
  clearTimeout(deadline);
  for (const request of pending.values()) clearTimeout(request.timer);
  socket.close();
}
console.log('PASS: Home/Settings and Story/Settings reuse fingerprinted decoded sprites');
