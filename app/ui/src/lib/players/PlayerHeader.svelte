<script lang="ts">
  import type { PlayerProfileView } from '../bindings/PlayerProfileView';
  import type { HomeVisuals } from '../home/visuals';
  import SpriteImage from '../components/SpriteImage.svelte';
  import { objectVisual } from '../bridge/visual-index';
  import { spriteUrl } from '../bridge/visuals';
  import { formatClockRange } from '../styles/format';
  import { durationText } from './player-view';
  import './players.css';
  let { name, classId, isLocal, live, partial, profile, inRunMs, presenceUntil = null, visuals, picker, prevHref, nextHref, prevName = null, nextName = null }: {
    name: string | null; classId: number | null; isLocal: boolean; live: boolean; partial: boolean;
    profile: PlayerProfileView | null; inRunMs: number | null; presenceUntil?: number | null; visuals: HomeVisuals;
    prevName?: string | null; nextName?: string | null;
    picker: { href: string | null; label: string; current: boolean }[]; prevHref: string | null; nextHref: string | null;
  } = $props();
  let visual = $derived(objectVisual(visuals.objects, classId));
</script>
<header class="player-header">
  <div class="player-avatar"><SpriteImage sprite={visual?.sprite ?? null} sheetUrl={visual?.sprite ? spriteUrl(visuals.client_fingerprint, visual.sprite) : null} fit={32} alt={visual?.name ?? 'Class not seen'} /></div>
  <div class="player-identity">
    <h2>{name ?? 'Not seen'} {#if isLocal}<span class="player-tag you">You</span>{/if}{#if live}<span class="player-tag live">Live</span>{/if}{#if partial}<span class="player-tag">partial</span>{/if}</h2>
    <p>{visual?.name ?? (classId === null ? 'Not seen' : `Class ${classId}`)} · Lv {profile?.level ?? '—'} · ★ {profile?.stars ?? '—'} · {profile?.maxed ?? '—'}/8 · Guild {profile?.guild ?? 'Not seen'} · in run {#if profile}{formatClockRange(profile.first_seen, presenceUntil === null ? profile.last_seen : String(Math.round(presenceUntil * 1000)))} ({durationText(inRunMs)}){:else}{durationText(inRunMs)}{/if}</p>
  </div>
  <nav class="player-switcher" aria-label="Player navigation">
    <details class="player-picker"><summary class="player-control">{picker.find(row => row.current)?.label ?? name ?? 'Not seen'} <span aria-hidden="true">▾</span></summary>
      <ul>{#each picker as row}<li>{#if row.href}<a href={row.href} aria-current={row.current ? 'page' : undefined}>{row.label}</a>
        {:else}<button type="button" disabled>{row.label}</button>{/if}</li>{/each}</ul>
    </details>
    {#if prevHref}<a class="player-control square" href={prevHref} aria-label={`Previous player: ${prevName ?? 'Unknown player'}`}>‹</a>{:else}<button class="player-control square" type="button" disabled aria-label="Previous player">‹</button>{/if}
    {#if nextHref}<a class="player-control square" href={nextHref} aria-label={`Next player: ${nextName ?? 'Unknown player'}`}>›</a>{:else}<button class="player-control square" type="button" disabled aria-label="Next player">›</button>{/if}
  </nav>
</header>
