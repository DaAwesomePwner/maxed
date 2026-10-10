import { describe, expect, it } from 'vitest';
import { createClient } from '../src/lib/bridge/client';
import { createFixtureTransport, fixtureScenarios } from '../src/lib/bridge/fixture';
import seed from './fixtures/t0-live';

describe('test-only demo scenarios', () => {
  it('steps deterministically through all seven states with ordered decimal watermarks', async () => {
    const fixture = createFixtureTransport({ ...seed, sequence: '9007199254740993' });
    const client = createClient(fixture);
    let watermark = 9007199254740993n;
    const order: string[] = [];
    await client.onEngineEvent((event) => {
      expect(BigInt(event.sequence)).toBe(++watermark);
      order.push('event');
    }, () => { throw new Error('Expected fixture single'); });
    await client.onSnapshot((snapshot) => {
      if ('base_sequence' in snapshot) throw new Error('Fixture ticks must be full snapshots');
      expect(BigInt(snapshot.sequence)).toBe(watermark);
      expect(order.at(-1)).toBe('event');
      order.push('snapshot');
    });
    for (const scenario of fixtureScenarios) {
      fixture.step(scenario);
      const snapshot = await client.querySnapshot();
      expect(snapshot.capture.message).toContain(scenario);
      expect(await client.queryCapture()).toEqual(snapshot.capture);
      if (scenario === 'empty' || scenario === 'waiting') {
        expect(snapshot.character).toBeNull();
        expect(snapshot.fights).toEqual([]);
        expect(snapshot.capture.state).toBe(scenario === 'empty' ? 'Stopped' : 'WaitingForGame');
      }
      if (scenario === 'live') expect(snapshot.capture.state).toBe('Live');
      if (scenario === 'recovery') expect(snapshot.capture.state).toBe('Recovering');
      if (scenario === 'unknown-identity') {
        expect(snapshot.character).toBeNull();
        expect(snapshot.issues).toContain('missing_identity');
        expect(snapshot.fights.flatMap((fight) => fight.players).some((p) => p.is_local)).toBe(false);
      }
      if (scenario === 'death') {
        expect(snapshot.character?.died_at).toBe(snapshot.at);
        expect(snapshot.run?.outcome).toBe('died');
      }
      if (scenario === 'drop') expect(snapshot.drops).toHaveLength(1);
      expect(await client.querySetup()).toMatchObject({ demo: true, live_confirmed: false, npcap: 'unchecked', access: 'unknown' });
    }
    expect(order.filter((event) => event === 'snapshot')).toHaveLength(7);
    expect(fixture.demo).toBe(true);
    expect(fixture.externalDeliveryAllowed).toBe(false);
  });

  it('isolates query/listener copies, respects disposal and never mutates its seed', async () => {
    const fixture = createFixtureTransport(seed);
    const client = createClient(fixture);
    const before = structuredClone(seed);
    let delivered = 0;
    const off = await client.onSnapshot((snapshot) => { if ('base_sequence' in snapshot) throw new Error('Expected full fixture'); delivered++; snapshot.capture.message = 'mutated'; });
    fixture.step('live');
    const query = await client.querySnapshot();
    expect(query.capture.message).toContain('Synthetic');
    query.fights.length = 0;
    expect((await client.querySnapshot()).fights.length).toBeGreaterThan(0);
    off(); off();
    fixture.step('drop');
    expect(delivered).toBe(1);
    expect(seed).toEqual(before);
    await expect(client.setDemo(true)).rejects.toThrow('Preview Only');
    await expect(client.startCapture()).rejects.toThrow('Preview Only');
  });

  it('produces the same scenario snapshots on independent transports without a timer', async () => {
    const first = createFixtureTransport(seed);
    const second = createFixtureTransport(seed);
    for (const scenario of fixtureScenarios) {
      first.step(scenario); second.step(scenario);
      expect(await createClient(first).querySnapshot()).toEqual(await createClient(second).querySnapshot());
    }
  });
});
