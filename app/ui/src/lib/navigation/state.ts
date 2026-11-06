import type { StatsSessionCursor } from '../bindings/StatsSessionCursor';
import { tick } from 'svelte';
import type { RunListSummary } from '../bindings/RunListSummary';
import type { RunFilter } from '../bindings/RunFilter';
import type { RunCursor } from '../bindings/RunCursor';
import type { AlertFilter } from '../bindings/AlertFilter';
import type { AlertCursor } from '../bindings/AlertCursor';
import type { DropFilter } from '../bindings/DropFilter';
import type { DropCursor } from '../bindings/DropCursor';
import type { ChatFilter } from '../bindings/ChatFilter';
import type { ChatCursor } from '../bindings/ChatCursor';
import type { EventFilter } from '../bindings/EventFilter';
import type { EventCursor } from '../bindings/EventCursor';
import { alertKinds } from '../alerts/filters';
import type { AlertKind } from '../bindings/AlertKind';
import type { TimeRange } from '../bindings/TimeRange';
import { dateRange, validRunCursor, validRunFilter } from '../runs/filters';

export type StatsRange = 'all' | '30d' | '7d';
export type StatsMapSort = 'map' | 'visits' | 'time' | 'fame' | 'rate' | 'clears' | 'deaths';
export interface StatsPosition {
  range: StatsRange;
  characterId: string | null;
  mapSort: { key: StatsMapSort; descending: boolean };
  minMapRuns: number;
  includeEmpty: boolean;
  sessions: { cursor: StatsSessionCursor | null; previous: Array<StatsSessionCursor | null> };
}
export const defaultStatsPosition = (): StatsPosition => ({
  range: 'all', characterId: null, mapSort: { key: 'rate', descending: true }, minMapRuns: 5, includeEmpty: false,
  sessions: { cursor: null, previous: [] },
});
const statsSorts: StatsMapSort[] = ['map', 'visits', 'time', 'fame', 'rate', 'clears', 'deaths'];
const statsCursor = (value: unknown): value is StatsSessionCursor | null => value === null
  || (record(value) && timestamp(value.started) && safeId(value.id));
const copyStatsCursor = (value: StatsSessionCursor | null) => value === null ? null : { started: value.started, id: value.id };

function statsPosition(value: unknown): StatsPosition | null {
  if (!record(value) || !record(value.mapSort) || !record(value.sessions) || !Array.isArray(value.sessions.previous)) return null;
  const sort = value.mapSort, pages = value.sessions;
  if (!['all', '30d', '7d'].includes(value.range as string)) return null;
  if (value.characterId !== null && !safeId(value.characterId)) return null;
  if (!statsSorts.includes(sort.key as StatsMapSort) || typeof sort.descending !== 'boolean') return null;
  if (typeof value.includeEmpty !== 'boolean' || !statsCursor(pages.cursor) || !Array.isArray(pages.previous) || !pages.previous.every(statsCursor)) return null;
  return {
    range: value.range as StatsRange,
    characterId: value.characterId as string | null,
    mapSort: { key: sort.key as StatsMapSort, descending: sort.descending },
    minMapRuns: typeof value.minMapRuns === 'number' && Number.isInteger(value.minMapRuns) && value.minMapRuns >= 1 && value.minMapRuns <= 10000 ? value.minMapRuns : 5,
    includeEmpty: value.includeEmpty,
    sessions: { cursor: copyStatsCursor(pages.cursor as StatsSessionCursor | null),
      previous: (pages.previous as Array<StatsSessionCursor | null>).slice(-100).map(copyStatsCursor) },
  };
}

