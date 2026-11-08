import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import Shell from '../../src/lib/shell/Shell.svelte';
import { setupFixture } from '../setup/fixture';
import { fakeHistory } from '../navigation/fake-history';

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it('retries failed storage before capture can start', async () => {
  const { client } = setupFixture();
  const ready = await client.queryStore();
  let failed = true;
  client.queryStore = vi.fn(async () => failed ? { ...ready, state: 'failed' as const, queued: 1 } : ready);
  client.retryStore = vi.fn(async () => { failed = false; });
  client.startCapture = vi.fn(async () => { if (failed) throw new Error('Pending summaries are not saved; retry storage before starting capture'); });
  render(Shell, { client, historyAdapter: fakeHistory('#/').adapter });
  await fireEvent.click(await screen.findByRole('button', { name: 'Retry saving' }));
  await waitFor(() => expect(client.retryStore).toHaveBeenCalledOnce());
  await waitFor(() => expect(screen.queryByRole('button', { name: 'Retry saving' })).toBeNull());
  await fireEvent.click(screen.getByRole('button', { name: 'Start Capture' }));
  await waitFor(() => expect(client.startCapture).toHaveBeenCalledOnce());
  expect(failed).toBe(false);
});

it('unsaved close stays open until the user explicitly confirms discard', async () => {
  const { client } = setupFixture();
  let warn = () => {};
  client.onUnsavedClose = vi.fn(async receive => { warn = receive; return () => {}; });
  client.exitWithoutSaving = vi.fn(async () => {});
  render(Shell, { client, historyAdapter: fakeHistory('#/').adapter });
  await waitFor(() => expect(client.onUnsavedClose).toHaveBeenCalledOnce());
  warn();
  await fireEvent.click(await screen.findByRole('button', { name: 'Keep Maxed open' }));
  expect(client.exitWithoutSaving).not.toHaveBeenCalled();
  warn();
  await fireEvent.click(await screen.findByRole('button', { name: 'Exit without saving' }));
  expect(client.exitWithoutSaving).toHaveBeenCalledOnce();
});
