import type { HistoryCache } from '../runs/history-cache';
import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { Readable } from 'svelte/store';
import type { CaptureStatus } from '../bindings/CaptureStatus';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { HomeLiveWindow } from '../home/v2/contracts';
import type { AlertSession } from '../alerts/session';

import type { createRecordingSession } from '../settings/recording';

export const SHELL_CONNECTION = Symbol('shell-connection');
export interface ShellConnection {
  historyCache?: HistoryCache;
  capture: Readable<CaptureStatus | null>;
  snapshot: Readable<LiveSnapshot | null>;
  homeWindow: Readable<HomeLiveWindow>;
  selectHomeFight: (id: string | null) => void;
  reconnect: () => void;
  /** Ordered accepted events; callbacks run before reactive render coalescing. */
  onEvent?: (receive: (event: EventEnvelope) => void) => () => void;
  /** Shell-owned accepted policy; borrowed dispose() is a no-op for route cleanup. */
  alerts?: AlertSession;
  recording?: ReturnType<typeof createRecordingSession>;
}
