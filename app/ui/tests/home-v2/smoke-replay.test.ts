import { execFile } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';
import { expect, it } from 'vitest';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import { createHomeLiveWindow, emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { realmView } from '../../src/lib/home/realm';
import { scopedFights } from '../../src/lib/shell/rail';

it('smoke replay retains the same realm visit and kills while excluding Ocean Trench time', async () => {
  const root = resolve(process.cwd(), '../..');
  const temp = mkdtempSync(join(tmpdir(), 'maxed-smoke-projection-'));
  const output = join(temp, 'projection.json');
  let window = emptyHomeLiveWindow();
  const live = createHomeLiveWindow(next => { window = next; });
  let fullWindow = emptyHomeLiveWindow(), reducedWindow = emptyHomeLiveWindow();
  const fullStream = createHomeLiveWindow(next => { fullWindow = next; });
  const reducedStream = createHomeLiveWindow(next => { reducedWindow = next; });
  try {
    // Replay the committed pcap on every run; no copied/synthetic event log can
    // hide an engine/projection contract regression. Use the configured build target.
    await promisify(execFile)('cargo', ['test', '--offline', '-p', 'maxed-cli', '--test', 'smoke_replay',
      'realm_return_kills_end_at_removal_and_short_tower_has_no_dps', '--', '--exact'], {
      cwd: root, timeout: 600_000, maxBuffer: 4 * 1024 * 1024, windowsHide: true,
      env: { ...process.env, CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? resolve(root, 'target'), MAXED_SMOKE_PROJECTION_PATH: output },
    });
    const replay = JSON.parse(readFileSync(output, 'utf8')) as {
      events: EventEnvelope[]; presentation_events: EventEnvelope[];
      snapshot: LiveSnapshot; live_snapshot: LiveSnapshot;
    };
    expect(replay.presentation_events).toHaveLength(replay.events.length);
    expect(replay.presentation_events).not.toEqual(replay.events);
    const expected = JSON.parse(readFileSync(join(root,
      'tests/fixtures/scrubbed/maxed-realm-dungeon-return/expected.json'), 'utf8')) as {
      connections: { mapinfo_us: string }[];
      bosses: { target_id: number; target_type: number; removal_us: string }[];
    };
    let firstVisit: string | null = null;
    let firstInstance: string | null = null;
    let returned = false;
    const expectedFightIds = new Set<string>();
    const targets = new Set(expected.bosses.map(b => b.target_id));
    // The complete pcap runs through the engine. At the projection boundary,
    // retain the boss-only oracle for detailed fight/rail assertions. The
    // independent full streams below exercise ordinary traffic and kill history.
    // All lifecycle/identity events and the real target DTOs remain unchanged.
    for (const [index, envelope] of replay.events.entries()) {
      // Compare native presentation compaction and bounded baselines against
      // the original stream, independently of the boss-only #47 oracle below.
      fullStream.event(envelope);
      reducedStream.event(replay.presentation_events[index]);
      const event = envelope.event;
      const fight = 'fight_started' in event ? event.fight_started
        : 'fight_updated' in event ? event.fight_updated : 'fight_ended' in event ? event.fight_ended : null;
      if (fight && !targets.has(fight.target_id)) continue;
      if (fight) expectedFightIds.add(fight.id);
      live.event(envelope);
      if ('map_entered' in envelope.event && envelope.event.map_entered.kind === 'realm') {
        const map = envelope.event.map_entered;
        if (firstVisit === null) {
          firstVisit = window.visitKey;
          firstInstance = map.realm_instance_key ?? null;
          expect(firstInstance).not.toBeNull();
        } else {
          returned = true;
          expect(map.realm_instance_key).toBe(firstInstance);
          expect(window.visitKey).toBe(firstVisit);
          for (const target of expected.bosses.filter(b => BigInt(b.removal_us) < BigInt(map.at))) {
            const fight = window.fights.find(f => f.target_id === target.target_id);
            expect(fight, `target ${target.target_id} retained after return`).toBeDefined();
            expect(fight?.kill?.provisional).toBe(false);
            expect(fight?.ended).toBe((BigInt(target.removal_us) / 1000n * 1000n).toString());
          }
        }
      }
    }
    expect(returned).toBe(true);
    fullStream.snapshot(replay.snapshot);
    reducedStream.snapshot(replay.live_snapshot);
    const retained = (value: typeof window, snapshot: LiveSnapshot) =>
      realmView(snapshot, value).kills.filter(kill =>
        kill.fightId !== null && expectedFightIds.has(kill.fightId));
    const fullRows = retained(fullWindow, replay.snapshot);
    const reducedRows = retained(reducedWindow, replay.live_snapshot);
    const fullKills = fullRows.map(row => row.fightId).sort();
    const reducedKills = reducedRows.map(row => row.fightId).sort();
    // Ordinary traffic must not evict visit kills from either payload stream.
    expect(expected.bosses).toHaveLength(9);
    expect(fullKills).toHaveLength(9);
    expect(reducedKills).toEqual(fullKills);
    // Native presentation omits zero-damage players. All other kill evidence
    // and positive local contribution metrics must survive unchanged.
    const comparable = (rows: typeof fullRows) => rows.map(row => ({ ...row,
      self: row.self && row.self.damage > 0 ? row.self : null }));
    expect(comparable(reducedRows)).toEqual(comparable(fullRows));
    expect(reducedWindow.fights.length).toBeLessThanOrEqual(64);
    const snapshot = { ...replay.snapshot, fights: replay.snapshot.fights.filter(f => targets.has(f.target_id)) };
    expect(snapshot.capture.state).toBe('Live');
    live.snapshot(snapshot);
    const view = realmView(snapshot, window);
    expect(window.visitKey).toBe(firstVisit);
    const rail = scopedFights(snapshot, window);
    for (const target of expected.bosses) {
      const fight = window.fights.find(f => f.target_id === target.target_id);
      expect(fight).toBeDefined();
      expect(rail.some(f => f.id === fight!.id)).toBe(true);
      expect(view.kills.some(k => k.fightId === fight!.id && !k.provisional)).toBe(true);
    }
    const [entry, trench, back] = expected.connections.map(c => BigInt(c.mapinfo_us) / 1000n * 1000n);
    const now = BigInt(replay.snapshot.at);
    expect(trench - entry).toBeGreaterThan(0n);
    expect(back - trench).toBeGreaterThan(7_000_000n);
    expect(window.realmPeriods?.map(p => [p.started, p.ended])).toEqual([
      [entry.toString(), trench.toString()], [back.toString(), null],
    ]);
    expect(view.visitElapsed).toBe((trench - entry + now - back).toString());
    expect(BigInt(view.visitElapsed!)).toBeLessThan(now - entry);
  } finally {
    live.dispose();
    fullStream.dispose(); reducedStream.dispose();
    rmSync(temp, { recursive: true, force: true });
  }
}, 620_000);
