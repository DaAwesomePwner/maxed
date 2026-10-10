<script lang="ts">
  import DebugNote from '../debug/DebugNote.svelte';
  import { recordingFailureMessages, type createRecordingSession } from './recording';
  let { session, checked = $bindable(false), disabled = false, demo = false, saved = false, limit }: {
    session: ReturnType<typeof createRecordingSession>; checked?: boolean; disabled?: boolean;
    demo?: boolean; saved?: boolean; limit: number;
  } = $props();
  // Recording is live-only. The privacy and auto-delete notes are consent disclosures, not data-quality caveats,
  // so they stay visible. Retention removes oldest closed owned files; active and unrelated files are preserved.
  const id = $props.id();
  let status = $derived($session.status);
  let stateLabel = $derived(status ? status.state[0].toUpperCase() + status.state.slice(1) : $session.busy ? 'Loading' : 'Unavailable');
</script>

<div class="recording">
  <div class="choice"><label class="check"><input type="checkbox" bind:checked disabled={disabled || (demo && !saved)} aria-describedby={`${id}-warning ${id}-retention`} /> Save Recordings</label><span>Saved: {saved ? 'On' : 'Off'}</span></div>
  <p id={`${id}-warning`} class="warning">May contain passwords and other players' data. Keep private.</p>
  {#if demo}<p>Demo</p>{/if}
  <p class:error={!!$session.error} role={status?.state === 'failed' || $session.error ? 'alert' : 'status'}>
    Recording: {stateLabel}
    {#if status?.state === 'failed'} · {status.failure ? recordingFailureMessages[status.failure] : 'Toggle recording to retry.'}{/if}
    {#if $session.error} {$session.error}{/if}
  </p>
  {#if status && status.state !== 'disabled'}<p>Retained: {status.retained_bytes.toLocaleString()} bytes</p>{/if}
  {#if status?.state === 'disabled'}<DebugNote text="Disk usage is not inventoried." />{/if}
  {#if status?.state === 'failed' || $session.error}<DebugNote text="Last known recording status and disk usage may be stale." />{/if}
  {#if status && status.dropped_frames > 0}
    <DebugNote>{status.dropped_frames.toLocaleString()} packets were skipped {#if status.state === 'writing'}during traffic bursts or recording changes; the recording continues.{:else}in the last recording.{/if}</DebugNote>
  {/if}
  <details>
    <summary>Retention</summary>
    <p>Limit: {#if limit === 1_000_000_000}1 GB{:else}{limit.toLocaleString()} bytes{/if}</p>
    <p id={`${id}-retention`}>Oldest recordings auto-delete at the limit.</p>
    {#if status && status.state !== 'disabled'}<p>Current Limit: {status.limit_bytes.toLocaleString()} bytes</p>{/if}
    {#if status && status.state !== 'failed' && !$session.error}<button type="button" disabled={$session.busy} onclick={() => session.refresh()}>Retry Recording</button>{/if}
  </details>
  {#if !status || status.state === 'failed' || $session.error}<button type="button" disabled={$session.busy} onclick={() => session.refresh()}>Retry Recording</button>{/if}
</div>

<style>
  .recording { border-top: 1px solid var(--border); margin-top: var(--space-2); padding-top: var(--space-1); }
  p { margin: var(--space-1) 0; color: var(--text-secondary); line-height: var(--leading-label); font-variant-numeric: tabular-nums; }
  .check { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); min-height: var(--target-min); font-size: var(--text-body); }
  .choice { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); }
  .choice span { margin-left: auto; color: var(--text-secondary); font-size: var(--text-label); }
  input { width: 1.25rem; height: 1.25rem; accent-color: var(--violet); }
  summary { min-height: var(--target-min); align-content: center; cursor: pointer; color: var(--violet-text); }
  summary:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
  button { min-height: var(--target-min); padding: var(--space-1) var(--space-2); font-size: var(--text-label); }
  .error { color: var(--danger); }
  .warning { color: var(--warning); }
</style>
