// Native window state supplements Page Visibility (WebView2 may leave the
// document visible while minimized). Each owner releases its own subscription.
let nativeVisible = true;
const observers = new Set<() => void>();
export const isPresentationVisible = () => nativeVisible
  && (typeof document === 'undefined' || document.visibilityState !== 'hidden');
export function setNativePresentationVisible(visible: boolean) {
  if (visible === nativeVisible) return;
  nativeVisible = visible;
  for (const notify of observers) notify();
}
export function observePresentationVisibility(receive: (visible: boolean) => void) {
  const notify = () => receive(isPresentationVisible());
  observers.add(notify);
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', notify);
  notify();
  return () => {
    observers.delete(notify);
    if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', notify);
  };
}

/** Suspend serial reads without a polling timer; disposal releases waiters. */
export function createVisibilityPause() {
  let disposed = false;
  const pending = new Set<(active: boolean) => void>();
  const wake = (active: boolean) => { for (const resolve of pending) resolve(active); pending.clear(); };
  const release = observePresentationVisibility(visible => { if (visible) wake(true); });
  return {
    wait: () => disposed ? Promise.resolve(false) : isPresentationVisible() ? Promise.resolve(true)
      : new Promise<boolean>(resolve => pending.add(resolve)),
    dispose() { disposed = true; release(); wake(false); },
  };
}
