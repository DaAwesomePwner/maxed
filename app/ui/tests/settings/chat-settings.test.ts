import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import ChatSettingsSection from '../../src/lib/logs/ChatSettingsSection.svelte';
import type { ChatSettings } from '../../src/lib/bindings/ChatSettings';

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
const saved = (): ChatSettings => ({ store: { public: true, guild: true, party: true, whisper: true }, retention_days: 90, hidden_senders: ['SynthAlpha'], hidden_keywords: [] });

it('saves channel toggles and retention', async () => {
  const update = vi.fn().mockResolvedValue(undefined);
  render(ChatSettingsSection, { settings: saved(), update, clear: vi.fn(), editable: true });
  await fireEvent.click(screen.getByLabelText('Store Guild chat'));
  await fireEvent.input(screen.getByLabelText('Keep chat for (days)'), { target: { value: '30' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save chat settings' }));
  await waitFor(() => expect(update).toHaveBeenCalledWith(expect.objectContaining({ retention_days: 30, store: expect.objectContaining({ guild: false }) })));
});
it('rejects retention outside 1-3650', async () => {
  const update = vi.fn();
  render(ChatSettingsSection, { settings: saved(), update, clear: vi.fn(), editable: true });
  await fireEvent.input(screen.getByLabelText('Keep chat for (days)'), { target: { value: '0' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save chat settings' }));
  expect(update).not.toHaveBeenCalled();
  expect(screen.getByText('Choose 1 to 3650 days.')).toBeTruthy();
});
it('removes a hidden sender', async () => {
  const update = vi.fn().mockResolvedValue(undefined);
  render(ChatSettingsSection, { settings: saved(), update, clear: vi.fn(), editable: true });
  await fireEvent.click(screen.getByRole('button', { name: 'Unhide SynthAlpha' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Save chat settings' }));
  await waitFor(() => expect(update).toHaveBeenCalledWith(expect.objectContaining({ hidden_senders: [] })));
});
it('asks before clearing chat history', async () => {
  const clear = vi.fn().mockResolvedValue(undefined);
  vi.spyOn(window, 'confirm').mockReturnValueOnce(false).mockReturnValueOnce(true);
  render(ChatSettingsSection, { settings: saved(), update: vi.fn(), clear, editable: true });
  await fireEvent.click(screen.getByRole('button', { name: 'Clear chat history' }));
  expect(clear).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: 'Clear chat history' }));
  await waitFor(() => expect(clear).toHaveBeenCalled());
});

it('adds and removes hidden keywords without duplicates', async () => {
  const update = vi.fn().mockResolvedValue(undefined);
  render(ChatSettingsSection, { settings: saved(), update, clear: vi.fn(), editable: true });
  for (let i = 0; i < 2; i++) {
    await fireEvent.input(screen.getByLabelText('Add keyword'), { target: { value: ' SynthBravo ' } });
    await fireEvent.submit(screen.getByRole('button', { name: 'Add' }).closest('form')!);
  }
  await fireEvent.click(screen.getByRole('button', { name: 'Save chat settings' }));
  await waitFor(() => expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ hidden_keywords: ['SynthBravo'] })));
  await fireEvent.click(screen.getByRole('button', { name: 'Remove SynthBravo' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Save chat settings' }));
  await waitFor(() => expect(update).toHaveBeenLastCalledWith(expect.objectContaining({ hidden_keywords: [] })));
});

it('disables preview edits and clearing', async () => {
  const update = vi.fn(), clear = vi.fn();
  const confirm = vi.spyOn(window, 'confirm');
  render(ChatSettingsSection, { settings: saved(), update, clear, editable: false });
  expect(screen.getByLabelText('Store Guild chat').matches(':disabled')).toBe(true);
  await fireEvent.click(screen.getByRole('button', { name: 'Save chat settings' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Clear chat history' }));
  expect(update).not.toHaveBeenCalled();
  expect(clear).not.toHaveBeenCalled();
  expect(confirm).not.toHaveBeenCalled();
});

it.each(['save', 'clear'])('shows a safe retry message after %s fails', async (action) => {
  vi.spyOn(window, 'confirm').mockReturnValue(true);
  const fail = vi.fn().mockRejectedValue(new Error('PRIVATE'));
  render(ChatSettingsSection, { settings: saved(), update: fail, clear: fail, editable: true });
  await fireEvent.click(screen.getByRole('button', { name: action === 'save' ? 'Save chat settings' : 'Clear chat history' }));
  await screen.findByText(action === 'save' ? "Couldn't save chat settings. Retry." : "Couldn't clear chat history. Retry.");
  expect(document.body.textContent).not.toContain('PRIVATE');
});


it.each(['😀'.repeat(17), 'a'.repeat(65), 'bad\u0000word', 'bad\u007fword', 'bad\u0085word', '   '])('refuses invalid keyword bytes or controls', async (keyword) => {
  const update = vi.fn().mockResolvedValue(undefined);
  render(ChatSettingsSection, { settings: saved(), update, clear: vi.fn(), editable: true });
  await fireEvent.input(screen.getByLabelText('Add keyword'), { target: { value: keyword } });
  await fireEvent.submit(screen.getByRole('button', { name: 'Add' }).closest('form')!);
  expect(screen.getByRole('status').textContent).toBe('Use 1–64 UTF-8 bytes with no control characters.');
  await fireEvent.click(screen.getByRole('button', { name: 'Save chat settings' }));
  await waitFor(() => expect(update).toHaveBeenCalledWith(expect.objectContaining({ hidden_keywords: [] })));
});
it('accepts a keyword at the UTF-8 byte boundary', async () => {
  const update = vi.fn().mockResolvedValue(undefined);
  render(ChatSettingsSection, { settings: saved(), update, clear: vi.fn(), editable: true });
  const keyword = '😀'.repeat(16);
  await fireEvent.input(screen.getByLabelText('Add keyword'), { target: { value: keyword } });
  await fireEvent.submit(screen.getByRole('button', { name: 'Add' }).closest('form')!);
  await fireEvent.click(screen.getByRole('button', { name: 'Save chat settings' }));
  await waitFor(() => expect(update).toHaveBeenCalledWith(expect.objectContaining({ hidden_keywords: [keyword] })));
});
