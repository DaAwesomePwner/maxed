<script lang="ts">
  import { observePresentationVisibility } from '../bridge/visibility';
  import DebugNote from '../debug/DebugNote.svelte';
  import { debugMode } from '../debug/debug';
  import { untrack } from 'svelte';
  import type { CaptureStatus } from '../bindings/CaptureStatus';
  import { captureGuidance, captureLabels, type RailView } from './rail';
  import { liveDuration } from './live-duration';
  import { formatDps } from '../styles/format';
  let { view, capture, busy, error, preview = false, onStart, onStop, onRetry }: {
    view: RailView; capture: CaptureStatus | null; busy: boolean; error: string; preview?: boolean;
    onStart: () => void; onStop: () => void; onRetry: () => void;
  } = $props();
  let active = $derived(capture !== null && ['WaitingForGame', 'Live', 'Recovering'].includes(capture.state));
  let failed = $derived(capture !== null && ['Failed', 'NpcapMissing', 'NpcapIncompatible', 'PermissionDenied'].includes(capture.state));
  let restartRequired = $derived(capture?.state === 'Failed' && ['Capture worker failed; restart Maxed'].includes(capture.message));
  // Native CaptureStatus.message is sanitized at the WebView boundary. Preserve
  // its specific remedy (including permanent worker failure), never exception text.
  let guidance = $derived(capture?.message || (capture ? captureGuidance[capture.state] : 'Connecting to capture status.'));
  let startUnavailable = $derived(preview || busy || !capture || active || restartRequired);
  let stopUnavailable = $derived(preview || busy || !capture || capture.state === 'Stopped' || restartRequired);

  let liveMs = $state(0);
  let clockStream: string | null = null;
  let captureState = $derived(capture?.state ?? null);
  let clockLive = $derived(captureState === 'Live' && !preview && !view.stale);
  let elapsed = $derived(liveDuration(liveMs));
  let reconnect = $derived((capture === null && error !== '') || capture?.message === 'Live updates interrupted; reconnect or restart Maxed');
  let actionLabel = $derived(reconnect ? 'Reconnect capture' : active ? 'Stop capture'
    : failed && !restartRequired ? 'Retry capture' : 'Start capture');
  let actionUnavailable = $derived(reconnect ? preview || busy : active ? stopUnavailable : startUnavailable);
  let stateText = $derived(view.stale && capture?.state === 'Live' ? 'Paused' : capture ? captureLabels[capture.state] : 'Connecting');
  let stream = $derived(view.snapshot?.stream_id ?? null);
  $effect(() => {
    const state = captureState, source = stream;
    if (source !== null && source !== clockStream) { clockStream = source; liveMs = 0; }
    if (!state || ['Stopped', 'Failed', 'NpcapMissing', 'NpcapIncompatible', 'PermissionDenied'].includes(state)) liveMs = 0;
    if (!clockLive) return;
    const base = untrack(() => liveMs), start = Date.now();
    let timer: ReturnType<typeof setInterval> | undefined;
    const update = () => { liveMs = base + Math.max(0, Date.now() - start); };
    const releaseVisibility = observePresentationVisibility(visible => {
      clearInterval(timer); timer = undefined;
      if (visible) { update(); timer = setInterval(update, 1000); }
    });
    return () => {
      releaseVisibility(); clearInterval(timer);
      // Preserve the partial second when capture pauses between display ticks.
      liveMs = base + Math.max(0, Date.now() - start);
    };
  });
  function toggle() {
    if (actionUnavailable) return;
    if (reconnect) onRetry();
    else if (active) onStop();
    else onStart();
  }
</script>

