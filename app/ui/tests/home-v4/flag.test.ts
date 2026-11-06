import { cleanup, render } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { productionFeatures } from '../../src/lib/navigation/routes';
import Shell from '../../src/lib/shell/Shell.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { homeSnapshot } from '../home/fixtures';
import { fakeHistory } from '../navigation/fake-history';
import { flush } from '../runs/fixtures';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

it('turns the redesign on in production', () => {
  expect(productionFeatures.homeRedesign).toBe(true);
});

it('renders v4 by default and preserves v3 with the flag explicitly off', async () => {
  const on = render(Shell, {
    client: createClient(createFixtureTransport(homeSnapshot())),
    preview: true, historyAdapter: fakeHistory('#/').adapter,
  });
  await flush();
  expect(on.container.querySelector('div.home.home-v4[data-home="v4"]')).not.toBeNull();
  expect(on.container.querySelector('[data-home="v3"]')).toBeNull();
  on.unmount();

  const off = render(Shell, {
    client: createClient(createFixtureTransport(homeSnapshot())),
    features: { ...productionFeatures, homeRedesign: false },
    preview: true, historyAdapter: fakeHistory('#/').adapter,
  });
  await flush();
  expect(off.container.querySelector('div.home[data-home="v3"]')).not.toBeNull();
  expect(off.container.querySelector('[data-home="v4"]')).toBeNull();
  expect(off.container.querySelector('.home-v4')).toBeNull();
});
