import { cleanup, render } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import FightByFight from '../../src/lib/home/v4/FightByFight.svelte';
import ChainStrip from '../../src/lib/home/v4/ChainStrip.svelte';
import VisitEventsCard from '../../src/lib/home/v4/VisitEventsCard.svelte';
import RecapBanner from '../../src/lib/home/v4/RecapBanner.svelte';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { runView, visitView } from './recap-image-fixtures';

afterEach(cleanup);
const visuals = { ...emptyHomeVisuals(), objects: [{ object_type: 30, name: 'Synthetic catalog boss', sprite: null, bag: null }] };
const common = { visuals, resolveSpriteUrl: () => '' };

it.each(['fight', 'chain', 'visit'] as const)('%s_party_comparison_reads_once_and_preserves_tone', kind => {
  const values = [1, -.5, .004, null];
  const page = kind === 'fight' ? render(FightByFight, { ...common, partial: false,
    rows: values.map((vsParty, i) => ({ ...runView().fights[0], fightId: `synthetic-${i}`, vsParty })) })
    : kind === 'visit' ? render(VisitEventsCard, { ...common,
      rows: values.map((vsParty, i) => ({ ...visitView().events[0], key: `synthetic-${i}`, vsParty })) })
      : render(ChainStrip, { ...common, chain: { label: 'Synthetic chain', totals: { time: '1:00', bags: 0, whites: 0 },
        steps: values.map((value, i) => ({ visit_id: `synthetic-${i}`, kind: 'castle' as const, run_id: null,
          name: 'Synthetic Castle', started: '0', ended: '1000000', local_dps: value === null ? null : 100 * (1 + value),
          party_dps: 100, bags: 0, whites: 0, item_types: [] })) } });
  const cells = [...page.container.querySelectorAll('tbody tr')].map(row => row.querySelectorAll('td')[kind === 'chain' ? 2 : 1]);
  expect(cells.map(cell => cell.getAttribute('data-tone'))).toEqual(['up', 'down', 'neutral', null]);
  expect(cells.map(cell => {
    const readable = cell.cloneNode(true) as HTMLElement;
    readable.querySelectorAll('[aria-hidden="true"]').forEach(node => node.remove());
    expect(readable.querySelector('[role="group"], [aria-label]')).toBeNull();
    return readable.textContent!.trim();
  })).toEqual(['100% above party average', '50% below party average', '0% vs party average', '—']);
  if (kind !== 'chain') {
    expect(page.getAllByText('Synthetic catalog boss')).toHaveLength(4);
    expect(page.queryByText('Synthetic Guardian')).toBeNull();
  } else {
    const region = page.getByRole('region', { name: 'Chain' });
    expect(region.getAttribute('aria-labelledby')).toBe(page.getByRole('heading', { name: 'Chain' }).id);
    expect(region.hasAttribute('aria-label')).toBe(false);
  }
});

it('banner_portal_and_bag_beside_visible_name_are_decorative', () => {
  const view = runView(); view.best!.drop.contents = []; view.best!.drop.items = []; view.best!.drop.contents_known = false;
  const page = render(RecapBanner, { ...common, view, portalType: 30, visuals: { ...visuals, objects: [
    ...visuals.objects, { object_type: 20, name: 'Synthetic named bag', sprite: null, bag: null },
  ] } });
  expect(page.getByText('Synthetic named bag')).toBeTruthy();
  expect(page.queryByRole('img', { name: 'Synthetic named bag' })).toBeNull();
  expect(page.queryByRole('img', { name: 'Synthetic catalog boss' })).toBeNull();
});
