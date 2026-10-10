import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import FixtureApp from '../shell/fixtures/FixtureApp.svelte';
import { logsQueries } from '../shell/fixtures/logs';
import type { ChatFilter } from '../../src/lib/bindings/ChatFilter';
import { fakeHistory } from '../navigation/fake-history';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import fixture from '../fixtures/t0-live';

afterEach(cleanup);
it('renders synthetic chat in the fixture preview', async () => {
  const browser = fakeHistory('#/logs');
  render(FixtureApp, { historyAdapter: browser.adapter });
  await waitFor(() => expect(screen.getAllByText('anyone for a synthetic lost halls run?')[0]).toBeTruthy());
});

it.each(['#/logs', '#/logs/from/Vexlin', '#/logs/chat/whisper/Vexlin'])('restores the channel’s saved navigation state through Back and Forward on %s', async path => {
  const browser = fakeHistory(path);
  const samples = logsQueries.listChat!({ channel: null, sender: null, query: '', run_id: null, show_npc: false, show_hidden: false, cursor: null, limit: 100 }).rows;
  const cursor = { at: '3000', id: 'synthetic-cursor' };
  const client = createClient(createFixtureTransport(fixture, undefined, undefined, undefined, undefined, undefined, undefined, undefined, undefined, {
    listChat: filter => ({ rows: filter.cursor ? samples.slice(2, 4) : samples.slice(0, 2), next: filter.cursor ? null : cursor }),
  }));
  const view = render(FixtureApp, { client, historyAdapter: browser.adapter });
  if (path === '#/logs/chat/whisper/Vexlin') {
    // Conversations restore filters and partner, but deliberately reload the newest page.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Load earlier' })).toBeTruthy());
    await fireEvent.input(screen.getByLabelText('Search'), { target: { value: 'synthetic' } });
    const whisperState = () => (browser.adapter.state as { logsPosition: { whisper: { query: string; showHidden: boolean; partner: string } } }).logsPosition.whisper;
    await waitFor(() => expect(whisperState().query).toBe('synthetic'));
    await fireEvent.click(screen.getByRole('button', { name: /^Hidden/ }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Load earlier' }).hasAttribute('disabled')).toBe(false));
    await fireEvent.click(screen.getByRole('button', { name: 'Load earlier' }));
    await waitFor(() => expect(view.container.querySelector('[data-row-id="preview:chat:3"]')).toBeTruthy());
    expect(view.container.querySelectorAll('[data-row-id]')).toHaveLength(4);
    await fireEvent.click(screen.getByRole('link', { name: 'Guild' }));
    await waitFor(() => expect(browser.adapter.hash).toBe('#/logs/chat/guild'));
    browser.adapter.go(-1);
    await waitFor(() => expect(screen.getByRole('region', { name: 'Whisper thread' })).toBeTruthy());
    await waitFor(() => expect(view.container.querySelectorAll('[data-row-id]')).toHaveLength(2));
    expect(view.container.querySelector('[data-row-id="preview:chat:3"]')).toBeNull();
    expect(whisperState()).toEqual({ query: 'synthetic', showHidden: true, partner: 'Vexlin' });
    expect((screen.getByLabelText('Search') as HTMLInputElement).value).toBe('synthetic');
    expect(screen.getByRole('button', { name: /^Hidden/ }).getAttribute('aria-pressed')).toBe('true');
    expect(screen.queryByRole('button', { name: 'NPC lines' })).toBeNull();
    expect(screen.getByRole('link', { name: 'All chat from Vexlin' })).toBeTruthy();
    browser.adapter.go(1);
    await waitFor(() => expect(screen.getByRole('link', { name: 'Guild' }).getAttribute('aria-current')).toBe('page'));
    return;
  }
  await waitFor(() => expect(screen.getByRole('button', { name: 'Older lines' })).toBeTruthy());
  await fireEvent.input(screen.getByLabelText('Search'), { target: { value: 'synthetic' } });
  await waitFor(() => expect((browser.adapter.state as { logsPosition: { chat: { filter: ChatFilter } } }).logsPosition.chat.filter.query).toBe('synthetic'));
  await fireEvent.click(screen.getByRole('button', { name: 'NPC lines' }));
  await fireEvent.click(screen.getByRole('button', { name: /^Hidden/ }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Older lines' }).hasAttribute('disabled')).toBe(false));
  await fireEvent.click(screen.getByRole('button', { name: 'Older lines' }));
  await waitFor(() => expect(view.container.querySelector('[data-row-id="preview:chat:3"]')).toBeTruthy());
  await fireEvent.click(view.container.querySelector('[data-row-id="preview:chat:3"]')!);
  await fireEvent.click(screen.getByRole('link', { name: 'Guild' }));
  await waitFor(() => expect(browser.adapter.hash).toBe('#/logs/chat/guild'));
  browser.adapter.go(-1);
  await waitFor(() => expect(view.container.querySelector('[data-row-id="preview:chat:3"]')?.getAttribute('aria-selected')).toBe('true'));
  expect((screen.getByLabelText('Search') as HTMLInputElement).value).toBe('synthetic');
  expect(screen.getByRole('button', { name: 'NPC lines' }).getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByRole('button', { name: /^Hidden/ }).getAttribute('aria-pressed')).toBe('true');
  if (path !== '#/logs') expect((browser.adapter.state as { logsPosition: { chat: { filter: ChatFilter } } }).logsPosition.chat.filter.sender).toBe('Vexlin');
  expect((browser.adapter.state as { logsPosition: { chat: { filter: ChatFilter } } }).logsPosition.chat.filter.cursor).toEqual(cursor);
  browser.adapter.go(1);
  await waitFor(() => expect(screen.getByRole('link', { name: 'Guild' }).getAttribute('aria-current')).toBe('page'));
});

const allChat: ChatFilter = { channel: null, sender: null, query: '', run_id: null,
  show_npc: true, show_hidden: false, cursor: null, limit: 50 };

it('provides every channel, both whisper directions and optional NPC lines', () => {
  const page = logsQueries.listChat!(allChat);
  expect(page.rows).toHaveLength(6);
  expect(new Set(page.rows.map(row => row.message.channel)))
    .toEqual(new Set(['public', 'guild', 'party', 'whisper', 'unknown']));
  const whispers = logsQueries.listChat!({ ...allChat, channel: 'whisper' });
  expect(whispers.rows.map(row => row.message.direction)).toEqual(['incoming', 'outgoing']);
  const players = logsQueries.listChat!({ ...allChat, show_npc: false });
  expect(players.rows).toHaveLength(5);
  expect(players.rows.every(row => row.message.sender_kind === 'player')).toBe(true);
  expect(page.rows.find(row => row.message.sender_kind === 'npc')?.message.sender).toBe('Oryx the Mad God');
});

it('filters instance events while preserving provisional departures', () => {
  const filter = { kinds: [], run_id: null, dungeon: null, cursor: null, limit: 50 };
  expect(logsQueries.listInstanceEvents!(filter).rows).toHaveLength(3);
  const departures = logsQueries.listInstanceEvents!({ ...filter, kinds: ['player_left'] });
  expect(departures.rows).toHaveLength(1);
  expect(departures.rows[0]).toMatchObject({ provisional: true, kind: { type: 'player_left' } });
});
