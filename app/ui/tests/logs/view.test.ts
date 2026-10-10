import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import RouteFrame from '../../src/routes/RouteFrame.svelte';

afterEach(cleanup);
it('renders Logs chat and signals unavailable rows through the route frame', async () => {
  const onRowsReady = vi.fn();
  render(RouteFrame, { route: { kind: 'logs', view: 'chat', channel: 'all' }, snapshot: null, entryKey: 'chat', onRowsReady });
  await waitFor(() => expect(onRowsReady).toHaveBeenCalledExactlyOnceWith('chat'));
  expect(screen.getByRole('navigation', { name: 'Logs views' })).toBeTruthy();
  expect(screen.getByRole('navigation', { name: 'Chat channels' })).toBeTruthy();
  expect(screen.getByText('Chat unavailable')).toBeTruthy();
});
it.each(['keys', 'events'] as const)('releases unavailable %s rows after rendering', async view => {
  const onRowsReady = vi.fn();
  render(RouteFrame, { route: { kind: 'logs', view, channel: 'all' }, snapshot: null, entryKey: view, onRowsReady });
  await waitFor(() => expect(onRowsReady).toHaveBeenCalledExactlyOnceWith(view));
  expect(screen.getByRole('heading', { name: view === 'keys' ? 'Key pops unavailable' : 'Events unavailable' })).toBeTruthy();
  if (view === 'keys') {
    expect(screen.getByRole('region', { name: 'Dungeons' })).toBeTruthy();
    expect(screen.getByRole('textbox', { name: 'Find who opened it' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Opened by me' }).getAttribute('aria-pressed')).toBe('false');
  }
  else {
    expect(screen.getByRole('group', { name: 'Event kind' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'All —' }).getAttribute('aria-pressed')).toBe('true');
  }
});
it('hides alert preferences in Logs while preserving the standalone Alerts route', () => {
  render(RouteFrame, { route: { kind: 'logs', view: 'alerts', channel: 'all' }, snapshot: null });
  expect(screen.getByRole('region', { name: 'Alert log' })).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Presentation' })).toBeNull();
  cleanup();
  render(RouteFrame, { route: { kind: 'alerts' }, snapshot: null });
  expect(screen.getByRole('heading', { name: 'Presentation' })).toBeTruthy();
});
