// Dependency-free, diagnostic CDP run. Never use its process memory as budget evidence.
// Usage: node memory-profile.mjs <localhost-port> <fresh-temp-directory> [seconds=300]
// Caller owns the launch lock and proves an isolated synthetic app-data profile.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
if (process.argv[2] === '--self-test') {
  const heap = { snapshot: { meta: { node_fields: ['type','name','self_size','detachedness'], node_types: [['object','array','native','string','closure']] } },
    strings: ['Array','(object elements)','HTMLImageElement','private/path/value','Blob'],
    nodes: [0,0,32,0, 1,1,80,0, 2,2,64,2, 3,3,40,0, 3,4,40,0, 4,2,32,0] };
  const result = summarizeHeap(heap);
  if (result.nodes !== 6 || result.shallow_bytes !== 288 || result.array_shallow_bytes !== 80 || result.detached_nodes !== 1
    || result.resources.HTMLImageElement !== 1 || result.resources.Blob !== 0 || JSON.stringify(result).includes('private/path/value')) throw Error('Heap accounting self-test failed');
  console.log('PASS: heap shallow accounting, detached/image counts and string-value redaction');
  process.exit(0);
}
const [portText, destination, secondsText = '300'] = process.argv.slice(2);
const port = Number(portText), seconds = Number(secondsText);
if (!Number.isInteger(port) || port < 1024 || port > 65535 || !Number.isFinite(seconds) || seconds < 10 || seconds > 3600) throw Error('Invalid profiling parameters');
const output = fs.realpathSync(path.resolve(destination ?? ''));
const repo = fs.realpathSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..'));
const within = (parent, child) => {
  const relative = path.relative(parent, child);
  return relative === '' || (relative !== '..' && !relative.startsWith('..' + path.sep) && !path.isAbsolute(relative));
};
if (within(repo, output) || !within(fs.realpathSync(os.tmpdir()), output)
  || !fs.statSync(output).isDirectory() || fs.readdirSync(output).length) throw Error('Use a fresh existing temporary directory outside the repo');
