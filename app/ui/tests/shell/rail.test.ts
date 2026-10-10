import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setDebugMode } from '../../src/lib/debug/debug';
afterEach(() => setDebugMode(false));
import type { CaptureState } from '../../src/lib/bindings/CaptureState';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import type { ObjectVisual } from '../../src/lib/bindings/ObjectVisual';
import { captureLabels, railView, selectBossFight } from '../../src/lib/shell/rail';
import HeroCard from '../../src/lib/shell/HeroCard.svelte';
import LiveStatusCard from '../../src/lib/shell/LiveStatusCard.svelte';
import fixture from '../fixtures/t0-live';

const known = (): LiveSnapshot => ({ ...structuredClone(fixture), fights: fixture.fights.map(row => ({ ...structuredClone(row), target_kind: 'boss' })), character: { ...structuredClone(fixture.character), identity_verified: true } });
// Fixed strings from runtime.rs's WebView boundary; preserve the native remedy.
const nativeMessages: Record<CaptureState, string> = {
  Stopped: 'Capture stopped.',
  NpcapMissing: 'Install Npcap, then retry capture.',
  NpcapIncompatible: 'Repair or update Npcap, then retry capture.',
  PermissionDenied: 'Check Npcap access settings, then retry capture.',
  WaitingForGame: 'Start the game and enter a realm. Reconnect if capture started during play.',
  Live: 'Game packets are being decoded.',
  Recovering: 'Waiting for decoder recovery. Reconnect if this continues.',
  Failed: 'Capture could not continue. Check Npcap and your network adapter, then retry.',
};
describe('rail projection', () => {
  it.each(['quest', 'miniboss', 'boss', 'event_boss'] as const)('keeps the focused live boss when a %s dies later in the same tick', target_kind => {
    const snapshot = known(), boss = { ...snapshot.fights[0], last_hit: '5000' };
    const killed = { ...boss, id: 'killed-target', target_kind, ended: '5100',
      kill: { at: '5100', provisional: false, evidence: 'hp_zero_removed' as const } };
    const context = { area: snapshot.area_kind, at: '5100', character: snapshot.character };
    expect(selectBossFight([boss, killed], boss.id, context)?.id).toBe(boss.id);
  });
  it.each(['quest', 'miniboss'] as const)('does not let a killed %s displace an open boss without live prior focus', target_kind => {
    const snapshot = known(), boss = { ...snapshot.fights[0], last_hit: '5000' };
    const killed = { ...boss, id: 'killed-target', target_kind, ended: '5100',
      kill: { at: '5100', provisional: false, evidence: 'hp_zero_removed' as const } };
    const context = { area: snapshot.area_kind, at: '5100', character: snapshot.character };
    for (const previous of [null, killed.id]) {
      expect(selectBossFight([boss, killed], previous, context)?.id).toBe(boss.id);
    }
  });
  it('keeps a confirmed dungeon kill focused until the next target or run end', () => {
    let snapshot = known(); const fight = snapshot.fights[0];
    const killed = { ...fight, ended: snapshot.at,
      kill: { at: snapshot.at, provisional: false, evidence: 'victory_quest_target' as const } };
    snapshot.fights = [killed];
    expect(railView(snapshot).fight?.id).toBe(killed.id);
    snapshot = { ...snapshot, fights: [...snapshot.fights, { ...fight, id: 'older-boss', last_hit: fight.started }] };
    expect(railView(snapshot, [], false, killed.id).fight?.id).toBe(killed.id);
    snapshot = { ...snapshot, fights: [...snapshot.fights, { ...fight, id: 'next-boss', last_hit: String(BigInt(snapshot.at) + 1n) }] };
    expect(railView(snapshot, [], false, killed.id).fight?.id).toBe('next-boss');
    snapshot = { ...snapshot, run: { ...snapshot.run!, ended: snapshot.at } };
    expect(railView(snapshot).fight).toBeNull();
  });
  it('keeps no-character, provisional identity and unknown values explicit', () => {
    expect(railView(null)).toMatchObject({ name: 'No Character', dps: null, rank: null, elapsed: null });
    const provisional = railView(fixture);
    expect(provisional).toMatchObject({ provisional: true, dps: null, rank: null, className: 'Class', sprite: null });
    expect(provisional.character?.maxed).toBeNull();
    const snapshot = known();
    snapshot.character = { ...snapshot.character!, name: null, level: null, fame: null, died_at: '5000000' };
    expect(railView(snapshot)).toMatchObject({ name: 'Character', died: true });
  });
  it('uses supplied catalog labels and supplied personal DPS/rank', () => {
    const visuals: ObjectVisual[] = [{ object_type: 100, name: 'Synthetic Knight', sprite: null, bag: null }, { object_type: 300, name: 'Synthetic target', sprite: null, bag: null }];
    expect(railView(known(), visuals)).toMatchObject({ className: 'Synthetic Knight', target: 'Synthetic target', dps: 150, rank: 1 });
    const snapshot = known(); snapshot.fights[0].players[0].is_local = false;
    expect(railView(snapshot)).toMatchObject({ dps: null, rank: null });
    snapshot.fights[0].players[0].is_local = true;
    snapshot.fights[0].players[0].dps = null; snapshot.fights[0].players[0].rank = null;
    expect(railView(snapshot)).toMatchObject({ dps: null, rank: null });
  });
  it('selects a boss deterministically without prior focus and returns to dungeon totals after it ends', () => {
    let snapshot = known();
    const fight = snapshot.fights[0];
    snapshot.fights = [
      { ...fight, id: 'closed', ended: '9000000', last_hit: '9000000' },
      { ...fight, id: 'b', last_hit: '9007199254740992' },
      { ...fight, id: 'a', last_hit: null, started: '9007199254740993' },
      { ...fight, id: 'z', last_hit: '9007199254740993' },
    ];
    expect(selectBossFight(snapshot.fights)?.id).toBe('a');
    expect(railView(snapshot).dps).toBe(150);
    snapshot = { ...snapshot, fights: snapshot.fights.map((row) => ({ ...row, ended: '9007199254740994' })) };
    expect(railView(snapshot)).toMatchObject({ fight: null, wholeRun: true, target: 'Dungeon Total' });
  });
  it('uses engine microseconds and freezes an ended run', () => {
    const snapshot = known(); snapshot.run!.ended = '4000000'; snapshot.at = '99999999999999999';
    expect(railView(snapshot).elapsed).toBe('0:03');
  });
});

