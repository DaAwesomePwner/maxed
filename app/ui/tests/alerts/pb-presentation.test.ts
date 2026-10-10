import { expect, it } from 'vitest';
import { alertTitle, createAlertController, type AlertPresentationState } from '../../src/lib/alerts/presentation';
import { alert, preferences } from '../phase2/fixtures';

it('distinguishes run-time and DPS records under the same preference', () => {
  let state: AlertPresentationState = { current: null, queued: 0, overflow: 0 };
  const controller = createAlertController(next => { state = next; }, () => 0, () => '1000000');
  controller.configure(preferences({ personal_best: 'banner' }), 'live-test');
  const view = alert({ kind: 'personal_best', presentation: 'banner', details: {
    type: 'run_time_personal_best', dungeon: 'Lair of Shaitan', duration_micros: 86_000_000, previous_micros: 87_000_000,
  } });
  controller.accept({ stream_id: 'live-test', sequence: '1', at: '1000000', event: { alert_fired: view } });
  expect(state.current?.title).toBe('New PB: Lair of Shaitan 1:26 (−0:01)');
  controller.dismiss(view.id);
  controller.accept({ stream_id: 'live-test', sequence: '2', at: '1000000', event: { alert_fired: alert({
    id: 'dps', kind: 'personal_best', presentation: 'banner', details: {
      type: 'personal_best', fight_id: 'fight', character_id: 'self', boss_type: 300, dps: 1172.4, previous_dps: 1000,
    },
  }) } });
  expect(state.current?.title).toBe('New DPS PB: 1,172');
  controller.dispose();
});

it('uses nonzero sub-second gain wording', () => {
  expect(alertTitle(alert({ kind: 'personal_best', details: {
    type: 'run_time_personal_best', dungeon: 'Lair of Shaitan', duration_micros: 86_500_000, previous_micros: 87_000_000,
  } }))).toBe('New PB: Lair of Shaitan 1:26 (under 1 s faster)');
});
