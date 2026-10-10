import { expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { serializeChatReads } from '../../src/lib/logs/chat-reader';
import { defaultLogsPosition } from '../../src/lib/navigation/state';
import fixture from '../fixtures/t0-live';
import { deferred } from '../runs/fixtures';

it('queues every Chat read behind the active read and continues after a failure', async () => {
  const client = createClient(createFixtureTransport(fixture));
  const pending = deferred<Awaited<ReturnType<typeof client.listChat>>>();
  client.listChat = vi.fn(() => pending.promise);
  client.getChatOverview = vi.fn().mockRejectedValue(new Error('synthetic'));
  client.getChatSender = vi.fn().mockResolvedValue({ status: 'ready', value: null });
  client.listWhisperPartners = vi.fn().mockResolvedValue({ status: 'ready', value: { rows: [], lines: 0, people: 0 } });
  client.getRun = vi.fn().mockResolvedValue({ status: 'ready', value: null });
  const reader = serializeChatReads(client);
  const page = reader.listChat(defaultLogsPosition().chat.filter);
  const overview = reader.getChatOverview({ run_id: null }).catch(() => null);
  const sender = reader.getChatSender({ name: 'Vexlin' });
  const partners = reader.listWhisperPartners({ query: '', show_hidden: false, limit: 200 });
  const run = reader.getRun('synthetic');
  await Promise.resolve();
  expect(client.listChat).toHaveBeenCalledTimes(1);
  for (const read of [client.getChatOverview, client.getChatSender, client.listWhisperPartners, client.getRun]) expect(read).not.toHaveBeenCalled();
  pending.resolve({ status: 'ready', value: { rows: [], next: null } });
  await Promise.all([page, overview, sender, partners, run]);
  for (const read of [client.listChat, client.getChatOverview, client.getChatSender, client.listWhisperPartners, client.getRun]) expect(read).toHaveBeenCalledTimes(1);
  expect([client.listChat, client.getChatOverview, client.getChatSender, client.listWhisperPartners, client.getRun].map(read => vi.mocked(read).mock.invocationCallOrder[0])).toEqual(
    [client.listChat, client.getChatOverview, client.getChatSender, client.listWhisperPartners, client.getRun].map(read => vi.mocked(read).mock.invocationCallOrder[0]).sort((a, b) => a - b),
  );
});
