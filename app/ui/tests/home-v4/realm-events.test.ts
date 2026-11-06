import { expect, it } from 'vitest';
import type { DropView } from '../../src/lib/bindings/DropView';
import type { RealmKillRow } from '../../src/lib/home/realm';
import { eventRows } from '../../src/lib/home/v4/realm-events';
import { homeSnapshot } from '../home/fixtures';

function kill(id: string, at: string, targetType: number | null = 300): RealmKillRow {
  return { id: `fight:${id}`, targetType, bossKey: null, at, evidence: 'hp_zero_removed', provisional: false,
    self: { ...homeSnapshot().fights[0].players[0], dps: 1500, rank: 2, share: .25 }, partial: false,
    killer: null, fightId: id, started: '1000000', ended: at };
}
function drop(id: string, at: string, source_enemy: number | null = 300): DropView {
  return { ...homeSnapshot().drops[0], id, at, run_id: null, source_enemy, source_estimated: false };
}

it('bags_link_to_latest_matching_kill', () => {
  const kills = [kill('later', '20000000'), kill('other', '22000000', 301), kill('first', '10000000')];
  const rows = eventRows(kills, null, [drop('between', '15000000'), drop('equal', '20000000'),
    drop('after', '25000000'), drop('before', '9000000'), drop('unknown', '25000000', null), drop('unmatched', '25000000', 999)]);
  expect(rows.map(row => row.key)).toEqual(['fight:other', 'fight:later', 'fight:first']);
  expect(rows.find(row => row.key === 'fight:later')?.bags.map(bag => bag.dropId)).toEqual(['equal', 'after']);
  expect(rows.find(row => row.key === 'fight:first')?.bags.map(bag => bag.dropId)).toEqual(['between']);
  expect(rows.flatMap(row => row.bags)).toHaveLength(3);
  expect(kills[0].id).toBe('fight:later');
});

it('estimated_source_marks_bag_estimated', () => {
  const rows = eventRows([kill('one', '10000000')], null, [{ ...drop('estimate', '11000000'), source_estimated: true }]);
  expect(rows[0].bags).toEqual([{ dropId: 'estimate', bagType: homeSnapshot().drops[0].bag_type, estimated: true }]);
});

it('announced_kills_have_no_dps', () => {
  const announced = { ...kill('notice', '10000000'), id: 'announcement:notice', evidence: 'realm_announcement' as const,
    fightId: null, provisional: true };
  expect(eventRows([announced], null, [drop('bag', '11000000')])[0]).toMatchObject({ state: 'announced',
    dps: null, rank: null, players: null, share: null, bags: [{ dropId: 'bag', estimated: false }] });
});

it('live_row_keeps_known_metrics_without_inventing_kill_player_counts', () => {
  const live = homeSnapshot().fights[0];
  const rows = eventRows([kill('prior', '1000000')], live, []);
  expect(rows[0]).toMatchObject({ key: `fight:${live.id}`, state: 'live', at: null, players: 1, bags: [] });
  expect(rows[1]).toMatchObject({ state: 'killed', dps: 1500, rank: 2, share: .25, players: null });
  expect(eventRows([kill(live.id, '10000000')], live, [])).toHaveLength(1);
});

it('bag_linking_preserves_microsecond_precision_and_ignores_invalid_times', () => {
  const rows = eventRows([kill('first', '9007199254740992'), kill('next', '9007199254740993'),
    kill('invalid', 'invalid'), kill('unknown-type', '9007199254740993', null)], null,
  [drop('before-next', '9007199254740992'), drop('after-next', '9007199254740994'), drop('invalid', 'invalid'),
    drop('unknown-source', '9007199254740994', null)]);
  expect(rows.find(row => row.key === 'fight:first')?.bags.map(bag => bag.dropId)).toEqual(['before-next']);
  expect(rows.find(row => row.key === 'fight:next')?.bags.map(bag => bag.dropId)).toEqual(['after-next']);
  expect(rows.flatMap(row => row.bags)).toHaveLength(2);
});
