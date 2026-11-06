import { writable } from 'svelte/store';

export const RECAP_BANNER_MS = 15_000;
export interface RecapPresentationState { banner: boolean; open: boolean }

/** Session-owned UI state; only mounted, visible Home time consumes the banner. */
export function createRecapPresentation(now = () => performance.now()) {
  const state = writable<RecapPresentationState>({ banner: false, open: false });
  let key: string | null = null, banner = false, open = false;
  let active = false, hovered = false, focused = false, remaining = RECAP_BANNER_MS;
  let started: number | null = null, timer: ReturnType<typeof setTimeout> | undefined;
  const publish = () => state.set({ banner, open });
  function pause() {
    clearTimeout(timer); timer = undefined;
    if (started !== null) remaining = Math.max(0, remaining - (now() - started));
    started = null;
  }
  function schedule() {
    pause();
    if (!banner || !active || hovered || focused || open) return;
    started = now();
    timer = setTimeout(hide, remaining);
  }
  function hide() { pause(); banner = false; publish(); }
  return {
    subscribe: state.subscribe,
    remember(next: string) {
      if (key === next) return;
      pause(); key = next; remaining = RECAP_BANNER_MS; banner = true; open = false;
      publish(); schedule();
    },
    hide,
    open() { if (key !== null) { hide(); open = true; publish(); } },
    back() { open = false; publish(); },
    setActive(value: boolean) { active = value; schedule(); },
    setHovered(value: boolean) { hovered = value; schedule(); },
    setFocused(value: boolean) { focused = value; schedule(); },
    reset() { pause(); key = null; banner = false; open = false; hovered = false; focused = false; publish(); },
    dispose() { pause(); },
  };
}