describe('rail presentation', () => {
  it('shows the defeated dungeon target until another boss engages', async () => {
    setDebugMode(true);
    let snapshot = known(); const fight = snapshot.fights[0];
    snapshot.fights = [{ ...fight, ended: snapshot.at,
      kill: { at: snapshot.at, provisional: false, evidence: 'victory_quest_target' } }];
    const props = { view: railView(snapshot), capture: snapshot.capture, busy: false, error: '',
      onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn() };
    const page = render(LiveStatusCard, props);
    expect(page.container.querySelector('.target')?.textContent).toContain('Defeated');
    snapshot = { ...snapshot, fights: [...snapshot.fights, { ...fight, id: 'next-boss', last_hit: String(BigInt(snapshot.at) + 1n) }] };
    await page.rerender({ ...props, view: railView(snapshot, [], false, fight.id) });
    expect(page.container.querySelector('.target')?.textContent).not.toContain('Defeated');
    expect(railView(snapshot, [], false, fight.id).fight?.id).toBe('next-boss');
  });
  it('shows unavailable stats, death and sprite fallback without a decorative tab stop', () => {
    const snapshot = known(); snapshot.character = { ...snapshot.character!, level: null, maxed: null, died_at: '5000000' };
    const { container } = render(HeroCard, { view: railView(snapshot) });
    expect(screen.getAllByText('Unavailable')).toHaveLength(2);
    expect(screen.getByText('Death')).toBeTruthy();
    expect(screen.getByRole('img', { name: 'Class' }).getAttribute('data-fallback')).toBe('true');
    expect(container.querySelector('a, button, [tabindex]')).toBeNull();
  });
  it.each(Object.keys(captureLabels) as CaptureState[])('renders %s with native recovery guidance and diagnostics outside the live region', async (state) => {
    setDebugMode(true);
    render(LiveStatusCard, {
      view: railView(known()), capture: { ...fixture.capture, state, message: nativeMessages[state], dropped_frames: 2, unknown_packets: 3, decode_failures: 4 },
      busy: false, error: '', onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn(),
    });
    expect(screen.getByRole('status').textContent).toBe(captureLabels[state]);
    const help = screen.queryByText('Details');
    if (help) {
      await userEvent.setup().click(help);
      expect(help.closest('details')?.open).toBe(true);
    }
    expect(screen.getByText(nativeMessages[state])).toBeTruthy();
    expect(screen.getByText('Dropped Frames')).toBeTruthy();
    expect(screen.getByRole('status').textContent).not.toContain('150');
    const active = ['Live', 'Recovering', 'WaitingForGame'].includes(state);
    const button = screen.getByRole('button', { name: active ? 'Stop Capture' : /Start Capture|Retry Capture/ });
    expect(button.getAttribute('aria-disabled')).toBe('false');
    expect(screen.getAllByRole('button')).toHaveLength(1);
  });
  it('shows the permanent worker failure remedy and does not offer a futile retry', async () => {
    const start = vi.fn(), stop = vi.fn();
    render(LiveStatusCard, {
      view: railView(known()), capture: { ...fixture.capture, state: 'Failed', message: 'Capture worker failed; restart Maxed' },
      busy: false, error: '', onStart: start, onStop: stop, onRetry: vi.fn(),
    });
    const message = screen.getByText('Restart Maxed');
    expect(screen.queryByText('Capture worker failed; restart Maxed')).toBeNull();
    expect(message.closest('details')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Retry Capture' })).toBeNull();
    const button = screen.getByRole('button', { name: 'Start Capture' });
    expect(button.getAttribute('aria-disabled')).toBe('true');
    await userEvent.setup().click(button);
    expect(screen.queryByRole('button', { name: 'Stop Capture' })).toBeNull();
    expect(start).not.toHaveBeenCalled(); expect(stop).not.toHaveBeenCalled();
  });
});
