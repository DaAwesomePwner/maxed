import assert from 'node:assert/strict';
import test from 'node:test';
import { subscribeCaptureStatus } from '../src/lib/bridge/capture-status.ts';

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const flush = () => new Promise((resolve) => setImmediate(resolve));
const status = (state) => ({ state, message: '', dropped_frames: 0, unknown_packets: 0, decode_failures: 0 });

test('a live event wins over a delayed initial stopped snapshot', async () => {
  const initial = deferred();
  const received = [];
  let event;
  let releases = 0;
  const dispose = subscribeCaptureStatus(
    async (receive) => { event = receive; return () => { releases++; }; },
    () => initial.promise,
    (value) => received.push(value.state),
    () => assert.fail('unexpected failure'),
  );
  await flush();
  event(status('Live'));
  initial.resolve(status('Stopped'));
  await flush();
  assert.deepEqual(received, ['Live']);
  dispose();
  dispose();
  event(status('Recovering'));
  assert.equal(releases, 1);
  assert.deepEqual(received, ['Live']);
});

test('unmount before listener registration releases the late listener and never queries', async () => {
  const registration = deferred();
  let releases = 0;
  const dispose = subscribeCaptureStatus(
    () => registration.promise,
    () => assert.fail('must not query after unmount'),
    () => assert.fail('must not update after unmount'),
    () => assert.fail('must not report after unmount'),
  );
  dispose();
  registration.resolve(() => { releases++; });
  await flush();
  assert.equal(releases, 1);
});

test('unmount while query is pending suppresses both success and rejection', async () => {
  for (const reject of [false, true]) {
    const initial = deferred();
    let releases = 0;
    const dispose = subscribeCaptureStatus(
      async () => () => { releases++; },
      () => initial.promise,
      () => assert.fail('must not update after unmount'),
      () => assert.fail('must not report after unmount'),
    );
    await flush();
    dispose();
    if (reject) initial.reject(new Error('private error'));
    else initial.resolve(status('Stopped'));
    await flush();
    assert.equal(releases, 1);
  }
});

test('registration failure becomes a fixed UI failure without leaking the rejection', async () => {
  let errors = 0;
  const dispose = subscribeCaptureStatus(
    async () => { throw new Error('private error'); },
    () => assert.fail('must not query without listener'),
    () => assert.fail('must not update'),
    (...args) => { assert.equal(args.length, 0); errors++; },
  );
  await flush();
  assert.equal(errors, 1);
  dispose();
});