export interface RunsPosition { previous: Array<RunCursor | null>; offset?: number; summary?: RunListSummary | null }
// window is retained only for the flag-off standalone Alerts route. Logs uses tile/selectedId.
export interface AlertsPosition { filter: AlertFilter; previous: Array<AlertCursor | null>; window?: number; tile?: 'all' | 'white' | 'pb' | 'keys' | 'messages'; selectedId?: string | null }
export interface LootPosition {
  range: TimeRange | null;
  feed: { filter: DropFilter; previous: Array<DropCursor | null>; selectedDropId?: string | null; seedApplied?: boolean };
  item: { type: number; previous: Array<DropCursor | null> } | null;
  statsRange: { first: string; last: string } | null;
  statsSort?: { key: string; descending: boolean } | null;
  collectionDungeon: string | null;
  collectionSort?: 'seen' | 'recent' | 'name' | null;
}
export interface LogsPosition {
  chat: { filter: ChatFilter; previous: Array<ChatCursor | null>; selectedId?: string | null };
  whisper: { query: string; showHidden: boolean; partner: string | null };
  keys: { filter: EventFilter; previous: Array<EventCursor | null>; selectedId?: string | null };
  events: { filter: EventFilter; previous: Array<EventCursor | null>; selectedId?: string | null };
  range: TimeRange | null;
}
export const defaultLogsPosition = (): LogsPosition => ({
  chat: { filter: { channel: null, sender: null, query: '', run_id: null, show_npc: false, show_hidden: false, cursor: null, limit: 50 }, previous: [] },
  whisper: { query: '', showHidden: false, partner: null },
  keys: { filter: { kinds: ['portal_opened'], run_id: null, dungeon: null, cursor: null, limit: 50 }, previous: [] },
  events: { filter: { kinds: [], run_id: null, dungeon: null, cursor: null, limit: 50 }, previous: [] },
  range: null,
});
export interface FightPosition { playerWindow: number; selectedPlayerId: string | null; sourceWindow: number; plotted?: string[] | null }
export interface StoryPosition { selectedTab?: string | null; allFights?: boolean; timelineWindow?: number; fightWindow?: number; lootWindow?: number; otherTargets?: boolean; otherFightWindow?: number; timelinePointId?: string | null }

export interface ListState {
  homeScope: 'session' | 'today' | null;
  filters: RunFilter;
  scrollTop: number;
  focusedRowId: string | null;
  runsPosition: RunsPosition | null;
  selectedFightId: string | null;
  storyPosition?: StoryPosition | null;
  alertsPosition?: AlertsPosition | null;
  lootPosition?: LootPosition | null;
  statsPosition?: StatsPosition | null;
  logsPosition?: LogsPosition | null;
  runsWindow?: number;
  fightPosition?: FightPosition | null;
}
export const defaultFilters = (): RunFilter => ({ dungeon: null, outcome: null, character_id: null, range: null, sort: 'newest', cursor: null, limit: 25 });
export const emptyListState = (): ListState => ({ homeScope: null, filters: defaultFilters(), scrollTop: 0, focusedRowId: null, runsPosition: null, selectedFightId: null, storyPosition: null, alertsPosition: null, lootPosition: null, logsPosition: null, statsPosition: null, runsWindow: 0, fightPosition: null });

const record = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === 'object' && !Array.isArray(value);
const windowIndex = (value: unknown) => typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : 0;
const safeId = (id: unknown): id is string => typeof id === 'string' && id.length > 0
  && new TextEncoder().encode(id).length <= 256 && !/[\u0000-\u001f\u007f-\u009f]/.test(id);
// Native history timestamps are nonnegative decimal microseconds within SQLite i64.
const timestamp = (value: unknown): value is string => typeof value === 'string' && value.length <= 19
  && /^(0|[1-9]\d*)$/.test(value) && BigInt(value) <= 9223372036854775807n;
const validAlertCursor = (value: unknown): value is AlertCursor | null => value === null
  || (record(value) && timestamp(value.at) && safeId(value.id));
const copyAlertCursor = (value: AlertCursor | null): AlertCursor | null => value === null ? null : { at: value.at, id: value.id };

