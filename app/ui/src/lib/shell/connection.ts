import type { Client } from '../bridge/client';
import { subscribeLive, type LiveState } from '../bridge/live';
import type { CaptureStatus } from '../bindings/CaptureStatus';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { VisualPage } from '../bindings/VisualPage';
import type { StoreStatus } from '../bindings/StoreStatus';
import type { EventEnvelope } from '../bindings/EventEnvelope';
import { isBossFight, scopedFights } from './rail';
import { objectVisualTypes } from '../bridge/visuals';
import { characterVisualTypes } from '../home/visuals';
import { createHomeLiveWindow } from '../home/v2/live';
import type { HomeLiveWindow } from '../home/v2/contracts';

export type ConnectionErrorSource = 'captureSubscription' | 'snapshot' | 'store';

/** One ordered connection owns capture, snapshots, resync and storage gating. */
export function connectShell(client: Client, receive: {
  snapshot: (value: LiveSnapshot) => void;
  capture: (value: CaptureStatus) => void;
  visuals: (value: VisualPage) => void;
  store: (value: StoreStatus) => void;
  state?: (value: LiveState) => void;
  event?: (value: EventEnvelope) => void;
  homeWindow?: (value: HomeLiveWindow) => void;
  liveState?: (value: LiveState) => void;
  historyReady?: (value: StoreStatus) => void;
  historyInvalidates?: (event: EventEnvelope) => boolean;
  historyBaseline?: (snapshot: LiveSnapshot) => void;
  error: (source: ConnectionErrorSource, value: string) => void;
}) {
  let disposed = false;
  let fightWindow: HomeLiveWindow | null = null;
  const homeWindow = createHomeLiveWindow(value => { fightWindow = value; receive.homeWindow?.(value); });
  let visualRevision = 0;
  let visualKey = '';
  let loadingVisuals = false;
  let pendingVisuals: number[] | undefined;
  let currentTypes: number[] | undefined;
  let visualFailures = 0;
  let visualTimer: ReturnType<typeof setTimeout> | undefined;
  const emptyVisuals = () => receive.visuals({ client_fingerprint: '', objects: [] });
  async function loadVisuals() {
    if (disposed || loadingVisuals || !pendingVisuals) return;
    const types = pendingVisuals, request = visualRevision;
    pendingVisuals = undefined;
    loadingVisuals = true;
    try {
      const page = await client.getObjectVisuals(types);
      if (!disposed && request === visualRevision) { visualFailures = 0; receive.visuals(page); }
    } catch {
      if (!disposed && request === visualRevision) {
        emptyVisuals();
        visualTimer = setTimeout(() => {
          visualTimer = undefined;
          if (disposed || request !== visualRevision) return;
          pendingVisuals = types;
          void loadVisuals();
        }, Math.min(1000 * 2 ** Math.min(visualFailures++, 5), 30_000));
      }
    } finally {
      loadingVisuals = false;
      if (!disposed) void loadVisuals();
    }
  }
  function refreshVisuals() {
    visualRevision++;
    clearTimeout(visualTimer); visualTimer = undefined;
    visualFailures = 0;
    emptyVisuals();
    pendingVisuals = currentTypes?.length ? currentTypes : undefined;
    void loadVisuals();
  }
  const releaseVisuals = client.onVisualsChanged(() => {
    if (disposed || !currentTypes) return;
    refreshVisuals();
  });
  const connection = subscribeLive(client, {
    batch: homeWindow.batch,
    capture: (value) => { homeWindow.capture(value); receive.capture(value); },
    store: receive.store,
    event: (value) => { homeWindow.event(value); receive.event?.(value); },
    historyReady: receive.historyReady,
    historyInvalidates: receive.historyInvalidates,
    historyBaseline: receive.historyBaseline,
    state: (state) => {
      if (disposed) return;
      homeWindow.state(state);
      receive.state?.(state);
      receive.liveState?.(state);
      if (state.phase === 'failed') {
        visualRevision++; pendingVisuals = undefined; currentTypes = undefined;
        clearTimeout(visualTimer); visualTimer = undefined;
      }
      receive.error('snapshot', state.phase === 'failed' ? state.message : '');
    },
    snapshot: (snapshot) => {
      homeWindow.snapshot(snapshot);
      receive.snapshot(snapshot);
      const types = objectVisualTypes([...characterVisualTypes(snapshot.character),
        ...scopedFights(snapshot, fightWindow).filter(isBossFight).map(fight => fight.target_type)]);
      currentTypes = types;
      const key = `${snapshot.stream_id}:${types.join(',')}`;
      if (key !== visualKey) {
        visualKey = key;
        refreshVisuals();
      }
    },
  });
  return Object.assign(() => {
    disposed = true; homeWindow.dispose(); visualRevision++; pendingVisuals = undefined;
    clearTimeout(visualTimer); releaseVisuals(); connection.dispose();
  }, { selectFight: homeWindow.select });
}
