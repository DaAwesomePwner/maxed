use super::*;
use crate::common::account_scope_fixture::ACCOUNT;

fn observed_fight(id: &str, local_first: u64, local_last: u64, amount: u16) -> FightView {
    use crate::engine_common as engine;
    let mut e = engine::engine();
    engine::setup(&mut e);
    engine::apply(
        &mut e,
        3000,
        engine::update(vec![engine::object(3, 100, vec![])], vec![]),
    );
    for (at, owner, bullet, damage) in [
        (1_000_000, 3, 1, 100),
        (local_first, 1, 2, amount),
        (local_last, 1, 3, amount),
    ] {
        engine::apply(&mut e, at, engine::tick(vec![]));
        engine::apply(&mut e, at, engine::damage(owner, bullet, damage));
    }
    let events = engine::apply(&mut e, local_last + 1_000_000, engine::map("Nexus"));
    let mut fight = events
        .into_iter()
        .find_map(|event| match event {
            EngineEvent::FightEnded(fight) => Some(fight),
            _ => None,
        })
        .unwrap();
    assert!(fight.complete_capture && fight.issues.is_empty());
    assert_eq!(fight.local_active_micros, Some(local_last - local_first));
    // Attach the observed fight to the synthetic session/run seeded by database().
    let template = eligible_fight(id, 1_000_000);
    fight.id = template.id;
    fight.session_id = template.session_id;
    fight.run_id = template.run_id;
    fight
}

#[test]
fn home_top_dps_rejects_three_second_local_burst_in_sixty_second_fight() {
    let (_dir, path, mut db) = database();
    let burst = observed_fight("burst", 58_000_000, 61_000_000, 30_000);
    let steady = observed_fight("steady", 1_000_000, 31_000_000, 1500);
    assert_eq!(
        burst.last_hit.unwrap().0 - burst.first_hit.unwrap().0,
        60_000_000
    );
    let local_dps = |f: &FightView| f.players.iter().find(|p| p.is_local).unwrap().dps.unwrap();
    assert!(local_dps(&burst) > local_dps(&steady));
    add(
        &mut db,
        vec![
            EngineEvent::FightEnded(burst),
            EngineEvent::FightEnded(steady.clone()),
        ],
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    for scope in [session(), today(0, 100_000_000)] {
        assert_eq!(
            reader.home_summary(scope, Some(ACCOUNT)).unwrap().top_fight,
            Some(steady.clone())
        );
    }
}

#[test]
fn home_top_dps_local_window_boundary_survives_projection_and_updates() {
    let (_dir, path, mut db) = database();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    for (micros, qualifies) in [(19_999_999, false), (20_000_000, true), (3_000_000, false)] {
        let mut fight = eligible_fight("boundary", 1_000_000);
        fight.last_hit = Some(Timestamp(61_000_000));
        fight.ended = fight.last_hit;
        fight.local_active_micros = Some(micros);
        add(&mut db, vec![EngineEvent::FightUpdated(fight.clone())]);
        let stored: i64 = db
            .query_row(
                "SELECT json_extract(json,'$.local_active_micros') FROM fights WHERE id='boundary'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, micros as i64);
        for scope in [session(), today(0, 100_000_000)] {
            assert_eq!(
                reader.home_summary(scope, Some(ACCOUNT)).unwrap().top_fight,
                qualifies.then(|| fight.clone())
            );
        }
    }
}

#[test]
fn home_legacy_local_window_is_readable_but_empty_until_a_qualifying_fight() {
    for explicit_null in [false, true] {
        let (_dir, path, mut db) = database();
        let mut legacy = eligible_fight("legacy", 2_000_000);
        legacy.players[0].dps = Some(9999.0);
        add(&mut db, vec![EngineEvent::FightEnded(legacy)]);
        db.execute(
            if explicit_null {
                "UPDATE fights SET json=json_set(json,'$.local_active_micros',NULL)"
            } else {
                "UPDATE fights SET json=json_remove(json,'$.local_active_micros')"
            },
            [],
        )
        .unwrap();
        let json: String = db
            .query_row("SELECT json FROM fights WHERE id='legacy'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let decoded: FightView = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.local_active_micros, None);
        let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
        for scope in [session(), today(0, 100_000_000)] {
            let home = reader.home_summary(scope, Some(ACCOUNT)).unwrap();
            // This is Home's existing empty-tile contract, not a query error.
            assert_eq!(home.top_fight, None);
            assert_eq!(home.runs, 1);
            assert!(!home.recent_runs.is_empty());
        }
        let current = eligible_fight("current", 2_000_000);
        add(&mut db, vec![EngineEvent::FightEnded(current.clone())]);
        for scope in [session(), today(0, 100_000_000)] {
            assert_eq!(
                reader.home_summary(scope, Some(ACCOUNT)).unwrap().top_fight,
                Some(current.clone())
            );
        }
    }
}
