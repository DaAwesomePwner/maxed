/// <reference types="vite/client" />
import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';

const target = document.getElementById('app');
if (!target) throw new Error('Missing app root');
// Compile-time development guard keeps synthetic data out of production bundles.
if (import.meta.env.DEV && new URLSearchParams(location.search).get('preview') === 'character') {
  const { default: CharacterPreview } = await import('../tests/preview/CharacterPreview.svelte');
  mount(CharacterPreview, { target });
} else if (import.meta.env.DEV && ['storage', 'upgrade'].includes(new URLSearchParams(location.search).get('preview') ?? '')) {
  const { default: StoragePreview } = await import('../tests/preview/StoragePreview.svelte');
  mount(StoragePreview, { target });
} else if (import.meta.env.DEV && new URLSearchParams(location.search).get('preview') === 'logs') {
  const { default: LogsPreview } = await import('../tests/preview/LogsPreview.svelte');
  mount(LogsPreview, { target });
} else if (import.meta.env.DEV && new URLSearchParams(location.search).get('preview') === 'loot') {
  const { default: LootPreview } = await import('../tests/preview/LootPreview.svelte');
  mount(LootPreview, { target });
} else if (import.meta.env.DEV && new URLSearchParams(location.search).get('preview') === 'party') {
  const { default: PartyPreview } = await import('../tests/preview/PartyPreview.svelte');
  mount(PartyPreview, { target });
} else if (import.meta.env.DEV && new URLSearchParams(location.search).get('preview') === 'runs') {
  const { default: RunsPreview } = await import('../tests/preview/RunsPreview.svelte');
  mount(RunsPreview, { target });
} else if (import.meta.env.DEV && ['dungeon', 'realm', 'nexus', 'home-v4-dungeon', 'home-v4-realm', 'home-v4-nexus',
  'home-v4-recap', 'home-v4-recap-died', 'home-v4-recap-realm'].includes(new URLSearchParams(location.search).get('preview') ?? '')) {
  const { default: HomePreview } = await import('../tests/preview/HomePreview.svelte');
  mount(HomePreview, { target });
} else if (import.meta.env.DEV && new URLSearchParams(location.search).has('fixture')) {
  const { default: FixtureApp } = await import('../tests/shell/fixtures/FixtureApp.svelte');
  mount(FixtureApp, { target });
} else {
  mount(App, { target });
}
