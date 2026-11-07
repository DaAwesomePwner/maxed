import { cleanup, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import Home from '../../src/routes/Home.svelte';
import { createHomeObservation } from '../../src/lib/home/mode';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { homeSnapshot, homeSummary, saved } from './fixtures';
import { flushHomeRender, mountHome } from './shell-fixture';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';

afterEach(cleanup);
function provisional() {
  const snapshot = homeSnapshot({ area_kind: 'realm', map: 'Realm', run: null });
  snapshot.character = { ...snapshot.character!, id: 'synthetic-provisional', name: 'Synthetic traveller',
    identity_verified: false, account_id: null };
  return snapshot;
}
function identity(snapshot = provisional()): EventEnvelope {
  return { stream_id: snapshot.stream_id, sequence: String(BigInt(snapshot.sequence) + 1n), at: snapshot.at,
    event: { local_identity_changed: { at: snapshot.at, account_id: 'synthetic-account-a', character_id: 'synthetic-verified' } } };
}

it.each([false, true])('provisional adoption retains Realm for pauseOnCaptureRecovery=%s', pauseOnCaptureRecovery => {
  const observe = createHomeObservation({ pauseOnCaptureRecovery });
  const snapshot = provisional();
  observe(snapshot, snapshot.capture, null, 0);
  expect(observe.onEvent(identity(snapshot))).toBe(false);
  const beforePacedSnapshot = observe(snapshot, snapshot.capture, null, 1);
  expect(beforePacedSnapshot.retained).toBe(snapshot);
  expect(beforePacedSnapshot.mode.mode).toBe('realm');
  const verified = { ...snapshot, character: { ...snapshot.character!, id: 'synthetic-verified',
    identity_verified: true, account_id: 'synthetic-account-a' } };
  expect(observe(verified, verified.capture, null, 2).retained).toBe(verified);
  // Adoption without a switch must not consume the next real switch notice.
  observe.historyChanged();
  expect(observe(verified, verified.capture, null, 3).retained).toBeNull();
});

it('adopts the same verified account despite a different character ID', () => {
  const observe = createHomeObservation();
  const snapshot = provisional();
  snapshot.character = { ...snapshot.character!, identity_verified: true, account_id: 'synthetic-account-a' };
  observe(snapshot, snapshot.capture, null, 0);
  expect(observe.onEvent(identity(snapshot))).toBe(false);
  expect(observe(snapshot, snapshot.capture, null, 1).retained).toBe(snapshot);
  observe.historyChanged();
  expect(observe(snapshot, snapshot.capture, null, 2).retained).toBeNull();
});

it('fresh Home keeps the same Realm screen until the paced verified snapshot', async () => {
  const snapshot = provisional();
  const page = render(Home, { snapshot });
  await flushHomeRender();
  const realm = page.container.querySelector('.realm-home');
  expect(realm).not.toBeNull();
  await page.rerender({ event: identity(snapshot) });
  await flushHomeRender();
  expect(page.container.querySelector('.realm-home')).toBe(realm);
  expect(page.container.querySelector('.nexus-home')).toBeNull();
  const verified = { ...snapshot, character: { ...snapshot.character!, id: 'synthetic-verified',
    identity_verified: true, account_id: 'synthetic-account-a' } };
  await page.rerender({ snapshot: verified });
  await flushHomeRender();
  expect(page.container.querySelector('.realm-home')).toBe(realm);
});

it('Shell keeps the provisional rail and Realm screen after first verified identity', async () => {
  const snapshot = provisional();
  const h = mountHome(snapshot);
  const rail = await screen.findByRole('complementary', { name: 'Character' });
  await within(rail).findByRole('heading', { name: 'Synthetic traveller' });
  const realm = h.container.querySelector('.realm-home');
  expect(realm).not.toBeNull();
  const event = identity(snapshot);
  h.transport.publishEngineEvent(event);
  await flushHomeRender();
  expect(within(rail).getByRole('heading', { name: 'Synthetic traveller' })).toBeTruthy();
  expect(h.container.querySelector('.realm-home')).toBe(realm);
  h.transport.publishSnapshot({ ...snapshot, sequence: event.sequence,
    character: { ...snapshot.character!, id: 'synthetic-verified', identity_verified: true, account_id: 'synthetic-account-a' } });
  await flushHomeRender();
  expect(within(rail).getByRole('heading', { name: 'Synthetic traveller' })).toBeTruthy();
  expect(h.container.querySelector('.realm-home')).toBe(realm);
});

it.each(['account_switched', 'legacy_recovered'] as const)('Home ignores duplicate %s revisions', async reason => {
  const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
  const fixture = createFixtureTransport(snapshot, undefined, homeSummary());
  const client = createClient(fixture);
  const query = vi.spyOn(client, 'queryHome');
  render(Home, { snapshot, client, store: saved() });
  await waitFor(() => expect(query).toHaveBeenCalledTimes(1));
  fixture.publishAccountHistoryChanged({ revision: 1, reason });
  await waitFor(() => expect(query).toHaveBeenCalledTimes(2));
  fixture.publishAccountHistoryChanged({ revision: 1, reason });
  await flushHomeRender();
  expect(query).toHaveBeenCalledTimes(2);
  fixture.publishAccountHistoryChanged({ revision: 2, reason });
  await waitFor(() => expect(query).toHaveBeenCalledTimes(3));
});