const deadline = setTimeout(() => { console.error('Profiling deadline exceeded'); process.exit(2); }, (seconds + 180) * 1000);
const tabs = await (await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(10000) })).json();
const target = tabs.find(tab => tab.type === 'page' && /^https?:\/\/tauri\.localhost\//.test(tab.url));
if (!target || !/^ws:\/\/(?:127\.0\.0\.1|localhost):\d+\//.test(target.webSocketDebuggerUrl)) throw Error('No local custom-protocol Maxed target');
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(Error('CDP connection timeout')), 10000);
  socket.onopen = () => { clearTimeout(timer); resolve(); };
  socket.onerror = () => { clearTimeout(timer); reject(Error('CDP connection failed')); };
});
let serial = 0, snapshotSink;
const pending = new Map(), sprites = [], routeVisits = [], samples = [];
const spriteRequests = new Map(), cachedRequests = new Set(), imageTrace = {};
const imageEvents = new Set(['Decode Image','ImageDecodeTask','PaintImage','Decode LazyPixelRef','Draw LazyPixelRef']);
let traceComplete;
socket.onmessage = event => {
  const message = JSON.parse(event.data);
  if (message.id) {
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id); clearTimeout(request.timer);
    message.error ? request.reject(Error(message.error.message)) : request.resolve(message.result);
  } else if (message.method === 'HeapProfiler.addHeapSnapshotChunk' && snapshotSink) {
    fs.writeSync(snapshotSink, message.params.chunk);
  } else if (message.method === 'Network.responseReceived') {
    const response = message.params.response;
    if (response.url.startsWith('http://maxed-sprite.localhost/')) {
      const cache = Object.entries(response.headers).find(([key]) => key.toLowerCase() === 'cache-control')?.[1];
      const name = new URL(response.url).pathname.split('/').at(-1);
      const row = { sheet: ['characters.png','characters_masks.png','groundTiles.png','mapObjects.png','enchantPips.png'].includes(name) ? name : 'other',
        status: response.status, cache_control: String(cache ?? '').slice(0,80),
        disk_cache: !!response.fromDiskCache, served_from_cache: cachedRequests.has(message.params.requestId),
        service_worker: !!response.fromServiceWorker, encoded_bytes: null };
      sprites.push(row); spriteRequests.set(message.params.requestId, row);
    }
  } else if (message.method === 'Network.requestServedFromCache') {
    cachedRequests.add(message.params.requestId);
    const row = spriteRequests.get(message.params.requestId); if (row) row.served_from_cache = true;
  } else if (message.method === 'Network.loadingFinished') {
    const row = spriteRequests.get(message.params.requestId); if (row) row.encoded_bytes = message.params.encodedDataLength;
  } else if (message.method === 'Tracing.dataCollected') {
    for (const entry of message.params.value) if (imageEvents.has(entry.name)) {
      const row = imageTrace[entry.name] ??= { count: 0, duration_us: 0 };
      row.count++; row.duration_us += entry.dur ?? 0;
    }
  } else if (message.method === 'Tracing.tracingComplete') {
    traceComplete?.();
  }
};
const call = (method, params = {}) => new Promise((resolve, reject) => {
  const id = ++serial;
  const timer = setTimeout(() => { pending.delete(id); reject(Error(`CDP timeout: ${method}`)); }, 60000);
  pending.set(id, { resolve, reject, timer }); socket.send(JSON.stringify({ id, method, params }));
});
const evaluate = async expression => {
  const result = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) throw Error('Synthetic route workload evaluation failed');
  return result.result.value;
};
function summarizeSnapshot(filename) {
  return summarizeHeap(JSON.parse(fs.readFileSync(filename, 'utf8')));
}
function summarizeHeap(heap) {
  const fields = heap.snapshot.meta.node_fields, stride = fields.length, types = heap.snapshot.meta.node_types[0];
  const typeAt = fields.indexOf('type'), nameAt = fields.indexOf('name'), sizeAt = fields.indexOf('self_size'), detachedAt = fields.indexOf('detachedness');
  const groups = new Map(); let detached = 0, arrayBytes = 0, totalBytes = 0;
  const resources = { HTMLImageElement: 0, ImageBitmap: 0, Blob: 0, ArrayBuffer: 0, EventListener: 0, V8EventListener: 0 };
  for (let offset = 0; offset < heap.nodes.length; offset += stride) {
    const type = types[heap.nodes[offset + typeAt]], bytes = heap.nodes[offset + sizeAt];
    const raw = heap.strings[heap.nodes[offset + nameAt]];
    // Summaries contain type names only, never string values, paths or script bodies.
    const name = ['object','native','closure','array'].includes(type) && /^[A-Za-z0-9_.$ ()<>:-]{1,100}$/.test(raw) ? raw : type;
    const key = `${type}:${name}`, row = groups.get(key) ?? { type, name, count: 0, shallow_bytes: 0 };
    row.count++; row.shallow_bytes += bytes; groups.set(key, row);
    if (detachedAt >= 0 && heap.nodes[offset + detachedAt] === 2) detached++;
    if (type === 'array') arrayBytes += bytes;
    // A string containing "Blob" or a constructor closure is not a Blob instance.
    if (type === 'object' || type === 'native') for (const resource of Object.keys(resources)) {
      if (raw === resource || raw === `system / ${resource}` || raw.startsWith(`${resource} / `)) resources[resource]++;
    }
    totalBytes += bytes;
  }
  const dtoShapes = { live_snapshots: 0, fight_views: 0, player_series: 0, run_stories: 0, run_fight_summaries: 0 };
  const edgeFields = heap.snapshot.meta.edge_fields;
  if (edgeFields && heap.edges) {
    const edgeStride = edgeFields.length, edgeTypeAt = edgeFields.indexOf('type'), edgeNameAt = edgeFields.indexOf('name_or_index');
    const edgeTypes = heap.snapshot.meta.edge_types[0], edgeCountAt = fields.indexOf('edge_count');
    let edgeOffset = 0;
    for (let nodeOffset=0; nodeOffset<heap.nodes.length; nodeOffset+=stride) {
      const properties = new Set();
      for (let i=0; i<heap.nodes[nodeOffset+edgeCountAt]; i++, edgeOffset+=edgeStride) {
        if (edgeTypes[heap.edges[edgeOffset+edgeTypeAt]] === 'property') properties.add(heap.strings[heap.edges[edgeOffset+edgeNameAt]]);
      }
      if (['stream_id','sequence','fights','capture'].every(key=>properties.has(key))) dtoShapes.live_snapshots++;
      if (['target_type','players','buckets','started'].every(key=>properties.has(key))) dtoShapes.fight_views++;
      if (['player_id','points'].every(key=>properties.has(key))) dtoShapes.player_series++;
      if (['run','fights','drops','timeline','comparison'].every(key=>properties.has(key))) dtoShapes.run_stories++;
      if (['ordinal','target_name','player_count','local_damage','started'].every(key=>properties.has(key))) dtoShapes.run_fight_summaries++;
    }
  }
  return { nodes: heap.nodes.length / stride, shallow_bytes: totalBytes, array_shallow_bytes: arrayBytes,
    detached_nodes: detached, resources, dto_shapes: dtoShapes, groups: [...groups.values()].sort((a,b) => b.shallow_bytes-a.shallow_bytes) };
}
async function snapshot(label) {
  await call('HeapProfiler.collectGarbage');
  const filename = path.join(output, `${label}.heapsnapshot`);
  snapshotSink = fs.openSync(filename, 'wx');
  try { await call('HeapProfiler.takeHeapSnapshot', { reportProgress: false, captureNumericValue: true }); }
  finally { fs.closeSync(snapshotSink); snapshotSink = undefined; }
  return summarizeSnapshot(filename);
}
async function sample(elapsed) {
  const [heap, dom, images] = await Promise.all([
    call('Runtime.getHeapUsage'), call('Memory.getDOMCounters'),
    evaluate(`({image_elements:document.images.length,loaded_images:[...document.images].filter(i=>i.complete&&i.naturalWidth>0).length,unique_image_sources:new Set([...document.images].map(i=>i.currentSrc)).size,blob_image_elements:[...document.images].filter(i=>i.currentSrc.startsWith('blob:')).length,sprite_resources:performance.getEntriesByType('resource').filter(r=>r.name.startsWith('http://maxed-sprite.localhost/')).length})`),
  ]);
  samples.push({ elapsed_seconds: elapsed, ...heap, ...dom, ...images });
}
try {
  await call('Runtime.enable'); await call('HeapProfiler.enable'); await call('Network.enable');
  // Never query settings or owner DB. The caller must build an isolated synthetic profile.
  const proof = await evaluate(`({ready:!!document.querySelector('a[href="#/"]'),tauri:!!window.__TAURI_INTERNALS__})`);
  if (!proof.ready || !proof.tauri) throw Error('Real UI rail not ready');
  const viewport = await evaluate(`({width:innerWidth,height:innerHeight,dpr:devicePixelRatio,hidden:document.hidden})`);
  // Diagnostic-only extra listener: retain scalar counts and one current stream,
  // never payloads or an ever-growing list of namespaces.
  await evaluate(`(async()=>{
    const stats={snapshots:0,stream_transitions:0,max_fights:0,max_buckets:0,max_player_series_points:0};
    let current;
    const handler=window.__TAURI_INTERNALS__.transformCallback(event=>{
      const p=event.payload; stats.snapshots++;
      if(current!==p.stream_id){stats.stream_transitions++;current=p.stream_id;}
      stats.max_fights=Math.max(stats.max_fights,p.fights.length);
      for(const f of p.fights){stats.max_buckets=Math.max(stats.max_buckets,f.buckets.length);
        for(const s of f.player_series??[])stats.max_player_series_points=Math.max(stats.max_player_series_points,s.points.length);}
    });
    const id=await window.__TAURI_INTERNALS__.invoke('plugin:event|listen',{event:'live-snapshot',target:{kind:'Any'},handler});
    window.__m0MemoryProbe={stats,id,handler}; return true;
  })()`);
  // Compare the same route. Namespace
  // transitions are still a workload confound and are reported by the caller.
  await evaluate(`document.querySelector('a[href="#/"]')?.click()`);
  await new Promise(resolve => setTimeout(resolve, 1500));
  const early = await snapshot('early'); await sample(0);
  await call('HeapProfiler.startSampling', { samplingInterval: 32768, includeObjectsCollectedByMajorGC: true, includeObjectsCollectedByMinorGC: true });
  await call('Tracing.start', { categories: 'devtools.timeline,disabled-by-default-devtools.timeline', transferMode: 'ReportEvents' });
  let tracing = true;
  async function stopTrace() {
    const completed = new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(Error('Image trace timeout')), 60000);
      traceComplete = () => { clearTimeout(timer); resolve(); };
    });
    await call('Tracing.end'); await completed; tracing = false;
  }
  const started = Date.now();
  let cycle = 0;
  while ((Date.now() - started) / 1000 < seconds) {
    const href = ['#/','#/runs','#/settings','#/runs'][cycle % 4];
    const visited = await evaluate(`(()=>{const link=[...document.querySelectorAll('a[href]')].find(a=>a.getAttribute('href')===${JSON.stringify(href)});if(!link)return false;link.click();return true})()`);
    await new Promise(resolve => setTimeout(resolve, 1000));
    const arrived = await evaluate(`location.hash===${JSON.stringify(href)} && !!document.querySelector('main')`);
    routeVisits.push({ route: href, invoked: visited, arrived });
    if (cycle % 4 === 3) {
      const story = await evaluate(`(()=>{const link=[...document.querySelectorAll('a[href]')].find(a=>{const h=a.getAttribute('href');return h.startsWith('#/runs/')&&h.split('/').length===3});if(!link)return false;link.click();return true})()`);
      await new Promise(resolve => setTimeout(resolve, 1500));
      const arrived = await evaluate(`/^#\\/runs\\/[^/]+$/.test(location.hash) && !!document.querySelector('main')`);
      const rendered = await evaluate(`({summary:!!document.querySelector('[data-story-header]'),visible_rows:document.querySelectorAll('[data-row-id]').length})`);
      routeVisits.push({ route: 'Run Story', invoked: story, arrived, rendered });
    }
    if (cycle % 10 === 0) await sample((Date.now() - started) / 1000);
    await new Promise(resolve => setTimeout(resolve, 2000)); cycle++;
    if (tracing && cycle >= 10) await stopTrace();
  }
  if (tracing) await stopTrace();
  const allocations = await call('HeapProfiler.stopSampling');
  const routeImageTrace = structuredClone(imageTrace);
  const allocationSizes = [];
  const allocationSites = [];
  const visit = node => {
    allocationSizes.push(node.selfSize);
    if (node.selfSize) allocationSites.push({
      function: /^[A-Za-z0-9_.$ ()<>:-]{0,80}$/.test(node.callFrame.functionName) ? node.callFrame.functionName : 'other',
      source: /^https?:\/\/tauri\.localhost\//.test(node.callFrame.url) ? 'app-script' : 'runtime-or-probe',
      line: node.callFrame.lineNumber, column: node.callFrame.columnNumber, sampled_bytes: node.selfSize,
    });
    node.children?.forEach(visit);
  };
  visit(allocations.profile.head);
  await evaluate(`document.querySelector('a[href="#/"]')?.click()`);
  await new Promise(resolve => setTimeout(resolve, 1500));
  const late = await snapshot('late'); await sample((Date.now() - started) / 1000);
  const workload = await evaluate(`window.__m0MemoryProbe.stats`);
  // Separate diagnostic microprobe after the route comparison. The ordinary
  // demo catalog has no production atlas fingerprint, so that case is skipped.
  // This creates/removes the same clipped whole-sheet <img> shape as SpriteImage,
  // without changing app state or reading settings/history.
  const spriteStart = sprites.length;
  for (const key of Object.keys(imageTrace)) delete imageTrace[key];
  await call('Tracing.start', { categories: 'devtools.timeline,disabled-by-default-devtools.timeline', transferMode: 'ReportEvents' });
  const spriteProbe = await evaluate(`(async()=>{
    let page;try{page=await window.__TAURI_INTERNALS__.invoke('get_object_visuals',{objectTypes:[782,768,775]});}catch{return {result:'unavailable'};}
    const sprite=page.objects.find(o=>o.sprite?.sheet==='characters.png')?.sprite;
    if(!sprite||!/^[a-f0-9]{64}$/.test(page.client_fingerprint))return {result:'no-atlas-in-current-namespace'};
    const url='http://maxed-sprite.localhost/'+page.client_fingerprint+'/'+sprite.sheet;
    let decoded=0,width=0,height=0;
    for(let i=0;i<12;i++){
      const frame=document.createElement('span'),img=document.createElement('img');
      frame.style.cssText='position:fixed;left:0;top:0;width:32px;height:32px;overflow:hidden;z-index:99999';
      img.style.cssText='position:absolute;max-width:none;transform-origin:top left;image-rendering:pixelated;transform:translate('+(-sprite.x*4)+'px,'+(-sprite.y*4)+'px) scale(4)';
      frame.append(img);document.body.append(frame);
      let timer;
      try{img.src=url;await Promise.race([img.decode(),new Promise((_,reject)=>{timer=setTimeout(()=>reject(Error('Image timeout')),8000)})]);
        decoded++;width=img.naturalWidth;height=img.naturalHeight;await new Promise(resolve=>setTimeout(resolve,250));
      }finally{clearTimeout(timer);frame.remove();}
      await new Promise(resolve=>setTimeout(resolve,250));
    }
    return {result:'completed',mounts:12,decoded,width,height,sheet:sprite.sheet};
  })()`);
  await stopTrace();
  const afterSprite = await snapshot('after-sprite'); await sample((Date.now() - started) / 1000);
  await evaluate(`(async()=>{const p=window.__m0MemoryProbe;window.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener('live-snapshot',p.id);await window.__TAURI_INTERNALS__.invoke('plugin:event|unlisten',{event:'live-snapshot',eventId:p.id});window.__TAURI_INTERNALS__.unregisterCallback?.(p.handler);delete window.__m0MemoryProbe;return true})()`);
  const earlyGroups = new Map(early.groups.map(row => [`${row.type}:${row.name}`,row]));
  const lateGroups = new Map(late.groups.map(row => [`${row.type}:${row.name}`,row]));
  const growth = [...new Set([...earlyGroups.keys(), ...lateGroups.keys()])].map(key => {
    const before = earlyGroups.get(key), after = lateGroups.get(key);
    return { type: (after ?? before).type, name: (after ?? before).name,
      count_delta: (after?.count ?? 0) - (before?.count ?? 0),
      shallow_delta: (after?.shallow_bytes ?? 0) - (before?.shallow_bytes ?? 0) };
  }).sort((a,b) => b.shallow_delta-a.shallow_delta);
  fs.writeFileSync(path.join(output, 'profile-summary.json'), JSON.stringify({ diagnostic_only: true, seconds,
    viewport, workload, early, late, growth, after_sprite: afterSprite, sprite_probe: {...spriteProbe, responses:sprites.length-spriteStart},
    samples, routeVisits, sprite_responses: sprites, image_trace_first_ten_cycles: routeImageTrace, image_trace_sprite_probe: imageTrace,
    sampled_allocation_bytes: allocationSizes.reduce((a,b)=>a+b,0),
    sampled_allocation_sites: allocationSites.sort((a,b)=>b.sampled_bytes-a.sampled_bytes).slice(0,30),
    limitations: 'Post-GC shallow sizes are not dominator retained sizes. Native decode/GPU memory is outside JS heap. No observed sprite requests cannot rule out a sprite leak. Caller must verify isolated build and browser args.' }, null, 2), { flag: 'wx' });
  console.log(JSON.stringify({ early_bytes: early.shallow_bytes, late_bytes: late.shallow_bytes,
    early_detached: early.detached_nodes, late_detached: late.detached_nodes, route_visits: routeVisits.length,
    successful_visits: routeVisits.filter(row=>row.invoked && row.arrived).length, sprite_responses: sprites.length }));
} finally {
  clearTimeout(deadline);
  for (const request of pending.values()) { clearTimeout(request.timer); request.reject(Error('CDP session closed')); }
  pending.clear();
  socket.close();
}
