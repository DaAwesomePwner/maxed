// @vitest-environment node
import { readFileSync, readdirSync } from 'node:fs';
import { expect, it } from 'vitest';
import { reasonLabel } from '../../src/lib/home/evidence';

it('labels every engine-emitted attribution reason, including all dialogue rules', () => {
  const directory = new URL('../../../../crates/maxed-engine/src/', import.meta.url);
  const sources = readdirSync(directory).filter(name => name.endsWith('.rs'))
    .map(name => readFileSync(new URL(name, directory), 'utf8'));
  const codes = new Set<string>();
  // These are completion IDs, synthetic identifiers and a perf-only feature,
  // not attribution reasons.
  // Inspect all remaining kebab-case engine literals so a new reason emitted
  // through a different Rust expression still requires an explicit label.
  const otherIds = new Set(['moonlight-kitsune', 'moonlight-miko', 'moonlight-revelry',
    'shatters-azamoth', 'shatters-king', 'void-entity', 'synthetic-maintenance', 'perf-live', 'synthetic-account']);
  for (const source of sources) {
    for (const match of source.matchAll(/"([a-z][a-z0-9]*(?:-[a-z0-9]+)+)"/g)) {
      if (!otherIds.has(match[1])) codes.add(match[1]);
    }
  }
  expect([...codes].sort()).toEqual([
    'ambiguous-nearby-killed-targets', 'ambiguous-nearby-removed-targets', 'client-projectile-reconstruction',
    'dialogue-archmage', 'dialogue-azamoth-hm-variant-unverified', 'dialogue-king', 'dialogue-kitsune', 'dialogue-miko',
    'dialogue-nox-hm-variant-unverified', 'dialogue-revelry-tr-variant-unverified', 'dialogue-sentinel',
    'dialogue-valen-hm-variant-unverified', 'dialogue-void', 'killed-target-same-tick', 'nearest-removed-hit-target',
    'resource-limit-source-summary', 'source-not-observed',
  ]);
  const unknown = reasonLabel('future-unknown-code');
  for (const code of codes) {
    expect(reasonLabel(code), code).not.toBe(unknown);
    expect(reasonLabel(code), code).not.toContain(code);
  }
  expect(reasonLabel('nearest-removed-hit-target')).toContain('nearest recently killed enemy');
  expect(reasonLabel('ambiguous-nearby-removed-targets')).toContain('Several nearby enemies');
  expect(reasonLabel('dialogue-valen-hm-variant-unverified')).toContain('variant is unverified');
});
