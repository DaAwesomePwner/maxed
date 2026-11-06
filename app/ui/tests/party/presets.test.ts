import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import Party from '../../src/routes/Party.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { PartySettings } from '../../src/lib/bindings/PartySettings';
import type { RequirementPreset } from '../../src/lib/bindings/RequirementPreset';
import { presetCopyText } from '../../src/lib/party/party-screen';
import { homeSnapshot } from '../home/fixtures';
import { deferred, flush } from '../runs/fixtures';

const preset = (id = 'synth-a', name = 'Synthetic A'): RequirementPreset => ({ id, name, min_tier: {}, ut_st_meets: true, auto_apply: [] });
async function mount(party: PartySettings = { presets: [preset()] }) {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  const settings = await client.querySettings();
  vi.spyOn(client, 'querySettings').mockResolvedValue({ ...settings, party });
  const save = vi.spyOn(client, 'updatePartySettings').mockResolvedValue(undefined);
  const generalSave = vi.spyOn(client, 'updateSettings');
  vi.spyOn(client, 'listCatalogDungeonOptions').mockResolvedValue([
    { name: 'Synthetic Sanctum', aliases: ['synth alias'], portal_type: null, kind: 'dungeon', completion_index: null },
    { name: 'Synthetic Cavern', aliases: [], portal_type: null, kind: 'dungeon', completion_index: null },
  ]);
  render(Party, { client, route: { kind: 'party-presets' } }); await flush();
  return { client, save, generalSave };
}
async function edit(name = 'Synthetic A') { await fireEvent.click(screen.getByRole('button', { name: `Edit ${name}` })); await flush(); }
const originalClipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks();
  if (originalClipboard) Object.defineProperty(navigator, 'clipboard', originalClipboard); else Reflect.deleteProperty(navigator, 'clipboard');
});

it('creates a UUID preset with default UT/ST, maxed, tier fields and the dedicated save', async () => {
  const id = '00000000-0000-4000-8000-000000000012'; vi.spyOn(crypto, 'randomUUID').mockReturnValue(id);
  const page = await mount({ presets: [] }); await fireEvent.click(screen.getByRole('button', { name: 'New preset' }));
  expect((screen.getByRole('checkbox', { name: 'UT / ST counts as meeting a tier' }) as HTMLInputElement).checked).toBe(true);
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Synthetic New' } });
  await fireEvent.change(screen.getByRole('combobox', { name: 'Maxed stats' }), { target: { value: '6' } });
  for (const [slot, tier] of [['Weapon', '11'], ['Ability', '5'], ['Armor', '12'], ['Ring', '4']]) {
    await fireEvent.change(screen.getByRole('combobox', { name: slot }), { target: { value: tier } });
  }
  await fireEvent.click(screen.getByRole('button', { name: 'Save' })); await flush();
  expect(page.save).toHaveBeenCalledWith({ presets: [{ id, name: 'Synthetic New', min_maxed: 6,
    min_tier: { weapon: 11, ability: 5, armor: 12, ring: 4 }, ut_st_meets: true, auto_apply: [] }], default_preset_id: null });
  expect(page.generalSave).not.toHaveBeenCalled();
  expect(screen.getByRole('table', { name: 'Requirement presets' }).textContent).toContain('Synthetic New');
});

