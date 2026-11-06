import { characterVariants } from './character-scenario';

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
async function until(check: () => boolean, label: string) {
  const deadline = performance.now() + 10000;
  while (!check()) {
    assert(performance.now() < deadline, 'Timed out: ' + label);
    await new Promise(resolve => setTimeout(resolve, 50));
  }
}
function visible(element: HTMLElement) {
  const style = element.ownerDocument.defaultView!.getComputedStyle(element);
  return style.display !== 'none' && style.visibility !== 'hidden' && element.getClientRects().length > 0
    && !element.closest('.sr-only, [hidden], [role="tooltip"]');
}
function describe(element: HTMLElement) {
  return element.tagName + '.' + element.className + ' ' + element.textContent?.trim().slice(0, 100);
}
function geometry(doc: Document, width: number) {
  const win = doc.defaultView!;
  const main = doc.querySelector<HTMLElement>('main');
  const rail = doc.querySelector<HTMLElement>('aside[aria-label="Character"]');
  assert(main && rail, 'Missing real Shell');
  assert(win.innerWidth === width && win.innerHeight === 760, 'Wrong CSS viewport');
  for (const root of [doc.documentElement, main, rail]) {
    assert(root.clientWidth > 0, 'Unmeasurable layout');
    assert(root.scrollWidth <= root.clientWidth + 1, 'Overflow: ' + describe(root));
  }
  const cards = [...main.querySelectorAll<HTMLElement>('.character-screen > section')];
  assert(cards.length === 5, 'Missing Character sections');
  for (const [index, card] of cards.entries()) {
    assert(card.scrollHeight <= card.clientHeight + 1, 'Card overflows vertically: ' + describe(card));
    for (const sibling of cards.slice(index + 1)) {
      const a = card.getBoundingClientRect(), b = sibling.getBoundingClientRect();
      assert(Math.min(a.right, b.right) - Math.max(a.left, b.left) <= 1
        || Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top) <= 1, 'Sibling cards overlap');
    }
  }
  let boxes = 0;
  for (const element of doc.querySelectorAll<HTMLElement>('aside *, .character-screen, .character-screen *')) {
    if (!visible(element)) continue;
    // SVG descendants use SVG coordinates. Measure the outer viewport only.
    if (element.namespaceURI === 'http://www.w3.org/2000/svg' && element.tagName !== 'svg') continue;
    const style = win.getComputedStyle(element);
    const owner = element.title ? element : element.parentElement;
    const truncates = style.textOverflow === 'ellipsis' && style.overflowX === 'hidden'
      && ((!!owner?.title && owner.title.includes(element.textContent?.trim() ?? ''))
        || !!element.closest('[data-item-hover]'));
    if (element.clientWidth > 0 && !truncates) {
      assert(element.scrollWidth <= element.clientWidth + 1,
        `Overflow ${element.scrollWidth} > ${element.clientWidth}: ${describe(element)}`);
      boxes++;
    }
    let parent = element.parentElement;
    while (parent && parent.clientWidth === 0) parent = parent.parentElement;
    if (parent && !['COL', 'COLGROUP'].includes(element.tagName)) {
      const box = element.getBoundingClientRect(), parentBox = parent.getBoundingClientRect();
      assert(box.left >= parentBox.left - 1 && box.right <= parentBox.right + 1,
        'Escapes parent: ' + describe(element));
    }
  }
  assert(boxes > 80, 'Too few real measured boxes');
  const settings = rail.querySelector<HTMLAnchorElement>('.settings-link');
  const railCards = rail.querySelector<HTMLElement>('.rail-cards');
  assert(settings && railCards, 'Missing rail controls');
  const settingsBox = settings.getBoundingClientRect();
  assert(settingsBox.top >= 0 && settingsBox.bottom <= 760, 'Settings unreachable');
  const railOverflows = railCards.scrollHeight > railCards.clientHeight + 1;
  for (const [label, container] of [['main', main], ['rail cards', railCards]] as const) {
    if (container.scrollHeight <= container.clientHeight + 1) continue;
    const overflowY = win.getComputedStyle(container).overflowY;
    assert(overflowY === 'auto' || overflowY === 'scroll',
      `User scrolling disabled for ${label} (overflow-y: ${overflowY})`);
  }
  const railTop = railCards.scrollTop;
  main.scrollTop = 0; railCards.scrollTop = 0;
  main.scrollTop = main.scrollHeight;
  assert(main.scrollTop > 0 && railCards.scrollTop === 0 && win.scrollY === 0, 'Main must scroll independently');
  const mainTop = main.scrollTop;
  railCards.scrollTop = railCards.scrollHeight;
  assert(!railOverflows || railCards.scrollTop > 0, 'Overflowing rail cards must scroll');
  assert(main.scrollTop === mainTop && win.scrollY === 0
    && settings.getBoundingClientRect().bottom === settingsBox.bottom,
    'Rail cards must scroll independently of Settings and main');
  railCards.scrollTop = railTop; main.scrollTop = 0;
  const controls = [...main.querySelectorAll<HTMLElement>('input, select, button, summary, a')]
    .filter(element => visible(element) && !element.matches(':disabled'));
  assert(controls.length >= 6 && controls.every(element => element.tabIndex === 0), 'Unexpected keyboard order');
  for (const element of controls) {
    element.focus(); assert(doc.activeElement === element, 'Keyboard focus unavailable: ' + describe(element));
  }
  main.scrollTop = 0;
  const tables = [...main.querySelectorAll<HTMLElement>('table')].map(table => {
    assert(table.scrollWidth <= table.clientWidth + 1, 'Table overflow');
    return { clientWidth: table.clientWidth, scrollWidth: table.scrollWidth };
  });
  const exclusions = main.querySelector<HTMLElement>('.exclusions');
  assert(exclusions && visible(exclusions) && !exclusions.closest('details'), 'Exclusions hidden behind hover/details');
  return { boxes, tables };
}

