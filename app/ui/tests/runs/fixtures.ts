import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { vi } from 'vitest';
import Shell from '../../src/lib/shell/Shell.svelte';
import { createClient, type Client } from '../../src/lib/bridge/client';
import { createFixtureTransport, type RunFixtureQueries } from '../../src/lib/bridge/fixture';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { RunPage } from '../../src/lib/bindings/RunPage';
import type { RunView } from '../../src/lib/bindings/RunView';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import type { RunFightSummary } from '../../src/lib/bindings/RunFightSummary';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { PlayerDamage } from '../../src/lib/bindings/PlayerDamage';
import type { SourceDamage } from '../../src/lib/bindings/SourceDamage';
import type { StoreStatus } from '../../src/lib/bindings/StoreStatus';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import { fakeHistory } from '../navigation/fake-history';
import { deferred, homeSnapshot, homeSummary, saved } from '../home/fixtures';
export { deferred, saved };

/** Shared test contract: invented DTOs based on the admitted synthetic t0 shape, never recordings.
 * Real Shell -> RouteFrame -> production components -> createClient -> fixture transport.
 * Runs list controls: Dungeon, Outcome segments, Character, Sort, Date range;
 * Clear filters, Next/Previous; Retry when available; Timeline,
 * Fights, Loot, Sources (named regions); Next/Previous Window within each;
 * Boss-tier timeline links; Other Targets (N) (button).
 * H1s: Runs, Run, Fight, Player. Data rows use data-row-id=run ID.
 * Outcome Evidence is a debug-only focusable button. Timeline entries use short titles;
 * saved explanations are debug-only; source groups expose kind names. Source/bag rows use native list items.
 * Outcome segments send wire enums; Character and Sort selects use wire values.
 * Debug-only estimated source without reasons: "Estimated damage; reconstruction reasons unavailable."
 * Player-row values are labeled DPS, Share, Biggest Hit; each displays a dash
 * for missing/invalid data and 0 (optionally with its unit) for known zero.
 * Asserted status copy patterns (case-insensitive): loading /Loading.*runs/;
 * empty /No.*runs/; read error /Run Error/; contention /busy/;
 * ambiguous identity /ambiguous|reused object/; unavailable nested relationship
 * /Player Unavailable/; cursor cap
 * /100.*previous|previous.*100/. These constrain meaning without exact sentences.
 * No CSS selectors, component imports other than Shell, or private controller seams.
 */
export const labels = { dungeon: 'Dungeon', outcome: 'Outcome', from: 'From', through: 'Through', sort: 'Sort' };
export const ready = <T>(value: T): HistoryReply<T> => ({ status: 'ready', value });
export function run(overrides: Partial<RunView> = {}): RunView {
  return { ...homeSnapshot().run!, id: 'run-1', dungeon: 'Invented Sanctum', started: '1000000',
    ended: '11000000', completed_at: '9000000', died_at: null, outcome: 'cleared',
    evidence: [{ at: '9000000', evidence: 'victory' }], party_size: 4, party_size_estimated: true,
    ...overrides };
}
export function runPage(rows = [run()], next: RunPage['next'] = null): RunPage { return { rows, next }; }
export function source(overrides: Partial<SourceDamage> = {}): SourceDamage {
  return { item_type: 1000, ability: false, kind: 'weapon', enchant: { state: 'recorded_empty', slots: 1, applied: 0, ids: [] },
    damage: 321, hits: 3, biggest_hit: 123, estimated: false, reasons: [], ...overrides };
}
export function player(overrides: Partial<PlayerDamage> = {}): PlayerDamage {
  return { object_id: 7, player_id: 'player-1', name: 'Invented Archer', class_id: 100, is_local: true,
    damage: 321, hits: 3, biggest_hit: 123, deaths: 0, dps: 17, share: 0.23, rank: 4,
    sources: [source()], ...overrides };
}
export function fight(overrides: Partial<FightView> = {}): FightView {
  return { ...homeSnapshot().fights[0], id: 'fight-1', run_id: 'run-1', target_type: 2000,
    boss_type: null, started: '2000000', ended: '8000000', first_hit: '3000000', last_hit: '7000000',
    total_damage: 1400, unattributed_damage: 79, players: [player()], buckets: [], ...overrides };
}
export function fightSummary(index = 1, overrides: Partial<RunFightSummary> = {}): RunFightSummary {
  return { id: `fight-${index}`, target_type: 2000 + index, target_name: `Invented target ${index}`,
    boss_type: null, ordinal: index, started: String(2000000 + index * 1000), ended: '8000000',
    first_hit: '3000000', last_hit: '7000000', total_damage: index * 100, local_damage: 321,
    local_dps: 17, local_rank: 4, player_count: 4, partial: false, estimated: false, ...overrides };
}
export function story(overrides: Partial<RunStory> = {}): RunStory {
  return { run: run(), character: { ...homeSnapshot().character!, name: 'Entry Knight' },
    boss_fights: [fight({ target_type: 2001, target_kind: 'boss' })], events: [], older_run_id: null, newer_run_id: null,
    fights: [fightSummary(1, { target_kind: 'boss' })], drops: [], comparison: { previous_clears: 3, mean_micros: 10000000, change_fraction: -0.2 },
    timeline: [
      { id: 'entry', at: '1000000', kind: 'entered', related_id: 'run-1', explanation: 'Entered Invented Sanctum' },
      { id: 'point-1', at: '2001000', kind: 'fight', related_id: 'fight-1', explanation: 'Encounter one' },
      { id: 'exit', at: '11000000', kind: 'exit', related_id: 'run-1', explanation: 'Observed exit' },
    ], ...overrides };
}
export function largeStory(count: number, bosses = false): RunStory {
  const fights = Array.from({ length: count }, (_, i) => fightSummary(i + 1, { target_kind: bosses ? 'boss' : 'ordinary' }));
  return story({ fights, boss_fights: bosses ? fights.slice(0, 64).map(row => fight({ ...row, target_kind: 'boss' })) : [], timeline: fights.map(f => ({ id: `point-${f.id}`, at: f.started,
    kind: 'fight', related_id: f.id, explanation: `Encounter ${f.ordinal}` })),
    drops: Array.from({ length: 101 }, (_, i) => ({ ...homeSnapshot().drops[0], id: `bag-${i}`,
      run_id: 'run-1', at: String(4000000 + i), contents_known: true, contents: [], items: [] })) });
}

