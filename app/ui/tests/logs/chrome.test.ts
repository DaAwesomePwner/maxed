import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import { channelTag, clockTime, dayLabel, perDay, countLine } from '../../src/lib/logs/logs-format';
import LogsHeader from '../../src/lib/logs/LogsHeader.svelte';
import Pager from '../../src/lib/logs/Pager.svelte';
import NewDataPill from '../../src/lib/logs/NewDataPill.svelte';
import RangeSwitch from '../../src/lib/loot/RangeSwitch.svelte';
import ErrorBar from '../../src/lib/logs/ErrorBar.svelte';
import RunScopeChip from '../../src/lib/logs/RunScopeChip.svelte';
import { deferred, injectChicagoDate, story } from '../runs/fixtures';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import fixture from '../fixtures/t0-live';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import type { ChatMessageView } from '../../src/lib/bindings/ChatMessageView';

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
const at = (date: string) => String(new Date(date).getTime() * 1000);
it('gives NPC senders precedence and retains unknown channels', () => {
  expect(channelTag({ sender_kind: 'npc', channel: 'guild' } as ChatMessageView)).toBe('npc');
  expect(channelTag({ sender_kind: 'player', channel: 'unknown' } as ChatMessageView)).toBe('unknown');
});
it('formats counts and rates with a known observation interval', () => {
  const now = new Date('2026-10-03T00:00:00Z'), first = at('2026-10-01T00:00:00Z');
  expect(perDay(10, { start: first, end: String(now.getTime() * 1000) }, null, now)).toBe('5.0 per day');
  expect(perDay(10, null, first, now)).toBe('5.0 per day');
  expect(perDay(10, null, null, now)).toBe('—');
  expect(countLine(0, 'key pops seen', first)).toBeNull();
  expect(countLine(null, 'events seen', first)).toBeNull();
  expect(countLine(612, 'key pops seen', at('2026-09-14T12:00:00Z'))).toBe('612 key pops seen · since Sep 14');
});
it('formats local calendar days across a month boundary', () => {
  injectChicagoDate();
  const now = new Date('2026-10-02T23:00:00-05:00');
  expect(dayLabel(at('2026-10-02T20:54:00-05:00'), now)).toBe('Today');
  expect(dayLabel(at('2026-10-01T20:54:00-05:00'), now)).toBe('Yesterday');
  expect(dayLabel(at('2026-09-30T20:54:00-05:00'), now)).toBe('Wed, Sep 30');
  expect(clockTime(at('2026-10-02T20:54:00-05:00'))).toBe('8:54 PM');
});
it('marks the active tab and leaves Chat unscoped', () => {
  render(LogsHeader, { view: 'keys', subline: null });
  expect(screen.getByRole('link', { name: 'Key pops' }).getAttribute('aria-current')).toBe('page');
  expect(screen.getByRole('link', { name: 'Chat' }).getAttribute('href')).toBe('#/logs');
});
it('labels paging actions and exposes loading text', async () => {
  const onOlder = vi.fn();
  const result = render(Pager, { atLatest: false, hasOlder: true, loading: false, noun: 'lines', onLatest: vi.fn(), onNewer: vi.fn(), onOlder });
  expect(screen.getByRole('button', { name: 'Newer lines' })).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Older lines' }));
  expect(onOlder).toHaveBeenCalledOnce();
  await result.rerender({ loading: true });
  expect(screen.getByText('Loading older…')).toBeTruthy();
});
it('hides an empty new-data pill and caps the visible count', async () => {
  const result = render(NewDataPill, { count: 0, noun: 'lines', onClick: vi.fn() });
  expect(screen.queryByRole('button')).toBeNull();
  await result.rerender({ count: 120 });
  expect(screen.getByRole('button').textContent).toContain('99+ new lines');
  for (const [noun, singular] of [['lines', 'line'], ['events', 'event'], ['key pops', 'key pop'], ['alerts', 'alert']]) {
    await result.rerender({ count: 1, noun });
    expect(screen.getByRole('button').textContent).toContain(`1 new ${singular}`);
    expect(screen.getByRole('button').textContent).not.toContain(`1 new ${noun}`);
  }
});
it('keeps the Loot range label by default and accepts a Logs label', async () => {
  const result = render(RangeSwitch, { range: null, onChange: vi.fn() });
  expect(screen.getByRole('group', { name: 'Loot date range' })).toBeTruthy();
  await result.rerender({ label: 'Logs date range' });
  expect(screen.getByRole('group', { name: 'Logs date range' })).toBeTruthy();
});
it('retries the error and keeps the run chip fallback until its history read completes', async () => {
  const onRetry = vi.fn();
  render(ErrorBar, { message: "Couldn't load chat. Your filters are kept.", onRetry });
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
  expect(onRetry).toHaveBeenCalledOnce();
  const client = createClient(createFixtureTransport(fixture));
  const pending = deferred<HistoryReply<RunStory | null>>();
  client.getRun = vi.fn(() => pending.promise);
  render(RunScopeChip, { runId: 'run-1', client, removeHref: '#/logs', links: [{ label: 'Run summary ›', href: '#/runs/run-1' }] });
  expect(screen.getByRole('link', { name: /^Remove run filter:/ }).textContent).toContain('Run');
  await waitFor(() => expect(client.getRun).toHaveBeenCalledWith('run-1'));
  pending.resolve({ status: 'ready', value: story() });
  await screen.findByText(/Run: Invented Sanctum/);
  expect(screen.getByRole('link', { name: /^Remove run filter: Run: Invented Sanctum/ })).toBeTruthy();
  expect(screen.getByRole('link', { name: 'Run summary ›' }).getAttribute('href')).toBe('#/runs/run-1');
});
