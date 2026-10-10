// Diagnostic only. The PowerShell driver owns launch, lock, profile and process counters.
// Raw traces and allocation URLs never leave the caller's temporary evidence directory.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';

export function allocatorRows(trace) {
  return (trace.traceEvents ?? []).filter(e => e.ph === 'v' && e.args?.dumps).map(e => {
    const dumps = e.args.dumps;
    const allocators = Object.entries(dumps.allocators ?? {}).map(([name, value]) => ({
      name,
      guid: value.guid,
      attributes: Object.fromEntries(Object.entries(value.attrs ?? {}).map(([key, attr]) =>
        [key, attr.type === 'scalar' ? { value: Number.parseInt(attr.value, 16), units: attr.units } : { value: attr.value, units: attr.units }]))
    }));
    return { pid: e.pid, id: e.id ?? e.id2?.local ?? e.id2?.global, timestamp_us: e.ts,
      level: dumps.level_of_detail, process_totals: dumps.process_totals ?? null,
      allocators, ownership_edges: dumps.allocators_graph ?? [] };
  });
}

export function allocationSites(profile) {
  const sites = new Map();
  const visit = node => {
    const frame = node.callFrame;
    const key = JSON.stringify(frame);
    const prior = sites.get(key) ?? { ...frame, self_bytes: 0 };
    prior.self_bytes += node.selfSize;
    sites.set(key, prior);
    for (const child of node.children ?? []) visit(child);
  };
  visit(profile.head);
  return [...sites.values()].filter(s => s.self_bytes > 0).sort((a, b) => b.self_bytes - a.self_bytes);
}

