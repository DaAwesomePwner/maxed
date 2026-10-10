import { cleanup } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { AlertView } from '../../src/lib/bindings/AlertView';
import type { StoreStatus } from '../../src/lib/bindings/StoreStatus';
import type { Route } from '../../src/lib/navigation/routes';
import { advance, flush, mountRuns, run, saved } from '../runs/fixtures';

// Observe the Shell's public route inputs without implementing the deferred log UI.
// Svelte passes getters, so these retain the current requirement/checkpoint values.
const { frames } = vi.hoisted(() => ({ frames: [] as Array<{
  route: Route; required: string | null; historyReady: StoreStatus | null;
}> }));
vi.mock('../../src/routes/RouteFrame.svelte', () => ({
  default: (_anchor: unknown, props: typeof frames[number]) => { frames.push(props); },
}));

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
});
afterEach(() => {
  cleanup();
  frames.length = 0;
  vi.clearAllTimers();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const alert: AlertView = {
  id: 'synthetic-alert', at: '5000000', kind: 'white_bag', run_id: null,
  explanation: 'Invented observed bag', details: null, presentation: 'banner',
};
const routeInput = () => frames.at(-1)!;

it.each(['alerts', 'home', 'runs'] as const)('alert_fired advances only the Alerts requirement (%s)', async route => {
  const page = mountRuns({ hash: route === 'home' ? '#/' : `#/${route}` });
  await flush();
  expect(routeInput().route).toEqual(route === 'alerts'
    ? { kind: 'logs', view: 'alerts', channel: 'all' } : { kind: route });
  expect(routeInput().required).toBe('12');
  expect(routeInput().historyReady?.durable).toBe('12');
  const polls = vi.mocked(page.client.queryStore).mock.calls.length;

  page.event({ alert_fired: alert }, '13');
  page.event({ alert_fired: alert }, '13');
  await flush();
  expect(routeInput().required).toBe(route === 'alerts' ? '13' : '12');
  expect(routeInput().historyReady?.durable).toBe('12');
  expect(page.client.queryStore).toHaveBeenCalledTimes(polls);

  // Seeing the event does not acknowledge persistence or cause immediate IPC.
  await advance();
  expect(routeInput().historyReady?.durable).toBe('12');
  expect(page.client.queryStore).toHaveBeenCalledTimes(polls + (route === 'alerts' ? 1 : 0));
  page.setStore(saved('13'));
  await advance();
  expect(routeInput().required).toBe(route === 'alerts' ? '13' : '12');
  if (route === 'alerts') {
    expect(routeInput().historyReady?.durable).toBe('13');
    const committedPolls = vi.mocked(page.client.queryStore).mock.calls.length;
    page.event({ alert_fired: alert }, '13');
    await advance();
    expect(page.client.queryStore).toHaveBeenCalledTimes(committedPolls);
  }

  // Positive control on every route: source revisions still use the base gate.
  page.event({ run_updated: run() }, '14');
  await flush();
  expect(routeInput().required).toBe('14');
  expect(routeInput().historyReady?.durable).not.toBe('14');
  page.setStore(saved('14'));
  await advance();
  expect(routeInput().historyReady?.durable).toBe('14');
});
