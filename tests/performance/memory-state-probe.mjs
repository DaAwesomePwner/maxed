// Diagnostic counts from the real state-owner implementations, using only
// invented DTOs. This observes Set sizes; it does not measure WebView memory.
// Requires the repository's installed UI esbuild dependency. No source is edited.
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const require = createRequire(path.join(repo, 'app/ui/package.json'));
const { build } = require('esbuild');
const sets = new Map();
globalThis.__m0WatchSet = (label, values) => {
  const set = new Set(values); sets.set(label, set); return set;
};
const replacements = [
  ['bridge/live.ts', 'const retired = new Set<string>();', "const retired = globalThis.__m0WatchSet('retired');"],
  ['home/v3/dungeon-bests.ts', 'const completed = new Set<string>();', "const completed = globalThis.__m0WatchSet('completed');"],
  ['home/v3/feedback.ts', 'new Set(initial.events.map(event => event.id))', "globalThis.__m0WatchSet('seen', initial.events.map(event => event.id))"],
  ['home/v3/feedback.ts', 'seen = new Set(input.events.map(event => event.id))', "seen = globalThis.__m0WatchSet('seen', input.events.map(event => event.id))"],
];
let observed = 0;
const compiled = await build({ stdin: { contents: `
  export {subscribeLive} from './src/lib/bridge/live';
  export {createClient} from './src/lib/bridge/client';
  export {createFixtureTransport} from './src/lib/bridge/fixture';
  export {createDungeonBests} from './src/lib/home/v3/dungeon-bests';
  export {arrivalFeedback} from './src/lib/home/v3/feedback';
  export {homeSnapshot} from './tests/home/fixtures';`, resolveDir: path.join(repo, 'app/ui'), loader: 'ts' },
  bundle: true, write: false, format: 'esm', platform: 'node', plugins: [{ name: 'observe-size-only', setup(builder) {
    builder.onLoad({ filter: /(?:live|dungeon-bests|feedback)\.ts$/ }, args => {
      let contents = fs.readFileSync(args.path, 'utf8');
      for (const [suffix, before, after] of replacements) if (args.path.replaceAll('\\','/').endsWith(suffix)) {
        assert.equal(contents.split(before).length, 2, `Observation seam changed: ${suffix}`);
        contents = contents.replace(before, after); observed++;
      }
      return { contents, loader: 'ts' };
    });
  } }] });
assert.equal(observed, 4);
const api = await import(`data:text/javascript;base64,${Buffer.from(compiled.outputFiles[0].text).toString('base64')}`);
const flush = async () => { for (let i=0; i<30; i++) await Promise.resolve(); };
const saved = stream_id => ({state:'ready', stream_id, queued:0, durable:'1000', message:''});
let stream = 'm0-stream-0', displayed = 0;
const transport = api.createFixtureTransport(api.homeSnapshot({stream_id:stream}));
const client = {...api.createClient(transport), queryStore: async () => saved(stream)};
const live = api.subscribeLive(client, {snapshot: () => { displayed++; }});
const bests = api.createDungeonBests({queryDungeonBests: async () => ({status:'ready', value:[]})}, () => {});
const feedback = api.arrivalFeedback({}, {scope:'same-visit', events:[]});
const rows = [];
try {
  await flush();
  for (let i=1; i<=1000; i++) {
    stream = `m0-stream-${i}`;
    transport.publishSnapshot(api.homeSnapshot({stream_id:stream}));
    const snapshot = api.homeSnapshot({stream_id:stream});
    snapshot.run = {...snapshot.run, id:`m0-run-${i}`, completed_at:snapshot.at, ended:snapshot.at};
    bests.update(snapshot, null, saved(stream));
    feedback.update({scope:'same-visit', events:[{id:`m0-event-${i}`, targets:()=>[]}]});
    await flush();
    if ([100,500,1000].includes(i)) rows.push({inputs:i, retired_stream_ids:sets.get('retired').size,
      completed_run_ids:sets.get('completed').size, feedback_seen_ids:sets.get('seen').size});
  }
  assert.equal(displayed, 1001, 'All synthetic stream replacements reached the observer');
  assert.deepEqual(rows.map(row => ({retired: row.retired_stream_ids <= 512,
    completed: row.completed_run_ids <= 512, feedback: row.feedback_seen_ids <= 1})),
    [100,500,1000].map(() => ({retired:true, completed:true, feedback:true})), 'Bookkeeping stays bounded');
  // Evicted old streams may request a resync, but only an authoritative query
  // may replace the displayed namespace. Never trust the late notification.
  client.querySnapshot = async () => api.homeSnapshot({stream_id:stream});
  transport.publishSnapshot(api.homeSnapshot({stream_id:'m0-stream-0'}));
  await flush();
  assert.equal(displayed, 1001, 'Evicted stale stream cannot replace current content');
  for (let i=1; i<=2000; i++) {
    const snapshot = api.homeSnapshot({stream_id:stream, sequence:String(i)});
    snapshot.run = {...snapshot.run, id:`same-stream-run-${i}`, completed_at:snapshot.at, ended:snapshot.at};
    bests.update(snapshot, null, {...saved(stream), durable:String(i)});
    await flush();
  }
  assert(sets.get('completed').size <= 512, 'A long single stream stays bounded too');
  feedback.update({scope:'next-visit', events:[{id:'baseline', targets:()=>[]}]});
  assert.equal(sets.get('seen').size, 1, 'Feedback resets at the next scope');
  console.log(JSON.stringify({diagnostic_only:true, rows, feedback_after_scope_change:1,
    limitation:'Set references are intentionally observed in this Node probe. Counts establish lifetime bookkeeping growth, not retained bytes or post-disposal GC behavior in WebView2.'}, null, 2));
} finally {
  live.dispose(); bests.dispose(); feedback.destroy();
  delete globalThis.__m0WatchSet; sets.clear();
}
