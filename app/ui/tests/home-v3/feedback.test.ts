import { tick } from 'svelte';
import { afterEach, expect, it, vi } from 'vitest';
import { arrivalFeedback, type FeedbackEvent } from '../../src/lib/home/v3/feedback';
afterEach(() => vi.unstubAllGlobals());

function fixture(reduced = false) {
  const listeners = new Set<() => void>();
  const media = { matches: reduced, addEventListener: (_: string, fn: () => void) => listeners.add(fn),
    removeEventListener: (_: string, fn: () => void) => listeners.delete(fn) };
  vi.stubGlobal('matchMedia', () => media);
  const node = document.createElement('div');
  const cancel = vi.fn(), animate = vi.fn((_frames: Keyframe[] | PropertyIndexedKeyframes | null, _options?: number | KeyframeAnimationOptions) => ({ cancel, onfinish: null }) as unknown as Animation);
  const marker = document.createElement('span'); node.append(marker);
  node.animate = animate; marker.animate = animate;
  const event = (id: string): FeedbackEvent => ({ id, targets: () => [
    { node, effect: 'glow', color: '#a77bff' }, { node: marker, effect: 'pulse' },
  ] });
  return { node, cancel, animate, event, media, listeners };
}
it('seeds existing IDs, glows in tier colour and pulses once for each new event at 400ms', async () => {
  const f = fixture(); const action = arrivalFeedback(f.node, { scope: 'run-a', events: [f.event('old')] });
  expect(f.animate).not.toHaveBeenCalled();
  action.update({ scope: 'run-a', events: [f.event('old'), f.event('bag:new')] }); await tick();
  expect(f.animate).toHaveBeenCalledTimes(2);
  expect(f.animate.mock.calls[0]).toEqual([
    [{ boxShadow: '0 0 0 transparent' }, { boxShadow: '0 0 14px #a77bff', offset: .35 }, { boxShadow: '0 0 0 transparent' }],
    { duration: 400, easing: 'ease-out', iterations: 1 },
  ]);
  for (let i = 0; i < 10; i++) action.update({ scope: 'run-a', events: [f.event('old'), f.event('bag:new')] });
  await tick(); expect(f.animate).toHaveBeenCalledTimes(2);
  action.update({ scope: 'run-b', events: [f.event('other-existing')] }); await tick();
  expect(f.cancel).toHaveBeenCalledTimes(2);
  expect(f.animate).toHaveBeenCalledTimes(2);
  action.destroy(); expect(f.listeners.size).toBe(0);
});
it('reduced motion consumes arrivals silently; toggling it on cancels active effects', async () => {
  const f = fixture(true); const action = arrivalFeedback(f.node, { scope: 'run', events: [] });
  action.update({ scope: 'run', events: [f.event('silent')] }); await tick();
  expect(f.animate).not.toHaveBeenCalled();
  f.media.matches = false;
  action.update({ scope: 'run', events: [f.event('silent'), f.event('new')] }); await tick();
  expect(f.animate).toHaveBeenCalledTimes(2);
  f.media.matches = true; f.listeners.forEach(fn => fn());
  expect(f.cancel).toHaveBeenCalledTimes(2);
  action.destroy();
});
it('does not replay hidden rows later or animate after unmount', async () => {
  const f = fixture(); const action = arrivalFeedback(f.node, { scope: 'run', events: [] });
  action.update({ scope: 'run', events: [{ id: 'hidden', targets: () => [] }] }); await tick();
  action.update({ scope: 'run', events: [f.event('hidden')] }); await tick();
  expect(f.animate).not.toHaveBeenCalled();
  action.update({ scope: 'run', events: [f.event('last')] }); action.destroy(); await tick();
  expect(f.animate).not.toHaveBeenCalled();
});

it('keeps retained hidden rows silent across a thousand rolling observation windows and remounts', async () => {
  const f = fixture(); const hidden = { id: 'hidden', targets: () => [] };
  const action = arrivalFeedback(f.node, { scope: 'run', events: [hidden] });
  for (let i = 0; i < 1000; i++) {
    action.update({ scope: 'run', events: [hidden, { id: `arrival-${i}`, targets: () => [] }] });
    await tick();
  }
  action.update({ scope: 'run', events: [f.event('hidden')] }); await tick();
  expect(f.animate).not.toHaveBeenCalled();
  action.destroy();
  const remount = arrivalFeedback(f.node, { scope: 'run', events: [f.event('hidden')] });
  remount.update({ scope: 'run', events: [f.event('hidden')] }); await tick();
  expect(f.animate).not.toHaveBeenCalled(); remount.destroy();
});
