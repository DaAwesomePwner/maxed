import { cleanup, fireEvent, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { setDebugMode } from '../../src/lib/debug/debug';
import { advance, saved, fight, fightSummary, flush, follow, main, mountRuns, player, ready, region, run, story } from './fixtures';

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { setDebugMode(false); cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('shows Home KPIs, boss damage and other targets without window controls', async () => {
  const local = player();
  const value = story({ boss_fights: [fight()], events: [], older_run_id: null, newer_run_id: 'run-2',
    fights: [fightSummary(1, { target_kind: 'boss' }), ...[2, 3, 4].map(i => fightSummary(i))],
    totals: { total_damage: 1400, unattributed_damage: 79, fights: 4, kills: 3, players: [local], local, fame_delta: 24, partial: false } });
  mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } }); await flush();
  for (const label of ['Clear time', 'Damage', 'Your share', 'Party', 'Run fame']) expect(main().getByText(label, { selector: 'dt' })).toBeTruthy();
  for (const value of ['0:08', '1,400', '23%', '~4', '+24']) expect(main().getAllByText(value).length).toBeGreaterThan(0);
  expect(main().getByRole('tab', { name: 'Whole run' }).getAttribute('aria-selected')).toBe('true');
  const boss = main().getAllByRole('tab')[1]; await fireEvent.click(boss); await flush();
  expect(region('Fight damage').getByText('Invented Archer')).toBeTruthy();
  expect(main().getByRole('link', { name: 'Open fight ›' }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-1');
  await fireEvent.click(main().getByRole('tab', { name: /3 other targets/ })); await flush();
  for (const i of [2, 3, 4]) expect(main().getByRole('link', { name: new RegExp(`Invented target ${i}`) }).getAttribute('href')).toBe(`#/runs/run-1/fights/fight-${i}`);
  const targetTable = within(main().getByRole('table', { name: 'Other targets' }));
  expect(targetTable.getAllByRole('row')).toHaveLength(4);
  for (const cell of targetTable.getAllByRole('cell')) expect(cell.closest('[role="row"], tr')).not.toBeNull();
  expect(main().getByLabelText('Older run').getAttribute('aria-disabled')).toBe('true');
  expect(main().getByRole('link', { name: 'Newer run' }).getAttribute('href')).toBe('#/runs/run-2');
  expect(main().queryByRole('button', { name: /Next Window|Previous Window|Next Bosses/ })).toBeNull();
});
it('renders an open zero-boss story with unknown clear and totals', async () => {
  mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({ boss_fights: [], fights: [], totals: null,
    run: run({ ended: null, completed_at: null, outcome: null }) })) } }); await flush();
  expect(main().getByText('In progress')).toBeTruthy();
  expect(main().getAllByRole('tab')).toHaveLength(1);
  expect(main().getByText('Clear time').parentElement?.textContent).toContain('—');
  expect(main().getByRole('region', { name: 'Timeline' })).toBeTruthy();
});
it('restores the selected boss tab through Back and Forward', async () => {
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({ boss_fights: [fight()] })) } }); await flush();
  await fireEvent.click(main().getAllByRole('tab')[1]); await flush();
  await follow('#/runs/run-1/fights/fight-1');
  page.browser.adapter.go(-1); await flush();
  expect(main().getAllByRole('tab')[1].getAttribute('aria-selected')).toBe('true');
  page.browser.adapter.go(1); await flush();
  expect(main().getByRole('heading', { name: 'Fight', level: 1 })).toBeTruthy();
});
it('keeps clear and death visible and evidence debug-only', async () => {
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({ run: run({ died_at: '10000000' }) })) } }); await flush();
  expect(main().getByText('Clear and death')).toBeTruthy();
  expect(main().queryByText('Run evidence')).toBeNull();
  setDebugMode(true); await flush();
  expect(main().getByText('Run evidence')).toBeTruthy();
  expect(page.queries.getFight).not.toHaveBeenCalled();
});
it('shows compact run log links only when enabled', async () => {
  const page = mountRuns({ hash: '#/runs/run-1' }); await flush();
  expect(main().getByRole('link', { name: 'Chat log' }).getAttribute('href')).toBe('#/logs/run/run-1');
  expect(main().getByRole('link', { name: 'Events' }).getAttribute('href')).toBe('#/logs/events/run/run-1');
  await page.rerender({ features: { lootBrowsing: true, logs: false, stats: false, party: false, character: false, homeRedesign: false } }); await flush();
  expect(main().queryByRole('navigation', { name: 'Run logs' })).toBeNull();
});
it('restores focused boss tabs and highlights the featured bag from the timeline', async () => {
  const { homeSnapshot } = await import('../home/fixtures');
  const drop = { ...homeSnapshot().drops[0], id: 'featured', run_id: 'run-1', at: '4000000' };
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({ drops: [drop] })) } }); await flush();
  const tab = main().getAllByRole('tab')[1]; await fireEvent.click(tab); await flush(); tab.focus();
  await follow('#/runs/run-1/fights/fight-1'); page.browser.adapter.go(-1); await flush();
  expect(document.activeElement).toBe(main().getAllByRole('tab')[1]);
  const marker = page.container.querySelector<HTMLElement>('[data-marker-id="drop:featured"]')!;
  await fireEvent.click(marker); await flush();
  expect(document.activeElement?.getAttribute('data-drop-id')).toBe('featured');
  expect(document.activeElement?.classList.contains('selected')).toBe(true);
});
it('shows saved clear on the timeline without claiming an ended boss was killed', async () => {
  const value = story({ boss_fights: [fight({ end_reason: 'session_idle', kill: null })] });
  mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } }); await flush();
  expect(main().getAllByRole('tab')[1].textContent).not.toContain('✓');
  expect(region('Timeline').getByLabelText('Cleared at 0:08')).toBeTruthy();
});
it.each(['missing', 'absent', 'legacy'] as const)('keeps missing run and death states explicit (%s)', async kind => {
  mountRuns({ hash: '#/runs/run-1/death', queries: { getRun: async () => ready(kind === 'missing' ? null : story({ run: run({ died_at: kind === 'legacy' ? '10000000' : null }) })) } }); await flush();
  if (kind === 'missing') expect(main().getByText('Run not found')).toBeTruthy();
  else expect(main().getByRole('region', { name: 'Death' }).textContent).toMatch(kind === 'legacy' ? /Saved death timestamp/ : /No death observation is saved/);
  expect(main().queryByRole('button', { name: 'Retry' })).toBeNull();
});
it('retains all outcome evidence and timestamps in the debug disclosure', async () => {
  setDebugMode(true);
  mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({ run: run({ evidence: [
    { at: '2000000', evidence: 'victory' },
    { at: '3000000', evidence: { completion_dialogue: { rule: 'invented-rule-17' } } },
    { at: '4000000', evidence: { completion_counter: { before: 10, after: 11, delay_micros: 2000000 } } },
    { at: '5000000', evidence: 'death_packet' }, { at: '6000000', evidence: 'observed_exit' },
    { at: '7000000', evidence: 'capture_gap' }, { at: '8000000', evidence: 'app_closed' },
  ] }) })) } }); await flush();
  const text = main().getByText('Run evidence').closest('details')!.textContent;
  for (const pattern of [/Victory/, /invented-rule-17/, /10.*11/, /Death observed/, /Exit observed/, /Capture gap/, /App closed/, /\d+:\d{2}/]) expect(text).toMatch(pattern);
});
it('preserves focus on the featured bag when late tiers populate its best item', async () => {
  const { homeSnapshot } = await import('../home/fixtures');
  const { deferred } = await import('./fixtures');
  const pending = deferred<Awaited<ReturnType<import('../../src/lib/bridge/client').Client['getObjectVisuals']>>>();
  const seed = homeSnapshot().drops[0];
  const drops = [{ ...seed, id: 'high-rank', at: '3000000', bag_rank: 6, items: [445], contents: [] },
    { ...seed, id: 'ut', at: '7000000', bag_rank: 1, items: [444], contents: [] }];
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({ drops })) } });
  page.visuals.mockReturnValue(pending.promise); await flush();
  await fireEvent.click(page.container.querySelector('[data-marker-id="drop:high-rank"]')!); await flush();
  const before = document.activeElement;
  expect(before?.getAttribute('data-drop-id')).toBe('high-rank');
  expect(before?.classList.contains('best-card')).toBe(true);
  expect(before?.querySelector('.item-box')).toBeNull();
  pending.resolve({ client_fingerprint: 'synthetic-c2', objects: [
    { object_type: 444, item_tier: 'UT', name: 'Synthetic relic', sprite: null, bag: null },
    { object_type: 445, item_tier: 'T12', name: 'Synthetic sword', sprite: null, bag: null },
  ] }); await flush();
  expect(document.activeElement).toBe(before);
  expect(document.activeElement?.getAttribute('data-drop-id')).toBe('high-rank');
  expect(document.activeElement?.classList.contains('best-card')).toBe(true);
  expect(document.activeElement?.querySelector('.item-box')).not.toBeNull();
  expect(document.activeElement?.textContent).toContain('Synthetic sword');
  expect(document.activeElement?.getAttribute('data-tier')).toBe('T12');
  // A lower-ranked bag's newly discovered UT cannot replace the featured bag.
  expect(page.container.querySelector('.best-card')?.getAttribute('data-drop-id')).toBe('high-rank');
});

