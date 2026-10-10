<script lang="ts">
  import { tick } from 'svelte';
  import type { ChatRow } from '../bindings/ChatRow';
  import type { WhisperPartner } from '../bindings/WhisperPartner';
  import { routeHref } from '../navigation/routes';
  import { clockTime } from './logs-format';
  import { threadItems, routablePartner } from './whisper-screen';
  let { name, person, pages, hasOlder, loading, capped, onEarlier, onHide, onBottomChange, onRemoved }: {
    name: string; person: WhisperPartner | null; pages: ChatRow[][]; hasOlder: boolean; loading: boolean; capped: boolean;
    onEarlier: () => void; onHide: () => void; onBottomChange: (bottom: boolean) => void; onRemoved: () => void;
  } = $props();
  let region: HTMLDivElement;
  let items = $derived(threadItems(pages, new Date()));
  const allChat = $derived(routeHref({ kind: 'logs', view: 'chat', channel: 'all', seedSender: name }));
  export function atBottom() { return !region || region.scrollHeight - region.scrollTop - region.clientHeight < 8; }
  export function anchor() {
    const top = region?.getBoundingClientRect().top ?? 0;
    const first = [...(region?.querySelectorAll<HTMLElement>('[data-row-id]') ?? [])].find(row => row.getBoundingClientRect().bottom > top);
    return first ? { id: first.dataset.rowId, offset: first.getBoundingClientRect().top - top } : null;
  }
  export async function restore(saved: ReturnType<typeof anchor>) {
    await tick();
    const row = [...(region?.querySelectorAll<HTMLElement>('[data-row-id]') ?? [])].find(row => row.dataset.rowId === saved?.id);
    if (region && row && saved) region.scrollTop += row.getBoundingClientRect().top - region.getBoundingClientRect().top - saved.offset;
    onBottomChange(atBottom());
  }
  export async function bottom() { await tick(); if (region) region.scrollTop = region.scrollHeight; onBottomChange(true); }
  $effect.pre(() => {
    void pages;
    const active = document.activeElement;
    const id = active instanceof HTMLElement && region?.contains(active) ? active.dataset.rowId : null;
    if (!id) return;
    void tick().then(() => {
      if (!region?.isConnected || active?.isConnected) return;
      const replacement = [...region.querySelectorAll<HTMLElement>('[data-row-id]')].find(row => row.dataset.rowId === id);
      if (replacement) replacement.focus({ preventScroll: true });
      else { region.closest('main')?.querySelector<HTMLElement>('h1')?.focus({ preventScroll: true }); onRemoved(); }
    });
  });
</script>
<section class="conversation" aria-label={`Whispers with ${name}`}>
  <header><span class="identity"><b title={name}>{name}</b><span>★ {person?.stars ?? '—'} · {person?.lines ?? '—'} whispers</span></span>{#if routablePartner(name)}<a class="ctl ghost" href={allChat}>All chat from {name}</a>{:else}<button class="ctl ghost" disabled title="This name cannot be opened in Chat">All chat from {name}</button>{/if}<button class="ctl ghost danger" onclick={onHide}>Hide</button></header>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex (The scroll region must support keyboard scrolling.) -->
  <div class="thread" bind:this={region} onscroll={() => onBottomChange(atBottom())} role="region" aria-label="Whisper thread" tabindex="0">
    {#each items as item (item.type === 'line' ? item.row.message.id : item)}
      {#if item.type === 'day'}<div class="day">{item.label}</div>
      {:else}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex (Observed lines are focus targets for history restoration.) -->
      <div class="line" class:out={item.outgoing} class:hidden={item.row.hidden} data-row-id={item.row.message.id} tabindex="0">
        <time>{clockTime(item.row.message.at)}</time><span class="sender" title={item.row.message.direction === 'unknown' || !item.row.message.direction ? 'Direction unknown' : item.row.message.sender}>{item.outgoing ? 'You' : item.row.message.direction === 'incoming' ? item.row.message.sender : '—'}</span><span class="message">{item.row.message.text}{item.row.message.truncated ? '…' : ''}</span>
      </div>{/if}
    {/each}
    {#if !items.length && !loading}<p>No whispers match this conversation.</p>{/if}
  </div>
  <footer><span>Oldest at the top.</span>{#if capped}<span>Showing the latest 500 lines {#if routablePartner(name)}<a class="ctl ghost" href={allChat}>Older lines in Chat ›</a>{:else}<button class="ctl ghost" disabled title="This name cannot be opened in Chat">Older lines in Chat ›</button>{/if}</span>{:else if hasOlder}<button class="ctl ghost" disabled={loading} onclick={onEarlier}>{loading ? 'Loading earlier…' : 'Load earlier'}</button>{/if}</footer>
</section>
<style>
  .conversation { min-width: 0; min-height: 0; display: flex; flex-direction: column; padding: 8px 6px 6px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); }
  header { display: flex; align-items: center; flex-wrap: wrap; gap: 6px; padding: 0 6px 8px; font-size: 11px; }
  .identity { display: flex; align-items: baseline; gap: 6px; min-width: 0; flex: 1; } .identity b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; } .identity span { white-space: nowrap; color: var(--text-muted); }
  header a { max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .danger { color: var(--danger, #f28b9d); }
  .thread { flex: 1; overflow-y: auto; overflow-anchor: none; min-height: 0; }
  .line { display: grid; grid-template-columns: 58px 74px minmax(0, 1fr); gap: 10px; align-items: baseline; padding: 4px 8px; border-top: 1px solid var(--border-soft, var(--border)); font-size: 12px; }
  .sender { font-size: 11px; color: var(--ch-whisper); font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .out .sender { color: var(--gold); }
  .message { overflow-wrap: anywhere; line-height: 1.4; } time { font-size: 11px; color: var(--text-muted); font-variant-numeric: tabular-nums; }
  .hidden { opacity: .55; font-style: italic; } .day { font-size: 11px; color: var(--text-secondary); padding: 10px 8px 4px; }
  .thread:focus-visible, .line:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
  footer { display: flex; justify-content: space-between; flex-wrap: wrap; align-items: center; gap: 6px; padding-top: 8px; font-size: 11px; color: var(--text-muted); } p { padding: 8px; font-size: 12px; }
  @media (max-width: 1000px) { .line { grid-template-columns: 52px 62px minmax(0, 1fr); gap: 6px; } }
</style>
