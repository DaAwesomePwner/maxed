<script lang="ts">
  import type { Client } from '../bridge/client';
  import type { StatsExportRequest } from '../bindings/StatsExportRequest';
  let { client, request, label = 'Export CSV' }: { client?: Client; request: StatsExportRequest; label?: string } = $props();
  let busy = $state(false), message = $state(''), failed = $state(false);
  async function save() {
    if (!client || busy) return;
    busy = true; message = ''; failed = false;
    try {
      const result = await client.exportStatsCsv($state.snapshot(request) as StatsExportRequest);
      // A cancelled dialog shows nothing.
      if (result.status === 'saved') message = `Saved ${result.rows.toLocaleString('en-US')} ${result.rows === 1 ? 'row' : 'rows'}`;
    } catch (error) {
      failed = true; message = error instanceof Error ? error.message : String(error);
    } finally { busy = false; }
  }
</script>

<span class="export">
  <button class="ctl" type="button" aria-disabled={!client || busy} onclick={save}>{busy ? 'Exporting…' : label}</button>
  {#if failed}<span role="alert">{message}</span>{:else}<span role="status">{message}</span>{/if}
</span>

<style>
  .export { display: inline-flex; align-items: center; gap: var(--space-2); }
  button:not([aria-disabled="true"]) { cursor: pointer; } button[aria-disabled="true"] { color: var(--text-muted); }
  [role="alert"] { color: var(--danger); }
</style>
