import { expect, test } from 'vitest';
import { createHomeObservation } from '../../src/lib/home/mode';
import type { LiveState } from '../../src/lib/bridge/live';
import { homeSnapshot } from './fixtures';

const live: LiveState = { phase: 'live', historyStale: false, historyFailed: false, historyLagging: false, message: '' };
test.each([false, true])('Nexus replaces the realm object graph in Home/rail (rail=%s)', pauseOnCaptureRecovery => {
  const observe = createHomeObservation({ pauseOnCaptureRecovery });
  const realm = homeSnapshot({ area_kind: 'realm', map: 'Realm', run: null });
  expect(observe(realm, realm.capture, live, 0).retained).toBe(realm);
  const nexus = { ...realm, area_kind: 'nexus' as const, map: 'Nexus', fights: [], drops: [], sequence: '100' };
  const left = observe(nexus, nexus.capture, live, 100);
  expect(left.retained).toBe(nexus);
  expect(left.mode.mode).toBe('nexus');
  expect(left.paused).toBe(false);
  expect(observe(nexus, nexus.capture, live, 2000).retained).toBe(nexus);
});