function alertsPosition(value: unknown): AlertsPosition | null {
  if (!record(value) || !record(value.filter) || !Array.isArray(value.previous)) return null;
  const filter = value.filter, range = filter.range, kind = filter.kind;
  if (kind !== null && !alertKinds.some(allowed => allowed === kind)) return null;
  if (range !== null && (!record(range) || !timestamp(range.start) || !timestamp(range.end) || BigInt(range.start) >= BigInt(range.end))) return null;
  if (typeof filter.query !== 'string') return null;
  const query = filter.query.trim();
  if ([...query].length > 128 || new TextEncoder().encode(query).length > 512 || query.includes('\u0000')) return null;
  if (!validAlertCursor(filter.cursor) || !value.previous.every(validAlertCursor)
    || typeof filter.limit !== 'number' || !Number.isInteger(filter.limit) || filter.limit < 1 || filter.limit > 100) return null;
  for (const kinds of [filter.kinds, filter.exclude_kinds]) {
    if (kinds !== undefined && (!Array.isArray(kinds) || kinds.length > alertKinds.length || !kinds.every(kind => alertKinds.includes(kind)))) return null;
  }
  if (kind !== null && Array.isArray(filter.kinds) && filter.kinds.length) return null;
  // Retain only the query contract, never extra fields or rendered page payloads.
  return {
    filter: { kind: kind as AlertKind | null, range: range === null ? null : { start: range.start as string, end: range.end as string },
      query, cursor: copyAlertCursor(filter.cursor), limit: 50,
      ...(filter.kinds === undefined ? {} : { kinds: [...filter.kinds as AlertKind[]] }),
      ...(filter.exclude_kinds === undefined ? {} : { exclude_kinds: [...filter.exclude_kinds as AlertKind[]] }) },
    previous: value.previous.slice(-100).map(copyAlertCursor),
    ...(value.window === undefined ? {} : { window: windowIndex(value.window) }),
    ...(value.tile === undefined ? {} : { tile: ['all', 'white', 'pb', 'keys', 'messages'].includes(value.tile as string) ? value.tile as AlertsPosition['tile'] : 'all' }),
    ...(value.selectedId === undefined ? {} : { selectedId: safeId(value.selectedId) ? value.selectedId : null }),
  };
}

const boundedInteger = (value: unknown, min: number, max: number): value is number =>
  typeof value === 'number' && Number.isInteger(value) && value >= min && value <= max;
// Drop and alert cursors share the same timestamp/id wire contract.
const dropCursor = (value: unknown): DropCursor | null => validAlertCursor(value) ? copyAlertCursor(value) : null;
const dropPrevious = (value: unknown): Array<DropCursor | null> =>
  Array.isArray(value) && value.every(validAlertCursor) ? value.slice(-100).map(copyAlertCursor) : [];
const calendarDay = (value: unknown): value is string => {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return false;
  const [year, month, day] = value.split('-').map(Number);
  const date = new Date(Date.UTC(year, month - 1, day));
  return year >= 1 && date.getUTCFullYear() === year && date.getUTCMonth() === month - 1 && date.getUTCDate() === day;
};

