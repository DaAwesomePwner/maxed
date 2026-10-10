import type { EventEnvelope } from '../bindings/EventEnvelope';
import { caveat } from '../debug/debug';

/** A failed side effect must not interrupt ordered delivery or other observers. */
export function notifyEventObserver(event: EventEnvelope, receive: (event: EventEnvelope) => void) {
  try { receive(event); }
  catch (error) { caveat('shell', 'Live event observer failed', error); }
}
