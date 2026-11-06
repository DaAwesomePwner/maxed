import { expect, it } from 'vitest';
import { fightPlayerHref, runPlayerHref } from '../../src/lib/players/links';
import { damageRows } from '../../src/lib/home/v3/screen-data';
import { fight, player } from '../runs/fixtures';

it('links a unique fight object id with optional run context', () => {
  const row = damageRows([player()], null)[0];
  expect(fightPlayerHref(fight(), row, 'run /1')).toBe('#/runs/run%20%2F1/fights/fight-1/players/7');
  expect(fightPlayerHref(fight(), row)).toBe('#/fights/fight-1/players/7');
});

it('does not link null, absent or reused fight object ids', () => {
  const row = damageRows([player()], null)[0];
  expect(fightPlayerHref(fight(), { ...row, object_id: null })).toBeNull();
  expect(fightPlayerHref(fight(), { ...row, object_id: 99 })).toBeNull();
  expect(fightPlayerHref(fight({ players: [player(), player({ player_id: 'second' })] }), row)).toBeNull();
});

it('links run identity without an object id and leaves missing identities plain', () => {
  const row = { ...damageRows([player()], null)[0], object_id: null, player_id: 'test:player:3:42:0' };
  expect(runPlayerHref('run /1', row)).toBe('#/runs/run%20%2F1/players/test%3Aplayer%3A3%3A42%3A0');
  expect(runPlayerHref('run-1', { ...row, player_id: '' })).toBeNull();
  expect(fightPlayerHref(fight(), { ...row, object_id: 7, player_id: '' })).toBeNull();
});
