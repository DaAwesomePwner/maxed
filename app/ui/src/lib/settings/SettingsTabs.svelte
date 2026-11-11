<script lang="ts">
  import { untrack, type Snippet } from 'svelte';
  import { settingsTabs, type SettingsTab } from '../navigation/routes';

  let { requestedTab, notificationsEnabled, general, notifications, data, assets }: {
    requestedTab?: SettingsTab; notificationsEnabled: boolean;
    general: Snippet; notifications: Snippet; data: Snippet; assets: Snippet;
  } = $props();
  const id = $props.id();
  const storageKey = 'maxed.settings.tab';
  function initialTab(): string {
    if (requestedTab) return requestedTab;
    try { return localStorage.getItem(storageKey) ?? 'general'; }
    catch { return 'general'; }
  }
  let selected = $state(initialTab());
  let previousRequestedTab = untrack(() => requestedTab);
  $effect(() => {
    const next = requestedTab;
    if (next === previousRequestedTab) return;
    previousRequestedTab = next;
    // Only route requests drive this effect, never manual selection or panel updates.
    untrack(() => {
      if (next && settingsTabs.includes(next)) {
        selected = next === 'notifications' && !notificationsEnabled ? 'general' : next;
      }
    });
  });
  let tabs = $derived([
    { key: 'general', label: 'General', content: general },
    ...(notificationsEnabled ? [{ key: 'notifications', label: 'Notifications', content: notifications }] : []),
    { key: 'data', label: 'Data & history', content: data },
    { key: 'assets', label: 'Game assets', content: assets },
  ]);
  let active = $derived(tabs.some(tab => tab.key === selected) ? selected : 'general');
  let buttons = $state<Record<string, HTMLButtonElement>>({});
  function select(key: string) {
    selected = key;
    try { localStorage.setItem(storageKey, key); } catch { /* Storage can be unavailable. */ }
  }
  function navigate(event: KeyboardEvent, index: number) {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    let next: number;
    switch (event.key) {
      case 'ArrowRight': next = (index + 1) % tabs.length; break;
      case 'ArrowLeft': next = (index + tabs.length - 1) % tabs.length; break;
      case 'Home': next = 0; break;
      case 'End': next = tabs.length - 1; break;
      default: return;
    }
    event.preventDefault();
    select(tabs[next].key);
    buttons[tabs[next].key]?.focus();
  }
</script>

<div role="tablist" aria-label="Settings" aria-orientation="horizontal">
  {#each tabs as tab, index (tab.key)}
    <button class="ctl" type="button" role="tab" id={`${id}-${tab.key}-tab`}
      aria-controls={`${id}-${tab.key}-panel`} aria-selected={active === tab.key}
      tabindex={active === tab.key ? 0 : -1} bind:this={buttons[tab.key]}
      onclick={() => select(tab.key)} onkeydown={event => navigate(event, index)}>{tab.label}</button>
  {/each}
</div>
{#each tabs as tab (tab.key)}
  <!-- Keep inactive panels mounted: alert/chat/assets controls own unsaved drafts. -->
  <div role="tabpanel" id={`${id}-${tab.key}-panel`} aria-labelledby={`${id}-${tab.key}-tab`}
    hidden={active !== tab.key}>
    {@render tab.content()}
  </div>
{/each}

<style>
  [role="tablist"] { display: flex; flex-wrap: wrap; gap: var(--space-1); min-width: 0; }
  /* Tabs: unselected is a quiet ghost; selected is a neutral surface step, never gold or violet. */
  button { border-color: transparent; background: transparent; color: var(--text-secondary); }
  button:hover:not([aria-selected="true"]) { color: var(--text-primary); }
  button[aria-selected="true"], button[aria-selected="true"]:hover { color: var(--text-primary); background: var(--surface-raised); border-color: transparent; box-shadow: var(--highlight-top); }
  [role="tabpanel"] { min-width: 0; }
  [role="tabpanel"]:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
</style>
