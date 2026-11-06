import type { ChatRow } from '../bindings/ChatRow';

const normalize = (text: string) => text.normalize('NFKC').toLowerCase().replace(/\s+/g, ' ').trim();

/** Conservative ad signals; ordinary links, trade requests and short callouts stay visible. */
export function isSpamAd(text: string): boolean {
  const body = normalize(text);
  const trustedHosts = ['realmeye.com', 'discord.gg', 'wiki.gg', 'fandom.com'];
  const hostPattern = /\b(?:[a-z0-9-]+\.)+(?:com|net|org|shop|io|xyz)\b/g;
  const domain = [...body.matchAll(hostPattern)]
    .some(([host]) => !trustedHosts.some(trusted => host === trusted || host.endsWith(`.${trusted}`)));
  const words = body.replace(hostPattern, ' ');
  const commerce = /\b(?:buy|cheap|sale|shop|discount|coupon|delivery|stock)\b/.test(words);
  return /\bcheap\s+(?:realm\s+)?gold\b/.test(body)
    || /\b(?:buy|sell|selling)\s+(?:realm|rotmg)\s+gold\b/.test(body)
    // Even gold/rotmg/shop-branded hosts need commerce language.
    || (domain && commerce)
    || (/\b(?:discount|promo|coupon)\s+code\s*[:=]?\s*[a-z0-9]{3,}\b/.test(body)
      && (domain || /\b(?:gold|rotmg)\b/.test(body)));
}

const scope = (row: ChatRow) => JSON.stringify([row.message.session_id, row.message.run_id,
  row.message.map, row.message.channel, row.message.direction, row.message.counterpart]);
const repeatKey = (row: ChatRow) => `${scope(row)}:${normalize(row.message.text)}`;
const eligible = (row: ChatRow) => row.message.channel === 'public'
  && row.message.sender_kind === 'player' && !row.message.is_local;

/** Page-local and reversible: no storage mutation, cross-page cache, or private chat logging. */
export function presentChat(input: ChatRow[], filterSpam: boolean): {
  rows: ChatRow[]; counts: Record<string, number>; representatives: Record<string, string>; hidden: number;
} {
  const repeats = new Map<string, Set<string>>();
  for (const row of input) {
    if (filterSpam && eligible(row) && normalize(row.message.text).length >= 40) {
      const key = repeatKey(row);
      const senders = repeats.get(key) ?? new Set<string>();
      senders.add(normalize(row.message.sender));
      repeats.set(key, senders);
    }
  }
  const rows: ChatRow[] = [], counts: Record<string, number> = {};
  const representatives: Record<string, string> = {};
  let hidden = 0, previous: ChatRow | null = null;
  for (const row of input) {
    if (filterSpam && eligible(row) && (isSpamAd(row.message.text) || (repeats.get(repeatKey(row))?.size ?? 0) >= 3)) {
      hidden++; previous = null; continue;
    }
    if (previous && scope(previous) === scope(row) && previous.message.sender === row.message.sender
      && previous.message.sender_kind === row.message.sender_kind && previous.message.is_local === row.message.is_local
      && previous.message.text === row.message.text && previous.message.truncated === row.message.truncated
      && previous.hidden === row.hidden) {
      counts[previous.message.id]++;
    } else {
      rows.push(row); counts[row.message.id] = 1; previous = row;
    }
    representatives[row.message.id] = previous.message.id;
  }
  return { rows, counts, representatives, hidden };
}
