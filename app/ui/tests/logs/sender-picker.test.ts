import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import SenderPicker from '../../src/lib/logs/SenderPicker.svelte';
afterEach(cleanup);
it('opens suggestions lazily and supports keyboard selection, exact names and clearing', async () => {
  const onOpen = vi.fn(), onChange = vi.fn();
  render(SenderPicker, { value: null, rows: [], partners: [{ name: 'Vexlin', lines: 3, last_at: '0', last_text: 'hello', last_direction: 'incoming', stars: 72 }], onOpen, onChange });
  expect(onOpen).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: /Any sender/ }));
  expect(onOpen).toHaveBeenCalledTimes(1);
  const input = screen.getByRole('combobox');
  await fireEvent.keyDown(input, { key: 'ArrowDown' }); await fireEvent.keyDown(input, { key: 'Enter' });
  expect(onChange).toHaveBeenLastCalledWith('Vexlin');
  await fireEvent.click(screen.getByRole('button', { name: /Any sender/ }));
  await fireEvent.input(screen.getByRole('combobox'), { target: { value: 'Quenby' } }); await fireEvent.keyDown(screen.getByRole('combobox'), { key: 'Enter' });
  expect(onChange).toHaveBeenLastCalledWith('Quenby');
  await fireEvent.click(screen.getByRole('button', { name: /Any sender/ }));
  await fireEvent.click(screen.getAllByRole('button', { name: 'Any sender' })[1]);
  expect(onChange).toHaveBeenLastCalledWith(null);
});
it('closes on Escape without changing the filter', async () => {
  const onChange = vi.fn(); render(SenderPicker, { value: 'Morrow', rows: [], partners: [], onOpen: vi.fn(), onChange });
  await fireEvent.click(screen.getByRole('button')); await fireEvent.keyDown(screen.getByRole('combobox'), { key: 'Escape' });
  expect(screen.queryByRole('combobox')).toBeNull(); expect(onChange).not.toHaveBeenCalled();
});
