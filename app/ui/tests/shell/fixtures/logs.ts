import type { LogsFixtureQueries } from '../../../src/lib/bridge/fixture';
import type { ChatMessageView } from '../../../src/lib/bindings/ChatMessageView';
import type { InstanceEventView } from '../../../src/lib/bindings/InstanceEventView';

const messageSeeds = [
  { sender: 'SynthAlpha', channel: 'public', text: 'anyone for a synthetic lost halls run?' },
  { sender: 'SynthBravo', channel: 'guild', text: 'synthetic guild run ready' },
  { sender: 'SynthCharlie', channel: 'party', text: 'synthetic party at the portal' },
  { sender: 'SynthDelta', channel: 'whisper', direction: 'incoming', counterpart: 'SynthDelta', text: 'synthetic invitation received' },
  { sender: 'SynthEcho', channel: 'whisper', direction: 'outgoing', counterpart: 'SynthFoxtrot', is_local: true, text: 'synthetic invitation sent' },
  { sender: 'Oryx the Mad God', channel: 'unknown', sender_kind: 'npc', text: 'A synthetic challenge awaits!' },
] satisfies (Pick<ChatMessageView, 'sender' | 'channel' | 'text'> & Partial<ChatMessageView>)[];

const messages: ChatMessageView[] = messageSeeds.map((message, index) => ({
  id: `preview:chat:${index}`, session_id: 'preview-session', run_id: null,
  map: 'Nexus', at: String(6000 - index * 1000), sender_kind: 'player',
  is_local: false, supporter: false, truncated: false, ...message,
}));

const events: InstanceEventView[] = [
  { id: 'preview:event:3', session_id: 'preview-session', map: 'Nexus', at: '6000',
    kind: { type: 'portal_opened', opener: 'SynthAlpha', dungeon: 'Lost Halls' }, provisional: false },
  { id: 'preview:event:2', session_id: 'preview-session', map: 'Lost Halls', at: '5000',
    kind: { type: 'player_death', player_name: 'SynthBravo', is_local: false }, provisional: false },
  { id: 'preview:event:1', session_id: 'preview-session', map: 'Lost Halls', at: '4000',
    kind: { type: 'player_left', object_id: 3, player_name: 'SynthCharlie' }, provisional: true },
];

export const logsQueries: LogsFixtureQueries = {
  listChat: filter => ({
    rows: messages.filter(message => (!filter.channel || message.channel === filter.channel)
      && (filter.show_npc || message.sender_kind !== 'npc'))
      .map(message => ({ message, hidden: false })),
    next: null,
  }),
  listInstanceEvents: filter => ({
    rows: events.filter(event => !filter.kinds.length || filter.kinds.includes(event.kind.type)),
    next: null,
  }),
};
