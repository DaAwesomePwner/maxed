import { expect, it } from 'vitest';
import { liveDuration } from '../../src/lib/shell/live-duration';

it.each([[0, '0:00'], [59999, '0:59'], [3599999, '59:59'], [3600000, '1:00:00'],
  [3727000, '1:02:07'], [360000000, '100:00:00']])('formats %i ms as %s', (ms, expected) => {
  expect(liveDuration(ms)).toBe(expected);
});
