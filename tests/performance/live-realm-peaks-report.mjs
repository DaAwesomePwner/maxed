// Offline aggregate review candidate. Raw identities and profiles remain in temp.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import { readCsv, stats, attributeDumps } from './live-realm-attribution-report.mjs';

export function concurrentSeries(rows, totals, busy, idle) {
  const groups = new Map();
  for (const row of rows) {
    const group = groups.get(row.utc) ?? { identities: new Set(), values: {} };
    const identity = `${row.pid},${row.start_utc}`;
    if (group.identities.has(identity)) throw Error('Duplicate process in sample');
    group.identities.add(identity);
    const key = ['Maxed', 'renderer', 'browser'].includes(row.role) ? row.role : row.utility_sub_type || row.process_type || 'other';
    const bytes = Number(row.private_bytes);
    if (!Number.isFinite(bytes) || bytes < 0) throw Error('Invalid private bytes');
    group.values[key] = (group.values[key] ?? 0) + bytes;
    groups.set(row.utc, group);
  }
  return totals.map(total => {
    const group = groups.get(total.utc);
    if (!group) throw Error('Missing process sample');
    const sum = Object.values(group.values).reduce((a, b) => a + b, 0);
    if (sum !== Number(total.private_bytes)) throw Error('Concurrent total mismatch');
    const at = Date.parse(total.utc);
    if (![at, busy].every(Number.isFinite) || (idle !== null && !Number.isFinite(idle))) throw Error('Invalid phase timestamp');
    return { busy_seconds: (at - busy) / 1000, idle_seconds: idle === null ? null : (at - idle) / 1000, combined: sum, ...group.values };
  });
}

export function summarize(series, start, end, clock = 'busy_seconds') {
  const rows = series.filter(row => row[clock] !== null && row[clock] >= start && row[clock] < end);
  if (!rows.length) return null;
  const keys = [...new Set(rows.flatMap(row => Object.keys(row)))].filter(k => !k.endsWith('_seconds'));
  const peak = rows.reduce((a, b) => a.combined >= b.combined ? a : b);
  return { start_seconds: start, end_seconds: end, clock, samples: rows.length,
    sampled_span_seconds: rows.at(-1)[clock] - rows[0][clock],
    bytes: Object.fromEntries(keys.map(k => [k, stats(rows.map(row => row[k] ?? 0))])), concurrent_peak: peak };
}

