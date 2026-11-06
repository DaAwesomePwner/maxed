import { cleanup, fireEvent, render } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createRecapPresentation } from '../../src/lib/home/v4/recap-presentation';
import { writable } from 'svelte/store';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { RecapState } from '../../src/lib/home/v4/recap';
import * as reads from '../../src/lib/home/v4/recap-reads';
import { SHELL_CONNECTION } from '../../src/lib/shell/context';
import Home from '../../src/routes/Home.svelte';
import { homeSnapshot, saved } from '../home/fixtures';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
const settle = () => vi.advanceTimersByTimeAsync(0);

async function mount(kind: 'run' | 'visit', title = 'Synthetic recap A', target?: HTMLElement) {
  const snapshot = homeSnapshot({ area_kind: 'nexus', run: null, fights: [], drops: [] });
  const client = createClient(createFixtureTransport(snapshot));
  const recap: RecapState = { source: kind === 'run'
    ? { kind, streamId: snapshot.stream_id, runId: 'synthetic-run-a' }
    : { kind, streamId: snapshot.stream_id, visitKey: 'synthetic-a', enteredSequence: '4' }, watermark: '13',
    summary: { kind, title, outcome: kind === 'run' ? 'cleared' : null, startedAt: '1000000', endedAt: '65000000',
      localDps: null, rank: null, players: null, fights: [], best: null, runId: kind === 'run' ? 'synthetic-run-a' : null } };
  const homeRecap = writable<RecapState | null>(recap), retry = vi.fn();
  let publish!: (load: reads.RecapLoad | null) => void;
  vi.spyOn(reads, 'createRecapReads').mockImplementation((_client, receive) => {
    publish = receive; return { update: vi.fn(), retry, dispose: vi.fn() };
  });
  const portal = vi.spyOn(client, 'getDungeonPortalType');
  const recapPresentation = createRecapPresentation(); recapPresentation.remember('synthetic-a');
  const page = render(Home, { props: { client, snapshot, store: saved('99'), redesign: true },
    context: new Map([[SHELL_CONNECTION, { homeRecap, recapPresentation, dismissHomeRecap: vi.fn() }]]), ...(target ? { target } : {}) });
  await settle();
  await fireEvent.click(page.getByRole('button', { name: /^Last recap:/ })); await settle();
  const data: reads.RunRecapData | reads.VisitRecapData = kind === 'run'
    ? { kind, story: { run: { ...homeSnapshot().run!, id: 'synthetic-run-a', dungeon: 'Synthetic saved A', outcome: 'cleared' },
      character: null, fights: [], drops: [], timeline: [], comparison: null }, extras: null,
      recordsUnavailable: false, profile: null, baselines: null, streak: null }
    : { kind, baselines: null, visit: { visit_id: `${snapshot.stream_id}:visit:4`, session_id: 'synthetic-session',
      character_id: null, title: 'Synthetic saved A', periods: [], fights: [], boss_fights: [], boss_fights_limited: false,
      announcements: [], announcements_limited: false, drops: [], drops_limited: false, fame_delta: null, best_dps: [], chain: null } };
  return { page, recap, homeRecap, publish, data, retry, portal, snapshot };
}

it.each(['run', 'visit'] as const)('saved_%s_A_does_not_render_under_recap_B', async kind => {
  const h = await mount(kind);
  h.publish({ status: 'saved', data: h.data }); await settle();
  expect(h.page.container.querySelector('.recap-screen')).not.toBeNull();
  h.homeRecap.set({ ...h.recap, source: kind === 'run'
    ? { kind, streamId: h.snapshot.stream_id, runId: 'synthetic-run-b' }
    : { kind, streamId: h.snapshot.stream_id, visitKey: 'synthetic-b', enteredSequence: '8' },
    summary: { ...h.recap.summary, title: 'Synthetic recap B' } });
  await settle();
  expect(h.page.container.querySelector('.recap-screen')).toBeNull();
  expect(h.page.getByRole('heading', { name: 'Synthetic recap B' })).toBeTruthy();
});

it('merged_visit_period_can_match_the_current_source', async () => {
  const h = await mount('visit'); if (h.data.kind !== 'visit') throw new Error('Expected visit');
  h.data.visit.periods = [{ visit_id: h.data.visit.visit_id, started: '1000000', ended: '65000000' }];
  h.data.visit.visit_id = 'synthetic-earlier-entry';
  h.publish({ status: 'saved', data: h.data }); await settle();
  expect(h.page.container.querySelector('.visit-recap-screen')).not.toBeNull();
});

it('record_failure_note_retries_the_recap_read', async () => {
  const h = await mount('run'); if (h.data.kind !== 'run') throw new Error('Expected run');
  h.publish({ status: 'saved', data: { ...h.data, recordsUnavailable: true } }); await settle();
  expect(h.page.getByText('Records unavailable')).toBeTruthy();
  await fireEvent.click(h.page.getByRole('button', { name: 'Retry' }));
  expect(h.retry).toHaveBeenCalledOnce();
  h.publish({ status: 'saved', data: h.data }); await settle();
  expect(h.page.queryByText('Records unavailable')).toBeNull();
  expect(h.page.queryByRole('button', { name: 'Retry' })).toBeNull();
});

it('retry_moves_focus_to_the_heading_when_the_saved_recap_is_replaced', async () => {
  const main = document.body.appendChild(document.createElement('main'));
  const heading = main.appendChild(document.createElement('h1')); heading.tabIndex = -1; heading.textContent = 'Home';
  try {
    const h = await mount('run', undefined, main.appendChild(document.createElement('div')));
    if (h.data.kind !== 'run') throw new Error('Expected run');
    h.publish({ status: 'saved', data: { ...h.data, recordsUnavailable: true } }); await settle();
    const button = h.page.getByRole('button', { name: 'Retry' }); button.focus();
    expect(document.activeElement).toBe(button);
    await fireEvent.click(button);
    h.publish({ status: 'saving' }); await settle();
    expect(button.isConnected).toBe(false);
    expect(document.activeElement).toBe(heading);
  } finally { main.remove(); }
});

it('portal_skips_unknown_summary_and_prefers_saved_title', async () => {
  const h = await mount('run', '—');
  expect(h.portal).not.toHaveBeenCalled();
  h.publish({ status: 'saved', data: h.data }); await settle();
  expect(h.portal).toHaveBeenCalledExactlyOnceWith('Synthetic saved A');
});