function lootPosition(value: unknown): LootPosition | null {
  if (!record(value)) return null;
  const feed = record(value.feed) ? value.feed : {};
  const filter = record(feed.filter) ? feed.filter : {};
  const place = filter.place, range = filter.range, item = value.item, statsRange = value.statsRange;
  const validRange = (candidate: unknown): TimeRange | null => record(candidate) && timestamp(candidate.start)
    && timestamp(candidate.end) && BigInt(candidate.start) < BigInt(candidate.end)
    ? { start: candidate.start, end: candidate.end } : null;
  let sharedRange = validRange(value.range);
  if (value.range === undefined) {
    sharedRange = validRange(range);
    if (!sharedRange && record(statsRange) && calendarDay(statsRange.first) && calendarDay(statsRange.last)) {
      try { sharedRange = dateRange(statsRange.first, statsRange.last); } catch { /* Invalid legacy dates have no range. */ }
    }
  }
  return {
    range: sharedRange,
    ...(value.statsSort === undefined ? {} : { statsSort: record(value.statsSort)
      && ['dungeon', 'runs_ended', 'clears', 'bags', 'whites', 'rate', 'runs_since_last_white', 'best'].includes(value.statsSort.key as string)
      && typeof value.statsSort.descending === 'boolean' ? { key: value.statsSort.key as string, descending: value.statsSort.descending } : null }),
    feed: {
      filter: {
        min_rank: boundedInteger(filter.min_rank, 0, 255) ? filter.min_rank : null,
        place: record(place) && place.kind === 'outside' ? { kind: 'outside' }
          : record(place) && place.kind === 'dungeon' && safeId(place.name) ? { kind: 'dungeon', name: place.name } : null,
        item_type: boundedInteger(filter.item_type, 0, 65535) ? filter.item_type : null,
        enchanted_only: filter.enchanted_only === true,
        range: record(range) && timestamp(range.start) && timestamp(range.end) && BigInt(range.start) < BigInt(range.end)
          ? { start: range.start, end: range.end } : null,
        cursor: dropCursor(filter.cursor),
        limit: boundedInteger(filter.limit, 1, 50) ? filter.limit : 50,
      },
      previous: dropPrevious(feed.previous),
      selectedDropId: safeId(feed.selectedDropId) ? feed.selectedDropId : null,
      ...(feed.seedApplied === true ? { seedApplied: true } : {}),
    },
    item: record(item) && boundedInteger(item.type, 0, 65535) ? { type: item.type, previous: dropPrevious(item.previous) } : null,
    statsRange: record(statsRange) && calendarDay(statsRange.first) && calendarDay(statsRange.last)
      && statsRange.first <= statsRange.last
      ? { first: statsRange.first, last: statsRange.last } : null,
    ...(value.collectionSort === undefined ? {} : { collectionSort: value.collectionSort === 'seen' || value.collectionSort === 'recent' || value.collectionSort === 'name' ? value.collectionSort : null }),
    collectionDungeon: safeId(value.collectionDungeon) ? value.collectionDungeon : null,
  };
}

const logsQuery = (value: unknown): value is string => typeof value === 'string'
  && [...value].length <= 128 && new TextEncoder().encode(value).length <= 512 && !value.includes('\u0000');
