import { afterEach, expect, it, vi } from 'vitest';
import { createAlertHistory, type AlertHistoryInput } from '../../src/lib/alerts/history';
import { setNativePresentationVisible } from '../../src/lib/bridge/visibility';

const releases: (() => void)[] = [];
const flush = async () => { for (let i = 0; i < 30; i++) await Promise.resolve(); };
const documentVisible = (visible: boolean) => {
  Object.defineProperty(document, 'visibilityState', { configurable: true, value: visible ? 'visible' : 'hidden' });
  document.dispatchEvent(new Event('visibilitychange'));
};
const input = (required = '12', durable = required): AlertHistoryInput => ({
  filter: { kind: null, range: null, query: '', cursor: null, limit: 50 },
  scopeKey: 'alerts', streamId: 'synthetic-stream', required,
  store: { state: 'ready', queued: 0, durable, stream_id: 'synthetic-stream', message: '' },
});
function setup() {
  const listAlerts = vi.fn().mockResolvedValue({ status: 'ready', value: { rows: [], next: null } });
  const receive = vi.fn();
  const history = createAlertHistory({ listAlerts }, receive);
  releases.push(history.dispose);
  return { listAlerts, receive, history };
}
afterEach(() => {
  releases.splice(0).forEach(release => release());
  documentVisible(true); setNativePresentationVisible(true);
  vi.restoreAllMocks();
});

it.each(['native', 'document'] as const)('pauses hidden reads and coalesces the %s backlog into one catch-up read', async mode => {
  const visible = mode === 'native' ? setNativePresentationVisible : documentVisible;
  visible(false);
  const { history, listAlerts, receive } = setup();
  history.update(input()); await flush();
  history.update(input('13')); await flush();
  history.update(input('14')); await flush();
  expect(listAlerts).not.toHaveBeenCalled();
  visible(true); await flush();
  expect(listAlerts).toHaveBeenCalledOnce();
  expect(receive).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'ready' }));
  visible(false); visible(true); await flush();
  expect(listAlerts).toHaveBeenCalledOnce();
});

it('keeps an older durable checkpoint when the newest hidden update is still saving', async () => {
  setNativePresentationVisible(false);
  const { history, listAlerts } = setup();
  history.update(input('12'));
  history.update(input('13', '12'));
  setNativePresentationVisible(true); await flush();
  expect(listAlerts).toHaveBeenCalledOnce();
  history.update(input('13')); await flush();
  expect(listAlerts).toHaveBeenCalledTimes(2);
});

it('defers pending work when an active read settles while hidden', async () => {
  const { history, listAlerts } = setup();
  let finish!: (value: unknown) => void;
  listAlerts.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  history.update(input()); await flush();
  setNativePresentationVisible(false);
  history.update(input('13'));
  finish({ status: 'ready', value: { rows: [], next: null } }); await flush();
  expect(listAlerts).toHaveBeenCalledOnce();
  setNativePresentationVisible(true); await flush();
  expect(listAlerts).toHaveBeenCalledTimes(2);
});

it('preserves visible updates and the single immediate busy retry', async () => {
  const { history, listAlerts, receive } = setup();
  listAlerts.mockResolvedValueOnce({ status: 'busy' });
  history.update(input()); await flush();
  expect(listAlerts).toHaveBeenCalledTimes(2);
  expect(receive).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'ready' }));
  history.update(input('13')); await flush();
  expect(listAlerts).toHaveBeenCalledTimes(3);
});

it('removes its visibility listener and discards hidden work on disposal', async () => {
  const add = vi.spyOn(document, 'addEventListener');
  const remove = vi.spyOn(document, 'removeEventListener');
  setNativePresentationVisible(false);
  const { history, listAlerts, receive } = setup();
  const listener = add.mock.calls.find(([type]) => type === 'visibilitychange')?.[1];
  expect(listener).toBeTypeOf('function');
  history.update(input()); await flush();
  history.dispose();
  expect(remove).toHaveBeenCalledWith('visibilitychange', listener);
  receive.mockClear();
  documentVisible(false); setNativePresentationVisible(true); documentVisible(true);
  history.update(input('13')); history.retry(); await flush();
  expect(listAlerts).not.toHaveBeenCalled();
  expect(receive).not.toHaveBeenCalled();
});
