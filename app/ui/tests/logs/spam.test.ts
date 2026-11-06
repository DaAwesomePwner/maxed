import { expect, it } from 'vitest';
import { isSpamAd, presentChat } from '../../src/lib/logs/spam';
import type { ChatRow } from '../../src/lib/bindings/ChatRow';

const row = (id: number, text: string, patch: Partial<ChatRow['message']> = {}): ChatRow => ({ hidden: false, message: {
  id: `synthetic:${id}`, session_id: 'synthetic-session', run_id: null, map: 'Synthetic hub', at: String(id * 1000),
  channel: 'public', direction: null, sender_kind: 'player', sender: 'SynthAlpha', counterpart: null,
  is_local: false, stars: null, supporter: false, text, truncated: false, ...patch,
} });

it.each(['Cheap gold here', 'Buy realm gold', 'synthgold-shop.com delivery', 'rotmg discount code SYNTH',
  'SYNTHETIC-GOLD.com delivery', 'ＣＨＥＡＰ ＧＯＬＤ', 'promo code SYNTH synthmarket.net',
  'Gold coupon code: SYNTH', 'buy at synthmarket.xyz', 'buy at realmeye.com.synthmarket.com',
  'buy at synthrealmeye.com', 'realmeye.com guide; cheap at synthmarket.com'])('recognizes synthetic ad pattern: %s', text => {
  expect(isSpamAd(text)).toBe(true);
});
it.each(['Ready for the next dungeon?', 'Trading a potion for a shield', 'See example.org for the guide',
  'Gold bag appeared', 'The realm gold color looks different', 'gg',
  'check my realmeye.com page', 'realmeye.com/player/SynthAlpha', 'price check on realmeye.com',
  'back to realm. gg', 'For sale: 4 def pots. GG', 'Selling life pots, cheap. gg',
  'Recruiting for o3, buy in discord.gg/synth', 'Shop discount today, skins 30% off',
  'promo event live, save your keys', 'Shop guide on realmeye.com', 'buy guide on rotmg.wiki.gg',
  'buy guide on realmofmadgod.fandom.com', 'shop on www.realmeye.com',
  'synthgold-shop.com', 'synthrotmg.com', 'synthshop.com', 'Save 20% with discount code SYNTH',
  'promo event rotmg', 'delivery at synthmarket . com', 'delivery at synthmarket.invalid'])('keeps ordinary chat: %s', text => {
  expect(isSpamAd(text)).toBe(false);
});
it('hides repeated long public bodies across senders only within the same observed scope', () => {
  const text = 'Synthetic repeated announcement body with more than forty characters';
  const input = [row(1, text), row(2, text, { sender: 'SynthBravo' }), row(3, text, { sender: 'SynthCharlie' }), row(4, text, { run_id: 'another-run' })];
  expect(presentChat(input, true)).toMatchObject({ hidden: 3, rows: [input[3]] });
  expect(presentChat(input, false).rows).toHaveLength(4);
  expect(input).toHaveLength(4);
});
it.each(['WTS exalted dex pot for 3 life pm me',
  'Synthetic guild recruiting players for friendly dungeon runs'])('collapses same-sender repeats: %s', text => {
  const input = [1, 2, 3].map(id => row(id, text));
  expect(presentChat(input, true)).toMatchObject({ rows: [input[0]], counts: { 'synthetic:1': 3 }, hidden: 0 });
});
it('keeps short thanks from three distinct players', () => {
  const input = ['SynthAlpha', 'SynthBravo', 'SynthCharlie'].map((sender, id) => row(id, 'thanks for the run everyone!', { sender }));
  expect(presentChat(input, true)).toMatchObject({ rows: input, hidden: 0 });
});
it('counts distinct senders rather than occurrences or capitalization', () => {
  const text = 'Synthetic guild recruiting players for friendly dungeon runs';
  const input = ['SynthAlpha', 'SynthBravo', 'synthalpha', 'SynthBravo'].map((sender, id) => row(id, text, { sender }));
  expect(presentChat(input, true)).toMatchObject({ rows: input, hidden: 0 });
});
it.each(['guild', 'party', 'whisper'] as const)('keeps ad-like bodies in %s chat', channel => {
  const input = [1, 2, 3].map(id => row(id, 'Cheap rotmg gold at synthgold-shop.com delivery', { channel }));
  expect(presentChat(input, true)).toMatchObject({ rows: [input[0]], counts: { 'synthetic:1': 3 }, hidden: 0 });
});
it('hides repeated ads even when sent by one player and restores the collapsed group', () => {
  const input = [1, 2, 3].map(id => row(id, 'Cheap gold'));
  expect(presentChat(input, true)).toMatchObject({ rows: [], hidden: 3 });
  expect(presentChat(input, false)).toMatchObject({ rows: [input[0]], counts: { 'synthetic:1': 3 }, hidden: 0 });
});
it('does not hide NPC, local, short repeated callouts or repeated private conversation', () => {
  const input = [row(1, 'Cheap gold', { sender_kind: 'npc' }), row(2, 'Cheap gold', { is_local: true }),
    ...[3, 4, 5].map(id => row(id, 'ready')), ...[6, 7, 8].map(id => row(id, 'Synthetic repeated conversation body', { channel: 'guild' }))];
  const result = presentChat(input, true);
  expect(result.hidden).toBe(0);
  expect(result.counts['synthetic:3']).toBe(3);
  expect(result.counts['synthetic:6']).toBe(3);
});
it('collapses consecutive identical messages without crossing sender, place or channel boundaries', () => {
  const input = [row(1, 'ready'), row(2, 'ready'), row(3, 'ready', { sender: 'SynthBravo' }),
    row(4, 'ready', { channel: 'guild' }), row(5, 'ready', { map: 'Synthetic dungeon' }), row(6, 'ready')];
  expect(presentChat(input, false)).toEqual({ rows: [input[0], ...input.slice(2)],
    counts: { 'synthetic:1': 2, 'synthetic:3': 1, 'synthetic:4': 1, 'synthetic:5': 1, 'synthetic:6': 1 },
    representatives: { 'synthetic:1': 'synthetic:1', 'synthetic:2': 'synthetic:1', 'synthetic:3': 'synthetic:3',
      'synthetic:4': 'synthetic:4', 'synthetic:5': 'synthetic:5', 'synthetic:6': 'synthetic:6' }, hidden: 0 });
});
it('does not join rows across a filtered message or distinct truncation evidence', () => {
  const input = [row(1, 'ready'), row(2, 'Cheap gold'), row(3, 'ready'), row(4, 'ready', { truncated: true })];
  expect(presentChat(input, true).counts).toEqual({ 'synthetic:1': 1, 'synthetic:3': 1, 'synthetic:4': 1 });
});