function selfTest() {
  const rows = allocatorRows({ traceEvents: [{ ph: 'v', pid: 12, id: 'a', ts: 10,
    args: { dumps: { level_of_detail: 'detailed', allocators: {
      'v8/isolate/heap': { attrs: { size: { type: 'scalar', units: 'bytes', value: '100' },
        object_count: { type: 'scalar', units: 'objects', value: 'a' } } },
      'v8/isolate/heap/space': { attrs: { size: { type: 'scalar', units: 'bytes', value: '80' } } }
    } } } }] });
  assert.equal(rows[0].allocators[0].attributes.size.value, 256);
  assert.equal(rows[0].allocators[0].attributes.object_count.value, 10);
  assert.equal(rows[0].allocators.length, 2); // Preserve hierarchy; never blindly sum parent + child.
  assert.deepEqual(allocatorRows({ traceEvents: [] }), []);
  const frame = { functionName: 'synthetic', url: '', lineNumber: 1, columnNumber: 2 };
  assert.equal(allocationSites({ head: { callFrame: frame, selfSize: 100,
    children: [{ callFrame: frame, selfSize: 40 }] } })[0].self_bytes, 140);
  console.log('PASS: attribution hex scalars, hierarchy, missing providers and allocation aggregation');
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const utc = () => new Date().toISOString();
export async function connect(url) {
  if (!/^ws:\/\/(127\.0\.0\.1|localhost):\d+\//.test(url)) throw Error('CDP must remain loopback');
  const socket = new WebSocket(url), pending = new Map(), listeners = new Map();
  let id = 0;
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  socket.onmessage = message => {
    const event = JSON.parse(message.data);
    if (event.id && pending.has(event.id)) {
      const request = pending.get(event.id); pending.delete(event.id); clearTimeout(request.timer);
      if (event.error) request.reject(Error(JSON.stringify(event.error))); else request.resolve(event.result);
    }
    for (const listener of listeners.get(event.method) ?? []) listener(event.params);
  };
  socket.onclose = () => { for (const request of pending.values()) { clearTimeout(request.timer); request.reject(Error('CDP closed')); } pending.clear(); };
  return {
    call(method, params = {}) {
      return new Promise((resolve, reject) => {
        const serial = ++id;
        const timer = setTimeout(() => { pending.delete(serial); reject(Error(`CDP timeout: ${method}`)); }, 45000);
        pending.set(serial, { resolve, reject, timer }); socket.send(JSON.stringify({ id: serial, method, params }));
      });
    },
    on(method, callback) { listeners.set(method, [...(listeners.get(method) ?? []), callback]); },
    close() { socket.close(); }
  };
}

async function run(portText, destination) {
  const port = Number(portText), output = fs.realpathSync(destination);
  if (!Number.isInteger(port) || port < 1024 || port > 65535 ||
    !output.toLowerCase().startsWith((fs.realpathSync(os.tmpdir()) + path.sep).toLowerCase())) throw Error('Use temporary evidence and a local port');
  const write = (name, value) => fs.writeFileSync(path.join(output, name), JSON.stringify(value, null, 2), { flag: 'wx' });
  const events = [], metrics = [], errors = [], dumps = [];
  const record = (action, result = {}) => { const row = { utc: utc(), action, ...result }; events.push(row); console.log(JSON.stringify(row)); };
  let page, browser;
  const deadline = Date.now() + 920000;
  try {
    while (Date.now() < deadline && !page) {
      try {
        const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
        const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
        const target = targets.find(t => t.type === 'page' && /^https?:\/\/tauri\.localhost\//.test(t.url));
        if (target) {
          browser = await connect(version.webSocketDebuggerUrl); page = await connect(target.webSocketDebuggerUrl);
          write('cdp-version.json', version);
        }
      } catch { /* The owned WebView is still starting. */ }
      if (!page) await sleep(1000);
    }
    if (!page) throw Error('Owned custom-protocol target unavailable');
    const attempt = async (action, operation, summarize = value => value) => {
      try { const result = await operation(); record(action, { result: summarize(result) }); return result; }
      catch (error) { const row = { utc: utc(), action, error: String(error) }; errors.push(row); record(action, { failed: true }); return null; }
    };
    await page.call('Performance.enable');
    await page.call('HeapProfiler.enable');
    await attempt('document-shape', async () => {
      const result = await page.call('Runtime.evaluate', { returnByValue: true,
        expression: '({images:document.images.length,decodedImages:[...document.images].filter(i=>i.naturalWidth>0).length,visibility:document.visibilityState})' });
      return result.result.value;
    });
    const tracing = await attempt('tracing-start', () => browser.call('Tracing.start', {
      transferMode: 'ReturnAsStream', streamFormat: 'json', streamCompression: 'none',
      traceConfig: { recordMode: 'recordContinuously', traceBufferSizeInKb: 32768,
        includedCategories: ['disabled-by-default-memory-infra'], excludedCategories: ['*'],
        memoryDumpConfig: { triggers: [] } }
    }));
    const memoryDump = async label => {
      if (tracing === null) return;
      const at = utc();
      const result = await attempt(`dump-${label}`, () => browser.call('Tracing.requestMemoryDump', { levelOfDetail: 'detailed', deterministic: false }));
      dumps.push({ label, utc: at, result });
      await attempt(`document-${label}`, async () => {
        const shape = await page.call('Runtime.evaluate', { returnByValue: true,
          expression: '({images:document.images.length,decodedImages:[...document.images].filter(i=>i.naturalWidth>0).length,visibility:document.visibilityState})' });
        return shape.result.value;
      });
    };
    let nextMetrics = 0, busy, idle;
    const done = new Set();
    const once = async (key, ready, action) => { if (ready && !done.has(key)) { done.add(key); await action(); } };
    while (Date.now() < deadline) {
      const log = fs.existsSync(path.join(output, 'driver.log')) ? fs.readFileSync(path.join(output, 'driver.log'), 'utf8') : '';
      const phaseDate = phase => { const match = log.match(new RegExp(`PERF_LIVE ${phase}[^\\r\\n]*utc=([^\\s]+)`)); return match ? Date.parse(match[1]) : undefined; };
      busy ??= phaseDate('busy'); idle ??= phaseDate('idle');
      const now = Date.now(), busySeconds = busy ? (now - busy) / 1000 : null, idleSeconds = idle ? (now - idle) / 1000 : null;
      if (now >= nextMetrics) {
        const result = await page.call('Performance.getMetrics');
        metrics.push({ utc: utc(), busy_seconds: busySeconds, idle_seconds: idleSeconds,
          metrics: Object.fromEntries(result.metrics.map(m => [m.name, m.value])) });
        nextMetrics = now + 10000;
      }
      await once('sampling-start', busySeconds >= 330 && !idle, () => attempt('sampling-start', () => page.call('HeapProfiler.startSampling', {
        samplingInterval: 32768, includeObjectsCollectedByMajorGC: true, includeObjectsCollectedByMinorGC: true
      })));
      await once('sampling-stop', busySeconds >= 390 && done.has('sampling-start'), async () => {
        const sampled = await attempt('sampling-stop', () => page.call('HeapProfiler.stopSampling'), value => ({ samples: value.profile.samples.length }));
        if (sampled) { write('allocation-profile.json', sampled.profile); write('allocation-sites.json', allocationSites(sampled.profile)); }
      });
      await once('busy300', busySeconds >= 300, () => memoryDump('busy300'));
      await once('busy600', busySeconds >= 600, () => memoryDump('busy600'));
      await once('idle30', idleSeconds >= 30, () => memoryDump('idle30'));
      await once('pressure-window', idleSeconds >= 90, async () => record('pressure-before-window'));
      await once('pressure', idleSeconds >= 120, async () => {
        await memoryDump('idle120-before-pressure');
        await attempt('critical-pressure', () => browser.call('Memory.simulatePressureNotification', { level: 'critical' }));
      });
      await once('idle150', idleSeconds >= 150, () => memoryDump('idle150-after-pressure'));
      await once('gc', idleSeconds >= 190 && log.includes('PERF_LIVE complete'), async () => {
        await memoryDump('idle190-before-gc');
        await attempt('collect-garbage', () => page.call('HeapProfiler.collectGarbage'));
      });
      await once('idle220', idleSeconds >= 220, () => memoryDump('idle220-after-gc'));
      if (idleSeconds >= 250) break;
      await sleep(500);
    }
    if (!done.has('idle220') || !done.has('gc')) throw Error('Diagnostic schedule incomplete');
    if (tracing !== null) {
      let complete;
      const completion = new Promise(resolve => { complete = resolve; });
      browser.on('Tracing.tracingComplete', complete);
      await browser.call('Tracing.end');
      let timer;
      const end = await Promise.race([completion, new Promise((_, reject) => { timer = setTimeout(() => reject(Error('Trace completion timeout')), 45000); })]);
      clearTimeout(timer);
      if (end.dataLossOccurred) errors.push({ action: 'trace', error: 'dataLossOccurred' });
      const sink = fs.openSync(path.join(output, 'memory-trace.json'), 'wx');
      try {
        for (;;) {
          const chunk = await browser.call('IO.read', { handle: end.stream, size: 1048576 });
          fs.writeSync(sink, chunk.base64Encoded ? Buffer.from(chunk.data, 'base64') : chunk.data);
          if (chunk.eof) break;
        }
      } finally { fs.closeSync(sink); await browser.call('IO.close', { handle: end.stream }); }
      const trace = JSON.parse(fs.readFileSync(path.join(output, 'memory-trace.json'), 'utf8'));
      write('allocator-dumps.json', allocatorRows(trace));
    }
  } finally {
    write('diagnostic-events.json', events); write('performance-metrics.json', metrics);
    write('diagnostic-errors.json', errors); write('dump-requests.json', dumps);
    page?.close(); browser?.close();
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv[2] === '--self-test') selfTest();
  else await run(...process.argv.slice(2));
}