export async function flush() {
  for (let i = 0; i < 6; i++) { await Promise.resolve(); await tick(); }
  await vi.advanceTimersByTimeAsync(0);
  await tick();
}
export async function advance(ms = 1000) { await vi.advanceTimersByTimeAsync(ms); await flush(); }
export const main = () => within(screen.getByRole('main'));
export const region = (name: string) => within(main().getByRole('region', { name }));
export async function click(name: string | RegExp, scope = main()) {
  await fireEvent.click(scope.getByRole('button', { name })); await flush();
}
export async function follow(href: string, scope = main()) {
  const link = scope.getAllByRole('link').find(node => node.getAttribute('href') === href);
  if (!link) throw new Error(`Missing accessible link to ${href}`);
  await fireEvent.click(link); await flush();
}
export async function edit(label: string, value: string) {
  const input = main().getByLabelText(label);
  await fireEvent.input(input, { target: { value } });
  await fireEvent.change(input, { target: { value } }); await flush();
}

export function mountRuns(options: {
  hash?: string; snapshot?: LiveSnapshot; demo?: boolean; queries?: Partial<RunFixtureQueries>;
  home?: Client['queryHome']; setup?: Client['querySetup']; store?: StoreStatus;
} = {}) {
  let snapshot = options.snapshot ?? homeSnapshot({ area_kind: 'nexus', run: null });
  let status = options.store ?? saved(snapshot.sequence, snapshot.stream_id);
  const queries = {
    listRuns: vi.fn<RunFixtureQueries['listRuns']>(options.queries?.listRuns ?? (async () => ready(runPage()))),
    getRun: vi.fn<RunFixtureQueries['getRun']>(options.queries?.getRun ?? (async id => ready(story({ run: run({ id }) })))),
    getRunFightStrip: vi.fn<NonNullable<RunFixtureQueries['getRunFightStrip']>>(options.queries?.getRunFightStrip ?? (async () => ready([]))),
    getFight: vi.fn<RunFixtureQueries['getFight']>(options.queries?.getFight ?? (async id => ready(fight({ id })))),
    getPlayerBreakdown: vi.fn<RunFixtureQueries['getPlayerBreakdown']>(options.queries?.getPlayerBreakdown ?? (async (_, object_id) => ready(player({ object_id })))),
  };
  const transport = createFixtureTransport(snapshot, { client_fingerprint: 'synthetic-c2', objects: [] }, homeSummary(), queries);
  const client = createClient(transport);
  const querySetup = client.querySetup;
  client.querySetup = vi.fn(options.setup ?? (async () => ({ ...await querySetup(), demo: options.demo ?? false })));
  client.queryStore = vi.fn(async () => structuredClone(status));
  client.queryHome = vi.fn(options.home ?? client.queryHome);
  const visuals = vi.spyOn(client, 'getObjectVisuals');
  const listeners = [vi.spyOn(client, 'onSnapshot'), vi.spyOn(client, 'onCapture'), vi.spyOn(client, 'onEngineEvent')];
  const browser = fakeHistory(options.hash ?? '#/runs');
  const page = render(Shell, { client, historyAdapter: browser.adapter });
  return { ...page, queries, client, transport, browser, visuals, listeners,
    get snapshot() { return snapshot; },
    setStore(next: StoreStatus) { status = next; },
    publish(next: LiveSnapshot) { snapshot = next; transport.publishSnapshot(next); },
    event(event: EngineEvent, sequence: string) {
      transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence, at: snapshot.at, event });
    },
  };
}

