/** Reactive inputs and an owned effect scope for testing rune helpers directly. */
export function reactive<T extends object>(initial: T): T {
  const state = $state(initial);
  return state;
}

export function effectRoot(run: () => void): () => void {
  return $effect.root(run);
}
