import { cleanup, render, screen } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { afterEach, expect, it, vi } from 'vitest';
import AlertPreferences from '../../src/lib/alerts/AlertPreferences.svelte';
import { createAlertSession } from '../../src/lib/alerts/session';
import StatusOverlay from '../../src/lib/status/StatusOverlay.svelte';
import { createStatusRegistry, STATUS_REGISTRY } from '../../src/lib/status/registry';
import type { AppSettings } from '../../src/lib/bindings/AppSettings';
import { deferred, microtasks, preferences } from './fixtures';

afterEach(cleanup);
const settings = (): AppSettings => ({ party: { presets: [], default_preset_id: null }, version: 1, adapter_id: null, assets_path: null,
  recap_image: { names: true, loot: true, fights: true },
  save_recordings: false, recording_limit_bytes: 1000000000, update_channel: 'stable',
  hardware_acceleration: false, alerts: preferences(), chat: { store: { public: true, guild: true, party: true, whisper: true }, retention_days: 90, hidden_senders: [], hidden_keywords: [], filter_spam: true }, loot: { chase: [] } });

it('publishes explicit loading, failed, retry and ready states', async () => {
  const read = deferred<AppSettings>();
  const querySettings = vi.fn().mockImplementationOnce(() => read.promise).mockResolvedValue(settings());
  const session = createAlertSession({ querySettings, updateAlertPreferences: vi.fn() });
  const pending = session.refresh();
  await microtasks();
  expect(get(session.status)).toBe('loading');
  const failure = expect(pending).rejects.toThrow('Saved alert preferences could not be read.');
  read.reject(new Error('Synthetic read failure')); await failure;
  expect(get(session.status)).toBe('failed');
  expect(get(session.preferences)).toBeNull();
  await session.refresh();
  expect(get(session.status)).toBe('ready');
  expect(get(session.preferences)).toEqual(preferences());
  session.dispose();
});

it('hides activity while disabled and after retirement, including late read completion', async () => {
  const read = deferred<AppSettings>();
  const session = createAlertSession({ querySettings: () => read.promise, updateAlertPreferences: vi.fn() });
  const registry = createStatusRegistry();
  render(StatusOverlay, { registry });
  const panel = render(AlertPreferences, { props: { session }, context: new Map([[STATUS_REGISTRY, registry]]) });
  const pending = session.refresh(); await microtasks();
  const overlay = screen.getByRole('status', { name: 'Background activity' });
  expect(overlay.textContent).toBe('Loading alert preferences');
  await panel.rerender({ enabled: false });
  expect(overlay.textContent).toBe('');
  await panel.rerender({ enabled: true });
  expect(overlay.textContent).toBe('Loading alert preferences');
  session.dispose(); await microtasks();
  expect(overlay.textContent).toBe('');
  read.resolve(settings()); await pending; await microtasks();
  expect(get(session.status)).toBe('retired');
  expect(get(session.preferences)).toBeNull();
  expect(overlay.textContent).toBe('');
});
