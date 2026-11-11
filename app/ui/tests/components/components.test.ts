import { fireEvent, render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { setDebugMode } from '../../src/lib/debug/debug';
afterEach(() => setDebugMode(false));
import Badge from '../../src/lib/components/Badge.svelte';
import EmptyState from '../../src/lib/components/EmptyState.svelte';
import SpriteImage from '../../src/lib/components/SpriteImage.svelte';
import type { SpriteRef } from '../../src/lib/bindings/SpriteRef';
import ContentHarness from './fixtures/ContentHarness.svelte';
import BannerHost from './fixtures/BannerHost.svelte';

describe('Card, EmptyState and ListRow', () => {
  it('uses unique labelled card sections and snippet content', () => {
    render(ContentHarness);
    const regions = screen.getAllByRole('region', { name: 'Session' });
    expect(regions).toHaveLength(2);
    expect(regions[0].getAttribute('aria-labelledby')).not.toBe(regions[1].getAttribute('aria-labelledby'));
    expect(screen.getAllByRole('heading', { name: 'Session', level: 2 })).toHaveLength(2);
    expect(screen.getByText('Observed session')).toBeTruthy();
  });
  it('shows a working retry action with no error spinner', async () => {
    const retry = vi.fn();
    render(ContentHarness, { retry });
    expect(screen.getByRole('region', { name: 'Capture unavailable' }).getAttribute('aria-busy')).toBe('false');
    await userEvent.setup().click(screen.getByRole('button', { name: 'Retry capture' }));
    expect(retry).toHaveBeenCalledOnce();
  });
  it('can leave loading for unavailable without an endless busy state', async () => {
    const { rerender } = render(EmptyState, { heading: 'Loading runs', description: 'Reading saved runs.', state: 'loading' });
    expect(screen.getByRole('region').getAttribute('aria-busy')).toBe('true');
    expect(screen.queryByText('Reading saved runs.')).toBeNull();
    await rerender({ heading: 'Runs unavailable', description: 'Try reopening the run history.', state: 'unavailable' });
    expect(screen.getByRole('region').getAttribute('aria-busy')).toBe('false');
  });
  it('preserves ordinary link behavior and keyboard focus without nesting actions', async () => {
    render(ContentHarness);
    const link = screen.getByRole('link', { name: 'The Shatters 11:04 · cleared' });
    expect(link.getAttribute('href')).toBe('/runs/1');
    expect(link.getAttribute('aria-current')).toBe('page');
    expect(link.querySelectorAll('button, a, [tabindex]')).toHaveLength(0);
    const user = userEvent.setup();
    await user.tab();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Retry capture' }));
    await user.tab();
    expect(document.activeElement).toBe(link);
    // Observe after the component's handlers, then suppress jsdom's unsupported navigation.
    const preventNavigation = vi.fn((event: MouseEvent) => {
      const componentPreventedDefault = event.defaultPrevented;
      event.preventDefault();
      return componentPreventedDefault;
    });
    window.addEventListener('click', preventNavigation, { once: true });
    const click = new MouseEvent('click', { bubbles: true, cancelable: true, ctrlKey: true });
    try {
      link.dispatchEvent(click);
    } finally {
      window.removeEventListener('click', preventNavigation);
    }
    expect(preventNavigation).toHaveBeenCalledOnce();
    expect(preventNavigation).toHaveReturnedWith(false);
    expect(click.defaultPrevented).toBe(true);
  });
});

describe('Badge and Tooltip evidence', () => {
  const estimated = { reason: 'Maximum players observed during this partial capture.' };
  it('keeps estimate markers and explanations out of production', () => {
    const { container } = render(Badge, { label: 'Party', tone: 'estimated', estimated });
    expect(screen.getByText('Party')).toBeTruthy();
    expect(container.textContent).not.toMatch(/estimated|partial|observed/i);
    expect(container.querySelector('[aria-describedby], [role="tooltip"]')).toBeNull();
  });
  it('explains estimates on keyboard focus and dismisses with Escape', async () => {
    const user = userEvent.setup();
    setDebugMode(true);
    render(Badge, { label: 'Party size', tone: 'estimated', estimated });
    expect(screen.getByText('· Estimated')).toBeTruthy();
    expect(screen.queryByRole('tooltip')).toBeNull();
    await user.tab();
    const tooltip = screen.getByRole('tooltip');
    expect(tooltip.textContent).toContain('Maximum players observed');
    expect(document.activeElement?.getAttribute('aria-describedby')).toBe(tooltip.id);
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('tooltip')).toBeNull();
  });
  it('keeps pointer-accessible explanations hoverable and independent', async () => {
    const user = userEvent.setup();
    setDebugMode(true);
    render(Badge, { label: 'First', tone: 'estimated', estimated });
    render(Badge, { label: 'Second', tone: 'estimated', estimated: { reason: 'A different observation.' } });
    const first = screen.getByRole('button', { name: 'First · Estimated' });
    await user.hover(first);
    const tooltip = screen.getByRole('tooltip');
    await user.hover(tooltip);
    expect(screen.getByRole('tooltip')).toBe(tooltip);
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('tooltip')).toBeNull();
    await user.hover(screen.getByRole('button', { name: 'Second · Estimated' }));
    expect(screen.getByRole('tooltip').textContent).toBe('A different observation.');
    expect(screen.getByRole('tooltip').id).not.toBe(first.getAttribute('aria-describedby'));
    await user.unhover(screen.getByRole('button', { name: 'Second · Estimated' }));
    expect(screen.queryByRole('tooltip')).toBeNull();
  });
  it('supports the standalone trigger snippet and closes on focus loss', async () => {
    render(ContentHarness);
    const button = screen.getByRole('button', { name: 'Evidence' });
    await fireEvent.focusIn(button);
    expect(screen.getByRole('tooltip').id).toBe('direct-tip');
    await fireEvent.focusOut(button);
    expect(screen.queryByRole('tooltip')).toBeNull();
  });
  it('keeps a tooltip inside the right edge and removes placement listeners on close', async () => {
    const remove = vi.spyOn(window, 'removeEventListener');
    const { container } = render(ContentHarness);
    const position = container.querySelector('.tooltip-position')!;
    const viewportWidth = window.innerWidth;
    vi.spyOn(position, 'getBoundingClientRect').mockReturnValue({ x: viewportWidth - 20, y: 40, left: viewportWidth - 20, top: 40, right: viewportWidth + 180, bottom: 100, width: 200, height: 60, toJSON: () => ({}) });
    await fireEvent.focusIn(screen.getByRole('button', { name: 'Evidence' }));
    expect((position as HTMLElement).style.transform).toBe('translateX(-188px)');
    await fireEvent.focusOut(screen.getByRole('button', { name: 'Evidence' }));
    expect(remove).toHaveBeenCalledWith('resize', expect.any(Function));
    expect(remove).toHaveBeenCalledWith('scroll', expect.any(Function), true);
  });
  it('rejects an empty explanation and an estimated tone without evidence', () => {
    expect(() => render(Badge, { label: 'Party', tone: 'estimated', estimated: { reason: '  ' } })).toThrow(/nonempty evidence/);
    expect(() => render(Badge, { label: 'Party', tone: 'estimated' })).toThrow(/nonempty evidence/);
  });
});

