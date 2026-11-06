import { cleanup, fireEvent, render } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { copyPng } from '../../src/lib/home/v4/clipboard';
import * as images from '../../src/lib/home/v4/recap-image';
import RecapCopyButton from '../../src/lib/home/v4/RecapCopyButton.svelte';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { deferred, setupFixture } from '../setup/fixture';
import { context, runView } from './recap-image-fixtures';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
const settle = () => vi.advanceTimersByTimeAsync(0);
function clipboard() {
  const write = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal('ClipboardItem', class { constructor(public data: Record<string, Blob>) {} });
  vi.stubGlobal('navigator', { clipboard: { write } });
  return write;
}
function mount() {
  const f = setupFixture();
  const props = { view: runView(), settings: f.client, visuals: emptyHomeVisuals(), context };
  return { f, props, page: render(RecapCopyButton, { props }) };
}
it('copies_png_and_reports_copied', async () => {
  const write = clipboard(), blob = new Blob(['synthetic'], { type: 'image/png' });
  vi.spyOn(images, 'renderRecapImage').mockResolvedValue(blob);
  const { page } = mount(); await settle();
  await fireEvent.click(page.getByRole('button', { name: 'Copy recap image' })); await settle();
  expect(write.mock.calls[0][0][0].data['image/png']).toBe(blob);
  expect(page.getByRole('status').textContent).toBe('Copied');
  expect(page.getAllByText('Copied')).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(3999); expect(page.getByRole('status').textContent).toBe('Copied');
  await vi.advanceTimersByTimeAsync(1); expect(page.getByRole('status').textContent).toBe('');
});
it('rejected_write_reports_failure', async () => {
  clipboard().mockRejectedValue(new Error('Synthetic rejection'));
  vi.spyOn(images, 'renderRecapImage').mockResolvedValue(new Blob(['synthetic'], { type: 'image/png' }));
  const { page } = mount(); await settle();
  await fireEvent.click(page.getByRole('button', { name: 'Copy recap image' })); await settle();
  expect(page.getByRole('status').textContent).toBe("Couldn't copy the image");
  expect(page.queryByRole('alert')).toBeNull();
});
it('missing_clipboard_api_reports_failure', async () => {
  clipboard(); vi.stubGlobal('navigator', {});
  expect(await copyPng(new Blob())).toBe('failed');
  clipboard(); vi.stubGlobal('ClipboardItem', undefined);
  expect(await copyPng(new Blob())).toBe('failed');
});
it('missing_clipboard_api_shows_failure_status', async () => {
  const write = clipboard(); vi.stubGlobal('ClipboardItem', undefined);
  vi.spyOn(images, 'renderRecapImage').mockResolvedValue(new Blob(['synthetic'], { type: 'image/png' }));
  const { page } = mount(); await settle();
  await fireEvent.click(page.getByRole('button', { name: 'Copy recap image' })); await settle();
  expect(page.getByRole('status').textContent).toBe("Couldn't copy the image");
  expect(write).not.toHaveBeenCalled();
});
it('status_timer_cleared_on_unmount', async () => {
  const write = clipboard();
  vi.spyOn(images, 'renderRecapImage').mockResolvedValue(new Blob(['synthetic'], { type: 'image/png' }));
  const { page } = mount(); await settle();
  await fireEvent.click(page.getByRole('button', { name: 'Copy recap image' })); await settle();
  const status = page.getByRole('status'); expect(status.textContent).toBe('Copied');
  expect(vi.getTimerCount()).toBe(1);
  page.unmount(); expect(vi.getTimerCount()).toBe(0);
  await vi.advanceTimersByTimeAsync(4000);
  expect(status.textContent).toBe('Copied'); expect(page.container.innerHTML).toBe('');
  expect(write).toHaveBeenCalledOnce();
});
it('no_render_until_click', async () => {
  clipboard(); const model = vi.spyOn(images, 'recapImageModel'), renderImage = vi.spyOn(images, 'renderRecapImage').mockResolvedValue(new Blob());
  const { page, props } = mount(); await settle();
  await page.rerender({ ...props, view: { ...props.view, title: 'Another synthetic recap' } }); await settle();
  await fireEvent.click(page.getByRole('button', { name: 'Recap image options' })); await settle();
  expect(model).not.toHaveBeenCalled(); expect(renderImage).not.toHaveBeenCalled();
  await fireEvent.click(page.getByRole('button', { name: 'Copy recap image' })); await settle();
  expect(model).toHaveBeenCalledOnce(); expect(renderImage).toHaveBeenCalledOnce();
});
it('toggles_persist_through_settings', async () => {
  const { page, f } = mount(); await settle();
  expect(f.client.querySettings).toHaveBeenCalledOnce();
  await fireEvent.click(page.getByRole('button', { name: 'Recap image options' }));
  f.setSettings({ hardware_acceleration: true, recap_image: { names: true, loot: false, fights: true } });
  await fireEvent.click(page.getByRole('checkbox', { name: 'Names' })); await settle();
  expect(f.client.querySettings).toHaveBeenCalledTimes(2);
  expect(f.client.updateSettings).toHaveBeenLastCalledWith(expect.objectContaining({ hardware_acceleration: true,
    recap_image: { names: false, loot: false, fights: true } }));
  expect((page.getByRole('checkbox', { name: 'Loot' }) as HTMLInputElement).checked).toBe(false);
});
it('failed_initial_read_defaults_true_and_failed_save_keeps_preferences', async () => {
  const f = setupFixture(); vi.mocked(f.client.querySettings).mockRejectedValue(new Error('Synthetic unavailable'));
  const page = render(RecapCopyButton, { props: { view: runView(), settings: f.client, visuals: emptyHomeVisuals(), context } });
  await settle(); await fireEvent.click(page.getByRole('button', { name: 'Recap image options' }));
  expect(page.getAllByRole('checkbox').every(input => (input as HTMLInputElement).checked)).toBe(true);
  await fireEvent.click(page.getByRole('checkbox', { name: 'Names' })); await settle();
  expect(f.client.updateSettings).not.toHaveBeenCalled();
  expect((page.getByRole('checkbox', { name: 'Names' }) as HTMLInputElement).checked).toBe(true);
});
it('render_failure_reports_failure_without_writing', async () => {
  const write = clipboard(); vi.spyOn(images, 'renderRecapImage').mockRejectedValue(new Error('Synthetic null blob'));
  const { page } = mount(); await settle();
  await fireEvent.click(page.getByRole('button', { name: 'Copy recap image' })); await settle();
  expect(write).not.toHaveBeenCalled(); expect(page.getByRole('status').textContent).toBe("Couldn't copy the image");
});