export type CharacterAcceptanceResult = {
  variant: string; width: number; height: number; debug: boolean; boxes: number;
  tables: { clientWidth: number; scrollWidth: number }[];
};
/** Real browser only. Frames and debug storage are restored even on probe failure. */
export async function runCharacterAcceptance(options: {
  injectOverflow?: boolean; injectHiddenScroll?: 'main' | 'rail' | 'both';
} = {}) {
  assert(/^http:\/\/(127\.0\.0\.1|localhost):\d+$/.test(location.origin), 'Run on your local Vite origin');
  const previousDebug = localStorage.getItem('maxed.debug');
  const results: CharacterAcceptanceResult[] = [];
  try {
    for (const width of [1100, 900]) for (const debug of [false, true]) for (const variant of characterVariants) {
      if (debug) localStorage.setItem('maxed.debug', '1'); else localStorage.removeItem('maxed.debug');
      const frame = document.createElement('iframe');
      frame.title = `Character acceptance ${variant}`;
      frame.style.cssText = `position:fixed;left:0;top:0;z-index:2147483647;border:0;width:${width}px;height:760px`;
      frame.src = `/?preview=character&variant=${variant}#/character`;
      document.body.append(frame);
      try {
        await until(() => {
          const main = frame.contentDocument?.querySelector('main');
          return !!main?.querySelector('.character-screen')
            && (variant !== 'stale' || !!frame.contentDocument?.querySelector('aside')?.textContent?.includes('Stopped'))
            && (variant === 'empty'
            ? main.textContent!.includes('No character observed.') : main.textContent!.includes('Applied defense: 0'));
        }, variant);
        const doc = frame.contentDocument!;
        await doc.fonts.ready;
        if (variant !== 'empty') assert(
          doc.querySelector('aside')!.textContent!.includes('Observed boost') === debug, 'Rail startup debug gate');
        if (debug) doc.querySelectorAll('details').forEach(details => { details.open = true; });
        await new Promise<void>(resolve => frame.contentWindow!.requestAnimationFrame(() =>
          frame.contentWindow!.requestAnimationFrame(() => resolve())));
        assert(!/NaN|Infinity|undefined/.test(doc.querySelector('main')!.textContent!), 'Invalid values');
        if (options.injectOverflow) {
          const cell = doc.querySelector<HTMLElement>('.character-screen td');
          assert(cell, 'Missing overflow probe');
          const probe = doc.createElement('div'); probe.style.width = '1600px';
          probe.textContent = 'Synthetic overflow probe'; cell.append(probe);
        }
        if (options.injectHiddenScroll) {
          if (options.injectHiddenScroll !== 'rail') doc.querySelector<HTMLElement>('main')!.style.overflowY = 'hidden';
          if (options.injectHiddenScroll !== 'main') {
            const cards = doc.querySelector<HTMLElement>('.rail-cards')!;
            // Ensure this probe exercises an overflowing rail even in compact scenes.
            cards.style.maxHeight = '80px'; cards.style.overflowY = 'hidden';
          }
        }
        const measured = geometry(doc, width);
        if (variant === 'normal' || variant === 'partial') {
          // Measure both the initial zero-comparison screen and two full comparison rows.
          const win = frame.contentWindow!;
          const search = doc.querySelector<HTMLInputElement>('input[type=search]')!;
          search.value = 'Synthetic';
          search.dispatchEvent(new (win as Window & typeof globalThis).Event('input', { bubbles: true }));
          const select = doc.querySelector<HTMLSelectElement>('select')!;
          await until(() => select.options.length === 3, 'catalog comparison options');
          for (const type of ['201', '202']) {
            select.value = type;
            select.dispatchEvent(new (win as Window & typeof globalThis).Event('change', { bubbles: true }));
            const add = [...doc.querySelectorAll<HTMLButtonElement>('main button')]
              .find(button => button.textContent === 'Add comparison')!;
            await until(() => !add.disabled, 'enabled comparison control');
            add.click();
            await until(() => !!doc.querySelector(`[aria-label="Comparison ${type}"]`), 'comparison result');
          }
          if (debug) doc.querySelectorAll('details').forEach(details => { details.open = true; });
          await new Promise<void>(resolve => win.requestAnimationFrame(() => resolve()));
          const compared = geometry(doc, width);
          measured.boxes += compared.boxes;
          measured.tables = compared.tables;
        }
        results.push({ variant, width, height: 760, debug, ...measured });
      } catch (error) {
        throw new Error(`${width}×760 debug=${debug} ${variant}: ${String(error)}`);
      } finally { frame.remove(); }
    }
  } finally {
    if (previousDebug === null) localStorage.removeItem('maxed.debug');
    else localStorage.setItem('maxed.debug', previousDebug);
  }
  console.info('PASS Character acceptance: 24 real-browser mounts at 1100/900 × 760', results);
  return results;
}
