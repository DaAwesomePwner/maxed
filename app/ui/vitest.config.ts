import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';

// Day-grouping and date-format tests assert Chicago calendar days across
// midnight and DST. Pin the zone before workers start so UTC CI runners agree.
process.env.TZ = 'America/Chicago';

export default defineConfig({
  plugins: [svelte(), svelteTesting()],
  test: {
    environment: 'jsdom',
    setupFiles: ['./tests/setup.ts'],
    include: ['tests/**/*.test.ts'],
    restoreMocks: true,
    // Several jsdom tests take 2-3s alone and exceed Vitest's 5s default when
    // the full suite saturates every core; allow for that load suite-wide.
    testTimeout: 20_000,
    hookTimeout: 20_000,
  },
});
