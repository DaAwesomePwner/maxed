import { render, screen } from '@testing-library/svelte';
import { expect, it } from 'vitest';
import ComponentPreview from './fixtures/ComponentPreview.svelte';

it('renders the development-only variant gallery without a production route', () => {
  const { container } = render(ComponentPreview);
  expect(screen.getByRole('heading', { name: 'Arcane Modern · component preview' })).toBeTruthy();
  expect(container.querySelectorAll('[data-bag-tone]')).toHaveLength(9);
  expect(container.querySelectorAll('.banner')).toHaveLength(4);
  expect(screen.getByRole('button', { name: 'Capture unavailable' }).hasAttribute('disabled')).toBe(true);
  expect(screen.getByRole('img', { name: 'Unknown class' }).dataset.fallback).toBe('true');
});
