import { describe, expect, it } from 'vitest';
import { potsLeft, statPotsLeft, statTone, vitalText } from '../../src/lib/shell/vitals';

describe('game rail numbers', () => {
  it('uses one point for core stats and five for HP/MP, rounding up', () => {
    expect(potsLeft(20, 45, 70)).toBe(25);
    expect(potsLeft(0, 1075, 1210)).toBe(27);
    expect(potsLeft(3, 350, 361)).toBe(3);
    expect(potsLeft(28, 80, 75)).toBe(0);
    expect(potsLeft(21, null, 25)).toBeNull();
    expect(potsLeft(22, 30, null)).toBeNull();
    expect(potsLeft(20, Number.NaN, 75)).toBeNull();
    expect(potsLeft(20, 20, Infinity)).toBeNull();
  });
  it('gives cap priority and treats either sign of a nonzero modifier as boosted', () => {
    expect(statTone(true, 105)).toBe('max');
    expect(statTone(true, null)).toBe('max');
    expect(statTone(false, 105)).toBe('boosted');
    expect(statTone(false, -2)).toBe('boosted');
    expect(statTone(false, 0)).toBe('plain');
    expect(statTone(false, null)).toBe('plain');
    expect(statTone(false, Number.NaN)).toBe('plain');
  });
  it('keeps stress values compact and missing boosts explicit', () => {
    expect(vitalText(1250, 1385, 27, 310)).toBe('1250/1385|27 (+310)');
    expect(vitalText(150, null, 25, 105)).toBe('150|25 (+105)');
    expect(vitalText(25, null, 0, -2)).toBe('25 (−2)');
    expect(vitalText(0, 800, 0, 0)).toBe('0/800');
    expect(vitalText(null, 800, null, null)).toBe('—/800 (—)');
    expect(vitalText(null, null, null, null)).toBe('— (—)');
    expect(vitalText(Number.NaN, 800, null, Infinity)).toBe('—/800 (—)');
  });
});

const eligible = { placement: 'provisional_inside_boost' as const, cap_eligible: true, reasons: [] };
it('requires shared evidence, observed exalt and a cap decision before counting pots', () => {
  const line = { stat_id: 20, total: 75, base: 73, cap: 75, exalt: 0, at_cap: false,
    split_evidence: eligible };
  expect(statPotsLeft(line)).toBe(2);
  expect(statPotsLeft({ ...line, exalt: 2 })).toBeNull();
  expect(statPotsLeft({ ...line, exalt: null })).toBeNull();
  expect(statPotsLeft({ ...line, exalt: -1 })).toBeNull();
  expect(statPotsLeft({ ...line, at_cap: null })).toBeNull();
  expect(statPotsLeft({ ...line, split_evidence: undefined })).toBeNull();
  expect(statPotsLeft({ ...line, split_evidence: { ...eligible, cap_eligible: false } })).toBeNull();
  expect(statPotsLeft(null)).toBeNull();
  expect(statPotsLeft(undefined)).toBeNull();
  expect(statPotsLeft({ ...line, base: -1 })).toBeNull();
  expect(statPotsLeft({ ...line, base: Number.NaN })).toBeNull();
});
it('counts only trained base for verified exalt placement, including HP/MP rounding', () => {
  const line = { stat_id: 20, total: 75, base: 73, cap: 75, exalt: 2, at_cap: false,
    split_evidence: { ...eligible, placement: 'verified_inside_base' as const } };
  expect(statPotsLeft(line)).toBe(2);
  expect(statPotsLeft({ ...line, stat_id: 0, base: 739, cap: 750, exalt: 10 })).toBe(3);
  expect(statPotsLeft({ ...line, stat_id: 3, base: 300, cap: 311, exalt: 10 })).toBe(3);
});
