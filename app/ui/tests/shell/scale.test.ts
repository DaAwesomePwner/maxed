import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { render, screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setDebugMode } from '../../src/lib/debug/debug';
import Shell from '../../src/lib/shell/Shell.svelte';
import Banner from '../../src/lib/components/Banner.svelte';
import { fakeHistory } from '../navigation/fake-history';
import { setupFixture } from '../setup/fixture';
import fixture from '../fixtures/t0-live';

const read = (path: string) => readFileSync(resolve(process.cwd(), `src/${path}`), 'utf8');
const css = (path: string) => read(path).match(/<style>([\s\S]*?)<\/style>/)![1];
let sheet: HTMLStyleElement | undefined;
afterEach(() => { setDebugMode(false); sheet?.remove(); sheet = undefined; });
function styles(paths: string[]) {
  sheet = document.createElement('style');
  sheet.textContent = [read('lib/styles/tokens.css'), ...paths.map(css)].join('\n');
  document.head.append(sheet);
}

describe('structural layout contracts (no jsdom geometry claims)', () => {
  it('preserves rail identity, independent scroll structure and long data (no viewport layout measurement)', async () => {
    setDebugMode(true);
    const f = setupFixture();
    const name = 'SyntheticCharacter'.repeat(12), map = 'SyntheticDungeon'.repeat(16), error = 'RecoveryGuidance'.repeat(30);
    const fame = 9_000_000_000;
    f.transport.publishSnapshot({ ...fixture, map, character: { ...fixture.character, name, fame }, sequence: '13' });
    f.client.startCapture = vi.fn().mockRejectedValue(new Error());
    styles(['lib/components/Card.svelte', 'lib/shell/HeroCard.svelte', 'lib/shell/LiveStatusCard.svelte', 'lib/shell/CharacterRail.svelte', 'lib/shell/Shell.svelte']);
    render(Shell, { client: f.client, historyAdapter: fakeHistory().adapter });
    const rail = screen.getByRole('complementary', { name: 'Character' });
    await within(rail).findByRole('heading', { name });
    expect(within(rail).getByRole('heading', { name: map })).toBeTruthy();
    expect(within(rail).getByRole('meter', { name: 'Fame' }).getAttribute('aria-valuetext')).toBe(String(fame));
    expect(within(rail).getByText('Unverified identity')).toBeTruthy();
    expect(within(rail).getByRole('img', { name: 'Class' }).getAttribute('data-fallback')).toBe('true');
    f.transport.publishCapture({ ...fixture.capture, state: 'Recovering', message: error, decode_failures: 2_000_000 });
    await within(rail).findByText('Recovering');
    expect(within(rail).getByText(error)).toBeTruthy();
    expect(within(rail).getByText('2000000')).toBeTruthy();
    const cards = rail.querySelector('.rail-cards')!;
    expect(getComputedStyle(cards).overflowY).toBe('auto');
    expect(getComputedStyle(cards).minHeight).toBe('0');
    expect(cards.contains(screen.getByRole('navigation', { name: 'Navigation' }))).toBe(false);
    expect(cards.contains(screen.getByRole('link', { name: 'Settings' }))).toBe(false);
    expect(getComputedStyle(screen.getByRole('main')).overflow).toBe('auto');
    expect(getComputedStyle(screen.getByRole('main')).minWidth).toBe('0');
    for (const section of rail.querySelectorAll('section')) {
      expect(getComputedStyle(section).minWidth).toBe('0');
      expect(getComputedStyle(section).overflowWrap).toBe('anywhere');
    }
    const shellCss = css('lib/shell/Shell.svelte');
    expect(shellCss).toContain('grid-template-columns: 15rem minmax(0, 1fr)');
    expect(shellCss).toMatch(/@media \(max-width: 900px\)[\s\S]*grid-template-columns: 12rem minmax\(0, 1fr\)/);
    expect(read('lib/styles/tokens.css')).toContain('--text-body: 1rem');
  });

  it.each(['setup', 'settings'] as const)('contains long local selections and text within the %s main scroller', async (route) => {
    const f = setupFixture();
    const path = `C:\\${'SyntheticLongDirectory'.repeat(30)}\\resources.assets`;
    f.setSettings({ assets_path: path });
    f.client.listAdapters = vi.fn(async () => [{ id: 'synthetic-long', description: 'Synthetic network adapter '.repeat(20) }]);
    styles([`routes/${route === 'setup' ? 'Setup' : 'Settings'}.svelte`, 'lib/setup/AssetsControl.svelte', 'lib/settings/AdapterControl.svelte']);
    render(Shell, { client: f.client, historyAdapter: fakeHistory(`#/${route}`).adapter });
    const main = screen.getByRole('main');
    const input = await within(main).findByRole('textbox', { name: 'Assets Path' }) as HTMLInputElement;
    // Saved settings arrive separately from the route mount.
    await within(main).findByRole('option', { name: /Synthetic network adapter/ });
    expect(input.value).toBe(path);
    expect(getComputedStyle(input).minWidth).toBe('0');
    expect(getComputedStyle(within(main).getByRole('combobox', { name: 'Capture Adapter' })).minWidth).toBe('0');
    const page = main.querySelector(`.${route}-page`)!;
    expect(getComputedStyle(page).overflowWrap).toBe('anywhere');
    expect(getComputedStyle(page).minWidth).toBe('0');
  });

  it('declares banner wrapping and shrinkable copy without measuring viewport layout', () => {
    styles(['lib/components/Banner.svelte']);
    const title = 'SyntheticMoment'.repeat(30);
    render(Banner, { props: { id: 'narrow', kind: 'white_bag', title, context: 'Synthetic context '.repeat(30), onDismiss: vi.fn() } });
    const banner = screen.getByRole('region', { name: 'White Bag' });
    expect(getComputedStyle(banner).flexWrap).toBe('wrap');
    expect(getComputedStyle(banner).minWidth).toBe('0');
    const copy = banner.querySelector('.copy')!;
    expect(getComputedStyle(copy).minWidth).toBe('0');
    expect(getComputedStyle(copy).overflowWrap).toBe('anywhere');
    // A zero basis lets short-titled toasts keep their actions on the same row
    // and squeeze the title to one character per line; a real basis wraps them.
    expect(getComputedStyle(copy).flexBasis).toBe('12rem');
    expect(screen.getByRole('heading', { name: title })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Dismiss White Bag' })).toBeTruthy();
  });
});