const logsName = (value: unknown): value is string => safeId(value) && new TextEncoder().encode(value).length <= 32;
function logsChat(value: unknown): LogsPosition['chat'] | null {
  if (!record(value) || !record(value.filter) || !Array.isArray(value.previous)) return null;
  const cf = value.filter;
  if (cf.channel !== null && !['public', 'guild', 'party', 'whisper', 'unknown'].includes(cf.channel as string)) return null;
  if (cf.sender !== null && (!safeId(cf.sender) || [...cf.sender].length > 32 || new TextEncoder().encode(cf.sender).length > 128)) return null;
  if (!logsQuery(cf.query) || typeof cf.show_npc !== 'boolean' || typeof cf.show_hidden !== 'boolean') return null;
  if ((cf.run_id !== null && !safeId(cf.run_id)) || !validAlertCursor(cf.cursor) || !boundedInteger(cf.limit, 1, 100)
    || !value.previous.every(validAlertCursor)) return null;
  return { filter: { channel: cf.channel as ChatFilter['channel'], sender: cf.sender as string | null,
    query: cf.query, run_id: cf.run_id as string | null, show_npc: cf.show_npc, show_hidden: cf.show_hidden,
    cursor: copyAlertCursor(cf.cursor), limit: 50 }, previous: value.previous.slice(-100).map(copyAlertCursor),
    ...(value.selectedId === undefined ? {} : { selectedId: safeId(value.selectedId) ? value.selectedId : null }) };
}
function logsEvents(value: unknown): LogsPosition['events'] | null {
  if (!record(value) || !record(value.filter) || !Array.isArray(value.previous)) return null;
  const ef = value.filter;
  if (!Array.isArray(ef.kinds) || !ef.kinds.every(kind => ['player_death', 'player_left', 'portal_opened', 'realm_event_killed'].includes(kind))) return null;
  if (ef.query !== undefined && !logsQuery(ef.query)) return null;
  if (ef.opener_local !== undefined && ef.opener_local !== null && typeof ef.opener_local !== 'boolean') return null;
  if (ef.range !== undefined && ef.range !== null && (!record(ef.range) || !timestamp(ef.range.start) || !timestamp(ef.range.end) || BigInt(ef.range.start) >= BigInt(ef.range.end))) return null;
  if ((ef.dungeon !== null && !safeId(ef.dungeon)) || (ef.run_id !== null && !safeId(ef.run_id))
    || !validAlertCursor(ef.cursor) || !boundedInteger(ef.limit, 1, 100) || !value.previous.every(validAlertCursor)) return null;
  return { filter: { kinds: [...ef.kinds] as EventFilter['kinds'], run_id: ef.run_id as string | null,
    dungeon: ef.dungeon as string | null, cursor: copyAlertCursor(ef.cursor), limit: 50,
    ...(ef.query === undefined ? {} : { query: ef.query as string }),
    ...(ef.opener_local === undefined ? {} : { opener_local: ef.opener_local as boolean | null }),
    ...(ef.range === undefined ? {} : { range: ef.range === null ? null : { start: (ef.range as TimeRange).start, end: (ef.range as TimeRange).end } }) },
    previous: value.previous.slice(-100).map(copyAlertCursor),
    ...(value.selectedId === undefined ? {} : { selectedId: safeId(value.selectedId) ? value.selectedId : null }) };
}
function logsPosition(value: unknown): LogsPosition | null {
  if (!record(value)) return null;
  const defaults = defaultLogsPosition(), events = logsEvents(value.events);
  const legacyKeys = value.keys === undefined && events?.filter.kinds.length === 1 && events.filter.kinds[0] === 'portal_opened';
  const keys = legacyKeys ? events : logsEvents(value.keys);
  const whisper = value.whisper, range = value.range;
  return {
    chat: logsChat(value.chat) ?? defaults.chat,
    whisper: record(whisper) && logsQuery(whisper.query) && typeof whisper.showHidden === 'boolean'
      && (whisper.partner === null || logsName(whisper.partner))
      ? { query: whisper.query, showHidden: whisper.showHidden, partner: whisper.partner as string | null } : defaults.whisper,
    keys: keys && keys.filter.kinds.length === 1 && keys.filter.kinds[0] === 'portal_opened' ? keys : defaults.keys,
    events: legacyKeys ? defaults.events : events ?? defaults.events,
    range: record(range) && timestamp(range.start) && timestamp(range.end) && BigInt(range.start) < BigInt(range.end)
      ? { start: range.start, end: range.end } : null,
  };
}

/** Cache only bounded summary fields, never row details or arbitrary history payloads. */
function runsSummary(value: unknown): RunListSummary | undefined {
  if (!record(value)) return undefined;
  const count = (value: unknown) => boundedInteger(value, 0, Number.MAX_SAFE_INTEGER);
  const counts = ['runs', 'cleared', 'died', 'left', 'unknown', 'time_in_runs_micros', 'personal_bests', 'fame_observed_runs', 'total_runs_all_time'];
  if (!counts.every(key => count(value[key]))
    || (value.fame_delta !== null && !boundedInteger(value.fame_delta, Number.MIN_SAFE_INTEGER, Number.MAX_SAFE_INTEGER))
    || (value.first_started !== null && !timestamp(value.first_started))) return undefined;
  const death = value.last_death;
  if (death !== null && (!record(death) || !safeId(death.run_id) || !safeId(death.dungeon) || !timestamp(death.at))) return undefined;
  if (!Array.isArray(value.characters) || value.characters.length > 50 || !value.characters.every(character => record(character)
    && safeId(character.id) && (character.name === null || character.name === '' || safeId(character.name))
    && (character.class_id === null || boundedInteger(character.class_id, 0, 65535)) && count(character.runs))) return undefined;
  const summary = value as unknown as RunListSummary;
  return {
    runs: summary.runs, cleared: summary.cleared, died: summary.died, left: summary.left, unknown: summary.unknown,
    time_in_runs_micros: summary.time_in_runs_micros, personal_bests: summary.personal_bests, fame_delta: summary.fame_delta,
    fame_observed_runs: summary.fame_observed_runs, first_started: summary.first_started, total_runs_all_time: summary.total_runs_all_time,
    last_death: summary.last_death === null ? null : { run_id: summary.last_death.run_id, dungeon: summary.last_death.dungeon, at: summary.last_death.at },
    characters: summary.characters.map(character => ({ id: character.id, name: character.name, class_id: character.class_id, runs: character.runs })),
  };
}

