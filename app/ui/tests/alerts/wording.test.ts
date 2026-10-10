import { alertTitle } from '../../src/lib/alerts/presentation';
import { render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, expect, it } from 'vitest';
import AlertLog from '../../src/lib/alerts/AlertLog.svelte';
import { setDebugMode } from '../../src/lib/debug/debug';
import { alert } from '../phase2/fixtures';

afterEach(() => setDebugMode(false));

it('shows alert values in production and reveals evidence only in Debug', async () => {
  render(AlertLog, { rows: [alert({ kind: 'white_bag', explanation: 'Estimated bag source.',
    details: { type: 'white_bag', drop_id: 'bag-1', bag_type: 200, ordinal: 3, window: null,
      source_enemy: null, source_estimated: true, contents_known: false } })] });
  expect(screen.getByText('Weekly Count: 3')).toBeTruthy();
  expect(screen.queryByText('Estimated bag source.')).toBeNull();
  expect(document.querySelector('[data-debug-note]')).toBeNull();
  setDebugMode(true);
  await tick();
  expect(screen.getByText('Estimated bag source.')).toBeTruthy();
  expect(screen.getByText(/Source estimated/)).toBeTruthy();
  setDebugMode(false);
  await tick();
  expect(document.querySelector('[data-debug-note]')).toBeNull();
});


it('key pops are intentionally unlinked even in Debug', () => {
  setDebugMode(true);
  const { container } = render(AlertLog, { rows: [alert({ kind: 'key_popped', run_id: null,
    explanation: 'Lost Halls key popped', details: { type: 'key_popped', dungeon: 'Lost Halls' } })] });
  expect(container.querySelector('a')).toBeNull();
  expect(container.textContent).not.toContain('relationship unavailable');
  expect(screen.getByRole('article', { name: 'Key Popped' })).toBeTruthy();
});

it('shows run-time PB wording and links the saved run', () => {
  const view = alert({ kind: 'personal_best', run_id: 'faster-run', details: {
    type: 'run_time_personal_best', dungeon: 'Lair of Shaitan', duration_micros: 86_000_000, previous_micros: 87_000_000,
  } });
  render(AlertLog, { rows: [view] });
  expect(screen.getByText('New PB: Lair of Shaitan 1:26 (−0:01)')).toBeTruthy();
  expect(screen.getByRole('link', { name: 'View run' }).getAttribute('href')).toContain('faster-run');
  expect(screen.queryByRole('link', { name: 'View fight' })).toBeNull();
});

it('titles chat alerts with channel and sender', () => {
  const base = { id: 'a', at: '1', run_id: null, explanation: '', presentation: null };
  expect(alertTitle({ ...base, kind: 'whisper', details: { type: 'chat', channel: 'whisper', sender: 'SynthAlpha', excerpt: 'hi' } })).toBe('Whisper from SynthAlpha');
  expect(alertTitle({ ...base, kind: 'guild_message', details: { type: 'chat', channel: 'guild', sender: 'SynthBravo', excerpt: 'hi' } })).toBe('Guild: SynthBravo');
});

it('shows the chat excerpt in the in-app banner without Debug', async () => {
  const { default: Banner } = await import('../../src/lib/components/Banner.svelte');
  const view = alert({ kind: 'whisper', details: { type: 'chat', channel: 'whisper', sender: 'SynthAlpha', excerpt: 'need a carry?' } });
  render(Banner, { props: { id: 'chat', kind: 'whisper', title: 'Whisper from SynthAlpha', context: '', view, presentation: 'toast', onDismiss: () => {} } });
  expect(screen.getByText('need a carry?')).toBeTruthy();
});
