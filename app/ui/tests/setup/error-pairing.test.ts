import { render, screen } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import Setup from '../../src/routes/Setup.svelte';
import Settings from '../../src/routes/Settings.svelte';
import { setupFixture } from './fixture';

type Fixture = ReturnType<typeof setupFixture>;
const reject = () => vi.fn().mockRejectedValue(new Error('PRIVATE failure'));

/** Each failure renders exactly once, as one line without a chained "Retry.". */
async function once(text: string) {
  await screen.findByText(text);
  const matches = screen.getAllByText(text);
  expect(matches).toHaveLength(1);
  expect(document.body.textContent).not.toContain(`${text}. Retry.`);
  return matches[0];
}

describe('Setup pairs each failure with its own retry', () => {
  it.each([
    ['setup', 'Couldn’t check setup', 'Retry checks', (f: Fixture) => { f.client.querySetup = reject(); }],
    ['adapters', 'Couldn’t list adapters', 'Retry adapters', (f: Fixture) => { f.client.listAdapters = reject(); }],
    ['history', 'Couldn’t check history upgrade', 'Retry history status and space', (f: Fixture) => { f.client.queryHistoryUpgradeStatus = reject(); }],
  ] as const)('shows the %s failure once, beside its retry button', async (_key, text, retry, fail) => {
    const f = setupFixture(); fail(f);
    render(Setup, { client: f.client });
    const message = await once(text);
    const buttons = screen.getAllByRole('button', { name: retry });
    expect(buttons.some(button => button.parentElement === message.parentElement)).toBe(true);
    expect(message.closest('[role="alert"]')).toBeNull();
    expect(message.closest('[aria-live="assertive"]')).toBe(message);
  });

  it('keeps the settings failure in the alert list with its own retry', async () => {
    const f = setupFixture(); f.client.querySettings = reject();
    render(Setup, { client: f.client });
    const message = await once('Couldn’t load settings');
    const row = message.closest('li')!;
    expect(row.closest('[role="alert"]')).toBeTruthy();
    expect(row.querySelector('button')?.textContent).toBe('Retry settings');
    expect(screen.getAllByRole('alert')).toHaveLength(1);
  });
});

describe('Settings keeps every failure visible on any tab', () => {
  beforeEach(() => { localStorage.removeItem('maxed.settings.tab'); window.history.replaceState(null, '', '#/settings'); });

  it.each(['general', 'data', 'assets'])('shows an adapter failure once in the top list with the %s tab active', async tab => {
    localStorage.setItem('maxed.settings.tab', tab);
    const f = setupFixture(); f.client.listAdapters = reject();
    render(Settings, { client: f.client });
    const message = await once('Couldn’t list adapters');
    expect(message.closest('[role="alert"]')).toBeTruthy();
    expect(message.closest('[hidden], [role="tabpanel"]')).toBeNull();
  });

  it('pairs the settings failure with Retry settings', async () => {
    const f = setupFixture(); f.client.querySettings = reject();
    render(Settings, { client: f.client });
    const message = await once('Couldn’t load settings');
    expect(message.closest('li')?.querySelector('button')?.textContent).toBe('Retry settings');
  });
});
