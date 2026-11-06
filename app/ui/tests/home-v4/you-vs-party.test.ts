import { cleanup, render } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import type { DamageBucket } from '../../src/lib/bindings/DamageBucket';
import { partySeries } from '../../src/lib/home/v4/you-vs-party';
import YouVsPartyChart from '../../src/lib/home/v4/YouVsPartyChart.svelte';

afterEach(cleanup);
const bucket = (patch: Partial<DamageBucket> = {}): DamageBucket => ({
  from: '0', until: '2000000', total: 1400, local: 600, others_damage: 800, others_presence: 4, ...patch,
});

it('party_line_divides_others_damage_by_active_players_and_seconds', () => {
  expect(partySeries([bucket()])).toEqual({ you: [300], party: [200], partyRecorded: true });
});

it('legacy_buckets_mark_party_unrecorded', () => {
  for (const missing of [undefined, null]) {
    for (const patch of [{ others_damage: missing }, { others_presence: missing }]) {
      expect(partySeries([bucket(), bucket(patch)]).partyRecorded).toBe(false);
      expect(partySeries([bucket(patch)]).party).toEqual([null]);
    }
  }
});

it('zero_active_others_gives_null_point', () => {
  expect(partySeries([bucket({ others_damage: 0, others_presence: 0 })]))
    .toEqual({ you: [300], party: [null], partyRecorded: true });
});

it('you_line_uses_local_per_second', () => {
  expect(partySeries([bucket({ local: 50, until: '500000' }), bucket({ local: null }), bucket({ local: 0 })]).you)
    .toEqual([100, null, 0]);
});

it('invalid_durations_and_rates_stay_unknown_and_large_timestamps_keep_precision', () => {
  expect(partySeries([bucket({ from: 'bad' }), bucket({ until: '0' }), bucket({ from: '3000000' })]).you)
    .toEqual([null, null, null]);
  expect(partySeries([bucket({ local: Infinity, others_damage: NaN }), bucket({ local: -1, others_presence: -1 })]))
    .toEqual({ you: [null, null], party: [null, null], partyRecorded: true });
  expect(partySeries([bucket({ from: '9007199254740992000', until: '9007199254740992001', local: 1 })]).you)
    .toEqual([1_000_000]);
});

it('chart_note_when_party_unrecorded', async () => {
  const page = render(YouVsPartyChart, { buckets: [bucket(), bucket({ others_damage: null })] });
  expect(page.getByText('You')).toBeTruthy();
  expect(page.queryByText('Party average')).toBeNull();
  expect(page.getByText('Party average line not recorded for this fight')).toBeTruthy();
  expect(page.container.querySelector('path.party')).toBeNull();
  await page.rerender({ buckets: [bucket()] });
  expect(page.getByText('Party average')).toBeTruthy();
  expect(page.queryByText('Party average line not recorded for this fight')).toBeNull();
});

it('chart_has_round_ticks_end_markers_and_fixed_height', () => {
  const page = render(YouVsPartyChart, { buckets: [bucket({ local: 8000, others_damage: 16000 })], height: 180 });
  expect(page.getByRole('img', { name: 'Your DPS and party average over the fight' })).toBeTruthy();
  expect(page.getByText('2k')).toBeTruthy();
  expect(page.getByText('4k')).toBeTruthy();
  expect(page.container.querySelectorAll('circle[r="4"]')).toHaveLength(2);
  expect((page.container.querySelector('.plot') as HTMLElement).style.height).toBe('180px');
  expect(page.getByText('0:02')).toBeTruthy();
});

it('chart_breaks_lines_at_missing_points_and_time_gaps', () => {
  const page = render(YouVsPartyChart, { buckets: [bucket(),
    bucket({ from: '2000000', until: '4000000', local: null, others_presence: 0 }),
    bucket({ from: '4000000', until: '6000000' }), bucket({ from: '8000000', until: '10000000' })] });
  for (const series of ['you', 'party']) {
    expect(page.container.querySelector(`path.${series}`)?.getAttribute('d')?.match(/M/g)).toHaveLength(3);
  }
});

it('empty_chart_does_not_invent_zero_dps', () => {
  const page = render(YouVsPartyChart, { buckets: [] });
  expect(page.getByText('No live fight')).toBeTruthy();
  expect(partySeries([]).partyRecorded).toBe(false);
  expect(page.queryByText('Party average')).toBeNull();
  expect(page.queryByText('Party average line not recorded for this fight')).toBeNull();
  expect(page.container.querySelector('path')).toBeNull();
});
