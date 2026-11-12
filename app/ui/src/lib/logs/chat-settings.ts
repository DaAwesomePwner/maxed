import type { Client } from '../bridge/client';
import type { ChatSettings } from '../bindings/ChatSettings';
import { validHiddenTerm } from './validation';

/** Shared by the chat sender panel and whisper conversations. */
export async function hideChatSender(client: Client, sender: string) {
  if (!validHiddenTerm(sender, 32)) throw new Error("Couldn't hide sender. Use 1–32 UTF-8 bytes with no control characters.");
  const settings = await client.querySettings();
  const next = { ...settings.chat, hidden_senders: [...new Set([...settings.chat.hidden_senders, sender])] };
  await client.updateChatSettings(next);
  return next;
}

export const SETTINGS_UNAVAILABLE = "Couldn't load chat settings";

/**
 * Whether spam is filtered for display. While the read is still loading the
 * historic default applies; once it has failed the preference is unknown, so
 * nothing is hidden on the user's behalf (`null`).
 */
export function spamFilterFor(settings: ChatSettings | null, unavailable: boolean): boolean | null {
  return settings ? settings.filter_spam : unavailable ? null : true;
}