<section class="compact-live" aria-label="Capture">
  <div class="status-card">
    <h2 class="where" title={view.map}><i class:live={capture?.state === 'Live' && !view.stale && !preview} aria-hidden="true"></i><span class="where-name">{view.map}</span></h2>
    <div class="state">
      <span class="capture-state" class:failed role="status" aria-live="polite" aria-atomic="true">{preview ? 'Preview · ' : ''}{stateText}</span>
      {#if active}<span class="separator" aria-hidden="true"> · </span><span class="elapsed" aria-label="Capture live elapsed">{preview ? '—' : elapsed}</span>{/if}
    </div>
    <button class="toggle" class:start={!active} aria-label={actionLabel} title={actionLabel}
      aria-disabled={actionUnavailable} aria-busy={busy} onclick={toggle}>
      <svg viewBox="0 0 16 16" aria-hidden="true">{#if active}<rect x="3" y="3" width="10" height="10" />{:else}<path d="M4 2 14 8 4 14Z" />{/if}</svg>
    </button>
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if !preview && restartRequired}<p class="guidance">Restart Maxed</p>{/if}
  {#if view.stale}<DebugNote text="Paused · last observation" />{/if}
  {#if $debugMode}
    <details>
      <summary>Details</summary>
      <DebugNote text="Live duration starts when this window observes capture Live; earlier capture time is unavailable." />
      <dl class="fight-stats" class:stale={view.stale}>
        <div><dt><span class="sr-only">Run </span>Time</dt><dd>{#if view.elapsed === null}{@render unavailable()}{:else}{view.elapsed}{/if}</dd></div>
        {#if view.fight || view.wholeRun}
          <div><dt><span class="sr-only">{view.wholeRun ? 'Dungeon' : 'Fight'} </span>DPS</dt><dd>{#if view.dps === null}{@render unavailable()}{:else}{formatDps(view.dps)}{/if}</dd></div>
          <div><dt>Rank</dt><dd>{#if view.rank === null}{@render unavailable()}{:else}#{view.rank}{/if}</dd></div>
        {/if}
      </dl>
      <p class="target" class:stale={view.stale}>{view.target}{#if view.fight?.kill && !view.fight.kill.provisional}{' · '}Defeated{:else if view.fight?.ended != null}{' · '}Ended{/if}</p>
      {#if !preview}<DebugNote text={guidance} />{/if}
      {#if capture && (capture.dropped_frames || capture.unknown_packets || capture.decode_failures)}
        <dl aria-label="Capture diagnostics" class="diagnostics">
          <div><dt>Dropped frames</dt><dd>{capture.dropped_frames}</dd></div>
          <div><dt>Unknown packets</dt><dd>{capture.unknown_packets}</dd></div>
          <div><dt>Decode failures</dt><dd>{capture.decode_failures}</dd></div>
        </dl>
      {/if}
    </details>
  {/if}
</section>
{#snippet unavailable()}<span aria-hidden="true">—</span><span class="sr-only">Unavailable</span>{/snippet}

<style>
  .compact-live { flex: 0 0 auto; min-width: 0; padding: var(--space-3); border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); }
  .status-card { display: grid; grid-template-columns: minmax(0, 1fr) var(--control-dense); gap: 0 var(--space-2); align-items: center; }
  .where { display: flex; align-items: center; gap: var(--space-2); min-width: 0; margin: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--text-caption); line-height: var(--leading-caption); font-weight: var(--weight-medium); }
  /* text-overflow does not apply to a flex container's bare text, so the name gets its own box. */
  .where-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .where i { width: 8px; height: 8px; flex: none; border-radius: var(--radius-pill); background: var(--text-disabled); }
  .where i.live { background: var(--success); }
  .state { grid-column: 1; min-width: 0; display: block; font-size: var(--text-meta); line-height: var(--leading-meta); font-weight: var(--weight-normal); color: var(--text-muted); }
  .separator { display: inline-block; padding-inline: var(--space-1); }
  .elapsed { font-variant-numeric: tabular-nums; white-space: nowrap; }
  .toggle { grid-column: 2; grid-row: 1 / span 2; box-sizing: border-box; width: var(--control-dense); height: var(--control-dense); min-height: 0; padding: 0; display: grid; place-items: center; border: 1px solid var(--border-strong); border-radius: var(--radius-control); background: var(--surface-raised); color: var(--danger); }
  .toggle.start { color: var(--success); }
  .toggle[aria-disabled="true"] { opacity: .5; }
  svg { width: 12px; height: 12px; fill: currentColor; }
  p, dl { margin: 0; font-size: var(--text-meta); line-height: var(--leading-meta); }
  .error, .failed { color: var(--danger); }
  .guidance, .target { color: var(--text-secondary); }
  .guidance { min-height: 0; margin-top: var(--space-1); }
  .stale { color: var(--text-muted); }
  details { margin-top: var(--space-1); font-size: var(--text-meta); overflow-wrap: anywhere; }
  summary { cursor: pointer; min-height: var(--target-min); align-content: center; color: var(--text-secondary); }
  summary:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
  .fight-stats { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); }
  dd { margin: 0; font-variant-numeric: tabular-nums; }
  .diagnostics div { display: flex; justify-content: space-between; gap: var(--space-1); }
</style>
