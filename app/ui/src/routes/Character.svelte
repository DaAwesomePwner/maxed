<script lang="ts">
  import { settingsHref } from '../lib/navigation/routes';
  import { getContext, onMount, tick } from 'svelte';
  import { readable } from 'svelte/store';
  import type { Client } from '../lib/bridge/client';
  import type { ItemDetail } from '../lib/bindings/ItemDetail';
  import type { ItemMatch } from '../lib/bindings/ItemMatch';
  import type { CharacterViewState } from '../lib/character/controller';
  import { SHELL_CONNECTION, type ShellConnection } from '../lib/shell/context';
  import StatsBreakdown from '../lib/character/StatsBreakdown.svelte';
  import WeaponRows from '../lib/character/WeaponRows.svelte';
  import Regeneration from '../lib/character/Regeneration.svelte';
  import Badge from '../lib/components/Badge.svelte';
  import { amount, reasonText } from '../lib/character/FormulaDetails.svelte';
  let { client }: { client?: Client } = $props();
  const controller = getContext<ShellConnection | undefined>(SHELL_CONNECTION)?.character;
  const viewState = controller?.state ?? readable<CharacterViewState>({ observed: null, calculation: null,
    freshness: null, defenseRaw: '0', defense: 0, defenseError: null, comparisons: [], comparisonError: null,
    loading: false, error: null });
  let observed = $derived($viewState.observed);
  let calculation = $derived($viewState.calculation);
  let catalogAvailable = $state<boolean | null>(null);
  let catalogError = $state('');
  let catalogEpoch = $state(0);
  let className = $state<string | null>(null);
  let items = $state<Record<number, ItemDetail | undefined>>({});
  let query = $state('');
  let matches = $state<ItemMatch[]>([]);
  let selected = $state('');
  let comparisonSelector: HTMLSelectElement | undefined;
  let screenElement: HTMLDivElement | undefined;
  let searchError = $state('');
  let searching = $state(false);
  const slots = ['Weapon', 'Ability', 'Armor', 'Ring'];
  // The native Character command accepts these catalog weapon slot types.
  const weaponSlots = new Set([1, 2, 3, 8, 17, 24]);
  let observedClassId = $derived(observed?.class_id ?? null);
  let catalogFingerprint = $derived(observed?.calculation_revision?.catalog_fingerprint);
  let itemKey = $derived(JSON.stringify([...new Set([
    ...(observed?.equipment.flatMap(slot => slot.item_type === null ? [] : [slot.item_type]) ?? []),
    ...$viewState.comparisons,
  ])]));
  let identityKey = $derived(JSON.stringify([observed?.calculation_revision?.stream_id, observed?.id]));

  onMount(() => client?.onVisualsChanged(() => { catalogEpoch++; }));
  $effect(() => {
    const owner = client; catalogEpoch; catalogFingerprint;
    let current = true;
    catalogAvailable = null; catalogError = '';
    if (owner) void owner.queryCatalogAvailable().then(value => { if (current) catalogAvailable = value; })
      .catch(() => { if (current) catalogError = 'Catalog availability unknown. Check Settings.'; });
    return () => { current = false; };
  });
  $effect(() => {
    const owner = client, classId = observedClassId; catalogEpoch; catalogFingerprint;
    let current = true; className = null;
    if (owner && classId != null) void owner.getObjectVisuals([classId]).then(page => {
      if (current) className = page.objects.find(object => object.object_type === classId)?.name ?? null;
    }).catch(() => {});
    return () => { current = false; };
  });
  $effect(() => {
    const owner = client, types: number[] = JSON.parse(itemKey); catalogEpoch; catalogFingerprint;
    let current = true; items = {};
    if (owner) void Promise.all(types.map(async (type) => {
      try { return [type, (await owner.getItemDetails(type, [])).item] as const; }
      catch { return [type, undefined] as const; }
    })).then(rows => { if (current) items = Object.fromEntries(rows); });
    return () => { current = false; };
  });
  $effect(() => { identityKey; query = ''; selected = ''; });
  $effect(() => {
    const owner = client, text = query.trim(); catalogEpoch; catalogFingerprint;
    let current = true; matches = []; selected = ''; searchError = ''; searching = false;
    const timer = setTimeout(async () => {
      if (!owner || !text) return;
      searching = true;
      try {
        const found = await owner.searchItems(text, 30);
        const checked = await Promise.all(found.map(async item => {
          try {
            const details = await owner.getItemDetails(item.object_type, []);
            return details.item.slot_type !== null && weaponSlots.has(details.item.slot_type) ? item : null;
          } catch { return null; }
        }));
        if (current) matches = checked.filter((item): item is ItemMatch => item !== null);
      } catch { if (current) searchError = 'Catalog weapon search unavailable. Check Settings.'; }
      finally { if (current) searching = false; }
    }, 150);
    return () => { current = false; clearTimeout(timer); };
  });
  function addComparison() {
    const type = Number(selected);
    if (!selected || !matches.some(item => item.object_type === type) || $viewState.comparisons.length >= 2) return;
    controller?.setComparisons([...$viewState.comparisons, type]);
    selected = '';
  }
  async function removeComparison(type: number | null) {
    if (type === null) return;
    const index = $viewState.comparisons.indexOf(type);
    const remaining = $viewState.comparisons.filter(item => item !== type);
    const next = remaining[index] ?? remaining[index - 1];
    controller?.setComparisons(remaining);
    await tick();
    if (!screenElement?.isConnected) return;
    const target = next === undefined ? comparisonSelector
      : screenElement.querySelector<HTMLButtonElement>(`[data-comparison-remove="${next}"]`);
    target?.focus();
  }
  function itemLabel(type: number) {
    return items[type]?.name ? `${items[type]!.name} (item ${type})` : `Item ${type} — metadata unavailable`;
  }
  function seenTime(value: string | null | undefined): string {
    if (!value) return '— · Observation time unavailable';
    const date = new Date(Number(value) / 1000);
    return Number.isFinite(date.getTime()) ? date.toLocaleTimeString() : '— · Observation time unavailable';
  }
