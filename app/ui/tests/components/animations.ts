import { onTestFinished, vi } from 'vitest';

/** jsdom has no Web Animations API. Keep Svelte's real transition lifecycle;
 * tests explicitly finish the browser animations instead of mocking Svelte. */
export function mockAnimations() {
  const original = Object.getOwnPropertyDescriptor(Element.prototype, 'animate');
  const animations: Array<{
    element: Element; frames: Keyframe[]; duration: number;
    animation: { currentTime: number; onfinish: (() => void) | null; cancel: () => void; finish: () => void };
  }> = [];
  Object.defineProperty(Element.prototype, 'animate', { configurable: true, value: vi.fn(function (
    this: Element, frames: Keyframe[], options: KeyframeAnimationOptions,
  ) {
    const duration = Number(options.duration ?? 0);
    const animation = {
      currentTime: 0, onfinish: null as (() => void) | null, cancel: vi.fn(),
      finish() { animation.currentTime = duration; animation.onfinish?.(); },
    };
    animations.push({ element: this, frames, duration, animation });
    return animation;
  }) });
  onTestFinished(() => {
    if (original) Object.defineProperty(Element.prototype, 'animate', original);
    else Reflect.deleteProperty(Element.prototype, 'animate');
  });
  return animations;
}
