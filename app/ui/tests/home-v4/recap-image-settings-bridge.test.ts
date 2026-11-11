import { expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import snapshot from '../fixtures/t0-live';

it('recap_image_patch_forwards_only_the_section_and_round_trips_in_preview', async () => {
  const transport = createFixtureTransport(structuredClone(snapshot));
  const invoke = vi.spyOn(transport, 'invoke');
  const client = createClient(transport);
  const before = await client.querySettings();
  const next = { names: false, loot: true, fights: false };
  await expect(client.updateRecapImageSettings(next)).resolves.toBeUndefined();
  expect(invoke).toHaveBeenCalledWith('update_recap_image_settings', { settings: next });
  expect(invoke).not.toHaveBeenCalledWith('update_settings', expect.anything());
  next.names = true;
  const after = await client.querySettings();
  expect(after).toEqual({ ...before, recap_image: { names: false, loot: true, fights: false } });
  after.recap_image.loot = false;
  expect((await client.querySettings()).recap_image).toEqual({ names: false, loot: true, fights: false });
  expect((await createClient(createFixtureTransport(structuredClone(snapshot))).querySettings()).recap_image)
    .toEqual({ names: true, loot: true, fights: true });
});
