<script lang="ts">
  import type { WhisperPartner } from '../bindings/WhisperPartner';
  import { routeHref } from '../navigation/routes';
  import { clockTime, dayLabel } from './logs-format';
  import { excerpt, routablePartner } from './whisper-screen';
  let { rows, count, selected }: { rows: WhisperPartner[]; count: number; selected: string | null } = $props();
</script>
<section class="people" aria-label="Whisper people">
  <header><span>People</span><span>{count.toLocaleString('en-US')}</span></header>
  <nav aria-label="Whisper conversations">
    {#each rows as person (person.name)}
      <svelte:element this={routablePartner(person.name) ? 'a' : 'div'} class="conv" class:on={person.name === selected} aria-current={person.name === selected ? 'page' : undefined} href={routablePartner(person.name) ? routeHref({ kind: 'logs', view: 'chat', channel: 'whisper', partner: person.name }) : undefined}>
        <span class="avatar" aria-hidden="true">{person.name[0]}</span>
        <span class="name" title={person.name}><b>{person.name}</b> <small>★ {person.stars ?? '—'}</small></span>
        <span class="time">{dayLabel(person.last_at, new Date()) === 'Today' ? clockTime(person.last_at) : dayLabel(person.last_at, new Date())}</span>
        <span class="excerpt" title={excerpt(person)}>{excerpt(person)}</span><span class="count">{person.lines}</span>
      </svelte:element>
    {/each}
  </nav>
</section>
<style>
  .people { min-width: 0; min-height: 0; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); display: flex; flex-direction: column; padding: 6px; }
  header { display: flex; justify-content: space-between; padding: 6px; color: var(--text-secondary); font-size: 11px; }
  header > :first-child { text-transform: uppercase; letter-spacing: .6px; }
  nav { overflow-y: auto; min-height: 0; }
  .conv { display: grid; grid-template-columns: 32px minmax(0, 1fr) auto; gap: 2px 10px; align-items: center; padding: 7px 8px; color: var(--text-primary); text-decoration: none; }
  .conv + .conv { border-top: 1px solid var(--border-soft, var(--border)); }
  .conv.on { background: #f2c14e14; box-shadow: inset 2px 0 0 var(--gold); }
  .conv:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
  .name, .excerpt { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .name { font-size: 12.5px; } small, .time, .excerpt, .count { color: var(--text-muted); font-size: 11px; }
  .time { text-align: right; } .count { justify-self: end; border: 1px solid var(--border); border-radius: 8px; padding: 0 5px; }
  .avatar { width: 32px; height: 32px; border-radius: 6px; display: grid; place-items: center; background: var(--surface-raised); font-weight: 700; font-size: 13px; color: var(--ch-whisper); grid-row: span 2; }
</style>
