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
  .people { min-width: 0; min-height: 0; border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); display: flex; flex-direction: column; padding: var(--space-2); }
  header { display: flex; justify-content: space-between; padding: var(--space-1) var(--space-2) var(--space-2); color: var(--text-secondary); font-size: var(--text-caption); line-height: var(--leading-caption); }
  header > :first-child { font-weight: var(--weight-medium); color: var(--text-primary); } header > :last-child { color: var(--text-muted); font-variant-numeric: tabular-nums; }
  nav { overflow-y: auto; min-height: 0; }
  .conv { display: grid; grid-template-columns: 32px minmax(0, 1fr) auto; gap: 0 var(--space-3); align-items: center; padding: var(--space-2); color: var(--text-primary); text-decoration: none;
    transition: background-color var(--motion-fast) var(--motion-ease); }
  .conv + .conv { border-top: 1px solid var(--hairline); }
  a.conv:hover { background: var(--surface-hover); }
  .conv.on, a.conv.on:hover { background: var(--surface-selected); box-shadow: var(--selected-edge); }
  .conv:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: -2px; }
  .name, .excerpt { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .name { font-size: var(--text-caption); line-height: var(--leading-caption); } .name b { font-weight: var(--weight-medium); }
  small, .time, .excerpt, .count { color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  .time { text-align: right; font-variant-numeric: tabular-nums; }
  .count { justify-self: end; box-sizing: border-box; min-height: 18px; padding: 0 var(--space-1); border: 1px solid var(--hairline-strong); border-radius: var(--radius-tag); font-variant-numeric: tabular-nums; }
  .avatar { width: 32px; height: 32px; border-radius: var(--radius-sprite); display: grid; place-items: center; background: var(--surface-raised); font-weight: var(--weight-medium); font-size: var(--text-caption); color: var(--ch-whisper); grid-row: span 2; }
</style>
