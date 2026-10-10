import { expect, it } from 'vitest';
import { createLogsPreview } from '../preview/logs-scenario';
import type { EventFilter } from '../../src/lib/bindings/EventFilter';

it.each(['default', 'stress'] as const)('computes %s event summaries and portal matches from synthetic observations', async variant => {
  const preview = createLogsPreview(variant), client = preview.client;
  try {
    const filter: EventFilter = { kinds: [], run_id: null, dungeon: null, cursor: null, limit: 100 };
    const summary = await client.getEventSummary({ filter });
    expect(summary.status).toBe('ready');
    if (summary.status !== 'ready') throw new Error('Expected synthetic summary');
    expect(summary.value.total).toBe(variant === 'stress' ? 1024 : 200);
    expect(Object.values(summary.value.by_kind).reduce((sum, value) => sum + value, 0)).toBe(summary.value.total);
    expect(await client.getEventSummary({ filter: { ...filter, query: 'no match', kinds: ['player_death'] } })).toEqual(summary);
    const empty = await client.listInstanceEvents({ ...filter, query: 'no match' });
    expect(empty).toEqual({ status: 'ready', value: { rows: [], next: null, entered_runs: [] } });
    let matched = 0;
    do {
      const result = await client.listInstanceEvents(filter);
      if (result.status !== 'ready') throw new Error('Expected synthetic page');
      for (const event of result.value.rows) {
        if (!event.run_id) { expect([null, 'Nexus']).toContain(event.map); continue; }
        const reply = await client.getRun(event.run_id);
        if (reply.status !== 'ready' || !reply.value) throw new Error('Expected synthetic run');
        expect(event.map).toBe(reply.value.run.dungeon);
        expect(BigInt(reply.value.run.ended!) - BigInt(reply.value.run.started)).toBe(1_450_000_000n);
      }
      for (const match of result.value.entered_runs ?? []) {
        const event = result.value.rows.find(row => row.id === match.event_id)!;
        expect(event.kind.type).toBe('portal_opened');
        expect(BigInt(match.started_at) - BigInt(event.at)).toBeGreaterThanOrEqual(0n);
        expect(BigInt(match.started_at) - BigInt(event.at)).toBeLessThanOrEqual(600_000_000n);
        matched++;
      }
      filter.cursor = result.value.next;
    } while (filter.cursor);
    expect(matched).toBeGreaterThan(0);
  } finally { preview.dispose(); }
});

it('searches event name prefixes and ignores punctuation-only queries', async () => {
  const preview = createLogsPreview('default');
  try {
    const filter: EventFilter = { kinds: [], run_id: null, dungeon: null, cursor: null, limit: 100 };
    for (const query of ['Vex', '"Vex*()']) {
      const page = await preview.client.listInstanceEvents({ ...filter, query });
      if (page.status !== 'ready') throw new Error('Expected synthetic page');
      expect(page.value.rows.length).toBeGreaterThan(0);
    }
    expect(await preview.client.listInstanceEvents({ ...filter, query: 'xlin' })).toEqual({
      status: 'ready', value: { rows: [], next: null, entered_runs: [] },
    });
    for (const kinds of [[], ['portal_opened']] as EventFilter['kinds'][]) {
      const scoped = { ...filter, kinds };
      const unfiltered = await preview.client.listInstanceEvents(scoped);
      for (const query of ['%', '*()"', '   ']) {
        expect(await preview.client.listInstanceEvents({ ...scoped, query })).toEqual(unfiltered);
      }
    }
  } finally { preview.dispose(); }
});