/** Deterministic native-equivalent admission model, NOT a replacement UI/controller.
 * Completion is explicit: an active SQL statement cannot be cancelled. Latest pending
 * replaces pending; same-key admission retires the active result. Abuse notices are
 * bounded and delivered only on the captured active completion epoch. All five query
 * kinds can submit here, including Home, using canonical command+argument keys.
 */
export function nativeQueue() {
  type Job = { key: string; retired: boolean; resolve: (value: HistoryReply<unknown>) => void; reject: (error: unknown) => void };
  let active: Job | null = null, pending: Job | null = null, notice: Job | null = null;
  let attempts: number[] = [];
  const started: string[] = [];
  const begin = (job: Job) => { active = job; started.push(job.key); };
  return {
    started,
    get activeKey() { return active?.key ?? null; },
    get pendingKey() { return pending?.key ?? null; },
    submit<T>(key: string): Promise<HistoryReply<T>> {
      return new Promise<HistoryReply<unknown>>((resolve, reject) => {
        const job: Job = { key, retired: false, resolve, reject };
        attempts = attempts.filter(at => Date.now() - at < 1000); attempts.push(Date.now());
        if (attempts.length > 32) {
          attempts = attempts.slice(-33);
          notice?.resolve({ status: 'superseded' });
          if (active) notice = job; else resolve({ status: 'busy' });
          return;
        }
        if (!active) { begin(job); return; }
        if (active.key === key) active.retired = true;
        pending?.resolve({ status: 'superseded' }); pending = job;
      }) as Promise<HistoryReply<T>>;
    },
    finish<T>(value: T) {
      if (!active) throw new Error('No native read to finish');
      active.resolve(active.retired ? { status: 'superseded' } : ready(value)); active = null;
      notice?.resolve({ status: 'busy' }); notice = null;
      if (pending) { const next = pending; pending = null; begin(next); }
    },
    fail(error: unknown) {
      if (!active) throw new Error('No native read to fail');
      active.reject(error); active = null; notice?.resolve({ status: 'busy' }); notice = null;
      if (pending) { const next = pending; pending = null; begin(next); }
    },
    retire() {
      if (active) active.retired = true;
      pending?.resolve({ status: 'superseded' }); pending = null;
      notice?.resolve({ status: 'superseded' }); notice = null; attempts = [];
    },
  };
}

const NativeDate = Date;
const midnights: Record<string, string> = {
  '2026-2-8': '2026-03-08T06:00:00Z', '2026-2-9': '2026-03-09T05:00:00Z',
  '2026-10-1': '2026-11-01T05:00:00Z', '2026-10-2': '2026-11-02T06:00:00Z',
};
/** Install after fake timers. UTC parsing stays native; all local getters agree with
 * America/Chicago. Local constructors resolve Chicago offsets, never the host TZ. */
export function injectChicagoDate() {
  const ClockDate = Date; // Capture the installed fake clock, not one frozen instant.
  class ChicagoDate extends NativeDate {
    constructor(value?: string | number, month?: number, day?: number) {
      if (month !== undefined) {
        const base = NativeDate.UTC(Number(value), month, day ?? 1);
        let midnight = base;
        for (let i = 0; i < 3; i++) {
          const parts = new Intl.DateTimeFormat('en-US', { timeZone: 'America/Chicago', timeZoneName: 'longOffset' }).formatToParts(new NativeDate(midnight));
          const offset = parts.find(part => part.type === 'timeZoneName')!.value;
          const match = /GMT([+-])(\d{2}):(\d{2})/.exec(offset)!;
          midnight = base - (match[1] === '-' ? -1 : 1) * (Number(match[2]) * 60 + Number(match[3])) * 60_000;
        }
        super(midnights[`${value}-${month}-${day ?? 1}`] ?? midnight);
      } else super(arguments.length === 0 ? new ClockDate().getTime() : value === undefined ? NaN : value);
    }
    static now() { return ClockDate.now(); }
    private parts(): Intl.DateTimeFormatPart[] {
      return new Intl.DateTimeFormat('en-US', { timeZone: 'America/Chicago', year: 'numeric', month: 'numeric', day: 'numeric' }).formatToParts(this);
    }
    private part(name: string): number {
      return Number.isNaN(this.getTime()) ? NaN : Number(this.parts().find(p => p.type === name)?.value);
    }
    getFullYear(): number { return this.part('year'); }
    getMonth(): number { return this.part('month') - 1; }
    getDate(): number { return this.part('day'); }
  }
  vi.stubGlobal('Date', ChicagoDate);
}
