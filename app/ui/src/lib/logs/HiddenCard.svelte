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
  .card { min-width: 0; padding: 10px 12px; background: var(--surface-panel); border: 1px solid var(--border); border-radius: 8px; font-size: 12px; } header { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  h3 { margin: 0; font-size: 11px; text-transform: uppercase; letter-spacing: .07em; color: var(--text-muted); } a { color: var(--violet-text); text-decoration: none; } dl { display: grid; grid-template-columns: 1fr auto; gap: 7px; } dd { margin: 0; } dt, p { color: var(--text-muted); } p { margin: 8px 0 0; line-height: 1.45; }
</style>
