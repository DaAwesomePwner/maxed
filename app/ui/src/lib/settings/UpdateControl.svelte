<script lang="ts">
  import DebugNote from '../debug/DebugNote.svelte';
  import type { Client } from '../bridge/client';
  import type { UpdateChannel } from '../bindings/UpdateChannel';
  import type { CaptureStatus } from '../bindings/CaptureStatus';
  import type { UpdateFailure } from '../bindings/UpdateFailure';
  import { createUpdateSession } from './updates';
  import { titleCase } from '../styles/format';
  import { errorLine } from '../setup/presentation';
  let { client, preview = false, channel, capture = null }: { client: Client; preview?: boolean; channel?: UpdateChannel; capture?: CaptureStatus | null } = $props();
  // Checks use the saved channel; draft changes take effect after Settings Apply.
  let session = $derived(createUpdateSession(client, preview));
  $effect(() => { channel; void session.refresh(); });
  $effect(() => { const current = session; return () => current.dispose(); });
  const failures: Record<UpdateFailure, string> = {
    offline: 'Connection failed',
    unavailable: 'Updates unsupported',
    invalid_metadata: 'Invalid metadata', invalid_signature: 'Invalid signature',
    unsupported_target: 'Unsupported update', too_large: 'Update too large',
    busy: 'Update running', stale_offer: 'Check again',
    shutdown_blocked: 'Check storage, then retry install',
    install_failed: 'Install failed; check again',
  };
  let canRestartCapture = $derived(!!capture && !['Live', 'WaitingForGame', 'Recovering'].includes(capture.state));
  let active = $derived(!!$session.status && ['checking', 'downloading', 'installing'].includes($session.status.phase));
</script>
<p>Current channel: {channel ? titleCase(channel) : '—'}</p>
<p role="status">Update: {$session.status ? titleCase($session.status.phase) : ($session.error ? 'Unavailable' : 'Loading')}{#if $session.error && $session.status}<DebugNote inline>Last known; stale</DebugNote>{/if}</p>
{#if $session.error}<p role="alert">{errorLine($session.error)}</p>{/if}
{#if $session.status?.failure}<p role="alert">{failures[$session.status.failure]}</p>{/if}
{#if $session.status?.offer}
  <p>Version: {$session.status.offer.version} ({$session.status.offer.channel})</p>
  {#if $session.status.offer.notes}<details><summary>Release notes</summary><pre>{$session.status.offer.notes}</pre></details>{/if}
{/if}
{#if $session.status?.phase === 'downloading'}
  <p>{$session.status.downloaded_bytes} bytes downloaded{#if $session.status.total_bytes !== null} of {$session.status.total_bytes}{:else}<DebugNote inline>Total size unavailable</DebugNote>{/if}</p>
{/if}
<button class="ctl" type="button" disabled={preview || $session.busy || active} onclick={() => session.check()}>Check updates</button>
<button class="ctl" type="button" disabled={$session.busy} onclick={() => session.refresh()}>Retry updates</button>
{#if $session.status?.offer && ['available', 'failed'].includes($session.status.phase) && !['shutdown_blocked', 'busy'].includes($session.status.failure ?? '')}
  <button class="ctl" type="button" disabled={preview || $session.busy} onclick={() => session.download()}>Download</button>
{/if}
{#if $session.status?.phase === 'ready' || ($session.status?.phase === 'failed' && $session.status.offer && ['shutdown_blocked', 'busy'].includes($session.status.failure ?? ''))}
  <button class="ctl" type="button" disabled={preview || $session.busy} onclick={() => session.install()}>{$session.status?.phase === 'ready' ? 'Install' : 'Retry install'}</button>
{/if}
{#if $session.status?.failure === 'install_failed' && canRestartCapture}
  <button class="ctl" type="button" disabled={preview || $session.busy} onclick={() => canRestartCapture && session.restartCapture()}>Restart capture</button>
{/if}
{#if active || $session.status?.offer}
  <button class="ctl" type="button" disabled={preview || $session.busy || $session.status?.phase === 'installing'} onclick={() => session.cancel()}>Cancel</button>
{/if}
<style>
  p { margin: var(--space-1) 0; color: var(--text-secondary); line-height: var(--leading-label); font-variant-numeric: tabular-nums; }
  p[role="alert"] { color: var(--danger); }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; font: inherit; }
  button { margin: var(--space-1) var(--space-1) 0 0; }
  summary { min-height: var(--target-min); align-content: center; cursor: pointer; color: var(--text-secondary); }
  summary:hover { color: var(--text-primary); }
  summary:focus-visible { outline: 2px solid var(--focus-ring); outline-offset: 2px; }
</style>
