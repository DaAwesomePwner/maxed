import type { Client } from '../bridge/client';

/** Share one queue between the page, sender, overview, picker and run chip. */
export function serializeChatReads(client: Client): Client {
  let tail: Promise<unknown> = Promise.resolve();
  function queue<T>(read: () => Promise<T>): Promise<T> {
    const result = tail.then(read);
    tail = result.catch(() => {});
    return result;
  }
  return {
    ...client,
    listChat: filter => queue(() => client.listChat(filter)),
    getChatOverview: request => queue(() => client.getChatOverview(request)),
    getChatSender: request => queue(() => client.getChatSender(request)),
    listWhisperPartners: filter => queue(() => client.listWhisperPartners(filter)),
    getRun: id => queue(() => client.getRun(id)),
  };
}
