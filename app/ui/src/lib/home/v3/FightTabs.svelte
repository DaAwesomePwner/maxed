<script lang="ts">
  import { objectVisual } from '../../bridge/visual-index';
  import { onMount, tick } from 'svelte';
  import type { FightView } from '../../bindings/FightView';
  import type { SpriteRef } from '../../bindings/SpriteRef';
  import SpriteImage from '../../components/SpriteImage.svelte';
  import { formatElapsed } from '../../styles/format';
  import { bossOutcome } from '../dungeon';
  import type { HomeVisuals } from '../visuals';
  let { fights, selected, visuals, resolveSpriteUrl, onselect, other = null, names = null, saved = false }: {
    saved?: boolean; other?: { label: string; sub: string } | null; names?: Record<string, string> | null;
    fights: FightView[]; selected: string; visuals: HomeVisuals;
    resolveSpriteUrl: (s: SpriteRef) => string | null; onselect: (id: string) => void;
  } = $props();
  let strip: HTMLDivElement;
  let root: HTMLDivElement;
  let otherTab = $state<HTMLButtonElement>();
  const otherTabId = $props.id();
  let width = $state(340), left = $state(0);
  const tabWidth = 170;
  let focusId = $derived(selected === 'whole' || (selected === 'others' && other !== null) || fights.some(fight => fight.id === selected) ? selected : 'whole');
  let content = $derived(fights.length * tabWidth);
  // Fixed-width tabs keep content reactive; allow for fractional scrollLeft at the end.
  let remaining = $derived(left + width >= content - 1
    ? 0 : Math.max(0, fights.length - Math.floor((left + width) / tabWidth)));
  let overflow = $derived(content > width);
  function measure() { if (strip) {
    width = strip.clientWidth || 340; left = strip.scrollLeft;
  } }
  onMount(() => {
    measure();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(strip);
    return () => observer?.disconnect();
  });
  async function keyboard(event: KeyboardEvent) {
    const keys = ['ArrowRight', 'ArrowLeft', 'Home', 'End'];
    if (!keys.includes(event.key)) return;
    event.preventDefault();
    const buttons = [...root.querySelectorAll<HTMLButtonElement>('[role="tab"]'), ...(other && otherTab ? [otherTab] : [])];
    const current = buttons.indexOf(event.target as HTMLButtonElement);
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1
      : (current + (event.key === 'ArrowRight' ? 1 : -1) + buttons.length) % buttons.length;
    onselect(next === 0 ? 'whole' : next > fights.length ? 'others' : fights[next - 1].id);
    await tick(); buttons[next].focus();
    buttons[next].scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
  }
  function more() {
    strip.scrollBy({ left: remaining > 0 ? width : -strip.scrollWidth, behavior: 'auto' });
  }
</script>

<div class="fight-tabs" class:saved>
 <div class="fight-tablist" bind:this={root} role="tablist" tabindex="-1" aria-label="Encounters" aria-owns={other ? otherTabId : undefined} onkeydown={keyboard}>
  <button class="whole" data-row-id={saved ? 'tab:whole' : undefined} type="button" role="tab" aria-selected={selected === 'whole'}
    tabindex={focusId === 'whole' ? 0 : -1} onclick={() => onselect('whole')}>Whole run</button>
  <div class="fight-scroll" bind:this={strip} onscroll={measure}>
    {#each fights as fight (fight.id)}
      {@const visual = objectVisual(visuals.objects, (fight.target_type ?? fight.boss_type))}
      {@const done = fight.ended !== null || Boolean(fight.kill)}
      <button type="button" role="tab" aria-label={visual?.name ?? names?.[fight.id] ?? 'Unknown Target'}
        data-row-id={saved ? `fight:${fight.id}` : undefined} class:live={!done} class:finished={done} aria-selected={selected === fight.id}
        tabindex={focusId === fight.id ? 0 : -1} onclick={() => onselect(fight.id)}>
        <SpriteImage sprite={visual?.sprite ?? null} fit={32}
          sheetUrl={visual?.sprite ? resolveSpriteUrl(visual.sprite) : null} alt={visual?.name ?? 'Boss'} />
        <span class="label"><span title={visual?.name ?? names?.[fight.id] ?? 'Unknown Target'}>{visual?.name ?? names?.[fight.id] ?? 'Unknown Target'}</span>
          <small>{(saved ? bossOutcome(fight) === 'killed' : done) ? '✓ ' : ''}{formatElapsed(fight.started, fight.ended ?? fight.kill?.at ?? fight.last_hit ?? fight.started)}</small></span>
      </button>
    {/each}
  </div>
 </div>
  {#if overflow}<button class="more" type="button" onclick={more}>{remaining > 0 ? `${remaining} more ›` : '‹ Newest'}</button>{/if}
    {#if other}<button bind:this={otherTab} id={otherTabId} type="button" role="tab" class="other" data-row-id={saved ? 'tab:others' : undefined} aria-selected={selected === 'others'}
      tabindex={focusId === 'others' ? 0 : -1} onclick={() => onselect('others')} onkeydown={keyboard}>
      <span class="label"><span>{other.label}</span><small>{other.sub}</small></span>
    </button>{/if}
</div>

<style>
  .fight-tabs { display: flex; align-items: stretch; gap: 4px; min-width: 0; height: 46px; }
  .fight-tablist { display: flex; flex: 1; min-width: 0; gap: 4px; }
  button { box-sizing: border-box; display: flex; align-items: center; gap: 5px; height: 46px; border: 1px solid transparent;
    border-radius: 5px; background: transparent; color: var(--text-secondary); font: inherit; font-size: 12px; cursor: pointer; }
  .whole { flex: 0 0 82px; justify-content: center; }
  .saved .whole { white-space: nowrap; }
  .more { flex: 0 0 100px; min-width: 100px; justify-content: center; align-self: center; height: 32px;
    padding: 0 10px; border-color: var(--border-strong); background: var(--surface-raised);
    white-space: nowrap; font-size: 12px; font-weight: 600; font-variant-numeric: tabular-nums; }
  .fight-scroll { display: flex; flex: 1; min-width: 0; overflow-x: auto; scrollbar-width: none; }
  .fight-scroll button { flex: 0 0 170px; width: 170px; padding: 4px; }
  .other { flex: 0 0 auto; padding: 4px 8px; border-style: dashed; border-color: var(--border); }
  .label { min-width: 0; text-align: left; } .label > span { display: block; max-width: 130px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  small { display: block; font-size: 11px; font-variant-numeric: tabular-nums; }
  .live small { color: var(--success, #75dfa1); } [aria-selected="true"] { background: var(--surface-raised); border-color: var(--border-strong); }
  button:focus-visible { outline: 2px solid var(--gold); outline-offset: -2px; }
</style>
