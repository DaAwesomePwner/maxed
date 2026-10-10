import { expect, it, vi } from 'vitest';
import { emptyListState, restorePosition } from '../../src/lib/navigation/state';

it('waits for rows-ready, clamps vanished content and focuses the originating row without scrolling', async () => {
  const pane = document.createElement('main');
  pane.innerHTML = '<h1 tabindex="-1">Runs</h1><div data-row-id="a"><a href="#/runs/a">Run A</a></div>';
  document.body.append(pane);
  Object.defineProperties(pane, { scrollHeight: { value: 500 }, clientHeight: { value: 200 } });
  const row = pane.querySelector('a')!; const focus = vi.spyOn(row, 'focus');
  let ready!: () => void;
  const pending = restorePosition(pane, { ...emptyListState(), scrollTop: 600, focusedRowId: 'a' }, new Promise<void>((resolve) => { ready = resolve; }));
  expect(pane.scrollTop).toBe(0); expect(focus).not.toHaveBeenCalled();
  ready(); expect(await pending).toBe(false);
  expect(pane.scrollTop).toBe(300); expect(focus).toHaveBeenCalledWith({ preventScroll: true });
  expect(document.activeElement).toBe(row); pane.remove();
});

it('offers a heading fallback for removed rows and cancels obsolete restorations', async () => {
  const pane = document.createElement('main'); pane.innerHTML = '<h1 tabindex="-1">Runs</h1>'; document.body.append(pane);
  expect(await restorePosition(pane, { ...emptyListState(), focusedRowId: 'removed' })).toBe(true);
  expect(document.activeElement).toBe(pane.querySelector('h1'));
  const focus = vi.spyOn(pane.querySelector('h1')!, 'focus'); focus.mockClear();
  await restorePosition(pane, emptyListState(), Promise.resolve(), () => false);
  expect(focus).not.toHaveBeenCalled(); pane.remove();
});

it('focuses a row inserted before rows-ready only after ready resolves', async () => {
  const pane = document.createElement('main'); pane.innerHTML = '<h1 tabindex="-1">Runs</h1>'; document.body.append(pane);
  let ready!: () => void;
  const pending = restorePosition(pane, { ...emptyListState(), runsWindow: 2, focusedRowId: 'run-13' }, new Promise<void>(resolve => { ready = resolve; }));
  expect(document.activeElement).not.toBe(pane.querySelector('h1'));
  pane.insertAdjacentHTML('beforeend', '<a data-row-id="run-13" href="#/runs/run-13">Saved run</a>');
  ready();
  expect(await pending).toBe(false);
  expect(document.activeElement).toBe(pane.querySelector('[data-row-id="run-13"]'));
  pane.remove();
});
