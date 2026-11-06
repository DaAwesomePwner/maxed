import { render, screen, waitFor } from '@testing-library/svelte';
import { expect, it, vi } from 'vitest';
import Settings from '../../src/routes/Settings.svelte';
import Setup from '../../src/routes/Setup.svelte';
import { nativeTransport } from '../../src/lib/bridge/native';
import { createStoragePreview, storageVariants, upgradeVariants } from './storage-scenario';

it.each(storageVariants)('mounts the synthetic storage %s scenario without native transport or mutation', async variant => {
  const f = createStoragePreview('storage', variant);
  const native = vi.spyOn(nativeTransport, 'invoke');
  const compact = vi.spyOn(f.client, 'compactStorage');
  render(Settings, { client: f.client });
  await waitFor(() => expect(screen.getByText(/Backup location:/).textContent).toContain('Synthetic'));
  expect(screen.getByText('Reusable pages')).toBeTruthy();
  expect(native).not.toHaveBeenCalled(); expect(compact).not.toHaveBeenCalled();
});
it.each(upgradeVariants)('mounts the synthetic upgrade %s scenario without native transport or mutation', async variant => {
  const f = createStoragePreview('upgrade', variant);
  const native = vi.spyOn(nativeTransport, 'invoke');
  const upgrade = vi.spyOn(f.client, 'backupAndUpgradeHistory');
  const query = vi.spyOn(f.client, 'queryHistoryUpgradeStatus');
  render(Setup, { client: f.client });
  await waitFor(() => expect(query).toHaveBeenCalled());
  if (variant !== 'ready') await screen.findByRole('heading', { name: 'History upgrade required' });
  else await waitFor(() => expect(screen.queryByRole('heading', { name: 'Checking history upgrade' })).toBeNull());
  expect(native).not.toHaveBeenCalled(); expect(upgrade).not.toHaveBeenCalled();
});
