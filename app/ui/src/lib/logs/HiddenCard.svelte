<script lang="ts">
  import { settingsHref } from '../navigation/routes';
  import type { ChatSettings } from '../bindings/ChatSettings';
  let { settings, hidden }: { settings: ChatSettings | null; hidden: number | null } = $props();
</script>
<section class="card"><header><h3>Hidden from chat</h3><a href={settingsHref('data')}>Manage ›</a></header>
  <dl><dt>Senders</dt><dd>{settings?.hidden_senders.length ?? '—'}</dd><dt>Keywords</dt><dd>{settings?.hidden_keywords.length ?? '—'}</dd><dt>Lines hidden on this page</dt><dd>{hidden ?? '—'}</dd></dl>
  <p>Hidden lines stay stored. While <b>Hidden</b> is on they show dimmed; turn it off to remove them from the list.</p>
</section>
<style>
  .card { min-width: 0; padding: var(--space-4); border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); font-size: var(--text-caption); line-height: var(--leading-caption); }
  header { display: flex; align-items: baseline; justify-content: space-between; gap: var(--space-2); margin-bottom: var(--space-3); } h3 { margin: 0; font-size: var(--text-label); line-height: var(--leading-label); font-weight: var(--weight-medium); }
  header a { color: var(--text-secondary); text-decoration: none; } header a:hover, header a:focus-visible { color: var(--text-primary); text-decoration: underline; text-underline-offset: 3px; }
  dl { display: grid; grid-template-columns: 1fr auto; gap: var(--space-2); margin: 0; } dd { margin: 0; font-variant-numeric: tabular-nums; } dt, p { color: var(--text-muted); } b { font-weight: var(--weight-medium); color: var(--text-secondary); } p { margin: var(--space-3) 0 0; }
</style>
