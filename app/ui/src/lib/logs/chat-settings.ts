import type { Client } from '../bridge/client';
import { validHiddenTerm } from './validation';

/** Shared by the chat sender panel and whisper conversations. */
export async function hideChatSender(client: Client, sender: string) {
  if (!validHiddenTerm(sender, 32)) throw new Error('Could not hide sender. Use 1–32 UTF-8 bytes with no control characters.');
  const settings = await client.querySettings();
  const next = { ...settings.chat, hidden_senders: [...new Set([...settings.chat.hidden_senders, sender])] };
  await client.updateChatSettings(next);
  return next;
}
