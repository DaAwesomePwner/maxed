import { cleanup, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import LogsPreview from './LogsPreview.svelte';
import { createLogsPreview } from './logs-scenario';
import type { ChatCursor } from '../../src/lib/bindings/ChatCursor';
import type { ChatMessageView } from '../../src/lib/bindings/ChatMessageView';
import type { ChatFilter } from '../../src/lib/bindings/ChatFilter';
import { alertKinds } from '../../src/lib/alerts/filters';

afterEach(cleanup);
const filter = (): ChatFilter => ({ channel: null, sender: null, query: '', run_id: null, show_npc: true, show_hidden: true, cursor: null, limit: 100 });
it('mounts synthetic chat and defaults to Logs', async () => {
  window.history.replaceState(null, '', '/?preview=logs'); render(LogsPreview);
  expect(await within(screen.getByRole('main')).findAllByText(/Synthetic chat line/)).not.toHaveLength(0);
  expect(location.hash).toBe('#/logs');
  expect(screen.getByRole('heading', { name: 'Logs', level: 1 }).classList.contains('sr-only')).toBe(true);
});
it('preserves a supplied deep link and supplies alerts to the strict preview transport', async () => {
  window.history.replaceState(null, '', '/?preview=logs#/logs/alerts'); render(LogsPreview);
  expect(await within(screen.getByRole('main')).findAllByText(/Key popped|White bag|Personal best/i)).not.toHaveLength(0);
  expect(location.hash).toBe('#/logs/alerts');
});
it.each(['default', 'stress'] as const)('provides %s sizes, paging, uncertainty and mutable hide settings', async variant => {
  const preview = createLogsPreview(variant), client = preview.client;
  try {
    const all: ChatMessageView[] = [];
    let cursor: ChatCursor | null = null, pages = 0;
    do {
      const reply = await client.listChat({ ...filter(), cursor });
      if (reply.status !== 'ready') throw new Error('Expected ready chat');
      expect(reply.value.rows).toHaveLength(100);
      all.push(...reply.value.rows.map(row => row.message)); cursor = reply.value.next; pages++;
    } while (cursor);
    expect(all).toHaveLength(variant === 'stress' ? 4000 : 300);
    expect(pages).toBe(variant === 'stress' ? 40 : 3);
    expect(new Set(all.map(message => message.id)).size).toBe(all.length);
    expect(all.slice(0, 12).every(message => message.run_id === all[0].run_id && message.map === all[0].map)).toBe(true);
    expect(all[12].run_id).not.toBe(all[0].run_id);
    const senderName = all.find(message => !message.is_local && message.sender_kind === 'player')!.sender;
    const sender = await client.getChatSender({ name: senderName });
    if (sender.status !== 'ready' || !sender.value) throw new Error('Expected sender summary');
    const senderChannels = new Set(all.filter(message => message.sender === senderName).map(message => message.channel));
    expect(sender.value.channels).toEqual(['public', 'guild', 'party', 'whisper', 'unknown'].filter(channel => senderChannels.has(channel as ChatMessageView['channel'])));
    const wrongCase = await client.getChatSender({ name: senderName.toUpperCase() });
    expect(wrongCase).toEqual({ status: 'ready', value: null });
    expect(new Set(all.map(message => new Date(Number(BigInt(message.at) / 1000n)).toDateString())).size).toBe(3);
    expect(new Set(all.filter(message => message.channel === 'whisper').map(message => message.is_local ? message.counterpart : message.sender)).size).toBe(variant === 'stress' ? 200 : 9);
    expect(all.some(message => message.direction === 'unknown')).toBe(true);
    expect(all.some(message => message.is_local)).toBe(true);
    expect(all.some(message => message.sender_kind === 'npc')).toBe(true);
    expect(all.some(message => message.truncated)).toBe(true);
    const settings = await client.querySettings();
    expect(settings.chat).toMatchObject({ retention_days: 30, store: { party: false } });
    expect(settings.chat.hidden_senders).toHaveLength(3); expect(settings.chat.hidden_keywords).toHaveLength(5);
    const dungeons = await client.listCatalogDungeons();
    expect(dungeons).toHaveLength(variant === 'stress' ? 128 : 9);
    if (variant === 'stress') {
      expect(dungeons.every(name => name.length === 64)).toBe(true);
      expect(all.every(message => new TextEncoder().encode(message.text).length === 1024)).toBe(true);
      expect(all.some(message => message.sender.length === 64)).toBe(true);
    }
    const selected = all.find(message => !message.is_local && message.sender_kind === 'player' && message.run_id && !message.text.includes('trade offer'))!;
    await client.updateChatSettings({ ...settings.chat, hidden_senders: [], hidden_keywords: [] });
    const scopedFilter = { ...filter(), channel: selected.channel, sender: selected.sender, run_id: selected.run_id ?? null, query: selected.text.slice(0, 40), show_hidden: false };
    const before = await client.listChat(scopedFilter);
    if (before.status !== 'ready') throw new Error('Expected ready chat');
    expect(before.value.rows.map(row => row.message.id)).toEqual([selected.id]);
    await client.updateChatSettings({ ...settings.chat, hidden_senders: [selected.sender], hidden_keywords: [] });
    const hidden = await client.listChat(scopedFilter);
    if (hidden.status !== 'ready') throw new Error('Expected ready chat');
    expect(hidden.value.rows).toHaveLength(0);
    const shown = await client.listChat({ ...scopedFilter, show_hidden: true });
    if (shown.status !== 'ready') throw new Error('Expected ready chat');
    expect(shown.value.rows[0].hidden).toBe(true);
    const run = await client.getRun(selected.run_id!);
    if (run.status !== 'ready') throw new Error('Expected ready run');
    expect(run.value?.run.id).toBe(selected.run_id);
    const first = await client.listInstanceEvents({ kinds: [], dungeon: null, run_id: null, cursor: null, limit: 100 });
    if (first.status !== 'ready') throw new Error('Expected ready events');
    const events = [...first.value.rows];
    let eventCursor = first.value.next;
    while (eventCursor) {
      const next = await client.listInstanceEvents({ kinds: [], dungeon: null, run_id: null, cursor: eventCursor, limit: 100 });
      if (next.status !== 'ready') throw new Error('Expected ready events');
      events.push(...next.value.rows); eventCursor = next.value.next;
    }
    expect(events).toHaveLength(variant === 'stress' ? 1024 : 200);
    const portals = events.filter(event => event.kind.type === 'portal_opened');
    expect(new Set(portals.map(event => event.kind.type === 'portal_opened' ? event.kind.opener_is_local : null))).toEqual(new Set([true, false, null]));
    const keySummary = await client.getEventSummary({ filter: { kinds: ['portal_opened'], dungeon: null, run_id: null, cursor: null, limit: 50 } });
    if (keySummary.status !== 'ready') throw new Error('Expected ready summary');
    expect(keySummary.value.dungeons).toHaveLength(variant === 'stress' ? 128 : 9);
    expect(settings.alerts.key_pop_excluded_dungeons).toHaveLength(3);
    expect(new Set(events.map(event => event.kind.type)).size).toBe(4);
    expect(events.some(event => event.provisional)).toBe(true);
    let alertCursor: ChatCursor | null = null, count = 0;
    const kinds = new Set<string>(); let unknownPresentation = false;
    do {
      const reply = await client.listAlerts({ kind: null, query: '', range: null, cursor: alertCursor, limit: 50 });
      if (reply.status !== 'ready') throw new Error('Expected ready alerts');
      count += reply.value.rows.length; alertCursor = reply.value.next;
      reply.value.rows.forEach(alert => {
        kinds.add(alert.kind); unknownPresentation ||= alert.presentation === null;
        if (variant === 'stress' && alert.details?.type === 'chat') {
          expect(alert.details.sender).toHaveLength(64);
          expect(new TextEncoder().encode(alert.details.excerpt)).toHaveLength(1024);
        }
      });
    } while (alertCursor);
    expect(count).toBe(variant === 'stress' ? 400 : 120); expect(kinds.size).toBe(alertKinds.length); expect(unknownPresentation).toBe(true);
  } finally { preview.dispose(); }
});
