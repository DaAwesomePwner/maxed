import { describe, expect, it, vi, afterEach } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { flushSync } from 'svelte';
import DebugNote from '../src/lib/debug/DebugNote.svelte';
import { caveat, setDebugMode } from '../src/lib/debug/debug';

afterEach(() => setDebugMode(false));

describe('debug mode', () => {
  it('hides notes and logs by default', () => {
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => {});
    render(DebugNote, { text: 'Partial coverage' });
    caveat('test', 'Partial coverage');
    expect(screen.queryByText('Partial coverage')).toBeNull();
    expect(debug).not.toHaveBeenCalled();
  });
  it('shows notes and logs when enabled', () => {
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => {});
    render(DebugNote, { text: 'Partial coverage' });
    setDebugMode(true); flushSync();
    caveat('test', 'Partial coverage');
    expect(screen.getByText('Partial coverage')).toBeTruthy();
    expect(debug).toHaveBeenCalledWith('[maxed:test] Partial coverage');
  });
});
