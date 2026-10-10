import { expect, test, vi } from 'vitest';
import { createClient } from '../src/lib/bridge/client';
import { createFixtureTransport } from '../src/lib/bridge/fixture';
import snapshot from './fixtures/t0-live';

test('setup queries preserve passive calls and forward only explicit retry options', async () => {
  const transport = createFixtureTransport(snapshot);
  const invoke = vi.spyOn(transport, 'invoke');
  const client = createClient(transport);

  await client.querySetup();
  await client.querySetup({ retry: true });
  await client.querySetup({ retry: false });
  await client.querySetup();

  expect(invoke.mock.calls).toEqual([
    ['get_setup_status', undefined],
    ['get_setup_status', { retry: true }],
    ['get_setup_status', { retry: false }],
    ['get_setup_status', undefined],
  ]);
});
