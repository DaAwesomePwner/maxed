import { render, screen } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import { setDebugMode } from '../../src/lib/debug/debug';
import DamageTable from '../../src/lib/home/v3/DamageTable.svelte';
import NexusHome from '../../src/lib/home/NexusHome.svelte';
import LiveStatusCard from '../../src/lib/shell/LiveStatusCard.svelte';
import FightPlayers from '../../src/lib/runs/FightPlayers.svelte';
import { runKpis } from '../../src/lib/runs/run-screen';
import AlertLog from '../../src/lib/alerts/AlertLog.svelte';
import { railView } from '../../src/lib/shell/rail';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { homeSnapshot, homeSummary } from '../home/fixtures';
import { fight, fightSummary, player, story } from '../runs/fixtures';
import { alert } from '../phase2/fixtures';

afterEach(() => setDebugMode(false));

it.each([1448.7, null])('Home damage table keeps rounded and unavailable DPS (%s)', dps => {
  const page = render(DamageTable, { rows: [player({ name: 'Player', dps })], total: 1,
    visuals: emptyHomeVisuals(), resolveSpriteUrl: () => null });
  expect(page.container.querySelector('tbody tr td:nth-child(5)')?.textContent).toBe(dps === null ? '—' : '1,449');
});
it('Top DPS rounds the supplied history value', () => {
  const summary = homeSummary({ top_fight: fight({ players: [player({ name: 'Player', dps: 6964.6 })] }) });
  const page = render(NexusHome, { snapshot: homeSnapshot(), choice: 'session', retry: vi.fn(),
    history: { phase: 'ready', summary, retryAvailable: true, message: '' } });
  expect(page.container.querySelector('[data-kpi="dps"] .value')?.textContent).toBe('6,965');
});
it('the rail rounds its DPS value', () => {
  setDebugMode(true);
  const snapshot = homeSnapshot();
  const page = render(LiveStatusCard, { view: { ...railView(snapshot), fight: snapshot.fights[0], dps: 1448.7 }, capture: snapshot.capture,
    busy: false, error: '', onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn() });
  expect(page.container.textContent).toContain('1,449');
});
it('saved fight players use whole-number DPS', () => {
  render(FightPlayers, { fight: fight({ players: [player({ name: 'Player', dps: 1448.7 })] }) });
  expect(screen.getByLabelText('DPS').textContent).toBe('1,449');
});
it('run fight rows and details use whole-number DPS', () => {
  const local = player({ dps: 1448.7 });
  const value = story({ totals: { fights: 1, kills: 0, total_damage: 321, unattributed_damage: 0, players: [local], local, partial: false } });
  expect(runKpis(value, null).find(cell => cell.key === 'share')?.sub).toContain('1,449 DPS');

});
it('PB history rounds both current and previous DPS', () => {
  const page = render(AlertLog, { rows: [alert({ kind: 'personal_best', details: {
    type: 'personal_best', fight_id: 'fight', character_id: 'self', boss_type: 300, dps: 1448.7, previous_dps: 1234.5,
  } })] });
  expect(page.container.textContent).toContain('DPS: 1,449');
  expect(page.container.textContent).toContain('Previous DPS: 1,235');
});
