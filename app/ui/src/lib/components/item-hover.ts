import type { ActionReturn } from 'svelte/action';
import { readonly, writable, type Readable } from 'svelte/store';
import type { EnchantView } from '../bindings/EnchantView';

export interface ItemHoverParams {
  type: number; enchant?: EnchantView | null; name?: string; tier?: string | null;
  suffix?: string; label?: string; note?: string;
}
export interface ItemHoverState { anchor: HTMLElement; params: ItemHoverParams; open: boolean }
export const ITEM_HOVER_ID = 'item-hover-card';
export const OPEN_DELAY_MS = 250;
export const WARM_MS = 300;
const state = writable<ItemHoverState | null>(null);
export const itemHoverState: Readable<ItemHoverState | null> = readonly(state);
const paramsByNode = new WeakMap<Element, ItemHoverParams>();
let current: ItemHoverState | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;
let warmUntil = -Infinity;

function describedBy(node: HTMLElement, open: boolean) {
  const tokens = (node.getAttribute('aria-describedby') ?? '').split(/\s+/).filter(token => token && token !== ITEM_HOVER_ID);
  if (open) tokens.push(ITEM_HOVER_ID);
  if (tokens.length) node.setAttribute('aria-describedby', tokens.join(' '));
  else node.removeAttribute('aria-describedby');
}
function publish(next: ItemHoverState | null) {
  if (current) describedBy(current.anchor, false);
  current = next;
  if (next?.open) describedBy(next.anchor, true);
  state.set(next);
}
export function closeItemHover(within?: Element): void {
  if (within && (!current || !within.contains(current.anchor))) return;
  clearTimeout(timer);
  timer = undefined;
  warmUntil = current?.open ? Date.now() + WARM_MS : -Infinity;
  publish(null);
}
export function itemHoverParams(node: Element): ItemHoverParams | undefined { return paramsByNode.get(node); }

export function openItemHover(anchor: HTMLElement, params: ItemHoverParams, immediate = false): void {
  clearTimeout(timer);
  timer = undefined;
  const open = immediate || !!current?.open || Date.now() < warmUntil;
  publish({ anchor, params, open });
  if (!open) timer = setTimeout(() => {
    timer = undefined;
    if (current?.anchor === anchor) publish({ ...current, open: true });
  }, OPEN_DELAY_MS);
}

export function itemHover(node: HTMLElement, params: ItemHoverParams): ActionReturn<ItemHoverParams> {
  function update(next: ItemHoverParams) {
    params = next;
    paramsByNode.set(node, params);
    node.dataset.itemHover = String(params.type);
    if (current?.anchor === node) publish({ ...current, params });
  }
  const pointerEnter = () => openItemHover(node, params);
  const focusIn = (event: FocusEvent) => {
    let keyboardFocus = false;
    try { keyboardFocus = event.target instanceof Element && event.target.matches(':focus-visible'); }
    catch { /* Unsupported selectors do not establish keyboard focus. */ }
    if (keyboardFocus) openItemHover(node, params, true);
  };
  const leave = () => { if (current?.anchor === node) closeItemHover(); };
  const focusOut = (event: FocusEvent) => {
    if (!(event.relatedTarget instanceof Node) || !node.contains(event.relatedTarget)) leave();
  };
  update(params);
  node.addEventListener('pointerenter', pointerEnter);
  node.addEventListener('pointerleave', leave);
  node.addEventListener('focusin', focusIn);
  node.addEventListener('focusout', focusOut);
  return {
    update,
    destroy() {
      leave();
      paramsByNode.delete(node);
      delete node.dataset.itemHover;
      node.removeEventListener('pointerenter', pointerEnter);
      node.removeEventListener('pointerleave', leave);
      node.removeEventListener('focusin', focusIn);
      node.removeEventListener('focusout', focusOut);
    },
  };
}
