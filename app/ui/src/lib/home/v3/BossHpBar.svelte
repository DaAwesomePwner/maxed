<script lang="ts">
  import { formatStat } from '../../styles/format';
  import { clockTime } from '../nexus';
  let { hp, max, ended = false, killedAt = null, statusInside = false, saved = false }: {
    hp: number | null; max: number | null; ended?: boolean; killedAt?: string | null; statusInside?: boolean; saved?: boolean;
  } = $props();
  let displayedHp = $derived(killedAt ? 0 : hp);
  let known = $derived(displayedHp !== null && max !== null && Number.isFinite(displayedHp) && displayedHp >= 0 && Number.isFinite(max) && max > 0);
  let fill = $derived(killedAt ? 0 : known ? Math.min(100, hp! / max! * 100) : 0);
  let text = $derived(known ? `${killedAt ? "Defeated \u00b7 " : ""}${formatStat(displayedHp)} / ${formatStat(max)} · ${formatStat(displayedHp! / max! * 100)}%` : '—');
  let savedKill = $derived(saved && Boolean(killedAt));
  let label = $derived(savedKill ? `Killed at ${clockTime(killedAt!) ?? '—'} · ${formatStat(max !== null && Number.isFinite(max) && max > 0 ? max : null)} HP`
    : statusInside && killedAt ? `Defeated · ${clockTime(killedAt) ?? '—'}` : text);
</script>

<div class="boss-hp" class:status-inside={statusInside || savedKill} class:saved-kill={savedKill} data-state={killedAt ? 'killed' : known ? (ended ? 'ended' : 'known') : 'unknown'}>
  <div class="track" class:unknown={!known && !((statusInside || savedKill) && killedAt)} class:killed={Boolean(killedAt)}
    role={known ? 'meter' : undefined} aria-label={known ? 'Target HP' : undefined}
    aria-valuemin={known ? 0 : undefined} aria-valuemax={known ? max! : undefined}
    aria-valuenow={known ? Math.min(displayedHp!, max!) : undefined}
    aria-valuetext={known ? (savedKill ? label : statusInside && killedAt ? `${text}; ${label}` : text) : undefined}>
    <i style:width={`${fill}%`}></i><span class="hp-text">{label}</span>
  </div>
  {#if !statusInside && !savedKill}<span class="boss-status">{killedAt ? `Killed · ${clockTime(killedAt) ?? '—'}` : ended ? 'Ended' : '\u00a0'}</span>{/if}
</div>

<style>
  .boss-hp { display: grid; grid-template-columns: minmax(0, 1fr) 18ch; gap: 6px;
    height: 24px; min-height: 24px; width: 100%; font-variant-numeric: tabular-nums; }
  .boss-hp.status-inside { grid-template-columns: minmax(0, 1fr); }
  .status-inside .track.killed { background: #2a2433; }
  .status-inside .track.killed i { background: #5e5270; }
  .boss-hp.saved-kill { height: 18px; min-height: 18px; }
  .saved-kill .track.killed { height: 18px; border: 0; border-radius: 2px; background: #2a1c22; }
  .saved-kill .hp-text { color: var(--text-muted); font-size: 11px; font-weight: 600; text-shadow: none; }
  .track { position: relative; box-sizing: border-box; height: 24px; border: 2px solid #111; border-radius: 3px;
    background: #5a1a1e; overflow: hidden; }
  i { position: absolute; inset: 0 auto 0 0; background: linear-gradient(#d94a51, #b32c34); transition: width 400ms linear; }
  .hp-text { position: absolute; inset: 0; display: grid; place-items: center; color: #fff; font-size: 12px; font-weight: 700;
    white-space: nowrap; text-shadow: 1px 0 #000, -1px 0 #000, 0 1px #000, 0 -1px #000; }
  .boss-status { display: grid; align-items: center; text-align: right; font-size: 11px; white-space: nowrap; }
  .unknown { visibility: hidden; }
  @media (prefers-reduced-motion: reduce) { i { transition: none; } }
</style>
