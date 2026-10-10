import { describe, expect, it } from 'vitest';
import { nextHomeMode, type HomeModeState } from '../../src/lib/home/mode';
import type { CaptureStatus } from '../../src/lib/bindings/CaptureStatus';
import { homeSnapshot } from './fixtures';

const initial = (): HomeModeState => ({ mode: 'nexus', streamId: null, runId: null, uncertainSinceMs: null, stale: false });
const snapshot = homeSnapshot();
const capture = (state: CaptureStatus['state']): CaptureStatus => ({ ...snapshot.capture, state });
const dungeon = () => nextHomeMode(initial(), snapshot, capture('Live'), 0);

describe('Home switching table', () => {
  it('recovery_grace_and_stream_retirement', () => {
    let state = dungeon();
    expect(state).toMatchObject({ mode: 'dungeon', streamId: snapshot.stream_id, runId: snapshot.run!.id, stale: false });
    state = nextHomeMode(state, snapshot, capture('Recovering'), 100);
    expect(state).toMatchObject({ mode: 'dungeon', stale: true, uncertainSinceMs: 100 });
    state = nextHomeMode(state, null, capture('WaitingForGame'), 1599);
    expect(state).toMatchObject({ mode: 'dungeon', stale: true, uncertainSinceMs: 100 });
    state = nextHomeMode(state, null, capture('Recovering'), 1600);
    expect(state.mode).toBe('nexus');
    const retired = nextHomeMode(dungeon(), homeSnapshot({ stream_id: 'demo-new', area_kind: 'unknown', run: null }), capture('Recovering'), 101);
    expect(retired).toMatchObject({ mode: 'nexus', streamId: 'demo-new', runId: null });
    expect(nextHomeMode(retired, homeSnapshot({ stream_id: 'demo-new' }), capture('Live'), 102).mode).toBe('dungeon');
  });

  it.each(['nexus', 'realm', 'other_hub'] as const)('explicit %s leaves dungeon immediately', area_kind => {
    expect(nextHomeMode(dungeon(), homeSnapshot({ area_kind }), capture('Live'), 1).mode).toBe(area_kind === 'realm' ? 'realm' : 'nexus');
  });
  it.each(['Stopped', 'Failed', 'NpcapMissing', 'NpcapIncompatible', 'PermissionDenied'] as const)('%s ends grace immediately', state => {
    expect(nextHomeMode(dungeon(), snapshot, capture(state), 1).mode).toBe('nexus');
  });
  it('ended run and idle session override stale dungeon area', () => {
    expect(nextHomeMode(dungeon(), homeSnapshot({ run: { ...snapshot.run!, ended: '6000000' } }), capture('Live'), 1).mode).toBe('nexus');
    expect(nextHomeMode(dungeon(), homeSnapshot({ session: { ...snapshot.session!, ended: '6000000' } }), capture('Live'), 1).mode).toBe('nexus');
  });
  it('missing identity, no players and zero duration do not suppress dungeon layout', () => {
    const unknown = homeSnapshot({ character: null, fights: [] });
    expect(nextHomeMode(initial(), unknown, capture('Live'), 0).mode).toBe('dungeon');
  });
  it('authoritative recovery resumes inside grace without erasing evidence', () => {
    const partial = homeSnapshot({ issues: ['midstream', 'stream_gap'] });
    const stale = nextHomeMode(dungeon(), null, capture('Recovering'), 100);
    expect(nextHomeMode(stale, partial, capture('Live'), 1599)).toMatchObject({ mode: 'dungeon', stale: false, uncertainSinceMs: null });
    expect(partial.issues).toEqual(['midstream', 'stream_gap']);
  });
  it('unknown area starts one grace interval even with Live capture', () => {
    const unknown = homeSnapshot({ area_kind: 'unknown' });
    let state = nextHomeMode(dungeon(), unknown, capture('Live'), 50);
    state = nextHomeMode(state, unknown, capture('Live'), 1549);
    expect(state).toMatchObject({ mode: 'dungeon', stale: true, uncertainSinceMs: 50 });
    expect(nextHomeMode(state, unknown, capture('Live'), 1550).mode).toBe('nexus');
  });
});