it.each([7, 85])('shows all %i players in rank order on whole-run and boss tabs', async count => {
  const players = Array.from({ length: count }, (_, i) => player({ player_id: `full-${i}`, name: `Full player ${i + 1}`,
    rank: i + 1, damage: (count - i) * 100, is_local: i === count - 1 }));
  const local = players[count - 1];
  const value = story({ boss_fights: [fight({ players: [...players].reverse() })],
    totals: { total_damage: 1000000, unattributed_damage: 0, fights: 1, kills: 1,
      players: [...players].reverse(), local, fame_delta: 1, partial: false } });
  mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } }); await flush();
  for (const tab of main().getAllByRole('tab')) {
    await fireEvent.click(tab); await flush();
    const table = region('Fight damage').getByRole('table', { name: 'Player Damage' });
    const rows = [...table.querySelectorAll('tr.damage-row')];
    expect(rows).toHaveLength(count);
    expect(rows.map(row => row.querySelector('.name')?.textContent)).toEqual(players.map(row => row.name));
    expect(rows[count - 1].classList.contains('self')).toBe(true);
    expect(region('Fight damage').queryByRole('link', { name: /more players/ })).toBeNull();
    expect(table.closest('.damage-table')?.classList.contains('scroll')).toBe(true);
  }
});

