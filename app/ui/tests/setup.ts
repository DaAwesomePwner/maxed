import { afterEach, vi } from 'vitest';
import { cleanup } from '@testing-library/svelte';

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.resetAllMocks();
});
