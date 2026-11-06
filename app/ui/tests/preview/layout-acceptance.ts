import { busyRail, caseDebugText, stressCases, stressRail, v4Cases } from './acceptance-cases';

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
  const v4 = !!main.querySelector('[data-home="v4"]');
  const cards = [...main.querySelectorAll<HTMLElement>('.area-card, .nexus-home .panel, .v4-card, .dps-hero, .bosses-card, .events-log')];
  assert(cards.length > 0, 'Missing Home cards');
  for (const card of cards) {
    assert(card.clientWidth > 0 && card.clientHeight > 0, 'Unmeasurable card');
    assert(card.scrollWidth <= card.clientWidth, 'Card overflows horizontally: ' + describe(card));
    assert(card.scrollHeight <= card.clientHeight, 'Card overflows vertically: ' + describe(card));
  }
  // Contained content can still cover an adjacent card when a grid track is
  // shorter than its item. Compare visible siblings, including spanning items.
  for (const [index, card] of cards.entries()) {
    if (!visible(card)) continue;
    for (const sibling of cards.slice(index + 1)) {
      if (card.parentElement !== sibling.parentElement || !visible(sibling)) continue;
      const first = card.getBoundingClientRect(), second = sibling.getBoundingClientRect();
      const overlapX = Math.min(first.right, second.right) - Math.max(first.left, second.left);
      const overlapY = Math.min(first.bottom, second.bottom) - Math.max(first.top, second.top);
      // Permit subpixel rounding at shared edges, never a covered row.
      assert(overlapX <= 1 || overlapY <= 1,
        `Sibling cards overlap (${overlapX.toFixed(1)} × ${overlapY.toFixed(1)}px): `
        + describe(card) + ' / ' + describe(sibling));
    }
  }
  const groups = [
    'aside[aria-label="Character"]',
    '[data-stat-id]',
    ...(v4 ? ['[data-hud]', '[data-home="v4"] .v4-screen, [data-home="v4"] .nexus-home, .recap-ready, .last-recap']
      : ['[data-kpi]', 'main table']),
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
        // SVG text/paths use SVG coordinates, not HTML client/scroll metrics.
        // Their outer SVG viewport is still measured below.
        if (element.namespaceURI === 'http://www.w3.org/2000/svg' && element.tagName !== 'svg') continue;
        const style = win.getComputedStyle(element);
        // Non-replaced inline boxes have clientWidth=0 even in a real browser.
        // Measure their rect against the nearest measurable ancestor instead.
        if (element.clientWidth === 0 && style.display !== 'inline'
          && !['COL', 'COLGROUP'].includes(element.tagName)) {
          const rect = element.getBoundingClientRect();
          if (rect.width === 0 && !element.textContent?.trim()) continue;
          // Border-only timeline markers have no content box; their border
          // rectangle is measurable and must still fit their parent below.
          assert(rect.width > 0 && !element.textContent?.trim(), 'Unmeasurable visible box: ' + describe(element));
        }
        // Deliberate truncation needs a title or an item hover card.
        const owner = element.title ? element : element.parentElement;
        const truncates = style.textOverflow === 'ellipsis' && style.overflowX === 'hidden'
          && ((!!owner?.title && owner.title.includes((element.textContent ?? '').trim()))
            || !!element.closest('[data-item-hover]'));
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
    for (const element of rail.querySelectorAll<HTMLElement>(selector)) {
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

type TableWidth = { card: string; clientWidth: number; scrollWidth: number };
type AcceptanceResult = { case: string; width: number; debug: boolean; boxes: number; tables: TableWidth[] };

function checkRecap(main: HTMLElement, mode: string, width: number): TableWidth[] {
  if (!mode.startsWith('home-v4-recap')) return [];
  assert(main.querySelector('.recap-screen'), 'Missing saved recap');
  assert(!main.querySelector('.recap-placeholder'), 'Saved recap still shows placeholder');
  assert(main.querySelector('.recap-banner, .death-banner'), 'Missing recap banner');
  assert(main.querySelectorAll('.recap-screen > .recap-tiles > .recap-tile').length === 5, 'Missing recap tiles');
  assert(main.querySelector('.v4-loot'), 'Missing recap Loot card');
  if (mode === 'home-v4-recap-realm') {
    assert(main.querySelectorAll('.visit-events-card tbody tr:not(.announced)').length === 5, 'Five killed events');
    assert(main.querySelectorAll('.visit-events-card tbody tr.announced').length === 2, 'Two announced-only events');
    assert(main.textContent!.includes('Castle') && main.textContent!.includes('inferred'), 'Inferred Castle chain');
  } else {
    assert(main.querySelectorAll('.recap-fights-card tbody tr').length === 3, 'Three saved bosses');
    assert(main.querySelector('.recap-chips [data-kind="best-dps"]'), 'BEST DPS record chip');
    assert(main.querySelectorAll('.recap-party > span').length === 4, 'Top three and five more party line');
    if (mode === 'home-v4-recap') {
      assert(main.querySelectorAll('.recap-chain tbody tr').length === 3, 'Three inferred chain steps');
      assert(main.querySelectorAll('.recap-chain tbody tr:last-child [data-item-hover]').length === 4, 'Four chain item sprites');
      assert(main.querySelector('.recap-banner')?.textContent?.includes('4:09'), 'Saved clear time');
      assert(main.querySelector('.recap-chips [data-kind="pb"]'), 'PB clear chip');
      assert(main.textContent!.includes('3rd Synthetic Moonlit Sanctum in a row tonight') && main.textContent!.includes('38'), 'Run records');
      assert(main.querySelector('.dry-streak-row')?.textContent?.includes('23'), 'White ended 23-run streak');
    } else {
      assert(main.querySelector('.death-banner')?.textContent?.includes('Killed by Moonlit Warden'), 'Catalog killer');
      assert(main.querySelectorAll('.hp-trace path').length === 2, 'Observed HP gap');
    }
  }
  const tables = [...main.querySelectorAll<HTMLElement>('.recap-table-scroll')];
  assert(tables.length > 0, 'Missing recap table scrollers');
  const measurements = tables.map(table => {
    assert(table.clientWidth > 0 && table.scrollWidth <= table.clientWidth + 1,
      `Recap table overflows at ${width}: ${table.scrollWidth} > ${table.clientWidth} + 1`);
    return { card: table.closest('.v4-card')?.querySelector('h3')?.textContent ?? '—',
      clientWidth: table.clientWidth, scrollWidth: table.scrollWidth };
  });
  if (width === 900) {
    const columns = main.querySelector<HTMLElement>('.recap-columns');
    assert(columns && columns.children.length === 2, '900px recap cards must stack');
    const [first, second] = [...columns.children].map(child => child.getBoundingClientRect());
    assert(Math.abs(first.left - second.left) <= 1 && second.top >= first.bottom, '900px recap cards must stack');
  }
  return measurements;
}

export async function runHomeAcceptance(options: { injectOverflow?: boolean; injectOverlap?: boolean } = {}): Promise<AcceptanceResult[]> {
  // Any local Vite port: the iframes below load same-origin preview URLs.
  assert(/^http:\/\/(127\.0\.0\.1|localhost):\d+$/.test(location.origin), 'Run in the Vite preview origin');
  const previousDebug = localStorage.getItem('maxed.debug');
  const results: AcceptanceResult[] = [];
  try {
    for (const width of [1100, 900]) for (const scenario of [...stressCases, ...v4Cases]) for (const debug of [false, true]) {
      if (debug) localStorage.setItem('maxed.debug', '1');
      else localStorage.removeItem('maxed.debug');
      const frame = document.createElement('iframe');
      frame.title = 'Home acceptance ' + scenario.label;
      frame.style.cssText = 'position:fixed;left:0;top:0;z-index:2147483647;border:0;height:760px;';
      frame.style.width = width + 'px';
      frame.src = '/' + scenario.query + '#/';
      document.body.append(frame);
      try {
        if (scenario.mode.startsWith('home-v4-recap')) {
          await until(() => !!frame.contentDocument?.querySelector('.recap-ready')
            && !!frame.contentDocument?.querySelector('.nexus-home'), 'Nexus with recap ready');
          const doc = frame.contentDocument!;
          await doc.fonts.ready;
          assert(!doc.querySelector('.recap-screen'), 'Recap took over Home');
          checkGeometry(doc, width);
          const view = [...doc.querySelectorAll<HTMLButtonElement>('.recap-ready button')]
            .find(button => button.textContent === 'View recap');
          assert(view, 'Missing View recap'); view.click();
        }
        await until(() => {
          const doc = frame.contentDocument;
          const main = doc?.querySelector('main');
          if ('root' in scenario) return !!main?.querySelector(`[data-home="v4"] ${scenario.root}`)
            && !!main.textContent?.includes(scenario.target)
            && main.querySelector('[data-hud="combat"]')?.textContent?.includes('+38%') === true
            && (scenario.rank === null || !!main.querySelector('.damage-row.self'));
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
        for (const text of 'root' in scenario ? scenario.debugText : caseDebugText(scenario)) {
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
        if ('root' in scenario) {
          assert(main.querySelectorAll('[data-hud]').length === 6, 'Missing HUD items');
          if (scenario.rank !== null) {
            assert(main.querySelector('.damage-row.self td')?.textContent?.trim() === String(scenario.rank), 'V4 local rank');
            const shown = scenario.rank > 6 ? 7 : 6;
            assert(main.querySelectorAll('.damage-row').length === shown, 'V4 top six plus self');
            assert(main.textContent!.includes(`+${scenario.participants - shown} more`), 'V4 hidden participants');
            assert(!!main.querySelector('.you-vs-party-chart path.party') === scenario.partyRecorded, 'Party line');
            assert(main.textContent!.includes('Party average line not recorded for this fight') === !scenario.partyRecorded, 'Legacy party note');
            if (scenario.rank > 6) assert(main.querySelector('.gap')?.textContent?.includes('2 players'), 'V4 rank gap');
          }
          if (scenario.mode === 'home-v4-dungeon') {
            assert(main.textContent!.includes('Boss 3 of 3'), 'Learned three-boss profile');
            assert(main.querySelector('.dry-streak-row')?.textContent?.includes('dry streak 23'), 'Dry streak');
          }
        } else if (scenario.mode !== 'nexus') {
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
        if (options.injectOverlap && scenario.mode === 'realm') {
          const fight = main.querySelector<HTMLElement>('.realm-fight');
          const kills = main.querySelector<HTMLElement>('.realm-kill-card');
          assert(fight && kills, 'Missing sibling overlap probe targets');
          // Move the whole card without overflowing its own content box.
          const offset = fight.getBoundingClientRect().bottom - kills.getBoundingClientRect().top - 20;
          kills.style.transform = `translateY(${offset}px)`;
        }
        const tables = checkRecap(main, scenario.mode, width);
        const boxes = checkGeometry(doc, width);
        if (!('root' in scenario)) checkChurn(doc, () => { checkGeometry(doc, width); });
        if (scenario.mode === 'home-v4-dungeon') {
          const whole = [...main.querySelectorAll<HTMLButtonElement>('.v4-meter-tabs button')]
            .find(button => button.textContent === 'Whole run');
          assert(whole, 'Missing v4 Whole run control');
          whole.click();
          await until(() => whole.getAttribute('aria-pressed') === 'true', 'V4 Whole run');
          checkGeometry(doc, width);
        }
        if (scenario.mode.startsWith('home-v4-recap')) {
          const back = [...main.querySelectorAll<HTMLButtonElement>('button')]
            .find(button => button.textContent === 'Back');
          assert(back, 'Missing recap dismissal'); back.click();
          await until(() => !!main.querySelector('.nexus-home'), 'Recap dismissed');
          assert(main.querySelector('.last-recap button'), 'Last recap lost after Back');
          checkGeometry(doc, width);
        }
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
        results.push({ case: scenario.label, width, debug, boxes, tables });
      } catch (error) {
        throw new Error(scenario.label + ' @ ' + width + ' debug=' + debug + ': ' + String(error));
      } finally {
        frame.remove();
      }
    }
    console.table(results);
    console.info(`PASS Home v3/v4 acceptance: ${results.length} real-browser mounts at 1100/900 × 760`);
    return results;
  } finally {
    if (previousDebug === null) localStorage.removeItem('maxed.debug');
    else localStorage.setItem('maxed.debug', previousDebug);
  }
}
