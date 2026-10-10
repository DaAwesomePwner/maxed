import { busyRail, caseDebugText, stressCases, stressRail } from './acceptance-cases';

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

async function until(check: () => boolean, label: string): Promise<void> {
  const limit = performance.now() + 10000;
  while (!check()) {
    if (performance.now() >= limit) throw new Error('Timed out: ' + label);
    await new Promise(resolve => setTimeout(resolve, 50));
  }
}

function visible(element: HTMLElement): boolean {
  const style = element.ownerDocument.defaultView!.getComputedStyle(element);
  return style.display !== 'none' && style.visibility !== 'hidden'
    && element.getClientRects().length > 0
    && !element.closest('.sr-only, [hidden], [role="tooltip"]');
}

function describe(element: HTMLElement): string {
  return element.tagName.toLowerCase() + '.' + element.className
    + ' ' + (element.textContent ?? '').trim().slice(0, 90);
}

function checkGeometry(doc: Document, width: number): number {
  const win = doc.defaultView!;
  const rail = doc.querySelector<HTMLElement>('aside[aria-label="Character"]');
  const main = doc.querySelector<HTMLElement>('main');
  assert(rail && main, 'Missing real Shell rail/main');
  assert(win.innerWidth === width && win.innerHeight === 760, 'Incorrect CSS viewport');
  const rem = parseFloat(win.getComputedStyle(doc.documentElement).fontSize);
  if (width === 900) assert(Math.abs(rail.getBoundingClientRect().width - 12 * rem) < 0.5,
    '900px rail must be 12rem');
  assert(rail.clientWidth > 0 && main.clientWidth > 0, 'Layout metrics unavailable');
  assert(rail.scrollWidth <= rail.clientWidth, 'Rail overflows');
  assert(main.scrollWidth <= main.clientWidth, 'Main overflows');
  assert(doc.documentElement.scrollWidth <= doc.documentElement.clientWidth, 'Document overflows');
  const cards = [...main.querySelectorAll<HTMLElement>('.area-card, .nexus-home .panel')];
  assert(cards.length > 0, 'Missing Home cards');
  for (const card of cards) {
    assert(card.clientWidth > 0 && card.clientHeight > 0, 'Unmeasurable card');
    assert(card.scrollWidth <= card.clientWidth, 'Card overflows horizontally: ' + describe(card));
    assert(card.scrollHeight <= card.clientHeight, 'Card overflows vertically: ' + describe(card));
  }
  const groups = [
    'aside[aria-label="Character"]',
    '[data-stat-id]',
    '[data-kpi]',
    'main table',
  ];
  let checked = 0;
  const seen = new Set<HTMLElement>();
  for (const selector of groups) {
    const roots = [...doc.querySelectorAll<HTMLElement>(selector)];
    assert(roots.length > 0, 'Missing layout group: ' + selector);
    for (const root of roots) {
      for (const element of [root, ...root.querySelectorAll<HTMLElement>('*')]) {
        if (seen.has(element) || !visible(element)) continue;
        seen.add(element);
        const style = win.getComputedStyle(element);
        // Non-replaced inline boxes have clientWidth=0 even in a real browser.
        // Measure their rect against the nearest measurable ancestor instead.
        if (element.clientWidth === 0 && style.display !== 'inline'
          && !['COL', 'COLGROUP'].includes(element.tagName)) {
          const rect = element.getBoundingClientRect();
          if (rect.width === 0 && !element.textContent?.trim()) continue;
          throw new Error('Unmeasurable visible box: ' + describe(element));
        }
        // Deliberate truncation needs the full text on this box or its direct parent.
        const owner = element.title ? element : element.parentElement;
        const truncates = style.textOverflow === 'ellipsis' && style.overflowX === 'hidden'
          && !!owner?.title && owner.title.includes((element.textContent ?? '').trim());
        if (element.clientWidth > 0 && !truncates) {
          assert(element.scrollWidth <= element.clientWidth,
            'Overflow ' + element.scrollWidth + ' > ' + element.clientWidth + ': ' + describe(element));
          checked++;
        }
        let parent = element.parentElement;
        while (parent && parent.clientWidth === 0) parent = parent.parentElement;
        if (parent && !['COL', 'COLGROUP'].includes(element.tagName)) {
          const childBox = element.getBoundingClientRect(), parentBox = parent.getBoundingClientRect();
          assert(childBox.left >= parentBox.left - 1 && childBox.right <= parentBox.right + 1,
            'Escapes parent: ' + describe(element));
        }
      }
    }
  }
  // Recent runs are link rows rather than a native table; Nexus-only.
  for (const row of doc.querySelectorAll<HTMLElement>('main [data-row-id]')) {
    for (const element of [row, ...row.querySelectorAll<HTMLElement>('*')]) {
      if (visible(element) && element.clientWidth > 0) {
        assert(element.scrollWidth <= element.clientWidth, 'Recent row overflow: ' + describe(element));
        checked++;
      }
    }
  }
  assert(checked > 30, 'Too few measured boxes; selector or metric regression');
  for (const selector of ['.cname', '[data-stat-id] dd', '[role="meter"]']) {
    for (const element of doc.querySelectorAll<HTMLElement>(selector)) {
      assert(element.scrollWidth <= element.clientWidth, 'Clipped rail value: ' + describe(element));
      assert(element.scrollHeight <= element.clientHeight, 'Wrapped rail value: ' + describe(element));
    }
  }
  return checked;
}

