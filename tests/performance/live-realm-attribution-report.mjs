// Offline analysis of temporary synthetic evidence. No app launch or CDP calls.
// Output is a review candidate, not automatic authorization to publish raw data.
import fs from 'node:fs';
import path from 'node:path';
import { SourceMap } from 'node:module';
import { pathToFileURL } from 'node:url';
import assert from 'node:assert/strict';

export function readCsv(text) {
  const records = []; let row = [], cell = '', quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === '"') {
      if (quoted && text[i + 1] === '"') { cell += '"'; i++; } else quoted = !quoted;
    } else if (!quoted && (c === ',' || c === '\n')) {
      row.push(cell.replace(/\r$/, '')); cell = '';
      if (c === '\n') { records.push(row); row = []; }
    } else if (i !== 0 || c !== '\uFEFF') cell += c;
  }
  if (quoted) throw Error('Unterminated CSV field');
  if (cell || row.length) { row.push(cell.replace(/\r$/, '')); records.push(row); }
  const header = records.shift();
  return records.filter(r => r.length > 1).map(r => {
    if (r.length !== header.length) throw Error('CSV column count mismatch');
    return Object.fromEntries(header.map((key, i) => [key, r[i]]));
  });
}

export function stats(values) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return { count: values.length, first: values[0], last: values.at(-1), min: sorted[0], max: sorted.at(-1),
    mean: values.reduce((a, b) => a + b, 0) / values.length,
    median: sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2 };
}

export function trends(rows) {
  const gauges = ['JSHeapUsedSize', 'JSHeapTotalSize', 'Nodes', 'Documents', 'JSEventListeners'];
  const counters = ['LayoutCount', 'RecalcStyleCount', 'ScriptDuration', 'TaskDuration'];
  const windows = {
    busy_30_300: r => r.idle_seconds === null && r.busy_seconds >= 30 && r.busy_seconds < 300,
    busy_400_600: r => r.idle_seconds === null && r.busy_seconds >= 400 && r.busy_seconds <= 610,
    idle_30_90: r => r.idle_seconds >= 30 && r.idle_seconds < 90,
    idle_130_180: r => r.idle_seconds >= 130 && r.idle_seconds < 180,
    idle_200_250: r => r.idle_seconds >= 200 && r.idle_seconds <= 250
  };
  return Object.fromEntries(Object.entries(windows).map(([name, predicate]) => {
    const selected = rows.filter(predicate), first = selected[0], last = selected.at(-1);
    const wall = selected.length > 1 ? (Date.parse(last.utc) - Date.parse(first.utc)) / 1000 : 0;
    return [name, { samples: selected.length, wall_seconds: wall,
      gauges: Object.fromEntries(gauges.map(key => [key, stats(selected.map(r => r.metrics[key]).filter(Number.isFinite))])),
      rates_per_second: Object.fromEntries(counters.map(key => [key, wall && Number.isFinite(last.metrics[key]) && Number.isFinite(first.metrics[key]) ?
        (last.metrics[key] - first.metrics[key]) / wall : null])) }];
  }));
}

export function processWindow(rows, start, end) {
  const selected = rows.filter(r => Date.parse(r.utc) >= start && Date.parse(r.utc) < end);
  const identities = new Map(), totals = new Map();
  for (const row of selected) {
    const key = `${row.pid},${row.start_utc}`;
    if (!identities.has(key)) identities.set(key, { role: row.role, process_type: row.process_type,
      utility_sub_type: row.utility_sub_type, values: [] });
    identities.get(key).values.push(Number(row.private_bytes));
    totals.set(row.utc, (totals.get(row.utc) ?? 0) + Number(row.private_bytes));
  }
  return { combined: stats([...totals.values()]), processes: [...identities.values()].map(({ values, ...labels }) =>
    ({ ...labels, private_bytes: stats(values) })).sort((a, b) => b.private_bytes.mean - a.private_bytes.mean) };
}

export function mapSites(sites, mapDirectory) {
  const maps = new Map();
  for (const file of fs.readdirSync(mapDirectory).filter(f => f.endsWith('.map'))) {
    maps.set(file.replace(/\.map$/, ''), new SourceMap(JSON.parse(fs.readFileSync(path.join(mapDirectory, file), 'utf8'))));
  }
  return sites.map(site => {
    const sourceMap = maps.get(site.url.split('/').at(-1));
    const entry = sourceMap?.findEntry(site.lineNumber, site.columnNumber);
    return { function: site.functionName || '(anonymous)', self_bytes: site.self_bytes,
      source: entry?.originalSource?.replace(/^(\.\.\/)+/, '') ?? (site.url ? '(unmapped bundle)' : '(native or runtime)'),
      line: entry?.originalLine === undefined ? null : entry.originalLine + 1,
      original_name: entry?.name ?? null };
  });
}