export function analyze(directory) {
  const json = name => JSON.parse(fs.readFileSync(path.join(directory, name), 'utf8').replace(/^\uFEFF/, ''));
  const csv = name => readCsv(fs.readFileSync(path.join(directory, name), 'utf8'));
  const launch = json('launch.json'), fresh = launch.mode === 'fresh-idle';
  const phases = json('phases.json'), busy = Date.parse(fresh ? launch.start_utc : phases.busy), idle = phases.idle ? Date.parse(phases.idle) : null;
  const processRows = csv('memory-processes.csv');
  const series = concurrentSeries(processRows, csv('memory-totals.csv'), busy, idle);
  const windows = {};
  for (const [name, start, end, clock = 'busy_seconds'] of (fresh ? [
    ['startup_seed_observed', series[0].busy_seconds, 30], ['fresh_idle', 30, 210]
  ] : [
    ['startup_seed_observed', series[0].busy_seconds, 0], ['early_busy', 0, 30], ['steady_busy', 30, 580],
    ['late_busy', 580, 610], ['transition_until_idle_log', 610, idle === null ? NaN : (idle - busy) / 1000],
    ['transition_plus30', 0, 30, 'idle_seconds'], ['gate_idle', 30, 175, 'idle_seconds'],
    ['idle_before_driver_end', 120, 175, 'idle_seconds'], ['idle_after_driver_end', 200, 300, 'idle_seconds'],
    ['idle_300_450', 300, 450, 'idle_seconds'], ['idle_450_600', 450, 600, 'idle_seconds'],
    ['idle_pre_gc', 570, 600, 'idle_seconds'], ['idle_post_gc', 625, 640, 'idle_seconds'],
    ['idle_post_reload', 670, 700, 'idle_seconds']
  ])) windows[name] = summarize(series, start, end, clock);
  const bins = [];
  for (let start = Math.floor(series[0].busy_seconds / 30) * 30; start <= series.at(-1).busy_seconds; start += 30) {
    const bin = summarize(series, start, start + 30); if (bin) bins.push(bin);
  }
  const result = { diagnostic: json('launch.json').diagnostic, executable_sha256: json('launch.json').executable_sha256,
    rendered: json('rendered.json'), collector: json('memory-summary.json').memory_result,
    load_confounded: json('ongoing-load.json').some(row => row.heavy_count > 0),
    mode: launch.mode, series_origin: fresh ? 'process_start' : 'busy_marker',
    logged_busy_seconds: fresh || idle === null ? null : (idle - busy) / 1000, transition_scheduled_busy_seconds: fresh ? null : 610,
    complete_busy_interval: !fresh && series.some(row => row.busy_seconds >= 610), logged_idle_observed: idle !== null,
    complete_idle_gate_interval: !fresh && series.some(row => row.idle_seconds !== null && row.idle_seconds >= 175),
    complete_fresh_idle_interval: fresh && series.some(row => row.busy_seconds >= 210),
    overall_concurrent_peak: series.reduce((a, b) => a.combined >= b.combined ? a : b),
    windows, thirty_second_bins: bins,
    boundary_samples: series.filter(row => (row.busy_seconds >= 580 && row.idle_seconds !== null && row.idle_seconds < 20) || row.busy_seconds < 30) };
  if (fs.existsSync(path.join(directory, 'allocator-dumps.json'))) {
    result.dumps = attributeDumps(json('allocator-dumps.json'), json('dump-requests.json'), processRows)
      .map(({ utc, ...dump }) => ({ ...dump, busy_seconds: (Date.parse(utc) - busy) / 1000, idle_seconds: idle === null ? null : (Date.parse(utc) - idle) / 1000 }));
    result.metrics = json('performance-metrics.json').map(({ utc, ...row }) => ({ ...row,
      busy_seconds: (Date.parse(utc) - busy) / 1000, idle_seconds: idle === null ? null : (Date.parse(utc) - idle) / 1000 }));
    result.diagnostic_errors = json('diagnostic-errors.json');
  }
  return result;
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv[2] === '--self-test') {
    const utc = '2026-01-01T00:00:00Z', origin = Date.parse(utc);
    const rows = [{ utc, pid: 1, start_utc: utc, role: 'renderer', private_bytes: 20 },
      { utc, pid: 2, start_utc: utc, role: 'Maxed', private_bytes: 10 }];
    const series = concurrentSeries(rows, [{ utc, private_bytes: 30 }], origin, origin + 610000);
    assert.equal(series[0].combined, 30); assert.equal(series[0].idle_seconds, -610);
    assert.throws(() => concurrentSeries([...rows, rows[0]], [{ utc, private_bytes: 30 }], origin, origin));
    assert.throws(() => concurrentSeries(rows, [{ utc, private_bytes: 50 }], origin, origin));
    assert.equal(summarize(series, 0, 1).concurrent_peak.Maxed, 10);
    assert.equal(summarize(series, 1, 2), null);
    const partial = concurrentSeries(rows, [{ utc, private_bytes: 30 }], origin, null);
    assert.equal(summarize(partial, 0, 180, 'idle_seconds'), null);
    assert.equal(summarize([...series, { ...series[0], busy_seconds: 1, renderer: 5, Maxed: 30, combined: 35 }], 0, 2).concurrent_peak.renderer, 5);
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'maxed-fresh-report-'));
    try {
      const write = (name, value) => fs.writeFileSync(path.join(directory, name), JSON.stringify(value));
      write('launch.json', { mode: 'fresh-idle', start_utc: utc, diagnostic: false });
      write('phases.json', { idle: new Date(origin + 30000).toISOString() });
      write('rendered.json', { idle: true, busy: false, subscription_failed: false });
      write('memory-summary.json', { memory_result: 'measured' });
      write('ongoing-load.json', [{ heavy_count: 0 }]);
      const samples = [[1, 100], [30, 20], [209, 30], [211, 200]];
      fs.writeFileSync(path.join(directory, 'memory-totals.csv'), 'utc,private_bytes\n' + samples.map(([s, bytes]) => `${new Date(origin + s * 1000).toISOString()},${bytes}`).join('\n'));
      fs.writeFileSync(path.join(directory, 'memory-processes.csv'), 'utc,pid,start_utc,role,private_bytes\n' + samples.map(([s, bytes]) => `${new Date(origin + s * 1000).toISOString()},1,${utc},Maxed,${bytes}`).join('\n'));
      const floor = analyze(directory);
      assert.equal(floor.windows.startup_seed_observed.bytes.combined.max, 100);
      assert.equal(floor.windows.fresh_idle.bytes.combined.max, 30);
      assert.equal(floor.windows.fresh_idle.bytes.combined.mean, 25);
      assert.equal(floor.windows.steady_busy, undefined);
      assert.equal(floor.logged_busy_seconds, null);
      assert.equal(floor.complete_busy_interval, false);
      assert.equal(floor.complete_idle_gate_interval, false);
      assert.equal(floor.complete_fresh_idle_interval, true);
    } finally {
      fs.rmSync(directory, { recursive: true });
    }
    console.log('PASS: peak timeline identity, concurrent sums, boundaries and non-additive peaks');
  } else {
    const report = analyze(process.argv[2]);
    fs.writeFileSync(path.join(process.argv[2], 'peaks-analysis.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify({ collector: report.collector, confounded: report.load_confounded,
      steady_mean: report.windows.steady_busy?.bytes.combined.mean ?? null,
      fresh_idle_mean: report.windows.fresh_idle?.bytes.combined.mean ?? null, overall_peak: report.overall_concurrent_peak }));
  }
}
