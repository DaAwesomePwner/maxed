import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import Card from './fixtures/Card.svelte';

describe('Svelte browser test harness', () => {
  it('renders an accessible Card and handles keyboard interaction', async () => {
    const user = userEvent.setup();
    render(Card, { title: 'Synthetic session' });
    expect(screen.getByRole('region', { name: 'Synthetic session' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: 'Synthetic session', level: 2 })).toBeTruthy();
    await user.tab();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Show details' }));
    await user.keyboard('{Enter}');
    expect(screen.getByRole('status').textContent).toBe('Synthetic details');
  });
});
