import { describe, expect, it } from 'vitest';
import { formatClockRange, formatDps, formatElapsed, formatShare, formatStat } from '../../src/lib/styles/format';
import { bagTone } from '../../src/lib/styles/rarity';
import type { BagDef } from '../../src/lib/bindings/BagDef';

describe('honest display formatting', () => {
  it.each([null, NaN, Infinity, -Infinity])('renders %s as unavailable', (value) => {
    expect(formatStat(value)).toBe('—');
  });
  it('keeps supplied zero and meaningful signed changes', () => {
    expect(formatStat(0)).toBe('0');
    expect(formatStat(-1234.5)).toBe('-1,234.5');
    expect(formatStat(987654321)).toBe('987,654,321');
  });
  it('subtracts microseconds before formatting even above safe integer range', () => {
    expect(formatElapsed('18446744073707551615', '18446744073709551615')).toBe('0:02');
    expect(formatElapsed('0', '65000000')).toBe('1:05');
    expect(formatElapsed('4', '4')).toBe('0:00');
    expect(formatElapsed('0', '999999')).toBe('0:00');
  });
  it.each([['2', '1'], ['-1', '0'], ['', '5'], ['0', 'NaN'], ['1.5', '2'], ['1e9', '2']])('rejects invalid interval %s %s', (start, end) => {
    expect(formatElapsed(start, end)).toBe('—');
  });
  it('formats supplied shares without calculating a denominator', () => {
    expect(formatShare(0)).toBe('0%');
    expect(formatShare(.312)).toBe('31.2%');
    expect(formatShare(.04, true)).toBe('4.0%');
    expect(formatShare(.039, true)).toBe('3.9%');
    expect(formatShare(0, true)).toBe('0.0%');
    for (const value of [null, NaN, Infinity, -1, 2]) expect(formatShare(value)).toBe('—');
  });
});

describe('bag presentation without invented ordering', () => {
  const bag: BagDef = { object_type: 12, base_type: 8, name: 'Synthetic bag', is_white: false, rank: null, capture: true };
  it('keeps unknown categories unknown, independent of name and rank', () => {
    expect(bagTone(null)).toBe('unknown');
    expect(bagTone({ ...bag, name: 'White bag', rank: 999 })).toBe('unknown');
  });
  it('uses reviewed base categories for aliases and the authoritative white flag', () => {
    const reviewed = new Map([[8, 'blue' as const]]);
    expect(bagTone(bag, reviewed)).toBe('blue');
    expect(bagTone({ ...bag, object_type: 999 }, reviewed)).toBe('blue');
    expect(bagTone({ ...bag, is_white: true }, reviewed)).toBe('white');
    expect(bag.rank).toBeNull();
  });
});


describe('whole-number DPS', () => {
  it.each([[1448.7, '1,449'], [1609, '1,609'], [1.5, '2'], [0.5, '1'],
    [1448.5, '1,449'], [999999999.5, '1,000,000,000'], [0, '0'], [-0, '0']] as const)('rounds %s to %s', (value, expected) => {
    expect(formatDps(value)).toBe(expected);
  });
  it.each([null, undefined, NaN, Infinity, -Infinity])('keeps unavailable DPS %s as a dash', value => {
    expect(formatDps(value)).toBe('—');
  });
});

describe('clock ranges', () => {
  const at = (clock: string) => String(BigInt(Date.parse(`2026-10-02T${clock}Z`)) * 1000n);
  const text = (value: string) => value.replace(/\s+/g, ' ');
  it('uses numeric hours and one shared suffix in a 12-hour locale', () => {
    expect(text(formatClockRange(at('19:35:00'), at('19:37:00'), 'en-US', { timeZone: 'UTC' })))
      .toBe('7:35:00 – 7:37:00 PM');
  });
  it('keeps both periods when crossing noon or midnight', () => {
    expect(text(formatClockRange(at('11:59:00'), at('12:01:00'), 'en-US', { timeZone: 'UTC' })))
      .toBe('11:59:00 AM – 12:01:00 PM');
    const nextDay = String(BigInt(at('00:01:00')) + 86_400_000_000n);
    expect(text(formatClockRange(at('23:59:00'), nextDay, 'en-US', { timeZone: 'UTC' })))
      .toBe('11:59:00 PM – 12:01:00 AM');
  });
  it('preserves 24-hour clocks and prefix day periods in other locales', () => {
    expect(text(formatClockRange(at('19:35:00'), at('19:37:00'), 'en-GB', { timeZone: 'UTC' })))
      .toBe('19:35:00 – 19:37:00');
    const options = { hour: 'numeric', minute: '2-digit', second: '2-digit', timeZone: 'UTC', hourCycle: 'h12' } as const;
    const formatter = new Intl.DateTimeFormat('zh-TW', options);
    const start = formatter.formatToParts(new Date('2026-10-02T19:35:00Z'));
    const end = formatter.formatToParts(new Date('2026-10-02T19:37:00Z'));
    const expected = `${start.map(part => part.value).join('')} – ${end.filter(part => part.type !== 'dayPeriod').map(part => part.value).join('').trim()}`;
    expect(text(formatClockRange(at('19:35:00'), at('19:37:00'), 'zh-TW', options))).toBe(text(expected));
  });
  it('preserves unknown endpoints', () => {
    expect(text(formatClockRange(at('19:35:00'), null, 'en-US', { timeZone: 'UTC' }))).toBe('7:35:00 PM – —');
    expect(formatClockRange('invalid', '99999999999999999999999999', 'en-US', { timeZone: 'UTC' })).toBe('— – —');
  });
});

// Decimal units match the existing 1,000,000,000-byte recording quota.
describe('storage size', () => {
  it.each([[0, '0 B'], [999, '999 B'], [1000, '1 KB'], [853735820, '854 MB'], [1000000000, '1 GB'],
    [null, 'Unavailable'], [NaN, 'Unavailable'], [-1, 'Unavailable']] as const)('formats %s as %s', async (input, expected) => {
    const { formatBytes } = await import('../../src/lib/styles/format');
    expect(formatBytes(input)).toBe(expected);
  });
});
