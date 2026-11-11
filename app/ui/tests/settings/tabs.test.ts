import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { readable } from 'svelte/store';
import { createAlertSession } from '../../src/lib/alerts/session';
import { SHELL_CONNECTION } from '../../src/lib/shell/context';
import Settings from '../../src/routes/Settings.svelte';
import { parseRoute, productionFeatures, routeHref } from '../../src/lib/navigation/routes';
import { browserHistory, createRouter } from '../../src/lib/navigation/history';
import { setupFixture } from '../setup/fixture';

beforeEach(() => { localStorage.removeItem('maxed.settings.tab'); window.history.replaceState(null, '', '#/settings'); });
afterEach(() => { cleanup(); vi.restoreAllMocks(); localStorage.removeItem('maxed.settings.tab'); window.history.replaceState(null, '', '#/settings'); });
const select = (name: string) => fireEvent.click(screen.getByRole('tab', { name }));

it('groups settings and preserves app and child drafts through tab switches', async () => {
  const f = setupFixture();
  const alerts = createAlertSession(f.client);
  await alerts.refresh();
  const view = render(Settings, { props: { client: f.client }, context: new Map([[SHELL_CONNECTION, { alerts, capture: readable(null) }]]) });
  const channel = await screen.findByRole('combobox', { name: 'Update channel' }) as HTMLSelectElement;
  await fireEvent.change(channel, { target: { value: 'beta' } });
  await select('Data & history');
  const retention = screen.getByLabelText('Keep chat for (days)') as HTMLInputElement;
  await fireEvent.input(retention, { target: { value: '42' } });
  await select('Notifications');
  expect(within(screen.getByRole('tabpanel', { name: 'Notifications' })).getByRole('heading', { name: 'Presentation' })).toBeTruthy();
  const volume = screen.getByRole('slider', { name: /^Volume/ }) as HTMLInputElement;
  await waitFor(() => expect(volume.disabled).toBe(false));
  await fireEvent.input(volume, { target: { value: '37' } });
  expect(screen.queryByRole('combobox', { name: 'Update channel' })).toBeNull();
  expect(screen.getByRole('button', { name: 'Apply' }).getAttribute('aria-disabled')).toBe('false');
  await select('General');
  expect(screen.getByRole('combobox', { name: 'Update channel' })).toBe(channel);
  expect(channel.value).toBe('beta');
  await select('Data & history');
  expect(screen.getByLabelText('Keep chat for (days)')).toBe(retention);
  expect(retention.value).toBe('42');
  await select('Notifications');
  expect(screen.getByRole('slider', { name: /^Volume/ })).toBe(volume);
  expect(volume.value).toBe('37');
  expect(f.client.updateSettings).not.toHaveBeenCalled();
  view.unmount();
  alerts.dispose();
});

it('supports roving focus, arrow wrapping, Home and End', async () => {
  render(Settings, { client: setupFixture().client });
  const tabs = screen.getAllByRole('tab');
  tabs[0].focus();
  for (const [key, index] of [['ArrowRight', 1], ['End', 3], ['ArrowRight', 0], ['ArrowLeft', 3], ['Home', 0]] as const) {
    await fireEvent.keyDown(document.activeElement!, { key });
    expect(document.activeElement).toBe(tabs[index]);
    expect(tabs[index].getAttribute('aria-selected')).toBe('true');
    expect(tabs.filter(tab => tab.tabIndex === 0)).toEqual([tabs[index]]);
    const panel = screen.getByRole('tabpanel');
    expect(tabs[index].getAttribute('aria-controls')).toBe(panel.id);
    expect(panel.getAttribute('aria-labelledby')).toBe(tabs[index].id);
  }
});

it('hides Notifications and falls back from a disabled deep link', async () => {
  window.history.replaceState(null, '', '#/settings?tab=notifications');
  render(Settings, { client: setupFixture().client, tab: 'notifications', features: { ...productionFeatures, logs: false } });
  expect(screen.queryByRole('tab', { name: 'Notifications' })).toBeNull();
  expect(screen.getByRole('tabpanel', { name: 'General' })).toBeTruthy();
  await fireEvent.keyDown(screen.getByRole('tab', { name: 'General' }), { key: 'ArrowRight' });
  expect(document.activeElement).toBe(screen.getByRole('tab', { name: 'Data & history' }));
});

it('preserves the Notifications deep link through router startup', () => {
  window.history.replaceState(null, '', '#/settings?tab=notifications');
  const router = createRouter(browserHistory(window));
  expect(routeHref(router.route)).toBe('#/settings?tab=notifications');
  render(Settings, { client: setupFixture().client, tab: router.route.kind === 'settings' ? router.route.tab : undefined });
  expect(screen.getByRole('tabpanel', { name: 'Notifications' })).toBeTruthy();
  router.dispose();
});

it('remembers a selected tab and tolerates unavailable storage and unknown tabs', async () => {
  const client = setupFixture().client;
  const view = render(Settings, { client });
  await select('Game assets');
  view.unmount();
  const next = render(Settings, { client });
  expect(screen.getByRole('tabpanel', { name: 'Game assets' })).toBeTruthy();
  next.unmount();
  vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new Error('unavailable'); });
  vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new Error('unavailable'); });
  window.history.replaceState(null, '', '#/settings?tab=unknown');
  expect(parseRoute(window.location.hash)).toEqual({ kind: 'settings' });
  render(Settings, { client });
  expect(screen.getByRole('tabpanel', { name: 'General' })).toBeTruthy();
  await select('Notifications');
  expect(screen.getByRole('tabpanel', { name: 'Notifications' })).toBeTruthy();
});


it('follows a changed route tab without remounting drafts or overriding manual selection', async () => {
  const client = setupFixture().client;
  const view = render(Settings, { client, tab: 'general' });
  const channel = await screen.findByRole('combobox', { name: 'Update channel' }) as HTMLSelectElement;
  await fireEvent.change(channel, { target: { value: 'beta' } });
  await view.rerender({ tab: 'data' });
  expect(screen.getByRole('tabpanel', { name: 'Data & history' })).toBeTruthy();
  expect(channel.isConnected).toBe(true);
  expect(channel.value).toBe('beta');
  expect(screen.getByRole('button', { name: 'Apply' }).getAttribute('aria-disabled')).toBe('false');
  await select('General');
  await view.rerender({ enabled: false, features: { ...productionFeatures } });
  expect(screen.getByRole('tabpanel', { name: 'General' })).toBeTruthy();
  expect(screen.getByRole('combobox', { name: 'Update channel' })).toBe(channel);
  expect(channel.value).toBe('beta');
  expect(client.updateSettings).not.toHaveBeenCalled();
});

it('gates a changed Notifications request without reacting again to feature updates', async () => {
  const view = render(Settings, { client: setupFixture().client, tab: 'data', features: { ...productionFeatures, logs: false } });
  await view.rerender({ tab: 'notifications' });
  expect(screen.queryByRole('tab', { name: 'Notifications' })).toBeNull();
  expect(screen.getByRole('tabpanel', { name: 'General' })).toBeTruthy();
  await select('Game assets');
  await view.rerender({ features: { ...productionFeatures } });
  expect(screen.getByRole('tabpanel', { name: 'Game assets' })).toBeTruthy();
  await view.rerender({ tab: 'data' });
  await view.rerender({ tab: 'notifications' });
  expect(screen.getByRole('tabpanel', { name: 'Notifications' })).toBeTruthy();
});
