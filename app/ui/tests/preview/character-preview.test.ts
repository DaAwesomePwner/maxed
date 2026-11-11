import { readFileSync } from 'node:fs';
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { beforeEach, expect, it, vi } from 'vitest';
import CharacterPreview from './CharacterPreview.svelte';
import { characterVariants, createCharacterPreview } from './character-scenario';
import { flush } from '../runs/fixtures';

beforeEach(() => { vi.useFakeTimers(); window.history.replaceState(null, '', '/?preview=character'); });
async function settle() { await vi.advanceTimersByTimeAsync(250); await flush(); }

it.each(characterVariants)('mounts real Character and its visible exclusions: %s', async variant => {
  render(CharacterPreview, { variant }); await settle();
  expect(location.hash).toBe('#/character');
  const main = within(screen.getByRole('main'));
  expect(main.getAllByRole('heading', { level: 2 }).map(h => h.textContent)).toEqual([
    'Character', 'Stats', 'Weapon', 'Regeneration', 'Exalt damage',
  ]);
  expect(main.getByText(/Excludes exalt damage/).closest('details')).toBeNull();
  expect(main.getByText('Not available yet; pending verification')).toBeTruthy();
  if (variant === 'empty') expect(main.getByText('No character observed. Start capture and enter the game.')).toBeTruthy();
  else expect(main.getByText('SynthAster')).toBeTruthy();
  if (variant === 'partial') {
    expect(main.getByRole('region', { name: 'Character' }).textContent).toContain('Partial');
    expect(main.getByRole('region', { name: 'Stats' }).textContent).toContain('negative other');
    expect(main.getByRole('region', { name: 'Regeneration' }).textContent).toContain('maximum not observed');
    expect(main.getByRole('article').textContent).toContain('unsupported schedule');
  }
  if (variant === 'stale') expect(main.getByRole('region', { name: 'Character' }).textContent).toContain('Last seen');
  if (variant === 'unknown-weapon') expect(main.getByRole('article').textContent).toContain('444');
  if (variant === 'missing-catalog') expect(main.getByText('Weapon calculations unavailable — extract game assets')).toBeTruthy();
});

it.each(['#/character', '#/settings'])('preserves supplied hash %s', async hash => {
  window.history.replaceState(null, '', '/?preview=character' + hash);
  render(CharacterPreview); await settle(); expect(location.hash).toBe(hash);
});

it('uses authored check vectors, independent replies and unavailable unlisted defenses', async () => {
  const p = createCharacterPreview('normal');
  const expected_revision = p.snapshot.character!.calculation_revision!;
  const request = { expected_revision, defense: 50, comparison_item_types: [201, 202] };
  const reply = await p.client.getCharacterCalculation(request);
  expect(reply.weapons).toHaveLength(3);
  expect(reply.weapons[0]).toMatchObject({ applied_att: 75, applied_dex: 75,
    hit_mean: { value: 150 }, attacks_per_sec: { value: 8 }, dps_total: { value: 1200 } });
  reply.weapons[0].dps_total.value = -1;
  expect((await p.client.getCharacterCalculation(request)).weapons[0].dps_total.value).toBe(1200);
  expect((await p.client.getCharacterCalculation({ ...request, defense: 51 })).weapons[0].dps_total.value).toBeNull();
  await expect(p.client.startCapture()).rejects.toThrow();
  await expect(p.client.openRealmeye('SynthAster')).rejects.toThrow();
});

it('exercises applied defense errors, zero/two comparisons and stale transitions', async () => {
  const preview = createCharacterPreview('normal'); render(CharacterPreview, { preview }); await settle();
  const main = within(screen.getByRole('main'));
  expect(main.getAllByRole('article')).toHaveLength(1);
  const defense = main.getByRole('spinbutton', { name: 'Enemy defense' });
  await fireEvent.input(defense, { target: { value: '50' } }); await settle();
  expect(main.getByText('Applied defense: 50', { selector: 'p' })).toBeTruthy();
  expect(main.getByRole('article').textContent).toContain('1200.00');
  await fireEvent.input(defense, { target: { value: '-1' } }); await settle();
  expect(defense.getAttribute('aria-invalid')).toBe('true');
  expect(main.getByText('Applied defense: 50', { selector: 'p' })).toBeTruthy();
  await fireEvent.click(main.getByRole('button', { name: 'Reset' })); await settle();
  expect(main.getByText('Applied defense: 0', { selector: 'p' })).toBeTruthy();
  expect(defense.getAttribute('aria-invalid')).toBeNull();
  await fireEvent.input(main.getByRole('searchbox'), { target: { value: 'Synthetic' } }); await settle();
  for (const value of ['201', '202']) {
    await fireEvent.change(main.getByRole('combobox', { name: 'Catalog weapon' }), { target: { value } });
    await fireEvent.click(main.getByRole('button', { name: 'Add comparison' })); await settle();
  }
  expect(main.getAllByRole('article')).toHaveLength(3);
  preview.setStale(true); await settle();
  expect(main.getByRole('region', { name: 'Character' }).textContent).toContain('Last seen');
  preview.setStale(false); await settle();
  expect(main.getByRole('region', { name: 'Character' }).textContent).toMatch(/\bLive\b/);
  for (const type of [201, 202]) {
    await fireEvent.click(main.getByRole('button', { name: `Remove comparison ${type}` })); await settle();
  }
  expect(main.getAllByRole('article')).toHaveLength(1);
});

it('navigates Settings and Character with Shell Back/Forward', async () => {
  render(CharacterPreview); await settle();
  await fireEvent.click(within(screen.getByRole('complementary')).getByRole('link', { name: 'Settings' })); await settle();
  expect(location.hash).toBe('#/settings');
  await fireEvent.click(screen.getByRole('button', { name: 'Back' })); await settle();
  expect(location.hash).toBe('#/character');
  await fireEvent.click(screen.getByRole('button', { name: 'Forward' })); await settle();
  expect(location.hash).toBe('#/settings');
});

it('wires the preview through a compile-time DEV-only dynamic import', () => {
  const source = readFileSync('src/main.ts', 'utf8');
  expect(source).toMatch(/import\.meta\.env\.DEV && new URLSearchParams\(location.search\).get\('preview'\) === 'character'/);
  expect(source).toContain("await import('../tests/preview/CharacterPreview.svelte')");
  expect(createCharacterPreview('normal').snapshot.stream_id).toBe('CHARACTER_SYNTHETIC_PREVIEW_ONLY');
});
