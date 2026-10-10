import { expect, test, vi } from 'vitest';
import fixture from './fixtures/t0-live';
import type { LiveSnapshot } from '../src/lib/bindings/LiveSnapshot';
import { render, screen } from '@testing-library/svelte';
import App from '../src/App.svelte';
import { createFixtureTransport } from '../src/lib/bridge/fixture';
import { flushHomeRender } from './home/shell-fixture';

const mocks = vi.hoisted(() => ({ listen: vi.fn(), invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke, Channel: class { onmessage = (_value: ArrayBuffer) => {}; } }));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
import { nativeTransport } from '../src/lib/bridge/native';

test('native live subscriptions decode byte channels and release native ownership', async () => {
  let channel!: { onmessage(value: ArrayBuffer): void };
  mocks.invoke.mockImplementation(async (command, args) => {
    if (command === 'subscribe_live_payload') { channel = args.channel; return 19; }
  });
  mocks.listen.mockResolvedValueOnce(() => {});
  const receive = vi.fn();
  const stop = await nativeTransport.listen('live-snapshot', receive);
  expect(channel).toBeDefined();
  channel.onmessage(new TextEncoder().encode(JSON.stringify(fixture)).buffer);
  expect(receive).toHaveBeenCalledExactlyOnceWith(fixture);
  stop();
  expect(mocks.invoke).toHaveBeenCalledWith('unsubscribe_live_payload', { id: 19 });
  channel.onmessage(new TextEncoder().encode(JSON.stringify(fixture)).buffer);
  expect(receive).toHaveBeenCalledTimes(1);
});

test('the production App uses the JSON-decoding native transport', async () => {
  const initial: LiveSnapshot = structuredClone(fixture);
  initial.capture.state = 'Live'; initial.character!.name = 'Initial local';
  const transport = createFixtureTransport(initial);
  const callbacks = new Map<string, (message: { payload: unknown }) => void>();
  mocks.invoke.mockImplementation((command, args) => command === 'subscribe_live_payload' ? Promise.resolve(20) : command === 'unsubscribe_live_payload' ? Promise.resolve() : transport.invoke(command, args));
  mocks.listen.mockImplementation((event, receive) => { callbacks.set(event, receive); return Promise.resolve(() => { callbacks.delete(event); }); });
  const page = render(App);
  try {
    for (let i = 0; i < 5; i++) await flushHomeRender();
    expect(screen.getByText('Initial local')).toBeTruthy();
    const next = structuredClone(initial); next.sequence = '13'; next.character!.name = 'Updated local';
    callbacks.get('live-snapshot')!({ payload: JSON.stringify(next) });
    for (let i = 0; i < 5; i++) await flushHomeRender();
    expect(screen.getByText('Updated local')).toBeTruthy();
  } finally { page.unmount(); }
});

test.each(['live-snapshot', 'engine-event'] as const)('decodes JSON text for %s without changing the logical payload', async event => {
  mocks.invoke.mockResolvedValue(21);
  let callback!: (message: { payload: unknown }) => void;
  const release = vi.fn();
  mocks.listen.mockImplementationOnce((_event, receive) => { callback = receive; return Promise.resolve(release); });
  const receive = vi.fn();
  const stop = await nativeTransport.listen(event, receive);
  const payload = event === 'live-snapshot' ? fixture : {
    stream_id: fixture.stream_id, sequence: '9007199254740993', at: fixture.at,
    event: { fight_updated: fixture.fights[0] },
  };
  callback({ payload: JSON.stringify(payload) });
  expect(receive).toHaveBeenLastCalledWith(payload);
  callback({ payload }); // Original before-mode and low-frequency typed events.
  expect(receive).toHaveBeenLastCalledWith(payload);
  stop(); expect(release).toHaveBeenCalledOnce();
});

test('a byte-channel event batch remains one callback with its complete range', async () => {
  let channel!: { onmessage(value: ArrayBuffer): void };
  mocks.invoke.mockImplementation(async (command, args) => {
    if (command === 'subscribe_live_payload') { channel = args.channel; return 22; }
  });
  mocks.listen.mockResolvedValueOnce(() => {});
  const receive = vi.fn(), stop = await nativeTransport.listen('engine-event', receive);
  const events = Array.from({ length: 1536 }, (_, i) => ({ stream_id: fixture.stream_id,
    sequence: String(i + 11), at: fixture.at, event: { fight_ended: fixture.fights[0] } }));
  const batch = { stream_id: fixture.stream_id, first_sequence: '11', last_sequence: '1546', events };
  channel.onmessage(new TextEncoder().encode(JSON.stringify(batch)).buffer);
  expect(receive).toHaveBeenCalledExactlyOnceWith(batch);
  stop();
});
