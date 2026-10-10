<script lang="ts">
  import DebugNote from '../debug/DebugNote.svelte';
  import type { Client } from '../bridge/client';
  import type { UpdateChannel } from '../bindings/UpdateChannel';
  import type { CaptureStatus } from '../bindings/CaptureStatus';
  import type { UpdateFailure } from '../bindings/UpdateFailure';
  import { createUpdateSession } from './updates';
  import { titleCase } from '../styles/format';
  let { client, preview = false, channel, capture = null }: { client: Client; preview?: boolean; channel?: UpdateChannel; capture?: CaptureStatus | null } = $props();
  // Checks use the saved channel; draft changes take effect after Settings Apply.
  let session = $derived(createUpdateSession(client, preview));
  $effect(() => { channel; void session.refresh(); });
  $effect(() => { const current = session; return () => current.dispose(); });
  const failures: Record<UpdateFailure, string> = {
    offline: 'Connection failed. Retry.',
    unavailable: 'Updates Unsupported',
    invalid_metadata: 'Invalid Metadata', invalid_signature: 'Invalid Signature',
    unsupported_target: 'Unsupported Update', too_large: 'Update Too Large',
    busy: 'Update Running', stale_offer: 'Check Again',
    shutdown_blocked: 'Check storage. Retry Install.',
    install_failed: 'Install failed. Check again.',
  };
  let canRestartCapture = $derived(!!capture && !['Live', 'WaitingForGame', 'Recovering'].includes(capture.state));
  let active = $derived(!!$session.status && ['checking', 'downloading', 'installing'].includes($session.status.phase));
</script>
<p>Saved Channel: {channel ? titleCase(channel) : '—'}</p>
<p role="status">Update: {$session.status ? titleCase($session.status.phase) : ($session.error ? 'Unavailable' : 'Loading')}{#if $session.error && $session.status}<DebugNote inline>Last known; stale</DebugNote>{/if}</p>
{#if $session.error}<p role="alert">{$session.error}</p>{/if}
{#if $session.status?.failure}<p role="alert">{failures[$session.status.failure]}</p>{/if}
{#if $session.status?.offer}
  <p>Version: {$session.status.offer.version} ({$session.status.offer.channel})</p>
  {#if $session.status.offer.notes}<details><summary>Release Notes</summary><pre>{$session.status.offer.notes}</pre></details>{/if}
{/if}
{#if $session.status?.phase === 'downloading'}
  <p>{$session.status.downloaded_bytes} bytes downloaded{#if $session.status.total_bytes !== null} of {$session.status.total_bytes}{:else}<DebugNote inline>Total size unavailable</DebugNote>{/if}</p>
{/if}
<button type="button" disabled={preview || $session.busy || active} onclick={() => session.check()}>Check Updates</button>
<button type="button" disabled={$session.busy} onclick={() => session.refresh()}>Retry Updates</button>
{#if $session.status?.offer && ['available', 'failed'].includes($session.status.phase) && !['shutdown_blocked', 'busy'].includes($session.status.failure ?? '')}
  <button type="button" disabled={preview || $session.busy} onclick={() => session.download()}>Download</button>
{/if}
{#if $session.status?.phase === 'ready' || ($session.status?.phase === 'failed' && $session.status.offer && ['shutdown_blocked', 'busy'].includes($session.status.failure ?? ''))}
  <button type="button" disabled={preview || $session.busy} onclick={() => session.install()}>{$session.status?.phase === 'ready' ? 'Install' : 'Retry Install'}</button>
{/if}
{#if $session.status?.failure === 'install_failed' && canRestartCapture}
  <button type="button" disabled={preview || $session.busy} onclick={() => canRestartCapture && session.restartCapture()}>Restart Capture</button>
{/if}
{#if active || $session.status?.offer}
  <button type="button" disabled={preview || $session.busy || $session.status?.phase === 'installing'} onclick={() => session.cancel()}>Cancel</button>
{/if}
<style>
  p { margin: var(--space-1) 0; color: var(--text-secondary); line-height: var(--leading-label); font-variant-numeric: tabular-nums; }
  p[role="alert"] { color: var(--danger); }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; font: inherit; }
  button { min-height: var(--target-min); margin: var(--space-1) var(--space-1) 0 0; padding: var(--space-1) var(--space-2); font-size: var(--text-label); }
  summary { min-height: var(--target-min); align-content: center; cursor: pointer; color: var(--violet-text); }
  summary:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
</style>
