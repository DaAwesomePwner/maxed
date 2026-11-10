import type { StoreStatus } from '../bindings/StoreStatus';

// Wire error code shared with bridge/channels.rs; not user-facing copy.
export const LIVE_SUBSCRIPTION_LOST = 'LIVE_SUBSCRIPTION_LOST';

const lease = Symbol('live subscription lease');
type Release = (() => void) & {
  [lease]?: { id: number; query: (ids: number[]) => Promise<StoreStatus> };
};

/** Keep native ownership on the release handle, never in a transport-wide set. */
export function withSubscriptionHealth(release: () => void, id: number,
  query: (ids: number[]) => Promise<StoreStatus>): Release {
  return Object.assign(release, { [lease]: { id, query } });
}

/** Only subscribeLive's existing poll opts into health for its own handles. */
export function queryOwnedStore(releases: readonly Release[], fallback: () => Promise<StoreStatus>) {
  const owned = releases.flatMap(release => release[lease] ? [release[lease]] : []);
  return owned.length ? owned[0].query(owned.map(value => value.id)) : fallback();
}

/** Stop payload traffic without removing capture or visibility listeners. */
export function releaseOwnedPayloads(releases: readonly Release[]): Release[] {
  return releases.filter(release => {
    if (!release[lease]) return true;
    release();
    return false;
  });
}