function checkChurn(doc: Document, check: () => void): void {
  const cell = doc.querySelector<HTMLElement>('[data-kpi]');
  const value = cell?.querySelector<HTMLElement>('dd .value');
  assert(cell && value, 'Missing KPI churn target');
  const nodes = [...value.childNodes];
  const low = '99', high = '9,999';
  try {
    value.textContent = low;
    const before = cell.getBoundingClientRect();
    value.textContent = high;
    const after = cell.getBoundingClientRect();
    assert(Math.abs(before.width - after.width) < 0.5
      && Math.abs(before.x - after.x) < 0.5, 'KPI moved on two-to-four-digit churn');
    check();
  } finally {
    value.replaceChildren(...nodes);
  }
}

export async function runHomeAcceptance(options: { injectOverflow?: boolean } = {}): Promise<void> {
  // Any local Vite port: the iframes below load same-origin preview URLs.
  assert(/^http:\/\/(127\.0\.0\.1|localhost):\d+$/.test(location.origin), 'Run in the Vite preview origin');
  const previousDebug = localStorage.getItem('maxed.debug');
  const results: Array<{ case: string; width: number; debug: boolean; boxes: number }> = [];
  try {
    for (const width of [1100, 900]) for (const scenario of stressCases) for (const debug of [false, true]) {
      if (debug) localStorage.setItem('maxed.debug', '1');
      else localStorage.removeItem('maxed.debug');
      const frame = document.createElement('iframe');
      frame.title = 'Home acceptance ' + scenario.label;
      frame.style.cssText = 'position:fixed;left:0;top:0;z-index:2147483647;border:0;height:760px;';
      frame.style.width = width + 'px';
      frame.src = '/' + scenario.query + '#/';
      document.body.append(frame);
      try {
        await until(() => {
          const doc = frame.contentDocument;
          const main = doc?.querySelector('main');
          return !!doc?.querySelector('[role="meter"][aria-label="HP"]')
            && !!main?.textContent?.includes(scenario.target)
            && (scenario.mode !== 'nexus' || doc.querySelectorAll('[data-loot-type]').length === 32)
            && (scenario.mode === 'nexus' || !!doc.querySelector('.damage-row.self'));
        }, scenario.label);
        const doc = frame.contentDocument!;
        await doc.fonts.ready;
        await new Promise<void>(resolve =>
          frame.contentWindow!.requestAnimationFrame(() =>
            frame.contentWindow!.requestAnimationFrame(() => resolve())));
        const main = doc.querySelector<HTMLElement>('main')!;
        const rail = doc.querySelector<HTMLElement>('aside[aria-label="Character"]')!;
        if (debug) {
          doc.querySelectorAll('details').forEach(details => { details.open = true; });
          await new Promise<void>(resolve => frame.contentWindow!.requestAnimationFrame(() => resolve()));
        }
        for (const text of caseDebugText(scenario)) {
          assert(main.textContent!.includes(text) === debug, 'Debug gate: ' + text);
          if (debug) assert(main.innerText.includes(text), 'Debug caveat remains visually hidden: ' + text);
        }
        assert(rail.textContent!.includes('Observed boost') === debug, 'Rail debug gate');
        const expectedRail = scenario.stress ? stressRail : busyRail;
        assert(rail.querySelector('.cname')?.textContent === expectedRail.name, 'Rail name');
        for (const [label, text] of [['HP', expectedRail.hp], ['MP', expectedRail.mp], ['Fame', expectedRail.fame]]) {
          assert(rail.querySelector('[role="meter"][aria-label="' + label + '"]')
            ?.getAttribute('aria-valuetext') === text, 'Missing stress ' + label);
        }
        for (const [id, text] of [[20, expectedRail.att], [26, expectedRail.vit], [22, expectedRail.spd]]) {
          assert(rail.querySelector(`[data-stat-id="${id}"] dd`)?.textContent === text, 'Rail stat ' + id);
        }
        if (scenario.stress) assert(rail.querySelectorAll('[data-pip="4"]').length === 4, 'Stress equipment pips');
        assert(!/NaN|undefined|Infinity/.test(main.textContent!), 'Invalid numeric output');
        if (scenario.mode !== 'nexus') {
          assert(main.querySelector('.damage-row.self td')?.textContent?.trim() === (scenario.stress ? '14' : '1'), 'Local rank');
          assert(main.textContent!.includes(scenario.stress ? '+41 more players' : '+2 more players'), 'Focused fight participant count');
          if (scenario.stress) assert(main.querySelector('.gap')?.textContent?.includes('9 players'), 'Rank gap');
          else assert(main.querySelector('[data-fight-id]')?.getAttribute('data-fight-id') === 'realm-retained-0', 'Busy retained focus');
          if (scenario.mode === 'realm') assert(main.querySelector('.kill-count')?.textContent === (scenario.stress ? '7' : '6'), 'Realm kill count');
          if (scenario.ended) assert(main.innerText.includes('Ended'), 'Busy Realm did not end');
        }
        if (options.injectOverflow) {
          const probe = rail.querySelector<HTMLElement>('[data-stat-id="20"] dd');
          assert(probe, 'Missing overflow probe target');
          probe.style.minWidth = '600px';
        }
        const boxes = checkGeometry(doc, width);
        checkChurn(doc, () => { checkGeometry(doc, width); });
        if (scenario.mode === 'nexus') {
          const today = [...main.querySelectorAll<HTMLButtonElement>('button')]
            .find(button => button.textContent?.trim() === 'Today');
          assert(today, 'Missing Today control');
          today.click();
          await until(() => today.getAttribute('aria-pressed') === 'true'
            && main.querySelectorAll('[data-loot-type]').length === 32, 'Today scope');
          checkGeometry(doc, width);
        } else if (scenario.mode === 'dungeon') {
          const whole = [...main.querySelectorAll<HTMLButtonElement>('[role="tab"]')]
            .find(tab => tab.textContent?.includes('Whole run'));
          assert(whole, 'Missing Whole run tab');
          whole.click();
          await until(() => whole.getAttribute('aria-selected') === 'true', 'Whole run');
          assert(main.textContent!.includes('64 fights'), 'Missing 64-fight run aggregate');
          checkGeometry(doc, width);
        }
        results.push({ case: scenario.label, width, debug, boxes });
      } catch (error) {
        throw new Error(scenario.label + ' @ ' + width + ' debug=' + debug + ': ' + String(error));
      } finally {
        frame.remove();
      }
    }
    console.table(results);
    console.info('PASS Home v3 acceptance: 16 real-browser mounts at 1100/900 × 760');
  } finally {
    if (previousDebug === null) localStorage.removeItem('maxed.debug');
    else localStorage.setItem('maxed.debug', previousDebug);
  }
}