</script>

<div class="character-screen" bind:this={screenElement}>
  <section aria-labelledby="character-observed">
    <h2 id="character-observed" class="sr-only">Character</h2>
    {#if !observed}<p>No character observed. Start capture and enter the game.</p>
    {:else}
      <div class="identity">
        <strong>{observed.name ?? '— · Name not observed'}</strong>
        <span>{className ?? (observed.class_id === null ? '— · Class not observed' : `Class ${observed.class_id} — name unavailable`)}</span>
        <span>Level {amount(observed.level)}{observed.level === null ? ' · Not observed' : ''}</span>
        <span class="freshness">{$viewState.freshness === 'died' ? 'Died' : $viewState.freshness === 'live' ? 'Live' : `Last seen ${seenTime(observed.observed_at)}`}</span>
      </div>
      {#if !observed.identity_verified || observed.issues.length}
        <p><Badge label="Partial" /> {!observed.identity_verified ? 'Local identity not verified. ' : ''}
          Coverage: {reasonText(observed.issues) || 'Limited identity evidence'}</p>
      {:else}<p>Coverage: local identity verified; observed fields may still be incomplete.</p>{/if}
    {/if}
    <dl class="equipment" aria-label="Equipment">
      {#each slots as label, index}
        {@const slot = observed?.equipment.find(slot => slot.slot === index)}
        <div><dt>{label}</dt><dd>{#if slot?.observed_empty === true}Empty
          {:else if slot?.item_type != null}{itemLabel(slot.item_type)}
          {:else}—<small>Slot not observed</small>{/if}</dd></div>
      {/each}
    </dl>
  </section>

  <StatsBreakdown panel={observed?.stat_panel} stats={observed?.stats} />

  <section aria-labelledby="character-weapon">
    <h2 id="character-weapon">Weapon</h2>
    <p class="note">Theoretical DPS at the normal firing rate. Enemy defense is a model input, not an observed enemy.</p>
    <div class="controls">
      <label>Enemy defense <input type="number" min="0" max="10000" step="1" value={$viewState.defenseRaw}
        aria-invalid={$viewState.defenseError ? 'true' : undefined} aria-describedby="defense-feedback"
        oninput={event => controller?.setDefense(event.currentTarget.value)} /></label>
      <button class="ctl" onclick={() => controller?.resetDefense()}>Reset</button>
    </div>
    <p id="defense-feedback" role="status">{$viewState.defenseError ?? ''}</p>
    <p>{calculation ? `Applied defense: ${calculation.applied_defense}` : 'Applied defense: — · No calculation available'}</p>
    {#if $viewState.loading}<p role="status">Calculating; previous results remain labelled with their applied inputs.</p>{/if}
    {#if observed && $viewState.error}<p role="status">{$viewState.error}</p>{/if}
    {#if catalogAvailable === false}
      <p>Weapon calculations unavailable — extract game assets</p><a href={settingsHref('assets')}>Settings</a>
    {:else if catalogError}<p>{catalogError} <a href={settingsHref('assets')}>Settings</a></p>{/if}
    <p class="note exclusions">Excludes exalt damage, Crucible, Blood Ritual, enchant damage and rate changes, unsupported
      special effects, Berserk, Dazed, Stunned and other firing limits; enemy conditions, armor break and
      vulnerability; misses, range falloff, repeat hits and target downtime. Assumes sustained fire with every
      projectile hitting one target once. Observed boosts already in ATT/DEX stay included.</p>
    <p class="note">At current stats; equipment bonuses are not recalculated.</p>
    <div class="controls">
      <label>Find catalog weapons <input type="search" bind:value={query} /></label>
      <label>Catalog weapon <select bind:value={selected} bind:this={comparisonSelector}>
        <option value="">Choose a weapon</option>
        {#each matches as match}<option value={String(match.object_type)} disabled={$viewState.comparisons.includes(match.object_type)}>{match.name} (item {match.object_type})</option>{/each}
      </select></label>
      <button class="ctl" onclick={addComparison} disabled={!selected || $viewState.comparisons.length >= 2}>Add comparison</button>
    </div>
    {#if searching}<p role="status">Searching catalog weapons…</p>{/if}
    {#if query.trim() && !searching && !matches.length && !searchError}<p>No eligible catalog weapons found. Known weapon slot metadata is required.</p>{/if}
    {#if searchError || $viewState.comparisonError}<p role="status">{searchError || $viewState.comparisonError}</p>{/if}
    {#each [null, ...$viewState.comparisons] as type, index}
      {@const row = index === 0 ? calculation?.weapons[0] : calculation?.weapons.slice(1).find(weapon => weapon.item_type === type)}
      {@const equippedSlot = observed?.equipment.find(slot => slot.slot === 0)}
      {@const itemType = index === 0 ? equippedSlot?.item_type : type}
      <article aria-label={index === 0 ? 'Equipped weapon' : `Comparison ${type}`}>
        <h3>{index === 0 ? 'Equipped weapon' : 'Comparison'} · {index === 0 && equippedSlot?.observed_empty === true ? 'Empty' : itemType == null ? '— · Weapon not observed' : itemLabel(itemType)}</h3>
        {#if index === 0}
          {@const enchant = observed?.equipment.find(slot => slot.slot === 0)?.enchant}
          <p>Equipped observed enchants: {enchant?.state === 'recorded_empty' ? 'Recorded empty' : enchant?.state === 'recorded'
            ? (enchant.ids.length ? enchant.ids.join(', ') : '— · Enchant IDs unavailable')
            : `— · ${reasonText([enchant?.state ?? 'not observed'])} enchant metadata`}</p>
        {:else}<p>Catalog-only comparison; no ownership claim or equipped enchant assumptions.</p>
          <button class="ctl" aria-label={`Remove comparison ${type}`} data-comparison-remove={type} onclick={() => removeComparison(type)}>Remove</button>{/if}
        {#if row && calculation && row.item_type === itemType}<WeaponRows weapon={row} version={calculation.formula_version} defense={calculation.applied_defense} />
        {:else}<p>— Calculation unavailable; waiting for matching character and catalog inputs.</p>{/if}
      </article>
    {/each}
  </section>
  <Regeneration regeneration={calculation?.regeneration} version={calculation?.formula_version} />
  <section aria-labelledby="character-exalt"><h2 id="character-exalt">Exalt damage</h2>
    <p class="note">Not available yet; pending verification</p></section>
</div>

<style>
  .character-screen { display: grid; gap: var(--space-5); min-width: 0; max-width: 1100px; }
  .character-screen > section, .character-screen > :global(section) { padding: var(--space-5); border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); min-width: 0; overflow-wrap: anywhere; }
  h2:not(.sr-only) { margin: 0 0 var(--space-3); font-size: var(--text-title); line-height: var(--leading-title); font-weight: var(--weight-medium); }
  h3 { margin: 0 0 var(--space-2); font-size: var(--text-body); line-height: var(--leading-body); font-weight: var(--weight-medium); }
  p { margin: 0 0 var(--space-2); font-size: var(--text-label); line-height: var(--leading-label); }
  a { color: var(--text-primary); text-decoration: none; text-underline-offset: 3px; }
  a:hover, a:focus-visible { text-decoration: underline; }
  .identity { display: flex; flex-wrap: wrap; gap: var(--space-1) var(--space-3); align-items: baseline; margin-bottom: var(--space-2); color: var(--text-secondary); font-size: var(--text-label); }
  .controls { display: flex; flex-wrap: wrap; gap: var(--space-3); align-items: end; margin-block: var(--space-3); }
  strong { font-weight: var(--weight-medium); }
  .identity strong { font-size: var(--text-title); line-height: var(--leading-title); font-weight: var(--weight-medium); color: var(--text-primary); }
  .freshness { color: var(--text-muted); }
  .equipment { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: var(--space-3); margin: var(--space-4) 0 0; }
  dd { margin: var(--space-1) 0 0; font-size: var(--text-label); } dt { color: var(--text-muted); font-size: var(--text-caption); }
  small { display: block; font-size: var(--text-meta); line-height: var(--leading-meta); color: var(--text-muted); }
  label { display: grid; gap: var(--space-1); min-width: 0; max-width: 100%; color: var(--text-secondary); font-size: var(--text-caption); }
  input, select { min-width: 0; width: 100%; max-width: 100%; box-sizing: border-box; }
  input[type='number'] { width: 10rem; }
  article { margin-top: var(--space-4); padding-top: var(--space-4); border-top: 1px solid var(--hairline); }
  .note { color: var(--text-secondary); }
  #defense-feedback:empty { display: none; }
  @media (max-width: 900px) { .equipment { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
