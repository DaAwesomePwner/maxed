import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeEach, expect, it, vi } from 'vitest';
import PartyPreview from './PartyPreview.svelte';
import { createPartyPreview } from './party-scenario';
import { flush } from '../runs/fixtures';

beforeEach(() => { vi.useFakeTimers(); window.history.replaceState(null, '', '/?preview=party'); });

it.each(['default', 'stress'] as const)('mounts the real dungeon Live roster with all synthetic states: %s', async variant => {
  window.history.replaceState(null, '', `/?preview=party&variant=${variant}`);
  render(PartyPreview); await flush();
  expect(location.hash).toBe('#/party');
  const table = within(screen.getByRole('table', { name: 'Party roster' }));
  expect(table.getAllByRole('row')).toHaveLength((variant === 'stress' ? 85 : 12) + 1);
  for (const text of ['Pass', 'Fail', 'Check', 'Not seen', 'No damage seen', 'Mostly trash', 'Watched', 'Banned', 'No name yet', 'You']) {
    expect(table.getAllByText(text).length).toBeGreaterThan(0);
  }
  expect(table.getByText(/^Out of view since/)).toBeTruthy();
  expect(table.getByText('Left', { exact: true })).toBeTruthy();
  const failed = table.getByText('SynthBravo').closest('tr')!;
  expect(failed.textContent).toContain('5/8 < 6/8');
  expect(failed.textContent).toContain('Mostly trash');
  expect(within(screen.getByRole('region', { name: 'Party summary' })).queryByText('Left early')).toBeNull();
});

it.each(['#/party/runs/synth-party-run', '#/party/players', '#/party/players/synthalpha', '#/party/presets'])
  ('preserves the %s deep link through startup', async hash => {
    window.history.replaceState(null, '', `/?preview=party${hash}`);
    render(PartyPreview); await vi.advanceTimersByTimeAsync(2000); await flush();
    expect(location.hash).toBe(hash);
    if (hash.includes('/runs/')) {
      const roster = within(screen.getByRole('table', { name: 'Party roster' }));
      expect(roster.getByText('Left early')).toBeTruthy();
      expect(roster.queryByText(/^Out of view/)).toBeNull();
    } else if (hash.includes('/players')) {
      expect(within(screen.getByRole('table', { name: 'Party players' })).getByText('SynthAlpha')).toBeTruthy();
      if (hash.endsWith('/synthalpha')) {
        const panel = within(screen.getByRole('complementary', { name: 'Player details' }));
        expect(panel.getByRole('heading', { name: 'SynthAlpha' })).toBeTruthy();
        expect(panel.getAllByRole('link', { name: /Synthetic Sanctum/ })).toHaveLength(3);
      }
    } else {
      expect(screen.getByRole('button', { name: 'Edit Synthetic standard' })).toBeTruthy();
      expect(screen.getByRole('button', { name: 'Edit Synthetic practice' })).toBeTruthy();
    }
  });

it('navigates Live, saved run, Players and Presets through the real Shell', async () => {
  render(PartyPreview); await flush();
  await fireEvent.change(screen.getByRole('combobox', { name: 'Party scope' }), { target: { value: 'synth-party-run' } }); await flush();
  expect(location.hash).toBe('#/party/runs/synth-party-run');
  await fireEvent.click(screen.getByRole('link', { name: 'Players' })); await flush();
  await fireEvent.click(within(screen.getByRole('table', { name: 'Party players' })).getByText('SynthAlpha')); await flush();
  expect(screen.getByRole('complementary', { name: 'Player details' })).toBeTruthy();
  await fireEvent.click(screen.getByRole('link', { name: 'Presets' })); await flush();
  expect(screen.getByRole('button', { name: 'Edit Synthetic practice' })).toBeTruthy();
});

it('keeps preview writes in memory, returns independent DTOs and never opens RealmEye', async () => {
  const preview = createPartyPreview('default');
  const reply = await preview.client.getPartyLive();
  if (reply.status !== 'ready') throw new Error('Expected synthetic roster');
  reply.value.members[0].name = 'SynthChanged';
  expect(JSON.stringify(await preview.client.getPartyLive())).not.toContain('SynthChanged');
  await preview.client.setPartyMark('synthalpha', 'SynthAlpha', 'watched', 'Synthetic note');
  const detail = await preview.client.getPartyPlayer('synthalpha', null);
  expect(detail).toMatchObject({ status: 'ready', value: { row: { mark: 'watched', note: 'Synthetic note' } } });
  await expect(preview.client.openRealmeye('SynthAlpha')).rejects.toThrow('Preview only');
  await preview.client.clearPartyHistory({ marks_and_notes: false });
  expect(await preview.client.getPartyPlayers('', 'all', 'last_seen', null)).toMatchObject({
    status: 'ready', value: { rows: ['synthalpha', 'synthban', 'synthwatch'].map(key => expect.objectContaining({ key, runs_together: 0 })) },
  });
  await preview.client.clearPartyHistory({ marks_and_notes: true });
  expect(await preview.client.getPartyPlayers('', 'all', 'last_seen', null)).toMatchObject({ status: 'ready', value: { rows: [] } });
});

it('retains unknown overflow totals and meaningful contribution flags in stress mode', async () => {
  const { client } = createPartyPreview('stress');
  const reply = await client.getPartyLive();
  if (reply.status !== 'ready') throw new Error('Expected synthetic roster');
  expect(reply.value.members.filter(member => member.damage == null)).toHaveLength(21);
  for (const member of reply.value.members) {
    if (member.damage == null) { expect(member.share).toBeNull(); expect(member.flags).not.toContain('no_damage_seen'); }
    if (member.flags.includes('mostly_trash')) {
      expect(member.share).toBeGreaterThanOrEqual(.01);
      expect(member.boss_share).toBeLessThan(.25 * member.share!);
    }
  }
});

it('saves a preset in memory and applies the selected requirements on Live', async () => {
  render(PartyPreview); await flush();
  await fireEvent.change(screen.getByRole('combobox', { name: 'Preset' }), { target: { value: 'synth-practice' } });
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(within(screen.getByRole('table', { name: 'Party roster' })).getByText('SynthBravo').closest('tr')?.textContent).toContain('Pass');
  await fireEvent.click(screen.getByRole('link', { name: 'Presets' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Edit Synthetic practice' })); await flush();
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Synthetic training' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save' })); await flush();
  expect(screen.getByRole('button', { name: 'Edit Synthetic training' })).toBeTruthy();
});
