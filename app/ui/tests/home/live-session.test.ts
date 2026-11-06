import { setDebugMode } from '../../src/lib/debug/debug';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import NexusHome from '../../src/lib/home/NexusHome.svelte';
import { homeSnapshot, homeSummary, saved } from './fixtures';
import { flushHomeRender, mountHome } from './shell-fixture';
import { initialHomeMode, nextHomeMode } from '../../src/lib/home/mode';

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
});
afterEach(() => { setDebugMode(false); vi.useRealTimers(); vi.restoreAllMocks(); });

describe('host live-session regressions', () => {
  it('keeps local top-fight coverage complete when only a remote source is estimated', async () => {
    setDebugMode(true);
    const summary = homeSummary();
    const top = summary.top_fight!;
    top.complete_capture = true; top.issues = []; summary.issues = [];
    const local = top.players.find(player => player.is_local)!;
    local.sources = local.sources.map(source => ({ ...source, estimated: false, reasons: [] }));
    top.players.push({ ...structuredClone(local), object_id: 99, player_id: 'remote-player', is_local: false,
      sources: [{ ...local.sources[0], estimated: true, reasons: ['nearest-removed-hit-target'] }] });
    const props = { snapshot: homeSnapshot(), history: { phase: 'ready' as const, summary, message: '', retryAvailable: true }, choice: 'session' as const, retry: () => {} };
    const page = render(NexusHome, props);
    await flushHomeRender();
    expect(screen.getByText('Top Active DPS')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /Top fight source/ })).toBeNull();
    expect(screen.queryByText('Top fight has partial observations.')).toBeNull();
    expect(screen.queryByText('Partial or estimated fight observations.')).toBeNull();
    const estimated = structuredClone(summary);
    estimated.top_fight!.players.find(player => player.is_local)!.sources[0] = {
      ...local.sources[0], estimated: true, reasons: ['client-projectile-reconstruction'],
    };
    await page.rerender({ ...props, history: { ...props.history, summary: estimated } });
    await flushHomeRender();
    expect(screen.getAllByRole('button', { name: /Top fight source/ })).toHaveLength(1);
    expect(screen.getByText('Top fight has partial observations.')).toBeTruthy();
    expect(screen.getByText('Partial or estimated fight observations.')).toBeTruthy();
    expect(page.container.textContent).not.toContain('Source inferred from the nearest recently killed enemy');
  });
  it('keeps history fresh through heartbeats and ordinary snapshots, but gates a run end on durable storage', async () => {
    const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
    const page = mountHome(snapshot);
    let durable = 12;
    page.client.queryStore = vi.fn(async () => saved(String(durable), snapshot.stream_id));
    await flushHomeRender();
    expect(page.query).toHaveBeenCalledOnce();
    expect(within(screen.getByRole('main')).getByRole('heading', { name: snapshot.map! })).toBeTruthy();
    expect(screen.getByRole('status', { name: 'Background activity' }).textContent).not.toContain('Refreshing history');
    for (let sequence = 13; sequence <= 36; sequence++) {
      durable = sequence - 1; // A healthy store stays one traffic frame behind.
      page.transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: String(sequence), at: snapshot.at,
        event: { session_updated: { ...snapshot.session!, last_traffic: String(sequence * 250000) } } });
      page.transport.publishSnapshot({ ...snapshot, sequence: String(sequence) });
      await vi.advanceTimersByTimeAsync(250);
      await flushHomeRender();
      expect(screen.getByRole('status', { name: 'Background activity' }).textContent).not.toContain('Refreshing history');
      expect(screen.queryByText('Waiting for saved history to catch up.')).toBeNull();
    }
    expect(page.query).toHaveBeenCalledOnce();
    expect(page.client.queryStore).toHaveBeenCalled();
    page.transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: '37', at: snapshot.at,
      event: { run_ended: homeSummary().recent_runs[0] } });
    await flushHomeRender();
    expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toContain('Refreshing history');
    await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
    expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toContain('Refreshing history');
    expect(page.query).toHaveBeenCalledOnce();
    durable = 37;
    await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
    expect(page.query).toHaveBeenCalledTimes(2);
    expect(screen.getByRole('status', { name: 'Background activity' }).textContent).not.toContain('Refreshing history');
  });

  it.each([
    ['Live', 'nexus', 'Nexus', 'Nexus'],
    ['Live', 'unknown', 'Synthetic Vault', 'Synthetic Vault'],
    ['Live', 'unknown', null, 'Live'],
    ['Stopped', 'unknown', null, 'Ready'],
    ['Recovering', 'unknown', null, 'Reconnecting'],
  ] as const)('shows the correct %s / %s hero (%s)', async (state, area_kind, map, heading) => {
    const snapshot = homeSnapshot({ area_kind, map, run: null });
    snapshot.capture.state = state;
    mountHome(snapshot);
    await flushHomeRender();
    expect(within(screen.getByRole('main')).getByRole('heading', { name: heading })).toBeTruthy();
    expect(screen.queryByText(/Start capture and enter a realm\./)).toBeNull();
    expect(screen.queryByText(/Waiting for your location to be identified/)).toBeNull();
  });

  it('shows Nexus Home without extra status text while waiting without a session', async () => {
    const snapshot = homeSnapshot({ area_kind: 'unknown', map: null, run: null, session: null });
    snapshot.capture.state = 'WaitingForGame';
    mountHome(snapshot);
    await flushHomeRender();
    const main = screen.getByRole('main');
    expect(main.textContent).not.toMatch(/Waiting for RotMG|Capture Offline|No Session|Reconnecting|Ready/);
    expect(within(main).getByRole('heading', { name: 'Nexus' })).toBeTruthy();
    expect(within(main).getByRole('group', { name: 'History Period' })).toBeTruthy();
  });

  it('keeps Nexus Home first through Live → WaitingForGame → Live', async () => {
    const snapshot = homeSnapshot({ area_kind: 'nexus', map: 'Nexus', run: null });
    const page = mountHome(snapshot);
    await flushHomeRender();
    const idle = screen.getByRole('main').querySelector('.home-idle')!;
    const first = idle.firstElementChild;
    expect(first?.classList.contains('nexus-home')).toBe(true);
    for (const state of ['WaitingForGame', 'Live'] as const) {
      page.transport.publishCapture({ ...snapshot.capture, state });
      await flushHomeRender();
      expect(idle.firstElementChild).toBe(first);
      expect(idle.children).toHaveLength(1);
      expect(within(screen.getByRole('main')).getByRole('heading', { name: 'Nexus' })).toBeTruthy();
    }
  });

  it('classifies a live unknown map without a run as a non-stale hub', () => {
    const snapshot = homeSnapshot({ area_kind: 'unknown', run: null });
    expect(nextHomeMode(initialHomeMode(), snapshot, snapshot.capture, 0)).toMatchObject({ mode: 'nexus', stale: false });
    expect(nextHomeMode(initialHomeMode(), snapshot, { ...snapshot.capture, state: 'Recovering' }, 0).stale).toBe(true);
  });

  it('deduplicates coverage notes and displays one estimated badge without raw codes in text or tooltips', async () => {
    setDebugMode(true);
    const summary = homeSummary();
    const top = summary.top_fight!;
    const source = top.players[0].sources[0];
    top.players[0].sources = [
      { ...source, estimated: true, reasons: ['client-projectile-reconstruction', 'client-projectile-reconstruction', 'unknown-private-code'] },
      { ...source, estimated: true, reasons: ['client-projectile-reconstruction', 'unknown-private-code'] },
    ];
    top.issues = ['missing_stats', 'missing_stats'];
    summary.issues = ['missing_stats', 'stream_gap'];
    const page = mountHome(undefined, summary);
    await flushHomeRender();
    const badges = screen.getAllByRole('button', { name: /Top fight source.*estimated/i });
    expect(badges).toHaveLength(1);
    const disclosure = screen.getByText('Coverage notes (4)');
    await fireEvent.click(disclosure); await flushHomeRender();
    const notes = within(disclosure.closest('details')!).getAllByRole('listitem');
    expect(notes.map(note => note.textContent)).toEqual([
      'Some stats are unavailable', 'Some traffic was missed',
      'Damage attributed from client-side projectile reconstruction',
      'Attribution is uncertain based on the available captured evidence.',
    ]);
    await fireEvent.focusIn(badges[0]); await flushHomeRender();
    expect(screen.getByRole('tooltip').textContent).toContain('Damage attributed from client-side projectile reconstruction');
    expect(page.container.innerHTML).not.toMatch(/client-projectile-reconstruction|unknown-private-code|missing_stats|stream_gap/);
  });
});
