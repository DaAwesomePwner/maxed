// @vitest-environment node
import { readFileSync, readdirSync } from 'node:fs';
import { expect, it } from 'vitest';
import { homeSnapshot } from './fixtures';
import { dungeonView } from '../../src/lib/home/dungeon';
import { damageUncertain, reasonLabel } from '../../src/lib/home/evidence';

it('labels every engine-emitted attribution reason, including all dialogue rules', () => {
  const directory = new URL('../../../../crates/maxed-engine/src/', import.meta.url);
  const sources = readdirSync(directory).filter(name => name.endsWith('.rs'))
    .map(name => readFileSync(new URL(name, directory), 'utf8'));
  // Shared reason constants also emit engine attribution codes.
  sources.push(readFileSync(new URL('../../../../crates/maxed-types/src/domain.rs', import.meta.url), 'utf8'));
  const codes = new Set<string>();
  // These are completion IDs, synthetic identifiers, a perf-only feature and
  // the Character formula version, not attribution reasons.
  // Inspect all remaining kebab-case engine literals so a new reason emitted
  // through a different Rust expression still requires an explicit label.
  const otherIds = new Set(['moonlight-kitsune', 'moonlight-miko', 'moonlight-revelry',
    'shatters-azamoth', 'shatters-king', 'void-entity', 'synthetic-maintenance', 'perf-live', 'synthetic-account', 'synthetic-presence', 'synthetic-second-local', 'character-v1']);
  for (const source of sources) {
    for (const match of source.matchAll(/"([a-z][a-z0-9]*(?:-[a-z0-9]+)+)"/g)) {
      if (!otherIds.has(match[1])) codes.add(match[1]);
    }
  }
  expect([...codes].sort()).toEqual([
    'ambiguous-nearby-killed-targets', 'ambiguous-nearby-removed-targets', 'client-projectile-reconstruction',
    'dialogue-archmage', 'dialogue-azamoth-hm-variant-unverified', 'dialogue-king', 'dialogue-kitsune', 'dialogue-miko',
    'dialogue-nox-hm-variant-unverified', 'dialogue-revelry-tr-variant-unverified', 'dialogue-sentinel',
    'dialogue-valen-hm-variant-unverified', 'dialogue-void', 'inferred-equipped-weapon', 'killed-target-same-tick', 'nearest-removed-hit-target',
    'resource-limit-source-summary', 'source-not-observed',
  ]);
  const unknown = reasonLabel('future-unknown-code');
  for (const code of codes) {
    expect(reasonLabel(code), code).not.toBe(unknown);
    expect(reasonLabel(code), code).not.toContain(code);
  }
  expect(reasonLabel('nearest-removed-hit-target')).toContain('nearest recently killed enemy');
  expect(reasonLabel('inferred-equipped-weapon')).toBe('Inferred from the equipped weapon');
  expect(reasonLabel('ambiguous-nearby-removed-targets')).toContain('Several nearby enemies');
  expect(reasonLabel('dialogue-valen-hm-variant-unverified')).toContain('variant is unverified');
});

it.each([
  [false, [], false], [true, [], true],
  [true, ['inferred-equipped-weapon'], false],
  [false, ['inferred-equipped-weapon', 'inferred-equipped-weapon'], false],
  [true, ['inferred-equipped-weapon', 'client-projectile-reconstruction'], true],
  [false, ['source-not-observed'], true],
] as [boolean, string[], boolean][])('damage uncertainty: estimated=%s reasons=%j', (estimated, reasons, expected) => {
  const source = homeSnapshot().fights[0].players[0].sources[0];
  expect(damageUncertain({ ...source, estimated, reasons })).toBe(expected);
});

it('dungeon partial ignores inferred labels and retains damage uncertainty', () => {
  const snapshot = homeSnapshot();
  snapshot.drops = [];
  for (const player of snapshot.fights[0].players) {
    for (const source of player.sources) {
      source.estimated = true;
      source.reasons = ['inferred-equipped-weapon'];
    }
  }
  expect(dungeonView(snapshot).partial).toBe(false);
  snapshot.fights[0].players[0].sources[0].reasons.push('client-projectile-reconstruction');
  expect(dungeonView(snapshot).partial).toBe(true);
});