it('all_toggles_save_and_escape_returns_focus_without_rendering', async () => {
  const raster = vi.spyOn(images, 'renderRecapImage'), { f, page } = mount(); await settle();
  await fireEvent.click(page.getByRole('button', { name: 'Recap image options' }));
  for (const label of ['Names', 'Loot', 'Fights']) {
    await fireEvent.click(page.getByRole('checkbox', { name: label })); await settle();
  }
  expect((await f.client.querySettings()).recap_image).toEqual({ names: false, loot: false, fights: false });
  await fireEvent.keyDown(page.getByRole('checkbox', { name: 'Fights' }), { key: 'Escape' });
  expect(page.queryByRole('checkbox')).toBeNull();
  expect(document.activeElement).toBe(page.getByRole('button', { name: 'Recap image options' }));
  expect(raster).not.toHaveBeenCalled();
});

it('pending_copy_is_single_and_unmount_cancels_clipboard_output', async () => {
  const write = clipboard(), pending = deferred<Blob>();
  const raster = vi.spyOn(images, 'renderRecapImage').mockReturnValue(pending.promise);
  const { page } = mount(); await settle();
  const button = page.getByRole('button', { name: 'Copy recap image' });
  await fireEvent.click(button); await fireEvent.click(button);
  expect(raster).toHaveBeenCalledOnce(); expect(button.getAttribute('aria-disabled')).toBe('true');
  expect((button as HTMLButtonElement).disabled).toBe(false);
  page.unmount(); pending.resolve(new Blob()); await settle(); expect(write).not.toHaveBeenCalled();
});

it('copy_and_options_keep_focus_while_busy_and_ignore_repeated_activation', async () => {
  clipboard(); const rendering = deferred<Blob>();
  const raster = vi.spyOn(images, 'renderRecapImage').mockReturnValue(rendering.promise);
  const { page, f } = mount(); await settle();
  const copy = page.getByRole('button', { name: 'Copy recap image' }); copy.focus();
  await fireEvent.click(copy); await fireEvent.click(copy);
  expect(copy.hasAttribute('disabled')).toBe(false); expect(copy.getAttribute('aria-disabled')).toBe('true');
  expect(document.activeElement).toBe(copy); expect(raster).toHaveBeenCalledOnce();
  rendering.resolve(new Blob()); await settle();
  await fireEvent.click(page.getByRole('button', { name: 'Recap image options' }));
  const saving = deferred<void>(); vi.mocked(f.client.updateSettings).mockReturnValue(saving.promise);
  const names = page.getByRole('checkbox', { name: 'Names' }) as HTMLInputElement; names.focus();
  await fireEvent.click(names); await settle();
  expect(names.matches(':disabled')).toBe(false); expect(names.getAttribute('aria-disabled')).toBe('true');
  expect(document.activeElement).toBe(names);
  await fireEvent.click(names); await settle(); expect(f.client.updateSettings).toHaveBeenCalledOnce();
  expect(names.checked).toBe(true);
  saving.resolve(); await settle(); expect(names.checked).toBe(false);
});
