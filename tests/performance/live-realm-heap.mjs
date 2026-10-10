// Diagnostic only; caller owns the launch lock and isolated synthetic profile.
// Heap values stay in the temporary directory; summary uses structural labels.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';

const [portText, destination, delayText = '90'] = process.argv.slice(2);
const port = Number(portText), output = fs.realpathSync(destination);
const delay = Number(delayText);
if (!Number.isInteger(port) || port < 1024 || port > 65535
  || !Number.isFinite(delay) || delay < 0 || delay > 120
  || !output.startsWith(fs.realpathSync(os.tmpdir()) + path.sep) || fs.readdirSync(output).length) throw Error('Use a fresh temporary directory and local port');
const deadline = setTimeout(() => process.exit(2), 300_000);
// Let natural post-leave retention settle before the snapshot forces collection.
await new Promise(resolve => setTimeout(resolve, delay * 1000));
const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const target = targets.find(row => row.type === 'page' && /^https?:\/\/tauri\.localhost\//.test(row.url));
if (!target || !/^ws:\/\/(127\.0\.0\.1|localhost):\d+\//.test(target.webSocketDebuggerUrl)) throw Error('Expected local Maxed custom-protocol page');
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
let id = 0, sink;
const pending = new Map();
socket.onmessage = message => {
  const event = JSON.parse(message.data);
  if (event.method === 'HeapProfiler.addHeapSnapshotChunk' && sink !== undefined) fs.writeSync(sink, event.params.chunk);
  if (event.id) {
    const callback = pending.get(event.id); pending.delete(event.id);
    if (callback) event.error ? callback.reject(Error('CDP diagnostic failed')) : callback.resolve(event.result);
  }
};
const call = (method, params = {}) => new Promise((resolve, reject) => {
  const serial = ++id; pending.set(serial, { resolve, reject }); socket.send(JSON.stringify({ id: serial, method, params }));
});
await call('HeapProfiler.enable');
const before = await call('Runtime.getHeapUsage');
const dom = await call('Memory.getDOMCounters');
const file = path.join(output, 'idle.heapsnapshot');
sink = fs.openSync(file, 'wx');
try { await call('HeapProfiler.takeHeapSnapshot', { reportProgress: false }); }
finally { fs.closeSync(sink); sink = undefined; }
const after = await call('Runtime.getHeapUsage');
socket.close(); clearTimeout(deadline);

const heap = JSON.parse(fs.readFileSync(file, 'utf8'));
const nf = heap.snapshot.meta.node_fields, ef = heap.snapshot.meta.edge_fields;
const ns = nf.length, es = ef.length, count = heap.nodes.length / ns;
const ni = key => nf.indexOf(key), ei = key => ef.indexOf(key);
const nt = heap.snapshot.meta.node_types[ni('type')], et = heap.snapshot.meta.edge_types[ei('type')];
const type = index => nt[heap.nodes[index * ns + ni('type')]];
const name = index => heap.strings[heap.nodes[index * ns + ni('name')]];
const bytes = index => heap.nodes[index * ns + ni('self_size')];
const heads = new Int32Array(count).fill(-1), edges = heap.edges.length / es;
const from = new Int32Array(edges), next = new Int32Array(edges), edgeNames = new Int32Array(edges);
const candidates = [], kinds = { fights: 0, snapshots: 0, player_series: 0 };
let edge = 0, shallow = 0, stringBytes = 0, wireStrings = 0, wireBytes = 0;
for (let index = 0; index < count; index++) {
  shallow += bytes(index);
  const label = name(index);
  if (type(index).includes('string')) {
    stringBytes += bytes(index);
    if (label.includes('fight_updated') || label.includes('live-snapshot') || label.includes('runCallback')) {
      wireStrings++; wireBytes += bytes(index);
      if (bytes(index) > 10000) candidates.push({ index, bytes: bytes(index), kind: 'wire-script-string' });
    }
  }
  const props = new Set();
  for (let k = 0, n = heap.nodes[index * ns + ni('edge_count')]; k < n; k++, edge++) {
    const offset = edge * es, kind = et[heap.edges[offset + ei('type')]];
    if (kind === 'weak') continue;
    const destination = heap.edges[offset + ei('to_node')] / ns;
    from[edge] = index; next[edge] = heads[destination]; heads[destination] = edge;
    edgeNames[edge] = ['property','context','internal'].includes(kind) ? heap.edges[offset + ei('name_or_index')] : -1;
    if (kind === 'property') props.add(heap.strings[heap.edges[offset + ei('name_or_index')]]);
  }
  if (props.has('target_generation') && props.has('players')) { kinds.fights++; candidates.push({ index, bytes: bytes(index), kind: 'FightView' }); }
  if (props.has('stream_id') && props.has('fights')) { kinds.snapshots++; candidates.push({ index, bytes: bytes(index), kind: 'LiveSnapshot' }); }
  if (props.has('player_id') && props.has('points')) kinds.player_series++;
}
const safeNames = new Set(['snapshot','wireSnapshot','pendingSnapshot','retained','observed','awaitingFresh','acceptedEvent',
  'fights','visits','window','value','context','system / Context','(GC roots)','(Global handles)','(Stack roots)',
  'script','source','feedback_cell','shared','Script','system / NativeContext','Object','Array','Map','Set']);
const safe = value => safeNames.has(value) ? value : '<structural>';
function rootPath(start) {
  const queue = [start], seen = new Set(queue), toward = new Map();
  let root;
  for (let i = 0; i < queue.length && i < 100000; i++) {
    const node = queue[i];
    if (node === 0 || type(node) === 'synthetic') { root = node; break; }
    for (let e = heads[node]; e !== -1; e = next[e]) {
      const parent = from[e];
      if (!seen.has(parent)) { seen.add(parent); toward.set(parent, { node, edge: e }); queue.push(parent); }
    }
  }
  if (root === undefined) return ['bounded-search-no-root'];
  const result = [];
  for (let node = root, length = 0; length < 30; length++) {
    result.push({ type: type(node), name: safe(name(node)) });
    const step = toward.get(node); if (!step) break;
    result.push({ via: edgeNames[step.edge] < 0 ? '<element>' : safe(heap.strings[edgeNames[step.edge]]) }); node = step.node;
  }
  return result;
}
const retainers = candidates.sort((a, b) => b.bytes - a.bytes).slice(0, 12)
  .map(row => ({ kind: row.kind, shallow_bytes: row.bytes, path: rootPath(row.index) }));
fs.writeFileSync(path.join(output, 'summary.json'), JSON.stringify({ diagnostic_only: true, before, after, dom,
  heap_nodes: count, shallow_bytes: shallow, string_bytes: stringBytes,
  wire_string_count: wireStrings, wire_string_bytes: wireBytes, dto_counts: kinds, retainers }, null, 2));
console.log(JSON.stringify({ diagnostic_only: true, shallow_bytes: shallow, dto_counts: kinds, wire_string_bytes: wireBytes }));
