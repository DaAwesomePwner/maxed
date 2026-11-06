import type { EventFilter } from '../../src/lib/bindings/EventFilter';
import type { EventSummary } from '../../src/lib/bindings/EventSummary';
import type { EnteredRun } from '../../src/lib/bindings/EnteredRun';
import type { Client } from '../../src/lib/bridge/client';
import type { ChatMessageView } from '../../src/lib/bindings/ChatMessageView';
import type { ChatSettings } from '../../src/lib/bindings/ChatSettings';
import type { InstanceEventView } from '../../src/lib/bindings/InstanceEventView';
import type { AlertView } from '../../src/lib/bindings/AlertView';
import type { AlertCursor } from '../../src/lib/bindings/AlertCursor';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import { alertKinds } from '../../src/lib/alerts/filters';
import { createHomePreview } from './scenario';

const marker = 'LOGS_SYNTHETIC_PREVIEW_ONLY';
/** Invented observations only. No native reads, game assets or account data. */
export function createLogsPreview(variant: 'default' | 'stress'): { client: Client; dispose(): void } {
  const preview = createHomePreview('nexus'), client = preview.client;
  const stress = variant === 'stress', now = new Date();
  const names = ['Vexlin', 'Quenby', 'Morrow', 'Kestrel', 'Pallo', 'Ithra', 'Sorel', 'Tamsin', 'Brack'];
  const name = (prefix: string, i: number) => stress ? `${prefix} ${i}`.padEnd(64, 'x') : `${prefix} ${i}`;
  const partners = Array.from({ length: stress ? 200 : 9 }, (_, i) => stress ? name('Vexlin', i) : names[i]);
  const dungeons = Array.from({ length: stress ? 128 : 9 }, (_, i) => name('Invented vault', i + 1));
  const timestamp = (i: number, total: number) => String(new Date(now.getFullYear(), now.getMonth(), now.getDate() - Math.floor(i * 3 / total), 20, 59, 59).getTime() * 1000 - i * 1_000_000);
  const runId = (i: number) => i % 5 === 0 ? null : `${marker}-run-${i % 9}`;
  const messages: ChatMessageView[] = Array.from({ length: stress ? 4000 : 300 }, (_, i) => {
    // Retain the preview's total rows and place groups while concentrating a long exchange.
    const longThread = i >= 24 && i < (stress ? 674 : 174);
    const channel = longThread ? 'whisper' : (['whisper', 'public', 'guild', 'party', 'unknown', 'public'] as const)[i % 6];
    const partnerIndex = longThread ? 0 : Math.floor(i / 6) % partners.length;
    const outgoing = channel === 'whisper' && (longThread ? i % 2 === 1 : Math.floor(i / (partners.length * 6)) % 2 === 1);
    const local = outgoing || channel !== 'whisper' && i % 7 === 0;
    const npc = i % 6 === 5;
    return { id: `${marker}-chat-${i}`, session_id: marker, run_id: runId(Math.floor(i / 12)), map: runId(Math.floor(i / 12)) ? dungeons[Math.floor(i / 12) % 9] : 'Nexus',
      at: timestamp(i, stress ? 4000 : 300), channel,
      direction: channel === 'whisper' ? partnerIndex === 8 ? 'unknown' : outgoing ? 'outgoing' : 'incoming' : null,
      sender_kind: npc ? 'npc' : 'player', sender: npc ? 'Invented guardian' : local ? 'Lethra' : partners[partnerIndex],
      counterpart: channel === 'whisper' ? outgoing ? partners[partnerIndex] : 'Lethra' : null,
      is_local: local, stars: i % 13 === 0 ? null : 20 + i % 60, supporter: i % 11 === 0,
      text: stress ? `Synthetic message ${i} `.padEnd(1024, 'x') : i > 0 && i % 17 === 0 ? `Synthetic trade offer ${i}` : `Synthetic chat line ${i}: ready for the next vault`,
      truncated: i % 29 === 0 };
  });
  let chat: ChatSettings = { filter_spam: true, store: { public: true, guild: true, party: false, whisper: true }, retention_days: 30,
    hidden_senders: ['Morrow', 'Pallo', 'Brack'], hidden_keywords: ['trade offer', 'synthetic sale', 'synthetic spam', 'synthetic advert', 'synthetic bargain'] };
  const events: InstanceEventView[] = Array.from({ length: stress ? 1024 : 200 }, (_, i) => ({
    id: `${marker}-event-${i}`, session_id: marker, run_id: runId(i), map: runId(i) ? dungeons[i % 9] : i % 9 === 0 ? null : 'Nexus',
    at: timestamp(i, stress ? 1024 : 200), provisional: i % 7 === 0,
    kind: i % 4 === 0 ? { type: 'player_death', player_name: partners[i % partners.length], level: i % 8 === 0 ? null : 20,
      killed_by: i % 8 === 0 ? null : 'Invented guardian', grave_type: null, class_id: null, is_local: i % 12 === 0 }
      : i % 4 === 1 ? { type: 'player_left', object_id: i, player_name: i % 9 === 0 ? null : partners[i % partners.length], class_id: null }
      : i % 4 === 2 ? { type: 'portal_opened', opener_is_local: i % 5 === 0 ? null : i % 3 === 0, opener: i % 10 === 0 ? null : partners[i % partners.length], portal_type: null, dungeon: i % 10 === 0 ? null : dungeons[Math.floor(i / 4) % dungeons.length] }
      : { type: 'realm_event_killed', boss_key: 'Invented guardian', object_type: null, killer: i % 7 === 0 ? null : names[i % 9] },
  }));
  const alerts: AlertView[] = Array.from({ length: stress ? 400 : 120 }, (_, i) => {
    const kind = alertKinds[i % alertKinds.length];
    return { id: `${marker}-alert-${i}`, at: timestamp(i, 120), kind, run_id: runId(i), explanation: `Synthetic ${kind} observation ${i}`,
      presentation: i % 5 === 0 ? null : (['off', 'quiet_log_only', 'toast', 'banner'] as const)[i % 4],
      details: kind === 'key_popped' ? { type: 'key_popped', dungeon: dungeons[i % dungeons.length] }
        : kind === 'death' ? { type: 'death', character_id: null }
        : kind === 'white_bag' ? { type: 'white_bag', drop_id: `${marker}-drop-${i}`, bag_type: 400, ordinal: null, window: null, source_enemy: null, source_estimated: true, contents_known: false }
        : kind === 'personal_best' ? { type: 'run_time_personal_best', dungeon: dungeons[i % dungeons.length], duration_micros: 120_000_000, previous_micros: 150_000_000 }
        : kind === 'flawless_run' ? null
        : { type: 'chat', channel: kind === 'whisper' ? 'whisper' : kind === 'guild_message' ? 'guild' : 'party', sender: stress ? name('Quenby', i) : names[i % 9], excerpt: stress ? 'Synthetic chat alert '.repeat(52).slice(0, 1024) : 'Synthetic chat alert' } };
  });
  const ready = <T>(value: T): HistoryReply<T> => ({ status: 'ready', value: structuredClone(value) });
  const page = <T extends { at: string; id: string }>(rows: T[], cursor: AlertCursor | null, limit: number) => {
    const matching = cursor ? rows.filter(row => BigInt(row.at) < BigInt(cursor.at) || row.at === cursor.at && row.id < cursor.id) : rows;
    const selected = matching.slice(0, Math.max(1, Math.min(100, limit))), last = selected.at(-1);
    return { rows: selected, next: last && selected.length < matching.length ? { at: last.at, id: last.id } : null };
  };
  const includes = (text: string, query: string) => text.toLowerCase().includes(query.toLowerCase());
  const hidden = (message: ChatMessageView) => chat.hidden_senders.some(sender => sender.toLowerCase() === message.sender.toLowerCase()
    || sender.toLowerCase() === message.counterpart?.toLowerCase()) || chat.hidden_keywords.some(term => includes(message.text, term));
  client.listChat = async filter => {
    const matching = messages.filter(message => (filter.channel === null || message.channel === filter.channel)
      && (filter.sender === null || message.sender.toLowerCase() === filter.sender.toLowerCase() || message.counterpart?.toLowerCase() === filter.sender.toLowerCase())
      && includes(message.text, filter.query) && (filter.run_id === null || message.run_id === filter.run_id)
      && (filter.show_npc || message.sender_kind !== 'npc') && (filter.show_hidden || !hidden(message)));
    const result = page(matching, filter.cursor, filter.limit);
    return ready({ rows: result.rows.map(message => ({ message, hidden: hidden(message) })), next: result.next });
  };
  client.getChatOverview = async request => {
    const rows = messages.filter(message => !request.run_id || message.run_id === request.run_id);
    const by_channel = { public: 0, guild: 0, party: 0, whisper: 0, unknown: 0 };
    for (const row of rows) by_channel[row.channel]++;
    return ready({ lines: rows.length, by_channel, first_at: rows.at(-1)?.at ?? null, latest_at: rows[0]?.at ?? null });
  };
  client.getChatSender = async request => {
    const rows = messages.filter(message => message.sender === request.name);
    const latest = rows[0];
    const channels = new Set(rows.map(row => row.channel));
    return ready(latest ? { name: latest.sender, lines: rows.length, first_at: rows.at(-1)!.at, last_at: latest.at,
      channels: (['public', 'guild', 'party', 'whisper', 'unknown'] as const).filter(channel => channels.has(channel)), stars: latest.stars ?? null, supporter: latest.supporter,
      whispers: messages.filter(row => row.channel === 'whisper' && (row.sender === request.name || row.counterpart === request.name)).length } : null);
  };
  client.listWhisperPartners = async filter => {
    const matching = partners.map(name => {
      const rows = messages.filter(message => message.channel === 'whisper' && (message.sender === name || message.counterpart === name) && (filter.show_hidden || !hidden(message))
        && (includes(name, filter.query) || includes(message.text, filter.query)));
      const latest = rows[0];
      return latest ? { name, lines: rows.length, last_at: latest.at, last_text: latest.text, last_direction: latest.direction ?? 'unknown' as const, stars: latest.stars ?? null } : null;
    }).filter(row => row !== null).sort((a, b) => Number(BigInt(b.last_at) - BigInt(a.last_at)));
    return ready({ rows: matching.slice(0, filter.limit), people: matching.length, lines: matching.reduce((sum, row) => sum + row.lines, 0) });
  };
  const eventScope = (filter: EventFilter) => events.filter(event => (!filter.run_id || event.run_id === filter.run_id)
    && (!filter.range || BigInt(event.at) >= BigInt(filter.range.start) && BigInt(event.at) < BigInt(filter.range.end)));
  client.getEventSummary = async ({ filter }) => {
    const rows = eventScope(filter);
    const summary: EventSummary = { total: rows.length, by_kind: { player_death: 0, player_left: 0, realm_event_killed: 0, portal_opened: 0 },
      dungeons: [], local_pops: 0, last_local_pop: null, flagged_since: events.at(-1)?.at ?? null, first_at: rows.at(-1)?.at ?? null };
    for (const row of rows) {
      summary.by_kind[row.kind.type]++;
      if (row.kind.type !== 'portal_opened') continue;
      if (row.kind.opener_is_local) {
        summary.local_pops++;
        if (!summary.last_local_pop && row.kind.dungeon) summary.last_local_pop = { dungeon: row.kind.dungeon, pops: 1, last_at: row.at };
      }
      if (!row.kind.dungeon) continue;
      const dungeon = summary.dungeons.find(dungeon => row.kind.type === 'portal_opened' && dungeon.dungeon === row.kind.dungeon);
      if (dungeon) dungeon.pops++;
      else summary.dungeons.push({ dungeon: row.kind.dungeon, pops: 1, last_at: row.at });
    }
    summary.dungeons.sort((a, b) => b.pops - a.pops || a.dungeon.localeCompare(b.dungeon));
    return ready(summary);
  };
  const eventNameMatches = (event: InstanceEventView, query: string) => {
    if (!query) return true;
    const terms = query.toLowerCase().match(/[\p{L}\p{N}]+/gu)?.slice(0, 16) ?? [];
    const names = Object.entries(event.kind)
      .filter(([field]) => ['player_name', 'opener', 'killed_by', 'killer', 'boss_key'].includes(field))
      .map(([, value]) => value).filter(value => typeof value === 'string').join(' ');
    const tokens = names.toLowerCase().match(/[\p{L}\p{N}]+/gu) ?? [];
    return terms.every(term => tokens.some(token => token.startsWith(term)));
  };
  client.listInstanceEvents = async filter => {
    const result = page(eventScope(filter).filter(event => (!filter.kinds.length || filter.kinds.includes(event.kind.type))
      && (filter.dungeon === null || event.kind.type === 'portal_opened' && event.kind.dungeon === filter.dungeon)
      && (filter.opener_local == null || event.kind.type === 'portal_opened' && event.kind.opener_is_local === filter.opener_local)
      && eventNameMatches(event, filter.query ?? '')), filter.cursor, filter.limit);
    const entered_runs: EnteredRun[] = [];
    for (const event of result.rows) {
      if (event.kind.type !== 'portal_opened' || !event.kind.dungeon) continue;
      const dungeon = event.kind.dungeon;
      const nextPop = events.filter(row => row.session_id === event.session_id && row.kind.type === 'portal_opened' && row.kind.dungeon === dungeon && BigInt(row.at) > BigInt(event.at))
        .sort((a, b) => Number(BigInt(a.at) - BigInt(b.at)))[0];
      const match = [...runs.values()].map(story => story.run).filter(run => run.session_id === event.session_id && run.dungeon === dungeon
        && BigInt(run.started) >= BigInt(event.at) && BigInt(run.started) <= BigInt(event.at) + 600_000_000n
        && (!nextPop || BigInt(run.started) < BigInt(nextPop.at))).sort((a, b) => Number(BigInt(a.started) - BigInt(b.started)))[0];
      if (match) entered_runs.push({ event_id: event.id, run_id: match.id, dungeon, started_at: match.started, outcome: match.outcome });
    }
    return ready({ ...result, entered_runs });
  };
  client.getAlertSummary = async ({ range }) => {
    const rows = alerts.filter(row => !range || BigInt(row.at) >= BigInt(range.start) && BigInt(row.at) < BigInt(range.end));
    const white = rows.find(row => row.kind === 'white_bag');
    return ready({ total: rows.length, by_kind: alertKinds.map(kind => ({ kind, count: rows.filter(row => row.kind === kind).length })),
      first_at: rows.at(-1)?.at ?? null, last_white: white ? { at: white.at, run_id: white.run_id, dungeon: white.run_id ? runs.get(white.run_id)?.run.dungeon ?? null : null } : null });
  };
  client.listAlerts = async filter => ready(page(alerts.filter(alert => (filter.kind === null || filter.kind === alert.kind)
    && (!filter.kinds?.length || filter.kinds.includes(alert.kind)) && !filter.exclude_kinds?.includes(alert.kind)
    && (!filter.range || BigInt(alert.at) >= BigInt(filter.range.start) && BigInt(alert.at) < BigInt(filter.range.end))
    && includes(`${alert.explanation} ${JSON.stringify(alert.details)}`, filter.query)), filter.cursor, Math.min(50, filter.limit)));
  const settings = client.querySettings;
  client.querySettings = async () => {
    const value = await settings();
    return { ...value, chat: structuredClone(chat), alerts: { ...value.alerts, key_pop_dungeons: null, key_pop_excluded_dungeons: dungeons.slice(1, 4) } };
  };
  client.updateChatSettings = async next => { chat = structuredClone(next); };
  client.listCatalogDungeons = async () => [...dungeons];
  const runs = new Map<string, RunStory>(Array.from({ length: 9 }, (_, i) => {
    const id = `${marker}-run-${i}`;
    const portal = events.find(event => event.kind.type === 'portal_opened' && event.kind.dungeon === dungeons[i]);
    const started = portal ? String(BigInt(portal.at) + 30_000_000n) : timestamp(299, 300);
    return [id, { run: { id, session_id: marker, character_id: null, dungeon: dungeons[i], area_kind: 'dungeon', started,
      ended: String(BigInt(started) + 1_450_000_000n), completed_at: null, died_at: null, outcome: 'left', evidence: [], party_size: null, party_size_estimated: false,
      complete_capture: false, issues: [] }, character: null, fights: [], drops: [], timeline: [], comparison: null, events: events.filter(event => event.run_id === id), events_limited: false }];
  }));
  client.getRun = async id => ready(runs.get(id) ?? null);
  return preview;
}
