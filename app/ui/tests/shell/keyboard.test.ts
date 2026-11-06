import { render, screen, waitFor, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import Shell from '../../src/lib/shell/Shell.svelte';
import { productionFeatures } from '../../src/lib/navigation/routes';
import Setup from '../../src/routes/Setup.svelte';
import Settings from '../../src/routes/Settings.svelte';
import FixtureApp from './fixtures/FixtureApp.svelte';
import { fakeHistory } from '../navigation/fake-history';
import { setupFixture } from '../setup/fixture';

async function tabOrder(expected: HTMLElement[]) {
  const user = userEvent.setup();
  for (const element of expected) {
    await user.tab(); expect(document.activeElement).toBe(element);
  }
  for (const element of expected.slice(0, -1).reverse()) {
    await user.tab({ shift: true }); expect(document.activeElement).toBe(element);
  }
  expect(document.querySelector('[tabindex]:not([tabindex="0"]):not([tabindex="-1"])')).toBeNull();
}
const button = (name: string) => screen.getByRole('button', { name });

describe('document keyboard order', () => {
  it.each([false, true])('traverses the rail without removed breadcrumb controls (loot=%s)', async lootBrowsing => {
    render(Shell, { client: setupFixture().client, historyAdapter: fakeHistory('#/runs').adapter, features: { lootBrowsing, logs: false, stats: false, party: false, character: false, homeRedesign: productionFeatures.homeRedesign } });
    await screen.findByRole('heading', { name: 'Runs', level: 1 });
    const nav = within(screen.getByRole('navigation', { name: 'Navigation' }));
    expect(screen.queryByRole('navigation', { name: 'Breadcrumb' })).toBeNull();
    expect(screen.queryByText('Details')).toBeNull();
    await tabOrder([
      screen.getByRole('link', { name: 'Skip to Content' }), button('Start Capture'),
      nav.getByRole('link', { name: 'Home' }), nav.getByRole('link', { name: 'Runs' }),
      ...(lootBrowsing ? [nav.getByRole('link', { name: 'Loot' })] : []),
      nav.getByRole('link', { name: 'Alerts' }), screen.getByRole('link', { name: 'Settings' }),
    ]);
    await userEvent.setup().keyboard('{Enter}');
    expect(document.activeElement).toBe(screen.getByRole('main'));
  });

  it('reaches every setup action in document order', async () => {
    render(Setup, { client: setupFixture().client });
    await screen.findByRole('option', { name: 'Synthetic Ethernet' });
    await tabOrder([
      button('Retry Checks'), screen.getByRole('link', { name: 'Settings' }), button('Check Access'),
      screen.getByRole('combobox', { name: 'Capture Adapter' }), button('Apply Adapter'), button('Retry Adapters'),
      screen.getByRole('textbox', { name: 'Assets Path' }), button('Extract'), button('Retry Detection'),
      button('Start Capture'), button('Stop Capture'),
    ]);
  });

  it('reaches Settings actions and skips unavailable native editing controls', async () => {
    const { container } = render(Settings, { client: setupFixture().client });
    // jsdom has no browser UA rule hiding the content of closed details.
    // Model that native rule so tab traversal checks the actual disclosure state.
    const nativeDetailsStyle = document.createElement('style');
    nativeDetailsStyle.textContent = 'details:not([open]) > :not(summary) { display: none; }';
    container.append(nativeDetailsStyle);
    await screen.findByRole('combobox', { name: 'Update Channel' });
    const setupLink = screen.getByRole('link', { name: 'Setup' });
    const delivery = screen.getByText('Delivery', { selector: 'summary' });
    const assets = screen.getByText('Assets Path', { selector: 'summary' });
    const recovery = screen.getByText('Recovery', { selector: 'summary' });
    const user = userEvent.setup();
    for (const expanded of [false, true]) {
      if (expanded) {
        await user.click(screen.getByRole('tab', { name: 'Game assets' }));
        for (const summary of [assets, recovery]) {
          await user.click(summary);
          expect((summary.parentElement as HTMLDetailsElement).open).toBe(true);
        }
      }
      for (const name of ['General', 'Data & history', 'Game assets', 'Notifications']) {
        const tab = screen.getByRole('tab', { name });
        await user.click(tab);
        const content = name === 'General' ? [
          screen.getByRole('combobox', { name: 'Capture Adapter' }), button('Retry Adapters'),
          screen.getByRole('checkbox', { name: 'Hardware Acceleration' }),
          screen.getByRole('combobox', { name: 'Update Channel' }), button('Check Updates'), button('Retry Updates'),
          screen.getByRole('switch', { name: 'Demo Mode' }), screen.getByRole('checkbox', { name: /Debug [Mm]ode/ }),
        ] : name === 'Data & history' ? [
          button('Refresh storage'), button('Estimate receipt storage'), screen.getByRole('checkbox', { name: 'Save Recordings' }), button('Retry Recording'),
          ...['All', 'Guild', 'Party', 'Whisper'].map(channel => screen.getByRole('checkbox', { name: `Store ${channel} chat` })),
          screen.getByRole('spinbutton', { name: 'Keep chat for (days)' }),
          screen.getByRole('checkbox', { name: 'Filter spam ads and repeated public messages' }),
          screen.getByRole('textbox', { name: 'Add keyword' }), button('Add'),
          button('Save chat settings'), button('Clear chat history'), button('Clear party history'),
        ] : name === 'Game assets' ? [
          assets, ...(expanded ? [screen.getByRole('textbox', { name: 'Assets Path' }), button('Extract'), button('Retry Detection')] : []),
        ] : [delivery];
        const order = [setupLink, tab, ...content, button('Apply'),
          screen.getByRole('link', { name: 'Alert history' }), recovery,
          ...(expanded ? [button('Retry Settings'), button('Retry Checks')] : [])];
        order[0].focus();
        for (const element of order.slice(1)) {
          await user.tab(); expect(document.activeElement).toBe(element);
        }
        for (const element of order.slice(0, -1).reverse()) {
          await user.tab({ shift: true }); expect(document.activeElement).toBe(element);
        }
        expect(document.querySelector('[tabindex]:not([tabindex="0"]):not([tabindex="-1"])')).toBeNull();
      }
    }
    expect((within(button('Apply').closest('form')!).getByRole('button', { name: 'Revert' }) as HTMLButtonElement).disabled).toBe(true);
    await user.click(screen.getByRole('tab', { name: 'Game assets' }));
    expect((button('Cancel') as HTMLButtonElement).disabled).toBe(true);
    await user.click(screen.getByRole('tab', { name: 'Data & history' }));
    expect((screen.getByRole('checkbox', { name: 'Save Recordings' }) as HTMLInputElement).disabled).toBe(false);
  });

  it('enters a drill-down by keyboard and restores row focus using Alt+Left/Right', async () => {
    render(FixtureApp, { historyAdapter: fakeHistory('#/runs').adapter });
    const row = await screen.findByRole('link', { name: /Synthetic Sanctum/ });
    const user = userEvent.setup(); row.focus(); await user.keyboard('{Enter}');
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('heading', { name: 'Run', level: 1 })));
    await user.keyboard('{Alt>}{ArrowLeft}{/Alt}');
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('link', { name: /Synthetic Sanctum/ })));
    await user.keyboard('{Alt>}{ArrowRight}{/Alt}');
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('heading', { name: 'Run', level: 1 })));
  });
});