describe('SpriteImage', () => {
  const sprite: SpriteRef = { key: 'synthetic', sheet: 'atlas.png', x: 8, y: 16, width: 8, height: 8 };
  it('keeps a name and fixed geometry on load failure', async () => {
    const { container } = render(SpriteImage, { sprite, sheetUrl: '/atlas.png', alt: 'Synthetic wizard' });
    const frame = screen.getByRole('img', { name: 'Synthetic wizard' });
    expect(frame.style.width).toBe('32px');
    expect(frame.style.height).toBe('32px');
    const image = container.querySelector('img')!;
    expect(image.style.transform).toBe('translate(0px, 0px) scale(4)');
    await fireEvent.error(image);
    expect(frame.style.width).toBe('32px');
    expect(frame.style.height).toBe('32px');
    expect(frame.dataset.fallback).toBe('true');
    expect(container.querySelector('img')).toBeNull();
  });
  it('rejects invalid and wrong tile dimensions and recovers for a new source', async () => {
    const { container, rerender } = render(SpriteImage, { sprite: { ...sprite, x: -1 }, sheetUrl: '/atlas.png', alt: 'Sprite', scale: 1.5 });
    expect(container.querySelector('img')).toBeNull();
    expect(screen.getByRole('img').style.width).toBe('32px');
    await rerender({ sprite, sheetUrl: '/atlas.png', alt: 'Sprite', scale: 2 });
    const image = container.querySelector('img')!;
    Object.defineProperties(image, { naturalWidth: { value: 7 }, naturalHeight: { value: 8 } });
    await fireEvent.load(image);
    expect(container.querySelector('img')).toBeNull();
    await rerender({ sprite, sheetUrl: '/new-atlas.png', alt: 'Sprite', scale: 2 });
    expect(container.querySelector('img')).toBeTruthy();
    expect(screen.getByRole('img').style.width).toBe('16px');
  });
  it.each([null, 'https://remote.invalid/atlas.png', '//remote.invalid/atlas.png', 'data:image/png;base64,AA==', '/\\remote.invalid/atlas.png'])('keeps missing or nonlocal sheet %s unavailable', (sheetUrl) => {
    const { container } = render(SpriteImage, { sprite, sheetUrl, alt: 'Missing sprite' });
    expect(screen.getByRole('img', { name: 'Missing sprite' }).dataset.fallback).toBe('true');
    expect(container.querySelector('img')).toBeNull();
  });
  it('shows null sprite as a labelled fallback', () => {
    render(SpriteImage, { sprite: null, sheetUrl: '/atlas.png', alt: 'Unknown class' });
    expect(screen.getByRole('img', { name: 'Unknown class' }).dataset.fallback).toBe('true');
  });
  it('accepts a valid tile and rejects extreme geometry without unsafe styles', async () => {
    const { container, rerender } = render(SpriteImage, { sprite, sheetUrl: '/atlas.png', alt: 'Sprite', scale: 3 });
    const image = container.querySelector('img')!;
    Object.defineProperties(image, { naturalWidth: { value: 8 }, naturalHeight: { value: 8 } });
    await fireEvent.load(image);
    expect(screen.getByRole('img').dataset.fallback).toBe('false');
    expect(screen.getByRole('img').style.width).toBe('24px');
    await rerender({ sprite: { ...sprite, width: Infinity }, sheetUrl: '/atlas.png', alt: 'Sprite', scale: Infinity });
    expect(container.querySelector('img')).toBeNull();
    expect(container.innerHTML).not.toMatch(/NaN|Infinity/);
  });
});

