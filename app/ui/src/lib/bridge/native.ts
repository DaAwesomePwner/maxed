import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { createClient } from './client';
import type { Events, Transport } from './client';

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
    if (event === 'capture-status') return () => { active = false; release(); };
    const channel = new Channel<ArrayBuffer>();
    const decoder = new TextDecoder();
    channel.onmessage = bytes => {
      if (active) receive(decodePayload<Events[K]>(decoder.decode(bytes)));
    };
    try {
      const id = await invoke<number>('subscribe_live_payload', { event, channel });
      return () => {
        active = false;
        release();
        void invoke('unsubscribe_live_payload', { id }).catch(() => {});
      };
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
