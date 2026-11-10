import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { createClient } from './client';
import { withSubscriptionHealth } from './subscription-health';
import type { Events, Transport } from './client';

// Keep in sync with LivePayloads::subscribe in bridge/channels.rs.
export const LIVE_PAYLOAD_EVENTS = new Set<keyof Events>(['live-snapshot', 'engine-event', 'party-changed']);

function decodePayload<T>(payload: T | string): T {
  if (typeof payload !== 'string') return payload;
  try { return JSON.parse(payload) as T; }
  catch { throw new Error('Live payload could not be decoded'); }
}

// The native command boundary returns generated DTOs, without fixture imports.
export const nativeTransport: Transport = {
  invoke: (command, args) => invoke(command, args),
  listen: async <K extends keyof Events>(event: K, receive: (payload: Events[K]) => void) => {
    let active = true;
    // Typed events support capture status and the perf-only original publisher.
    const release = await listen<Events[K] | string>(event, (message) => {
      if (active) receive(decodePayload<Events[K]>(message.payload));
    });
    // Only these events also support byte channels; all others use the Tauri listener alone.
    if (!LIVE_PAYLOAD_EVENTS.has(event)) return () => { active = false; release(); };
    const channel = new Channel<ArrayBuffer>();
    const decoder = new TextDecoder();
    channel.onmessage = bytes => {
      if (active) receive(decodePayload<Events[K]>(decoder.decode(bytes)));
    };
    try {
      const id = await invoke<number>('subscribe_live_payload', { event, channel });
      const stop = () => {
        if (!active) return;
        active = false;
        release();
        void invoke('unsubscribe_live_payload', { id }).catch(() => {});
      };
      return event === 'engine-event' || event === 'live-snapshot'
        ? withSubscriptionHealth(stop, id, subscriptionIds => invoke('get_store_status', { subscriptionIds }))
        : stop;
    } catch (error) {
      active = false;
      release();
      throw error;
    }
  },
};

export function createNativeClient() {
  return createClient(nativeTransport);
}
