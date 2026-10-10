import { expect, it, vi } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/svelte';
import Shell from '../src/lib/shell/Shell.svelte';
import { setupFixture, deferred } from './setup/fixture';
import { fakeHistory } from './navigation/fake-history';
import { flushHomeRender } from './home/shell-fixture';
import snapshot from './fixtures/t0-live';

it('connection-error page emits no interactive receipt', async () => {
  const f = setupFixture();
  f.client.onSnapshot = vi.fn(async () => { throw new Error('broken pipe'); });
  f.client.reportInteractiveRail = vi.fn(async () => {});
  render(Shell, { client: f.client, historyAdapter: fakeHistory('#/').adapter });
  await screen.findByText('Disconnected. Reconnect.');
  await new Promise(resolve => setTimeout(resolve, 50));
  expect(f.client.reportInteractiveRail).not.toHaveBeenCalled();
});

it('interactive_marker_requires_rendered_rail', async () => {
  vi.useFakeTimers();
  const frames = new Map<number, FrameRequestCallback>();
  let sequence = 0;
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    frames.set(++sequence, callback); return sequence;
  });
  vi.stubGlobal('cancelAnimationFrame', (id: number) => frames.delete(id));
  const paint = async () => {
    const pending = [...frames.values()]; frames.clear();
    pending.forEach(callback => callback(performance.now()));
    await flushHomeRender();
  };
  try {
    const f = setupFixture();
    const marker = vi.fn(async () => { throw new Error('private measurement failure'); });
    f.client.reportInteractiveRail = marker;
    const loading = deferred<typeof snapshot>();
    f.client.querySnapshot = vi.fn(() => loading.promise);
    const page = render(Shell, { client: f.client, historyAdapter: fakeHistory('#/').adapter });
    await flushHomeRender(); await paint(); await paint();
    expect(marker).not.toHaveBeenCalled(); // The loading rail exists, but is not ready.
    loading.resolve(snapshot);
    await flushHomeRender();
    // Error between frame callbacks cancels the pending success.
    f.capture('Failed'); await flushHomeRender(); await paint(); await paint();
    expect(marker).not.toHaveBeenCalled();
    f.capture('Stopped'); await flushHomeRender();
    const rail = screen.getByRole('complementary', { name: 'Character' });
    const parent = rail.parentElement!;
    const sibling = rail.nextSibling;
    rail.remove(); await paint(); await paint();
    expect(marker).not.toHaveBeenCalled(); // Missing actual DOM rail.
    parent.insertBefore(rail, sibling);
    f.capture('Failed'); await flushHomeRender();
    f.capture('Live'); await flushHomeRender();
    await paint(); expect(marker).not.toHaveBeenCalled();
    await paint(); expect(marker).toHaveBeenCalledTimes(1);
    expect(screen.queryByText('private measurement failure')).toBeNull();
    await fireEvent.click(screen.getByRole('link', { name: 'Runs' }));
    f.capture('Stopped'); await flushHomeRender(); await paint(); await paint();
    expect(marker).toHaveBeenCalledTimes(1);
    page.unmount(); await flushHomeRender();
    render(Shell, { client: f.client, historyAdapter: fakeHistory('#/').adapter });
    await flushHomeRender(); await paint(); await paint();
    expect(marker).toHaveBeenCalledTimes(1); // Same window lifetime, including remount.
  } finally {
    cleanup(); vi.unstubAllGlobals(); vi.useRealTimers();
  }
});
