import { tick } from 'svelte';
export type FeedbackTarget = { node: HTMLElement | null; effect: 'glow' | 'pulse' | 'slide'; color?: string; duration?: number };
export type FeedbackEvent = { id: string; targets: (root: HTMLElement) => FeedbackTarget[] };
type Input = { scope: string; events: FeedbackEvent[] };

export function arrivalFeedback(root: HTMLElement, initial: Input) {
  let scope = initial.scope, epoch = 0, disposed = false;
  let seen = new Set(initial.events.map(event => event.id));
  const active = new Set<Animation>();
  const media = globalThis.matchMedia?.('(prefers-reduced-motion: reduce)');
  const cancel = () => { active.forEach(animation => animation.cancel()); active.clear(); };
  const preference = () => { if (media?.matches) cancel(); };
  media?.addEventListener('change', preference);
  return {
    update(input: Input) {
      if (input.scope !== scope) {
        scope = input.scope; epoch++; cancel(); seen = new Set(input.events.map(event => event.id)); return;
      }
      const fresh = input.events.filter(event => !seen.has(event.id));
      fresh.forEach(event => seen.add(event.id));
      // Callers supply the complete bounded observation horizon, including
      // hidden rows. Keep those IDs silent until they leave that horizon.
      const retained = new Set(input.events.map(event => event.id));
      for (const id of seen) if (!retained.has(id)) seen.delete(id);
      const revision = epoch;
      void tick().then(() => {
        if (disposed || revision !== epoch || media?.matches) return;
        const animated = new Set<HTMLElement>();
        for (const event of fresh) for (const target of event.targets(root)) {
          if (!target.node || animated.has(target.node)) continue;
          animated.add(target.node);
          const frames: Keyframe[] = target.effect === 'glow'
            ? [{ boxShadow: '0 0 0 transparent' }, { boxShadow: `0 0 14px ${target.color ?? '#f2c14e'}`, offset: .35 }, { boxShadow: '0 0 0 transparent' }]
            : target.effect === 'slide' ? [{ transform: 'translateY(-12px)', opacity: 0 }, { transform: 'translateY(0)', opacity: 1 }]
            : [{ transform: 'scale(1)' }, { transform: 'scale(1.18)', offset: .35 }, { transform: 'scale(1)' }];
          const animation = target.node.animate?.(frames, { duration: target.duration ?? 400, easing: 'ease-out', iterations: 1 });
          if (animation) { active.add(animation); animation.onfinish = () => active.delete(animation); }
        }
      });
    },
    destroy() { disposed = true; epoch++; cancel(); media?.removeEventListener('change', preference); },
  };
}
