// Diagnostic CDP only. The maintained PowerShell driver owns launch and cleanup.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import { connect, allocatorRows, allocationSites } from './live-realm-attribution.mjs';

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
export function schedule(mode) {
  if (!['natural', 'seed-gc', 'optimize-size', 'seed-no-atlas'].includes(mode)) throw Error('Unknown diagnostic');
  if (mode === 'seed-no-atlas') return [0, 1, 10, 30, 60].map(seconds => ({ label: `busy${seconds}`, phase: 'busy', seconds }));
  return [
    ...[0, 1, 10, 30, 300, 590, 605, 609, 610, 611, 612, 613, 615, 620].map(seconds => ({ label: `busy${seconds}`, phase: 'busy', seconds })),
    ...[1, 3, 10, 30, ...(mode === 'optimize-size' ? [] : [120, 175, 200]), ...(mode === 'natural' ? [300, 450, 600, 625, 660, 700] : [])]
      .map(seconds => ({ label: `idle${seconds}`, phase: 'idle', seconds }))
  ];
}

async function run(portText, destination, mode) {
  const checkpoints = schedule(mode), port = Number(portText), output = fs.realpathSync(destination);
  if (!Number.isInteger(port) || port < 1024 || port > 65535 ||
    !output.toLowerCase().startsWith((fs.realpathSync(os.tmpdir()) + path.sep).toLowerCase())) throw Error('Use temporary evidence and a local port');
  const write = (name, value) => fs.writeFileSync(path.join(output, name), JSON.stringify(value, null, 2), { flag: 'wx' });
  const events = [], metrics = [], requests = [], errors = [];
  const record = (action, result = {}) => { const row = { utc: new Date().toISOString(), action, ...result }; events.push(row); console.log(JSON.stringify(row)); };
  let page, browser, tracing = false;
  const deadline = Date.now() + (mode === 'natural' ? 1380000 : 910000);
  try {
    const attachDeadline = Date.now() + 60000;
    while (!page && Date.now() < attachDeadline) {
      try {
        const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
        const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
        const target = targets.find(t => t.type === 'page' && /^https?:\/\/tauri\.localhost\//.test(t.url));
        if (target) {
          browser = await connect(version.webSocketDebuggerUrl); page = await connect(target.webSocketDebuggerUrl);
          write('cdp-version.json', version);
        }
      } catch { /* Owned WebView is starting. */ }
      if (!page) await sleep(100);
    }
    if (!page) throw Error('Owned custom-protocol target unavailable');
    if (mode === 'seed-no-atlas') {
      await page.call('Network.enable');
      await page.call('Network.setBlockedURLs', { urls: ['http://maxed-sprite.localhost/*'] });
      record('diagnostic-atlas-requests-blocked');
    }
    await page.call('Performance.enable');
    await page.call('HeapProfiler.enable');
    // Counters only: no command arguments, identities or response contents.
    // Later probe runs use this audit; record its presence in the evidence.
    const audit = await page.call('Runtime.evaluate', { returnByValue: true, expression: `(() => {
      const owner = window.__TAURI_INTERNALS__;
      if (!owner || window.__maxedPerfCommands) return;
      const counts = window.__maxedPerfCommands = {};
      const allowed = new Set(['get_live_snapshot', 'get_home_summary', 'get_dungeon_bests', 'get_scope_loot', 'get_object_visuals']);
      const original = owner.invoke;
      const wrapped = function(command, ...args) {
        if (allowed.has(command)) counts[command] = (counts[command] || 0) + 1;
        return original.call(this, command, ...args);
      };
      owner.invoke = wrapped;
      return { installed: owner.invoke === wrapped };
    })()` });
    record('command-counter-audit', { result: audit.result.value ?? { installed: false } });
    await browser.call('Tracing.start', { transferMode: 'ReturnAsStream', streamFormat: 'json', streamCompression: 'none',
      traceConfig: { recordMode: 'recordUntilFull', traceBufferSizeInKb: 65536,
        includedCategories: ['disabled-by-default-memory-infra'], excludedCategories: ['*'], memoryDumpConfig: { triggers: [] } } });
    tracing = true;
    let nativeSampling = false;
    const nativeProbe = async (action, operation) => {
      try { const result = await operation(); record(action, { supported: true }); return result; }
      catch (error) { record(action, { supported: false, error: String(error) }); return null; }
    };
    const dump = async label => {
      const utc = new Date().toISOString();
      const result = await browser.call('Tracing.requestMemoryDump', { levelOfDetail: 'detailed', deterministic: false });
      requests.push({ label, utc, result }); record(`dump-${label}`, { result });
      if (!result.success) throw Error(`Failed dump ${label}`);
      if (nativeSampling && ['busy612', 'idle3'].includes(label)) {
        const sampled = await nativeProbe(`native-profile-${label}`, () => page.call('Memory.getSamplingProfile'));
        if (sampled) write(`native-profile-${label}.json`, sampled.profile);
      }
      if (['busy0', 'busy30', 'busy605', 'idle10', 'idle30'].includes(label)) {
        const shape = await page.call('Runtime.evaluate', { returnByValue: true,
          expression: '({images:document.images.length,decodedImages:[...document.images].filter(i=>i.naturalWidth>0).length,commands:window.__maxedPerfCommands ?? null})' });
        record(`shape-${label}`, { result: shape.result.value });
      }
    };
    await dump('attached-seed');
    let busy, idle, nextMetrics = 0, nextSeed = Date.now() + 2000;
    const done = new Set();
    while (Date.now() < deadline) {
      const log = fs.readFileSync(path.join(output, 'driver.log'), 'utf8');
      const phase = name => { const match = log.match(new RegExp(`PERF_LIVE ${name}[^\\r\\n]*utc=([^\\s]+)`)); return match ? Date.parse(match[1]) : undefined; };
      busy ??= phase('busy'); idle ??= phase('idle');
      const now = Date.now(), b = busy === undefined ? null : (now - busy) / 1000, i = idle === undefined ? null : (now - idle) / 1000;
      if (now >= nextMetrics) {
        const value = await page.call('Performance.getMetrics');
        metrics.push({ utc: new Date().toISOString(), busy_seconds: b, idle_seconds: i,
          metrics: Object.fromEntries(value.metrics.map(m => [m.name, m.value])) });
        nextMetrics = now + ((b === null || b < 35 || (b > 588 && (i === null || i < 15))) ? 250 : 10000);
      }
      if (b === null && now >= nextSeed) { await dump(`seed-${requests.length}`); nextSeed = Date.now() + 2000; }
      for (const checkpoint of checkpoints) {
        const seconds = checkpoint.phase === 'busy' ? b : i;
        if (seconds !== null && seconds >= checkpoint.seconds && !done.has(checkpoint.label)) {
          done.add(checkpoint.label); await dump(checkpoint.label);
          if (checkpoint.label === 'busy0' && mode === 'seed-gc') {
            await page.call('HeapProfiler.collectGarbage'); record('post-seed-gc'); await dump('seed-after-gc');
          }
          if (checkpoint.label === 'busy605' && mode === 'seed-gc') {
            await page.call('HeapProfiler.startSampling', { samplingInterval: 32768,
              includeObjectsCollectedByMajorGC: true, includeObjectsCollectedByMinorGC: true });
            record('transition-sampling-start');
          }
          if (checkpoint.label === 'busy605' && mode === 'optimize-size') {
            nativeSampling = await nativeProbe('native-sampling-start', () => page.call('Memory.startSampling', { samplingInterval: 32768 })) !== null;
          }
          if (checkpoint.label === 'idle10' && nativeSampling) {
            await nativeProbe('native-sampling-stop', () => page.call('Memory.stopSampling')); nativeSampling = false;
          }
          if (checkpoint.label === 'idle10' && mode === 'seed-gc') {
            const sampled = await page.call('HeapProfiler.stopSampling');
            write('transition-allocation-profile.json', sampled.profile);
            write('transition-allocation-sites.json', allocationSites(sampled.profile));
            record('transition-sampling-stop', { samples: sampled.profile.samples.length });
          }
        }
      }
      // Preserve ten minutes of natural idle before destructive diagnostic probes.
      if (mode === 'natural' && i !== null && i >= 610 && !done.has('idle-gc')) {
        done.add('idle-gc'); await page.call('HeapProfiler.collectGarbage'); record('idle-gc');
      }
      if (mode === 'natural' && i !== null && i >= 640 && !done.has('idle-reload')) {
        done.add('idle-reload'); await page.call('Page.reload', { ignoreCache: false }); record('idle-reload');
      }
      if (mode === 'seed-no-atlas' && b !== null && b >= 65) break;
      if (i !== null && i >= (mode === 'natural' ? 705 : mode === 'optimize-size' ? 35 : 205)) break;
      await sleep(50);
    }
    if (!done.has(mode === 'seed-no-atlas' ? 'busy60' : mode === 'natural' ? 'idle700' : mode === 'optimize-size' ? 'idle30' : 'idle200')) throw Error('Peak diagnostic schedule incomplete');
    let resolveComplete;
    const completed = new Promise(resolve => { resolveComplete = resolve; });
    browser.on('Tracing.tracingComplete', resolveComplete);
    await browser.call('Tracing.end'); tracing = false;
    let timer;
    const end = await Promise.race([completed, new Promise((_, reject) => { timer = setTimeout(() => reject(Error('Trace completion timeout')), 45000); })]);
    clearTimeout(timer);
    if (end.dataLossOccurred) throw Error('Trace data loss');
    const sink = fs.openSync(path.join(output, 'memory-trace.json'), 'wx');
    try {
      for (;;) {
        const chunk = await browser.call('IO.read', { handle: end.stream, size: 1048576 });
        fs.writeSync(sink, chunk.base64Encoded ? Buffer.from(chunk.data, 'base64') : chunk.data);
        if (chunk.eof) break;
      }
    } finally { fs.closeSync(sink); await browser.call('IO.close', { handle: end.stream }); }
    write('allocator-dumps.json', allocatorRows(JSON.parse(fs.readFileSync(path.join(output, 'memory-trace.json'), 'utf8'))));
  } catch (error) { errors.push({ error: String(error) }); throw error; }
  finally {
    if (tracing) await browser?.call('Tracing.end').catch(() => {});
    write('diagnostic-events.json', events); write('performance-metrics.json', metrics);
    write('dump-requests.json', requests); write('diagnostic-errors.json', errors);
    page?.close(); browser?.close();
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv[2] === '--self-test') {
    assert.throws(() => schedule('production'));
    assert(schedule('natural').some(s => s.phase === 'idle' && s.seconds === 600));
    assert(!schedule('seed-gc').some(s => s.seconds === 600));
    assert(schedule('optimize-size').some(s => s.phase === 'busy' && s.seconds === 610));
    assert.equal(new Set(schedule('natural').map(s => s.label)).size, schedule('natural').length);
    assert(schedule('seed-no-atlas').every(s => s.phase === 'busy' && s.seconds <= 60));
    console.log('PASS: diagnostic peak schedules, transition boundary, long idle and distinct labels');
  } else await run(...process.argv.slice(2));
}
