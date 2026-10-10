import { expect, it } from 'vitest';
import { createLogsPreview } from '../preview/logs-scenario';
import { defaultAlertFilter } from '../../src/lib/alerts/filters';
import { tileFilter } from '../../src/lib/alerts/alerts-screen';
it.each(['default', 'stress'] as const)('computes %s summaries and filters synthetic alerts before paging', async variant => {
  const preview = createLogsPreview(variant);
  try {
    const client = preview.client, summary = await client.getAlertSummary({ range: null });
    if (summary.status !== 'ready') throw new Error('Expected summary');
    expect(summary.value.total).toBe(variant === 'stress' ? 400 : 120);
    expect(summary.value.by_kind.reduce((sum, row) => sum + row.count, 0)).toBe(summary.value.total);
    const filter = { ...defaultAlertFilter(), ...tileFilter('messages'), exclude_kinds: ['party_message' as const] };
    let count = 0;
    do {
      const reply = await client.listAlerts(filter); if (reply.status !== 'ready') throw new Error('Expected page');
      expect(reply.value.rows.every(row => row.kind === 'whisper' || row.kind === 'guild_message')).toBe(true);
      if (reply.value.next) expect(reply.value.rows).toHaveLength(50);
      count += reply.value.rows.length; filter.cursor = reply.value.next;
    } while (filter.cursor);
    expect(count).toBe(summary.value.by_kind.filter(row => row.kind === 'whisper' || row.kind === 'guild_message').reduce((sum, row) => sum + row.count, 0));
    expect(await client.getAlertSummary({ range: { start: '0', end: '1' } })).toEqual({ status: 'ready', value: { total: 0, by_kind: summary.value.by_kind.map(row => ({ ...row, count: 0 })), last_white: null, first_at: null } });
  } finally { preview.dispose(); }
});
