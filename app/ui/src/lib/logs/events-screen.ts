import type { EventSummary } from '../bindings/EventSummary';
import type { InstanceEventView } from '../bindings/InstanceEventView';
import type { InstanceEventTag } from '../bindings/InstanceEventTag';
import type { RunStory } from '../bindings/RunStory';

export type EventKindKey = 'all' | 'death' | 'left' | 'realm' | 'key';
export const eventKinds: Record<Exclude<EventKindKey, 'all'>, InstanceEventTag> = {
  death: 'player_death', left: 'player_left', realm: 'realm_event_killed', key: 'portal_opened',
};
export const eventLabels = { death: 'Death', left: 'Left', realm: 'Realm kill', key: 'Key pop', all: 'All' };
export const eventGlyphs = { death: 'skull', left: 'door', realm: 'crown', key: 'portal', all: '' };
export function kindSegments(summary: EventSummary | null): Array<{ key: EventKindKey; label: string; count: string }> {
  return (['all', 'death', 'left', 'realm', 'key'] as const).map((key, index) => ({
    key, label: ['All', 'Deaths', 'Departures', 'Realm kills', 'Key pops'][index],
    count: summary === null ? '—' : (key === 'all' ? summary.total : summary.by_kind[eventKinds[key]]).toLocaleString('en-US'),
  }));
}
export function eventParts(view: InstanceEventView, className: (id: number) => string | null): { kind: EventKindKey; who: string | null; detail: string; where: string } {
  const event = view.kind, where = view.map ?? '—';
  switch (event.type) {
    case 'player_death': return { kind: 'death', who: event.player_name || null, where,
      detail: [event.level == null ? null : `Level ${event.level}`, event.class_id == null ? null : className(event.class_id), event.killed_by ? `killed by ${event.killed_by}` : null].filter(Boolean).join(' · ') };
    case 'player_left': return { kind: 'left', who: event.player_name ?? null, where,
      detail: ['left', event.class_id == null ? null : className(event.class_id)].filter(Boolean).join(' · ') };
    case 'realm_event_killed': return { kind: 'realm', who: event.boss_key || null, where, detail: event.killer ? `killed by ${event.killer}` : 'killed' };
    case 'portal_opened': return { kind: 'key', who: event.opener ?? null, where, detail: event.dungeon ? `opened ${event.dungeon}` : 'opened a portal' };
  }
}
export function runCounts(story: RunStory): { deaths: number; departures: number } {
  return { deaths: story.events?.filter(event => event.kind.type === 'player_death').length ?? 0,
    departures: story.events?.filter(event => event.kind.type === 'player_left').length ?? 0 };
}