it('keeps other targets outside the boss overflow and reachable with End', async () => {
  const bosses = Array.from({ length: 64 }, (_, i) => fight({ id: `boss-${i}`, target_kind: 'boss' }));
  const value = story({ boss_fights: bosses, fights: [fightSummary(100, { target_kind: 'ordinary' })] });
  mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } }); await flush();
  const other = main().getByRole('tab', { name: /^1 other target / });
  expect(other.closest('.fight-scroll')).toBeNull();
  const more = main().getByRole('button', { name: /more ›/ });
  expect(more.closest('[role="tablist"]')).toBeNull();
  const tablist = main().getByRole('tablist', { name: 'Encounters' });
  expect(tablist.getAttribute('aria-owns')).toBe(other.id);
  expect(other.id).not.toBe('');
  await fireEvent.keyDown(main().getByRole('tab', { name: 'Whole run' }), { key: 'End' }); await flush();
  expect(document.activeElement).toBe(other);
  expect(other.getAttribute('aria-selected')).toBe('true');
  expect(main().getByRole('table', { name: 'Other targets' })).toBeTruthy();
  await fireEvent.keyDown(other, { key: 'ArrowLeft' }); await flush();
  expect(document.activeElement).toBe(main().getAllByRole('tab')[64]);
  await fireEvent.keyDown(document.activeElement!, { key: 'ArrowRight' }); await flush();
  expect(document.activeElement).toBe(other);
  await fireEvent.keyDown(other, { key: 'ArrowRight' }); await flush();
  expect(document.activeElement).toBe(main().getByRole('tab', { name: 'Whole run' }));
});

it('shows one compact killed label with max HP, retaining HP left for an unconfirmed end', async () => {
  const killed = fight({ target_hp: 0, max_hp: 336000, end_reason: 'killed',
    kill: { at: '8000000', evidence: 'hp_zero_removed', provisional: false } });
  const ended = fight({ id: 'ended-boss', target_hp: 168000, max_hp: 336000, end_reason: 'session_idle', kill: null });
  mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({ boss_fights: [killed, ended] })) } }); await flush();
  await fireEvent.click(main().getAllByRole('tab')[1]); await flush();
  const label = region('Fight damage').getByText(/^Killed at .* · 336,000 HP$/);
  expect(label.closest('.boss-hp')?.textContent).not.toMatch(/Defeated|0%|0 \/|Killed ·/);
  expect(label.closest('.boss-hp')?.querySelector('.boss-status')).toBeNull();
  await fireEvent.click(main().getAllByRole('tab')[2]); await flush();
  expect(region('Fight damage').getByText('168,000 / 336,000 · 50%')).toBeTruthy();
  expect(region('Fight damage').getByText('Ended')).toBeTruthy();
});

it('restores focus when a saved bag revision replaces its list row with the featured card', async () => {
  const { homeSnapshot } = await import('../home/fixtures');
  const seed = homeSnapshot().drops[0];
  const drops = [{ ...seed, id: 'old-best', run_id: 'run-1', at: '3000000', bag_rank: 90 },
    { ...seed, id: 'focused', run_id: 'run-1', at: '7000000', bag_rank: 1 }];
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({ drops })) } });
  await flush();
  await fireEvent.click(page.container.querySelector('[data-marker-id="drop:focused"]')!); await flush();
  const before = document.activeElement;
  expect(before?.getAttribute('data-drop-id')).toBe('focused');
  expect(before?.classList.contains('row')).toBe(true);
  const revised = { ...drops[1], bag_rank: 255 };
  page.queries.getRun.mockResolvedValue(ready(story({ drops: [drops[0], revised] })));
  page.event({ drop_updated: revised }, '13'); page.setStore(saved('13', page.snapshot.stream_id)); await advance();
  expect(before?.isConnected).toBe(false);
  expect(document.activeElement).not.toBe(before);
  expect(document.activeElement?.getAttribute('data-drop-id')).toBe('focused');
  expect(document.activeElement?.classList.contains('best-card')).toBe(true);
});
