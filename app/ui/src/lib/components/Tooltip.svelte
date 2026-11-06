<script lang="ts">
  import type { Snippet } from 'svelte';
  import { scrollBounds } from './placement';
  // `content` replaces `text` when an explanation needs structure, such as a list.
  let { id, text = '', trigger, content }: { id: string; text?: string; trigger: Snippet<[string]>; content?: Snippet } = $props();
  let hovered = $state(false);
  let focused = $state(false);
  let dismissed = $state(false);
  let open = $derived((hovered || focused) && !dismissed);
  let position: HTMLSpanElement | undefined = $state();
  $effect(() => {
    if (!open || !position) return;
    const element = position;
    // Measure only while an explanation is open, never on live-stat updates.
    const place = () => {
      element.style.transform = '';
      element.style.top = '100%';
      element.style.bottom = 'auto';
      element.style.paddingTop = '.5rem';
      element.style.paddingBottom = '0';
      const width = document.documentElement.clientWidth || window.innerWidth;
      const height = document.documentElement.clientHeight || window.innerHeight;
      // A scrolling ancestor narrower than the viewport would otherwise gain a horizontal scrollbar,
      // so the tooltip is both capped to and kept inside its width.
      const { start, stop } = scrollBounds(element.parentElement?.parentElement ?? null, width);
      element.style.maxWidth = stop < width || start > 0 ? `min(20rem, ${Math.max(0, stop - start - 16)}px)` : '';
      const rect = element.getBoundingClientRect();
      const left = Math.max(start + 8, Math.min(rect.left, stop - rect.width - 8));
      element.style.transform = `translateX(${left - rect.left}px)`;
      const root = element.parentElement!.getBoundingClientRect();
      if (rect.bottom > height - 8 && root.top >= rect.height + 8) {
        element.style.top = 'auto';
        element.style.bottom = '100%';
        element.style.paddingTop = '0';
        element.style.paddingBottom = '.5rem';
      }
    };
    place();
    window.addEventListener('resize', place);
    window.addEventListener('scroll', place, true);
    return () => {
      window.removeEventListener('resize', place);
      window.removeEventListener('scroll', place, true);
    };
  });
  function leaveFocus(event: FocusEvent) {
    if (!(event.currentTarget instanceof HTMLElement) || !(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) focused = false;
  }
</script>

<svelte:window onkeydown={(event) => { if (event.key === 'Escape' && open) dismissed = true; }} />
<span class="tooltip-root" role="group"
  onpointerenter={() => { hovered = true; dismissed = false; }}
  onpointerleave={() => { hovered = false; }}
  onfocusin={() => { focused = true; dismissed = false; }} onfocusout={leaveFocus}>
  {@render trigger(id)}
  <span class="tooltip-position" hidden={!open} bind:this={position}>
    <span class="tooltip" role="tooltip" {id}>{#if content}{@render content()}{:else}{text}{/if}</span>
  </span>
</span>

<style>
  .tooltip-root { position: relative; display: inline-flex; max-width: 100%; }
  .tooltip-position { position: absolute; z-index: 20; top: 100%; inset-inline-start: 0; padding-top: var(--space-2); width: max-content; max-width: min(20rem, calc(100vw - 1rem)); }
  .tooltip-position[hidden] { display: none; }
  .tooltip { display: block; padding: var(--space-3); border: 1px solid var(--border-strong); border-radius: var(--radius-control); background: var(--surface-raised); color: var(--text-primary); box-shadow: var(--elevation-popover); font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-normal); overflow-wrap: anywhere; white-space: normal; text-align: start; }
</style>
