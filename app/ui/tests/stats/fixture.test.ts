import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import FixtureApp from '../shell/fixtures/FixtureApp.svelte';
import { fakeHistory } from '../navigation/fake-history';

afterEach(cleanup);
it.each([
  ['#/stats', 'Lost Halls'],
  ['#/stats/sessions', 'In progress'],
  ['#/stats/sessions/preview-session-1', 'Unrecorded'],
  ['#/stats/dungeons', 'Pirate Cave'],
])('the preview renders %s', async (hash, text) => {
  const browser = fakeHistory(hash);
  render(FixtureApp, { historyAdapter: browser.adapter });
  await waitFor(() => expect(screen.getAllByText(text).length).toBeGreaterThan(0));
});