export function attributeDumps(dumps, requests, processRows) {
  // Chromium's JSON exporter renumbers dump GUIDs (observed 0x0..0x6), so
  // match ordered unique timestamps to successful requests and check spacing.
  const times = [...new Set(dumps.map(d => d.timestamp_us))].sort((a, b) => a - b);
  const successful = requests.filter(r => r.result?.success).sort((a, b) => Date.parse(a.utc) - Date.parse(b.utc));
  if (!times.length || times.length !== successful.length) throw Error('Dump/request coverage mismatch');
  const origin = Date.parse(successful[0].utc), traceOrigin = times[0];
  const values = allocator => allocator ? Object.fromEntries(Object.entries(allocator.attributes)
    .filter(([, a]) => a.units === 'bytes').map(([key, a]) => [key, a.value])) : null;
  return times.map((timestamp, index) => {
    const request = successful[index], at = Date.parse(request.utc);
    const spacingError = (timestamp - traceOrigin) / 1000 - (at - origin);
    if (Math.abs(spacingError) > 2000) throw Error('Dump/request timing mismatch');
    const parts = dumps.filter(d => d.timestamp_us === timestamp);
    const sampleTimes = [...new Set(processRows.map(r => r.utc))];
    const nearest = sampleTimes.sort((a, b) => Math.abs(Date.parse(a) - at) - Math.abs(Date.parse(b) - at))[0];
    if (!nearest || Math.abs(Date.parse(nearest) - at) > 2000) throw Error('No nearby process sample');
    const identities = processRows.filter(r => r.utc === nearest);
    return { label: request.label, utc: request.utc, request_spacing_error_ms: spacingError,
      counter_offset_ms: Date.parse(nearest) - at,
      processes: identities.map(identity => {
        const owned = parts.filter(d => d.pid === Number(identity.pid));
        // The exporter attaches a global ownership graph to one arbitrary PID;
        // it is not that process's own allocator and must not be counted there.
        const allocators = owned.flatMap(d => d.allocators).filter(a => a.name !== 'global' && !a.name.startsWith('global/'));
        const lookup = name => allocators.find(a => a.name === name);
        const totals = owned.find(d => d.process_totals)?.process_totals;
        const dumpPrivate = totals?.private_footprint_bytes === undefined ? null : Number.parseInt(totals.private_footprint_bytes, 16);
        const pools = ['malloc', 'blink_gc', 'partition_alloc'].map(name => ({ name, bytes: lookup(name)?.attributes.size?.value ?? null }));
        const isolates = [...new Set(allocators.filter(a => /^v8\/[^/]+\/heap\//.test(a.name))
          .map(a => `${a.name.split('/')[1]}/${a.name.match(/isolate_[^/]+/)?.[0] ?? 'main'}`))];
        const v8 = isolates.map((key, isolate) => {
          const [kind, rawIdentity] = key.split('/');
          const spaces = allocators.filter(a => a.name.startsWith(`v8/${kind}/heap/`) &&
            (rawIdentity === 'main' ? !a.name.includes('isolate_') : a.name.includes(rawIdentity)));
          const sum = attr => spaces.reduce((total, a) => total + (a.attributes[attr]?.value ?? 0), 0);
          return { isolate: isolate + 1, kind, physical_bytes: sum('size'), heap_space_bytes: sum('virtual_size'),
            allocated_objects_bytes: sum('allocated_objects_size') };
        });
        if (v8.length) pools.push({ name: 'v8_heap_spaces', bytes: v8.reduce((s, v) => s + v.physical_bytes, 0) });
        const selected = pools.filter(p => p.bytes !== null);
        const poolTotal = selected.reduce((s, p) => s + p.bytes, 0);
        const families = ['v8', 'blink_gc', 'blink_objects', 'partition_alloc', 'malloc', 'cc', 'skia',
          'discardable', 'web_cache', 'font_caches', 'sqlite', 'leveldatabase', 'shared_memory', 'parkable_images'];
        const partitions = allocators.filter(a => /^(malloc|partition_alloc)\/partitions\/[^/]+$/.test(a.name))
          .map(a => ({ name: a.name, ...values(a) }));
        return { role: identity.role, process_type: identity.process_type, utility_sub_type: identity.utility_sub_type,
          concurrent_counter_private_bytes: Number(identity.private_bytes), dump_private_bytes: dumpPrivate,
          allocator_coverage: selected.length ? 'reported' : 'not_reported',
          selected_pools: selected, selected_pool_total_bytes: poolTotal,
          unattributed_basis_gap_bytes: (dumpPrivate ?? Number(identity.private_bytes)) - poolTotal,
          reconciliation_basis: 'Private bytes minus distinct malloc, Blink GC, Blink PartitionAlloc and V8 heap physical pools; provider size and private commit are different measures. Caches/shared/disk and cross-family ownership are not added.',
          v8_isolates: v8, partitions,
          families: Object.fromEntries(families.map(name => [name, lookup(name) ? values(lookup(name)) : null])),
          blink_heap: values(lookup('blink_gc/main/heap')),
          decoded_image_cache: values(lookup('cc/image_memory')),
          skia_glyph_cache: values(lookup('skia/sk_glyph_cache')),
          web_cache_images: values(lookup('web_cache/Image_resources'))
        };
      }) };
  });
}

function selfTest() {
  assert.deepEqual(readCsv('\uFEFF"a","b"\r\n"one,two","say ""yes"""\r\n'), [{ a: 'one,two', b: 'say "yes"' }]);
  assert.throws(() => readCsv('a,b\n"bad'));
  assert.equal(stats([4, 1, 2, 3]).median, 2.5);
  const rows = [{ utc: '2026-01-01T00:00:00Z', pid: '1', start_utc: 'birth', private_bytes: '5', role: 'renderer' },
    { utc: '2026-01-01T00:00:00Z', pid: '2', start_utc: 'birth', private_bytes: '10', role: 'browser' }];
  const window = processWindow(rows, Date.parse(rows[0].utc), Date.parse(rows[0].utc) + 1000);
  assert.equal(window.combined.max, 15);
  assert.equal(window.processes.length, 2);
  assert.equal(trends([]).busy_30_300.samples, 0);
  const allocator = (name, size) => ({ name, attributes: { size: { value: size, units: 'bytes' } } });
  const attributed = attributeDumps([{ timestamp_us: 1000, id: 'renumbered', pid: 1,
    allocators: [allocator('malloc', 2), allocator('malloc/child', 1), allocator('global', 100)], process_totals: { private_footprint_bytes: '5' } }],
  [{ label: 'synthetic', utc: rows[0].utc, result: { success: true, dumpGuid: 'different' } }], rows);
  assert.equal(attributed[0].processes[0].unattributed_basis_gap_bytes, 3);
  assert.equal(attributed[0].processes[1].allocator_coverage, 'not_reported');
  assert.equal(attributed[0].processes[0].decoded_image_cache, null);
  assert.throws(() => attributeDumps([], [], rows));
  assert.throws(() => attributeDumps([{ timestamp_us: 1000, id: 'x', pid: 1, allocators: [] }],
    [{ utc: '2026-01-01T00:01:00Z', result: { success: true } }], rows), /nearby process/);
  console.log('PASS: CSV escaping, medians, concurrent process windows and missing metrics');
}

function run(root) {
  const read = name => JSON.parse(fs.readFileSync(path.join(root, name), 'utf8').replace(/^\uFEFF/, ''));
  const processRows = readCsv(fs.readFileSync(path.join(root, 'diagnostic/memory-processes.csv'), 'utf8'));
  const events = read('diagnostic/diagnostic-events.json');
  const time = action => Date.parse(events.find(e => e.action === action && !e.failed)?.utc);
  const pressure = time('critical-pressure'), gc = time('collect-garbage');
  const summary = {
    diagnostic_only: true,
    allocator_attribution: attributeDumps(read('diagnostic/allocator-dumps.json'), read('diagnostic/dump-requests.json'), processRows),
    metrics: trends(read('diagnostic/performance-metrics.json')),
    allocation_sites: mapSites(read('diagnostic/allocation-sites.json'), path.join(root, 'source-maps')),
    pressure: { before_30s: processWindow(processRows, pressure - 30000, pressure),
      after_0_30s: processWindow(processRows, pressure, pressure + 30000),
      after_30_60s: processWindow(processRows, pressure + 30000, pressure + 60000) },
    forced_gc: { before_10s: processWindow(processRows, gc - 10000, gc),
      after_0_30s: processWindow(processRows, gc, gc + 30000),
      after_30_60s: processWindow(processRows, gc + 30000, gc + 60000) },
    errors: read('diagnostic/diagnostic-errors.json')
  };
  fs.writeFileSync(path.join(root, 'diagnostic-analysis.json'), JSON.stringify(summary, null, 2));
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv[2] === '--self-test') selfTest();
  else run(process.argv[2]);
}
