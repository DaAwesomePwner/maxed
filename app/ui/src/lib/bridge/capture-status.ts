import type { CaptureStatus } from '../bindings/CaptureStatus';

// Subscribe before querying. A delayed initial snapshot cannot replace newer events.
export function subscribeCaptureStatus(
  listen: (receive: (status: CaptureStatus) => void) => Promise<() => void>,
  query: () => Promise<CaptureStatus>,
  receive: (status: CaptureStatus) => void,
  failed: () => void,
): () => void {
  let disposed = false;
  let revision = 0;
  let unlisten: (() => void) | undefined;
  void (async () => {
    try {
      const release = await listen((status) => {
        if (!disposed) {
          revision++;
          receive(status);
        }
      });
      if (disposed) {
        release();
        return;
      }
      unlisten = release;
      const before = revision;
      const initial = await query();
      if (!disposed && before === revision) receive(initial);
    } catch {
      if (!disposed) failed();
    }
  })();
  return () => {
    if (disposed) return;
    disposed = true;
    unlisten?.();
  };
}
