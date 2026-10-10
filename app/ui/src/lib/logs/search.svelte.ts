import { untrack } from 'svelte';

export function validLogsQuery(text: string): boolean {
  return [...text].length <= 128 && new TextEncoder().encode(text).length <= 512 && !text.includes('\u0000');
}

/** Keep in-progress input across unrelated filter changes, including paging. */
export function createLogsSearch(options: { query: () => string | undefined; apply: (text: string) => void }) {
  let query = $state(untrack(() => options.query() ?? ''));
  $effect(() => {
    const text = query;
    if (text.trim() === options.query()) return;
    const timer = setTimeout(() => untrack(() => options.apply(text)), 250);
    return () => clearTimeout(timer);
  });
  return {
    get query() { return query; }, set query(value: string) { query = value; },
    accept(next: string | undefined) {
      const text = (next ?? '').trim();
      if (next !== options.query() && text !== query.trim()) query = text;
      return text;
    },
  };
}
