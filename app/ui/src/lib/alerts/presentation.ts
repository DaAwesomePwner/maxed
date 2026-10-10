import { allowsAlert } from './preferences';
import { caveat } from '../debug/debug';
import { formatDps, formatElapsed, formatTimeDelta } from '../styles/format';
import type { AlertKind } from '../bindings/AlertKind';
import type { AlertLoudness } from '../bindings/AlertLoudness';
import type { AlertPreferences } from '../bindings/AlertPreferences';
import type { AlertView } from '../bindings/AlertView';
import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { Timestamp } from '../bindings/Timestamp';

export interface AlertPresentation {
  id: string; kind: AlertKind; title: string; context: string;
  view: AlertView; committedAt: Timestamp; loudness: 'toast' | 'banner';
}
export interface AlertPresentationState {
  current: AlertPresentation | null; queued: number; overflow: number;
}
export interface AlertController {
  accept(event: EventEnvelope): void;
  /** Null policy fails closed without forgetting consumed IDs in this stream. */
  configure(preferences: AlertPreferences | null, streamId: string | null): void;
  /** Hidden surfaces discard new arrivals; hover/focus only pause the dwell. */
  setPaused(paused: boolean, hidden?: boolean): void;
  dismiss(id: string): void;
  dispose(): void;
}

const rank: Record<AlertLoudness, number> = { off: 0, quiet_log_only: 1, toast: 2, banner: 3 };
const titles: Record<AlertKind, string> = {
  whisper: 'Whisper', guild_message: 'Guild Message', party_message: 'Party Message',
  white_bag: 'White Bag', personal_best: 'Personal Best',
  flawless_run: 'Flawless Run', death: 'Death', key_popped: 'Key Popped',
};
const dwell = (value: AlertPresentation) => value.loudness === 'banner' ? 8000 : 5000;

export function alertTitle(view: AlertView): string {
  const detail = view.details;
  if (detail?.type === 'chat') {
    const prefix = detail.channel === 'whisper' ? 'Whisper from' : detail.channel === 'guild' ? 'Guild:' : 'Party:';
    return `${prefix} ${detail.sender}`;
  }
  if (view.kind === 'key_popped' && detail?.type === 'key_popped') return `${detail.dungeon} key popped`;
  if (view.kind === 'personal_best') {
    if (detail?.type === 'personal_best') return `New DPS PB: ${formatDps(detail.dps)}`;
    if (detail?.type === 'run_time_personal_best') {
      const { duration_micros: duration, previous_micros: previous } = detail;
      if (Number.isSafeInteger(duration) && Number.isSafeInteger(previous) && duration > 0 && previous > duration) {
        return `New PB: ${detail.dungeon} ${formatElapsed('0', String(duration))} (${formatTimeDelta(BigInt(duration) - BigInt(previous))})`;
      }
      return `New PB: ${detail.dungeon}`;
    }
  }
  return titles[view.kind];
}

/** Presentation only. Native delivery and durable log rows have separate owners. */
export function createAlertController(receive: (state: AlertPresentationState) => void,
  now: () => number, eventNow: () => Timestamp | null): AlertController {
  let preferences: AlertPreferences | null = null, stream: string | null = null;
  let current: AlertPresentation | null = null, waiting: AlertPresentation[] = [];
  const recent = new Set<string>();
  let overflow = 0, paused = false, hidden = false, disposed = false, remaining = 0, started = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const publish = () => receive({ current, queued: waiting.length, overflow });
  const clear = () => { clearTimeout(timer); timer = undefined; };
  function quieter(value: AlertPresentation): AlertPresentation | null {
    if (!preferences || value.kind === 'death' || !allowsAlert(preferences, value.view)) return null;
    const level = Math.min(rank[value.loudness] ?? 0, rank[preferences[value.kind]] ?? 0);
    return level < 2 ? null : { ...value, loudness: level === 2 ? 'toast' : 'banner' };
  }
  function fresh(value: AlertPresentation) {
    const clock = eventNow();
    if (clock === null || !/^\d+$/.test(clock) || !/^\d+$/.test(value.committedAt)) return false;
    const age = BigInt(clock) - BigInt(value.committedAt);
    return age >= 0n && age <= 30_000_000n;
  }
  function account() {
    if (current && !paused) remaining = Math.max(0, remaining - Math.max(0, now() - started));
    started = now();
    clear();
  }
  function schedule() {
    clear();
    if (disposed || paused || !current) return;
    started = now();
    timer = setTimeout(() => {
      account();
      if (remaining > 0) { schedule(); return; }
      current = null;
      advance();
    }, Math.max(1, remaining));
  }
  function advance() {
    if (!current && !paused) {
      while (waiting.length) {
        const candidate = quieter(waiting.shift()!);
        if (candidate && fresh(candidate)) { current = candidate; remaining = dwell(candidate); break; }
      }
    }
    schedule(); publish();
  }
  return {
    accept(envelope) {
      if (disposed || stream === null || envelope.stream_id !== stream || !('alert_fired' in envelope.event)) return;
      const view = envelope.event.alert_fired;
      // Active IDs remain protected even after a burst evicts their recent entry.
      if (recent.has(view.id) || current?.id === view.id || waiting.some(value => value.id === view.id)) return;
      recent.add(view.id);
      if (recent.size > 256) recent.delete(recent.values().next().value!);
      const original = view.presentation ?? 'quiet_log_only';
      if ((rank[original] ?? 0) < 2 || !preferences) return;
      // Backend explanations can include evidence caveats; keep banner copy to its title.
      caveat('alerts', view.explanation);
      const value = quieter({ id: view.id, kind: view.kind, title: alertTitle(view),
        context: '', view: structuredClone(view), committedAt: envelope.at,
        loudness: original === 'banner' ? 'banner' : 'toast' });
      if (!value || !fresh(value)) return;
      // A hidden window never collects a new playback backlog. Durable rows survive.
      if (hidden || waiting.length >= 5) { overflow++; publish(); return; }
      waiting.push(value);
      if (current) publish(); else advance();
    },
    configure(next, nextStream) {
      if (disposed) return;
      account();
      if (nextStream !== stream) {
        current = null; waiting = []; recent.clear(); overflow = 0;
      }
      stream = nextStream; preferences = next ? { ...next } : null;
      if (current) {
        const previous = current;
        current = quieter(current);
        if (current && current.loudness !== previous.loudness) {
          remaining = Math.max(0, remaining - (dwell(previous) - dwell(current)));
        }
      }
      waiting = waiting.flatMap(value => { const next = quieter(value); return next ? [next] : []; });
      advance();
    },
    setPaused(next, nextHidden = false) {
      if (disposed) return;
      hidden = nextHidden;
      if (paused === next) return;
      account(); paused = next;
      advance();
    },
    dismiss(id) {
      if (disposed || current?.id !== id) return;
      clear(); current = null; remaining = 0;
      advance();
    },
    dispose() {
      if (disposed) return;
      disposed = true; clear(); current = null; waiting = []; recent.clear(); overflow = 0;
      publish();
    },
  };
}