describe('Banner', () => {
  it('keeps the same empty live region mounted before and between moments', async () => {
    const { container, rerender } = render(BannerHost, { visible: false });
    const status = screen.getByRole('status');
    expect(status.textContent).toBe('');
    expect(container.querySelector('.banner')).toBeNull();
    await rerender({ visible: true });
    expect(screen.getAllByRole('status')).toHaveLength(1);
    expect(screen.getByRole('status')).toBe(status);
    expect(status.textContent).toContain('Synthetic drop');
    await rerender({ visible: false });
    expect(screen.getByRole('status')).toBe(status);
    expect(status.textContent).toBe('');
    expect(container.querySelector('.banner')).toBeNull();
    await rerender({ visible: true, id: 'event-2', title: 'After the gap' });
    expect(screen.getByRole('status')).toBe(status);
    expect(status.textContent).toContain('After the gap');
  });
  it.each(['white_bag', 'personal_best', 'flawless_run', 'death'] as const)('presents %s politely without stealing focus and dismisses', async (kind) => {
    const onDismiss = vi.fn();
    // "context" is also a Svelte mount option, so disambiguate component props.
    const { container } = render(BannerHost, { props: { kind, onDismiss, context: 'Long synthetic dungeon context '.repeat(12) } });
    expect(screen.getByRole('status').getAttribute('aria-live')).toBe('polite');
    expect(screen.getByRole('status').textContent).toContain('Synthetic drop');
    expect(container.querySelector('[data-kind]')?.getAttribute('data-kind')).toBe(kind);
    expect(document.activeElement).toBe(document.body);
    await userEvent.setup().click(screen.getByRole('button', { name: /Dismiss/ }));
    expect(onDismiss).toHaveBeenCalledOnce();
  });
  it('announces once per id across updates and host remounts', async () => {
    const { rerender } = render(BannerHost);
    const original = screen.getByRole('status').textContent;
    await rerender({ id: 'event-1', title: 'Updated presentation' });
    expect(screen.getByRole('status').textContent).toBe(original);
    await rerender({ visible: false });
    await rerender({ visible: true, id: 'event-1' });
    expect(screen.getByRole('status').textContent).toBe('');
    await rerender({ id: 'event-2', title: 'Another event' });
    expect(screen.getByRole('status').textContent).toContain('Another event');
  });
  it('clears stale announcement text when an already announced ID replaces another event', async () => {
    const { container, rerender } = render(BannerHost);
    const status = screen.getByRole('status');
    expect(status.textContent).toContain('Synthetic drop');
    await rerender({ id: 'event-2', title: 'Another event' });
    expect(status.textContent).toContain('Another event');
    await rerender({ id: 'event-1', title: 'Previously announced event' });
    expect(screen.getByRole('status')).toBe(status);
    expect(status.textContent).toBe('');
    expect(screen.getByRole('heading', { name: 'Previously announced event' })).toBeTruthy();
    expect(container.querySelector('.banner')?.classList.contains('celebrate')).toBe(false);
  });
  it('pauses decorative work when hidden and removes its visibility listener', async () => {
    const remove = vi.spyOn(document, 'removeEventListener');
    const { container, unmount } = render(BannerHost);
    vi.spyOn(document, 'hidden', 'get').mockReturnValue(true);
    await fireEvent(document, new Event('visibilitychange'));
    expect(container.querySelector('.banner')?.classList.contains('paused')).toBe(true);
    unmount();
    expect(remove).toHaveBeenCalledWith('visibilitychange', expect.any(Function));
  });
});
