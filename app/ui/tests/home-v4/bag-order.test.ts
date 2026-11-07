import { cleanup, render } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import { bestPreviewBag } from '../../src/lib/home/nexus';
import VisitTimeline from '../../src/lib/home/v4/VisitTimeline.svelte';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { homeSnapshot } from '../home/fixtures';

afterEach(cleanup);
const bag = (id: string, at: string, bag_rank: number | null = 90) => ({
  ...homeSnapshot().drops[0], id, at, bag_rank, run_id: 'synthetic-run',
});
it('recent previews use rank, exact time and descending IDs with last-revision semantics', () => {
  const drops = [bag('z-old', '9007199254740992'), bag('a', '9007199254740993'), bag('Z', '9007199254740993'),
    bag('higher', '1', 255), bag('unranked', '9007199254740994', null)];
  expect(bestPreviewBag(drops, 'synthetic-run')?.id).toBe('higher');
  expect(bestPreviewBag(drops.filter(drop => drop.id !== 'higher'), 'synthetic-run')?.id).toBe('a');
  expect(bestPreviewBag([...drops, { ...drops[3], bag_rank: 1 }], 'synthetic-run')?.id).toBe('a');
  expect(bestPreviewBag(drops, 'other-run')).toBeNull();
  expect(bestPreviewBag([drops[4]], 'synthetic-run')).toBeNull();
});
it('clustered timeline bags use store order for rank, time and ID ties independently of input order', () => {
  const cases = [
    { drops: [bag('z-old', '1000000'), bag('new', '2000000')], expected: 'new' },
    { drops: [bag('Z', '1000000'), bag('a', '1000000')], expected: 'a' },
    { drops: [bag('high', '1000000', 255), bag('new', '2000000')], expected: 'high' },
  ];
  for (const { drops, expected } of cases) for (const input of [drops, [...drops].reverse()]) {
    const page = render(VisitTimeline, { rows: [], kills: [], liveFight: null, drops: input,
      started: '0', at: '100000000', visuals: emptyHomeVisuals(), resolveSpriteUrl: () => '', partial: false });
    const pips = page.container.querySelectorAll('.bag-pip');
    expect(pips).toHaveLength(1);
    expect(pips[0].getAttribute('data-drop-id')).toBe(expected);
    page.unmount();
  }
});
