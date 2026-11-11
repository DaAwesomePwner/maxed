import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import LiveStatusCard from '../../src/lib/shell/LiveStatusCard.svelte';
import { railView } from '../../src/lib/shell/rail';
import fixture from '../fixtures/t0-live';

describe('capture error recovery actions', () => {
  it.each(['Stopped', 'Live'] as const)('keeps a failed capture action visible without offering status reconnect (%s)', (state) => {
    render(LiveStatusCard, {
      view: railView(null), capture: { ...fixture.capture, state }, busy: false,
      error: 'Capture could not change state. Check the capture status for recovery guidance.',
      onStart: vi.fn(), onStop: vi.fn(), onRetry: vi.fn(),
    });
    expect(screen.getByRole('alert').textContent).toContain('Capture could not change state');
    expect(screen.queryByRole('button', { name: 'Reconnect capture' })).toBeNull();
    const retry = screen.getByRole('button', { name: state === 'Stopped' ? 'Start capture' : 'Stop capture' });
    expect(retry.getAttribute('aria-disabled')).toBe('false');
  });

  it('offers reconnect when the capture subscription failed', async () => {
    const onRetry = vi.fn();
    render(LiveStatusCard, {
      view: railView(null), capture: null, busy: false,
      error: 'Capture status is unavailable. Reconnect status to retry.',
      onStart: vi.fn(), onStop: vi.fn(), onRetry,
    });
    await userEvent.setup().click(screen.getByRole('button', { name: 'Reconnect capture' }));
    expect(onRetry).toHaveBeenCalledOnce();
  });
});