/** Old and malformed browser entries cannot become native query arguments. */
export function normalizeListState(input: unknown): ListState {
  const value: Partial<ListState> = record(input) ? input : {};
  const filters = validRunFilter(value.filters) ? { ...value.filters, character_id: value.filters.character_id ?? null, limit: 25 } : defaultFilters();
  const previous = value.runsPosition?.previous;
  const summary = runsSummary(value.runsPosition?.summary);
  const position = value.storyPosition;
  return {
    homeScope: value.homeScope === 'session' || value.homeScope === 'today' ? value.homeScope : null,
    filters,
    scrollTop: typeof value.scrollTop === 'number' && Number.isFinite(value.scrollTop) ? Math.max(0, value.scrollTop) : 0,
    focusedRowId: safeId(value.focusedRowId) ? value.focusedRowId : null,
    selectedFightId: safeId(value.selectedFightId) ? value.selectedFightId : null,
    alertsPosition: alertsPosition(value.alertsPosition),
    lootPosition: lootPosition(value.lootPosition),
    logsPosition: logsPosition(value.logsPosition),
    statsPosition: statsPosition(value.statsPosition),
    runsWindow: windowIndex(value.runsWindow),
    fightPosition: record(value.fightPosition) ? {
      playerWindow: windowIndex(value.fightPosition.playerWindow), sourceWindow: windowIndex(value.fightPosition.sourceWindow),
      selectedPlayerId: safeId(value.fightPosition.selectedPlayerId) ? value.fightPosition.selectedPlayerId : null,
      plotted: Array.isArray(value.fightPosition.plotted) && value.fightPosition.plotted.every(safeId)
        ? [...new Set(value.fightPosition.plotted)].slice(0, 256) : null,
    } : null,
    storyPosition: record(position) ? {
      selectedTab: safeId(position.selectedTab) ? position.selectedTab : null,
      allFights: position.allFights === true, otherTargets: position.otherTargets === true,
      timelineWindow: windowIndex(position.timelineWindow), fightWindow: windowIndex(position.fightWindow), lootWindow: windowIndex(position.lootWindow),
      otherFightWindow: windowIndex(position.otherFightWindow),
      timelinePointId: safeId(position.timelinePointId) ? position.timelinePointId : null,
    } : null,
    runsPosition: validRunFilter(value.filters) && Array.isArray(previous) && previous.every(cursor => validRunCursor(cursor)
      && (cursor?.sort_value !== '-1' || filters.sort === 'duration_descending')) ? { previous: previous.slice(-100), offset: windowIndex(value.runsPosition?.offset) + Math.max(0, previous.length - 100), ...(summary ? { summary } : {}) } : null,
  };
}

/** Call only after the rows-ready promise resolves. Obsolete transitions cancel. */
export async function restorePosition(pane: HTMLElement, state: ListState, rowsReady: Promise<unknown> = Promise.resolve(), current = () => true): Promise<boolean> {
  await rowsReady;
  await tick();
  if (!current()) return false;
  const row = state.focusedRowId
    ? [...pane.querySelectorAll<HTMLElement>('[data-row-id]')].find((el) => el.dataset.rowId === state.focusedRowId)
    : undefined;
  const focusTarget = row?.matches('a, button, [tabindex]') ? row : row?.querySelector<HTMLElement>('a, button, [tabindex]');
  (focusTarget ?? pane.querySelector<HTMLElement>('h1'))?.focus({ preventScroll: true });
  pane.scrollTop = Math.max(0, Math.min(state.scrollTop, pane.scrollHeight - pane.clientHeight));
  return state.focusedRowId !== null && !focusTarget;
}

export function focusedRow(pane: HTMLElement): string | null {
  const active = pane.ownerDocument.activeElement;
  return active instanceof HTMLElement && pane.contains(active) ? active.closest<HTMLElement>('[data-row-id]')?.dataset.rowId ?? null : null;
}
