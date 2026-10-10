import type { Client } from '../bridge/client';

/** The native history reader is shared by pages, summaries and selected-run cards. */
export function serializeEventReads(client: Client): Client {
  let tail: Promise<unknown> = Promise.resolve();
  function queue<T>(read: () => Promise<T>): Promise<T> {
    const result = tail.then(read);
    tail = result.catch(() => {});
    return result;
  }
  return {
    ...client,
    listInstanceEvents: filter => queue(() => client.listInstanceEvents(filter)),
    getEventSummary: request => queue(() => client.getEventSummary(request)),
    getRun: id => queue(() => client.getRun(id)),
    getChatOverview: request => queue(() => client.getChatOverview(request)),
  };
}
