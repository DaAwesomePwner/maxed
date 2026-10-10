import { render, screen, within } from '@testing-library/svelte';
import { expect, it } from 'vitest';
import Breadcrumbs from '../../src/lib/runs/Breadcrumbs.svelte';

it('links every crumb except the current page, even when it has an href', () => {
  render(Breadcrumbs, { items: [
    { label: 'Runs', href: '#/runs' },
    { label: 'The Shatters · Oct 2', href: '#/runs/r1' },
    { label: 'Forgotten King', href: '#/runs/r1/fights/f1' },
  ] });
  const nav = screen.getByRole('navigation', { name: 'Breadcrumb' });
  expect(within(nav).getByRole('list')).toBeTruthy();
  expect(within(nav).getAllByRole('link').map(link => link.getAttribute('href'))).toEqual(['#/runs', '#/runs/r1']);
  expect(screen.getByText('Forgotten King').closest('[aria-current="page"]')).not.toBeNull();
  expect(screen.getByText('Forgotten King').closest('a')).toBeNull();
});