it('edits an existing preset, clears a tier to any and toggles UT/ST without changing its id', async () => {
  const page = await mount({ presets: [{ ...preset(), min_maxed: 6, min_tier: { weapon: 11 } }] }); await edit();
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Synthetic Edited' } });
  await fireEvent.change(screen.getByRole('combobox', { name: 'Maxed stats' }), { target: { value: '' } });
  await fireEvent.change(screen.getByRole('combobox', { name: 'Weapon' }), { target: { value: '' } });
  await fireEvent.click(screen.getByRole('checkbox', { name: 'UT / ST counts as meeting a tier' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Save' })); await flush();
  expect(page.save.mock.calls[0][0].presets[0]).toMatchObject({ id: 'synth-a', name: 'Synthetic Edited', min_maxed: null, min_tier: { weapon: null }, ut_st_meets: false });
  expect(page.generalSave).not.toHaveBeenCalled();
});

it('searches dungeon aliases, moves auto-apply from another preset with a note and removes chips', async () => {
  const page = await mount({ presets: [{ ...preset(), auto_apply: ['Synthetic Sanctum'] }, preset('synth-b', 'Synthetic B')] }); await edit('Synthetic B');
  await fireEvent.input(screen.getByRole('searchbox', { name: 'Search dungeons' }), { target: { value: 'synth alias' } });
  expect(screen.queryByRole('checkbox', { name: 'Synthetic Cavern' })).toBeNull();
  await fireEvent.click(screen.getByRole('checkbox', { name: 'Synthetic Sanctum' }));
  expect(screen.getByText('Moved from Synthetic A')).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Remove Synthetic Sanctum' })).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Save' })); await flush();
  expect(page.save.mock.calls[0][0].presets.map(p => p.auto_apply)).toEqual([[], ['Synthetic Sanctum']]);
  await fireEvent.click(screen.getByRole('button', { name: 'Remove Synthetic Sanctum' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Save' })); await flush();
  expect(page.save.mock.calls[1][0].presets.map(p => p.auto_apply)).toEqual([[], []]);
});

it('sets the single default to another preset and can turn it off', async () => {
  const page = await mount({ presets: [preset(), preset('synth-b', 'Synthetic B')], default_preset_id: 'synth-a' }); await edit('Synthetic B');
  await fireEvent.click(screen.getByRole('checkbox', { name: 'Default' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Save' })); await flush();
  expect(page.save.mock.calls[0][0].default_preset_id).toBe('synth-b');
  await fireEvent.click(screen.getByRole('checkbox', { name: 'Default' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Save' })); await flush();
  expect(page.save.mock.calls[1][0].default_preset_id).toBeNull();
});

it('shows native validation errors verbatim and preserves the editor for correction', async () => {
  const page = await mount(); const message = 'Preset names must be unique'; page.save.mockRejectedValueOnce(message); await edit();
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Synthetic Duplicate' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save' })); await flush();
  expect(screen.getByRole('alert').textContent).toBe(message);
  expect((screen.getByRole('textbox', { name: 'Name' }) as HTMLInputElement).value).toBe('Synthetic Duplicate');
  expect(screen.getByRole('table', { name: 'Requirement presets' }).textContent).toContain('Synthetic A');
  await fireEvent.click(screen.getByRole('button', { name: 'Save' })); await flush(); expect(screen.queryByRole('alert')).toBeNull();
  expect(page.generalSave).not.toHaveBeenCalled();
});

it('copies the current draft using presetCopyText with confirmation', async () => {
  const writeText = vi.fn(async () => {}); Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  await mount(); await edit();
  await fireEvent.change(screen.getByRole('combobox', { name: 'Maxed stats' }), { target: { value: '8' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Copy as text' })); await flush();
  expect(writeText).toHaveBeenCalledWith(presetCopyText({ ...preset(), min_maxed: 8 })); expect(screen.getByText('Copied')).toBeTruthy();
});

it('requires delete confirmation and removes the default through the dedicated save', async () => {
  const page = await mount({ presets: [preset()], default_preset_id: 'synth-a' }); await edit();
  await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
  await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Cancel' })); expect(page.save).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
  await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Confirm' })); await flush();
  expect(page.save).toHaveBeenCalledWith({ presets: [], default_preset_id: null }); expect(page.generalSave).not.toHaveBeenCalled();
  expect(screen.queryByRole('textbox', { name: 'Name' })).toBeNull();
});

it('disables New preset at 32 presets but permits editing', async () => {
  await mount({ presets: Array.from({ length: 32 }, (_, i) => preset(`synth-${i}`, `Synthetic ${i}`)) });
  expect((screen.getByRole('button', { name: 'New preset' }) as HTMLButtonElement).disabled).toBe(true);
  await edit('Synthetic 0'); expect(screen.getByRole('button', { name: 'Save' })).toBeTruthy();
});

it('cancels an auto-apply move without changing either saved preset', async () => {
  const party = { presets: [{ ...preset(), auto_apply: ['Synthetic Sanctum'] }, preset('synth-b', 'Synthetic B')] };
  const page = await mount(party); await edit('Synthetic B');
  await fireEvent.click(screen.getByRole('checkbox', { name: 'Synthetic Sanctum' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  await fireEvent.click(within(screen.getByRole('dialog', { name: 'Discard unsaved changes?' })).getByRole('button', { name: 'Confirm' }));
  expect(page.save).not.toHaveBeenCalled(); expect(party.presets.map(p => p.auto_apply)).toEqual([['Synthetic Sanctum'], []]);
  await edit('Synthetic A'); expect(screen.getByRole('button', { name: 'Remove Synthetic Sanctum' })).toBeTruthy();
});

it('preserves saved dungeon chips when the catalog fails and displays Error messages verbatim', async () => {
  const page = await mount({ presets: [{ ...preset(), auto_apply: ['Synthetic old dungeon'] }] });
  vi.mocked(page.client.listCatalogDungeonOptions).mockRejectedValueOnce(new Error('synthetic catalog failure'));
  await edit(); expect(screen.getByRole('button', { name: 'Remove Synthetic old dungeon' })).toBeTruthy();
  expect(screen.getByText('Dungeon catalog unavailable. Check assets in Settings.')).toBeTruthy();
  const message = 'Preset names must be 1 to 40 characters'; page.save.mockRejectedValueOnce(new Error(message));
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: '' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Save' })); await flush(); expect(screen.getByRole('alert').textContent).toBe(message);
});


it('offers only Any and T1 through T20 for every equipment slot', async () => {
  await mount(); await edit();
  for (const slot of ['Weapon', 'Ability', 'Armor', 'Ring']) {
    const select = screen.getByRole('combobox', { name: slot }) as HTMLSelectElement;
    expect([...select.options].map(option => [option.value, option.text])).toEqual([
      ['', 'Any'], ...Array.from({ length: 20 }, (_, i) => [String(i + 1), `T${i + 1}`]),
    ]);
  }
});

it.each([0, 21, 255])('displays and preserves a saved out-of-range tier %s', async tier => {
  const page = await mount({ presets: [{ ...preset(), min_tier: { weapon: tier, ability: tier, armor: tier, ring: tier } }] });
  await edit();
  for (const slot of ['Weapon', 'Ability', 'Armor', 'Ring']) {
    const select = screen.getByRole('combobox', { name: slot }) as HTMLSelectElement;
    expect(select.value).toBe(String(tier));
    expect(select.selectedOptions[0].textContent).toBe(`T${tier}`);
  }
  await fireEvent.click(screen.getByRole('button', { name: 'Save' })); await flush();
  expect(page.save.mock.calls[0][0].presets[0].min_tier).toEqual({ weapon: tier, ability: tier, armor: tier, ring: tier });
});

it('confirms Cancel with unsaved edits and retains the draft when discard is cancelled', async () => {
  const page = await mount(); await edit();
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Synthetic Draft' } });
  screen.getByRole('button', { name: 'Cancel' }).focus();
  await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  await fireEvent.click(within(screen.getByRole('dialog', { name: 'Discard unsaved changes?' })).getByRole('button', { name: 'Cancel' }));
  expect((screen.getByRole('textbox', { name: 'Name' }) as HTMLInputElement).value).toBe('Synthetic Draft');
  await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  await fireEvent.click(within(screen.getByRole('dialog', { name: 'Discard unsaved changes?' })).getByRole('button', { name: 'Confirm' }));
  expect(screen.queryByRole('region', { name: 'Preset editor' })).toBeNull();
  expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Edit Synthetic A' }));
  expect(page.save).not.toHaveBeenCalled();
  await edit();
  expect((screen.getByRole('textbox', { name: 'Name' }) as HTMLInputElement).value).toBe('Synthetic A');
});

it('cancels a clean preset without a discard prompt', async () => {
  await mount(); await edit();
  screen.getByRole('button', { name: 'Cancel' }).focus();
  await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(screen.queryByRole('region', { name: 'Preset editor' })).toBeNull();
  expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Edit Synthetic A' }));
});

it.each([false, true])('returns focus to New preset after cancelling a new draft (dirty=%s)', async dirty => {
  await mount({ presets: [] });
  const trigger = screen.getByRole('button', { name: 'New preset' });
  await fireEvent.click(trigger);
  if (dirty) await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Synthetic Draft' } });
  const cancel = screen.getByRole('button', { name: 'Cancel' }); cancel.focus();
  await fireEvent.click(cancel);
  if (dirty) await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Confirm' }));
  expect(screen.queryByRole('region', { name: 'Preset editor' })).toBeNull();
  expect(document.activeElement).toBe(trigger);
});

it.each(['Edit Synthetic B', 'New preset'])('confirms unsaved changes before %s', async action => {
  const page = await mount({ presets: [preset(), preset('synth-b', 'Synthetic B')] }); await edit();
  await fireEvent.input(screen.getByRole('textbox', { name: 'Name' }), { target: { value: 'Synthetic Draft' } });
  await fireEvent.click(screen.getByRole('button', { name: action }));
  let dialog = screen.getByRole('dialog', { name: 'Discard unsaved changes?' });
  await fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
  expect((screen.getByRole('textbox', { name: 'Name' }) as HTMLInputElement).value).toBe('Synthetic Draft');
  await fireEvent.click(screen.getByRole('button', { name: action }));
  dialog = screen.getByRole('dialog', { name: 'Discard unsaved changes?' });
  await fireEvent.click(within(dialog).getByRole('button', { name: 'Confirm' }));
  expect((screen.getByRole('textbox', { name: 'Name' }) as HTMLInputElement).value).toBe(action === 'New preset' ? '' : 'Synthetic B');
  expect(page.save).not.toHaveBeenCalled();
});

it('does not prompt after reverting or saving the draft', async () => {
  await mount({ presets: [preset(), preset('synth-b', 'Synthetic B')] }); await edit();
  await fireEvent.change(screen.getByRole('combobox', { name: 'Weapon' }), { target: { value: '20' } });
  await fireEvent.change(screen.getByRole('combobox', { name: 'Weapon' }), { target: { value: '' } });
  await edit('Synthetic B'); expect(screen.queryByRole('dialog')).toBeNull();
  await fireEvent.click(screen.getByRole('checkbox', { name: 'Default' }));
  await fireEvent.click(screen.getByRole('button', { name: 'Save' })); await flush();
  await edit(); expect(screen.queryByRole('dialog')).toBeNull();
});

it('syncs delete state on Escape and native close and preserves late errors', async () => {
  const page = await mount(); await edit();
  const trigger = screen.getByRole('button', { name: 'Delete' }); trigger.focus();
  await fireEvent.click(trigger);
  await fireEvent(screen.getByRole('dialog'), new Event('cancel', { cancelable: true }));
  expect(screen.queryByRole('dialog')).toBeNull(); expect(document.activeElement).toBe(trigger);
  const pending = deferred<void>(); page.save.mockImplementationOnce(() => pending.promise);
  await fireEvent.click(trigger);
  await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Confirm' }));
  await fireEvent(screen.getByRole('dialog'), new Event('cancel', { cancelable: true }));
  expect(screen.getByRole('dialog')).toBeTruthy();
  await fireEvent(screen.getByRole('dialog'), new Event('close'));
  expect(screen.queryByRole('dialog')).toBeNull();
  pending.reject(new Error('Synthetic delete failure')); await flush();
  expect(screen.getByRole('alert').textContent).toBe('Synthetic delete failure');
  expect(document.activeElement).toBe(trigger);
  await fireEvent.click(trigger); expect(screen.getByRole('dialog')).toBeTruthy();
});
