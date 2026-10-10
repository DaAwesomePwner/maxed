import { cleanup, render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { breadcrumbs, parseRoute, productionFeatures, routeHref, type Route } from '../../src/lib/navigation/routes';
import CharacterRail from '../../src/lib/shell/CharacterRail.svelte';
import { railView } from '../../src/lib/shell/rail';
import { createRouter } from '../../src/lib/navigation/history';
import { defaultLogsPosition } from '../../src/lib/navigation/state';
import { fakeHistory } from '../navigation/fake-history';

const on = { lootBrowsing: true, logs: true, stats: false }, off = { lootBrowsing: true, logs: false, stats: false };
const routes: Array<[string, Route]> = [
  ['#/logs/chat/guild/run/r1', { kind: 'logs', view: 'chat', channel: 'guild', runId: 'r1' }],
  ['#/logs/chat/party/run/r1', { kind: 'logs', view: 'chat', channel: 'party', runId: 'r1' }],
  ['#/logs/chat/whisper/run/r1', { kind: 'logs', view: 'chat', channel: 'whisper', runId: 'r1' }],
  ['#/logs/chat/whisper/Quenby', { kind: 'logs', view: 'chat', channel: 'whisper', partner: 'Quenby' }],
  ['#/logs/from/Vexlin', { kind: 'logs', view: 'chat', channel: 'all', seedSender: 'Vexlin' }],
  ['#/logs', { kind: 'logs', view: 'chat', channel: 'all' }],
  ['#/logs/chat/guild', { kind: 'logs', view: 'chat', channel: 'guild' }],
  ['#/logs/chat/party', { kind: 'logs', view: 'chat', channel: 'party' }],
  ['#/logs/chat/whisper', { kind: 'logs', view: 'chat', channel: 'whisper' }],
  ['#/logs/run/r1', { kind: 'logs', view: 'chat', channel: 'all', runId: 'r1' }],
  ['#/logs/keys', { kind: 'logs', view: 'keys', channel: 'all' }],
  ['#/logs/events', { kind: 'logs', view: 'events', channel: 'all' }],
  ['#/logs/events/run/r1', { kind: 'logs', view: 'events', channel: 'all', runId: 'r1' }],
  ['#/logs/alerts', { kind: 'logs', view: 'alerts', channel: 'all' }],
];
afterEach(cleanup);
describe('logs routes', () => {
  it('carries slot filters and range between views, resetting pages and selections', () => {
    const router = createRouter(fakeHistory('#/logs').adapter);
    const position = defaultLogsPosition();
    position.chat.filter.query = 'carry'; position.chat.filter.cursor = { at: '1', id: 'line' };
    position.chat.previous = [null]; position.chat.selectedId = 'line';
    position.keys.selectedId = 'portal';
    position.range = { start: '1', end: '2000' };
    router.save({ logsPosition: position });
    router.navigate(parseRoute('#/logs/keys'));
    expect(router.state.entry.logsPosition?.keys.selectedId).toBeNull();
    router.navigate(parseRoute('#/logs'));
    expect(router.state.entry.logsPosition?.chat).toMatchObject({ filter: { query: 'carry', cursor: null }, previous: [], selectedId: null });
    router.navigate(parseRoute('#/logs/keys'));
    router.navigate(parseRoute('#/logs/events'));
    expect(router.state.entry.logsPosition?.range).toEqual(position.range);
    router.back(); router.back(); router.back(); router.back();
    expect(router.state.entry.logsPosition?.chat.filter.cursor).toEqual({ at: '1', id: 'line' });
    router.dispose();
  });
  it('applies sender and partner seeds only to their entry and own slot', () => {
    const router = createRouter(fakeHistory('#/logs').adapter);
    const position = defaultLogsPosition(); position.chat.filter.query = 'old'; position.keys.filter.dungeon = 'Invented vault';
    router.save({ logsPosition: position }); router.navigate(parseRoute('#/logs/from/Vexlin'));
    expect(router.state.entry.logsPosition?.chat.filter).toMatchObject({ sender: 'Vexlin', query: '' });
    expect(router.state.entry.logsPosition?.keys.filter.dungeon).toBe('Invented vault');
    router.navigate(parseRoute('#/logs/chat/whisper/Quenby'));
    expect(router.state.entry.logsPosition?.whisper.partner).toBe('Quenby');
    expect(router.state.entry.logsPosition?.chat.filter.sender).toBeNull();
    router.back(); router.back();
    expect(router.state.entry.logsPosition?.chat.filter.query).toBe('old'); router.dispose();
  });
  it('rejects unsafe or oversized names and keeps run scope in channel links', () => {
    for (const name of ['x'.repeat(33), 'é'.repeat(17), '%00', '%C2%85', '%']) {
      expect(parseRoute(`#/logs/chat/whisper/${name}`)).toEqual({ kind: 'not-found' });
      expect(parseRoute(`#/logs/from/${name}`)).toEqual({ kind: 'not-found' });
    }
    const scoped = parseRoute('#/logs/run/r1');
    if (scoped.kind !== 'logs') throw new Error('Expected Logs');
    expect(routeHref({ ...scoped, channel: 'guild' })).toBe('#/logs/chat/guild/run/r1');
  });
  it.each(routes)('parses and round-trips %s', (hash, route) => {
    expect(parseRoute(hash, on)).toEqual(route);
    expect(routeHref(route)).toBe(hash);
  });
  it.each(routes)('is not found with the flag off: %s', hash => expect(parseRoute(hash, off)).toEqual({ kind: 'not-found' }));
  it('redirects #/alerts to Logs › Alerts only with the flag on', () => {
    expect(parseRoute('#/alerts', on)).toEqual({ kind: 'logs', view: 'alerts', channel: 'all' });
    expect(parseRoute('#/alerts', off)).toEqual({ kind: 'alerts' });
  });
  it.each(['#/logs/chat', '#/logs/chat/public', '#/logs/nope', '#/logs/run', '#/logs/events/run'])('rejects %s', hash => {
    expect(parseRoute(hash, on)).toEqual({ kind: 'not-found' });
  });
  it('turns the flag on in production', () => expect(productionFeatures.logs).toBe(true));
  it('builds breadcrumbs', () => {
    expect(breadcrumbs(parseRoute('#/logs/keys', on)).map(c => c.label)).toEqual(['Home', 'Logs', 'Key Pops']);
  });
  it('shows Logs instead of Alerts in the rail when on', () => {
    render(CharacterRail, { route: { kind: 'logs', view: 'alerts', channel: 'all' }, features: on, view: railView(null), capture: null,
      preview: true, busy: false, error: '', onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn() });
    const nav = within(screen.getByRole('navigation', { name: 'Navigation' }));
    expect(nav.queryByRole('link', { name: 'Alerts' })).toBeNull();
    expect(nav.getByRole('link', { name: 'Logs' }).getAttribute('aria-current')).toBe('page');
  });
});
