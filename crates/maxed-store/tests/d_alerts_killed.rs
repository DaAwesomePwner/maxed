use crate::common;
use maxed_store::*;
use maxed_types::*;
use rusqlite::Connection;
#[test]
fn pb_alert_fires_for_killed_boss_fight() {
    check(FightEnd::MapChanged); // Positive control for fixture eligibility.
    check(FightEnd::Killed);
}
fn check(reason: FightEnd) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("alerts");
    let writer = start_writer(
        path.clone(),
        WriterConfig {
            stream_id: "test-stream".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let send = |event| {
        writer
            .try_submit_with_alert_context(
                event,
                AlertContext {
                    chat: None,
                    live: true,
                    preferences: AlertPreferences {
                        personal_best: AlertLoudness::Banner,
                        ..Default::default()
                    },
                    key_pop_dungeon: None,
                    bag_window: None,
                },
            )
            .unwrap();
        writer.flush().unwrap();
    };
    let mut character = demo::demo_snapshot().character.unwrap();
    character.identity_verified = true;
    send(common::envelope(
        1,
        EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
    ));
    send(common::envelope(
        2,
        EngineEvent::CharacterUpdated(character.clone()),
    ));
    for (i, damage) in [100., 200.].into_iter().enumerate() {
        let mut f = demo::demo_snapshot().fights.remove(0);
        f.id = format!("killed-{i}");
        f.run_id = None;
        f.boss_type = Some(42);
        f.complete_capture = true;
        f.issues.clear();
        f.ended = Some(Timestamp(3_000_000));
        f.first_hit = Some(Timestamp(1_000_000));
        f.last_hit = Some(Timestamp(2_000_000));
        f.end_reason = Some(reason.clone());
        f.target_kind = Some(TargetKind::Boss);
        f.kill = Some(KillView {
            at: Timestamp(2_000_000),
            evidence: KillEvidence::HpZeroRemoved,
            provisional: false,
        });
        f.players.retain(|p| p.is_local);
        f.players[0].player_id = character.id.clone();
        f.players[0].damage = damage;
        f.players[0].dps = Some(damage);
        f.total_damage = damage;
        f.unattributed_damage = 0.;
        for s in &mut f.players[0].sources {
            s.estimated = false;
            s.reasons.clear();
        }
        send(common::envelope(i as u64 + 3, EngineEvent::FightEnded(f)));
    }
    let db = Connection::open(path).unwrap();

    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM alert_occurrences WHERE kind='personal_best' AND state='pending'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1,
        "{reason:?}"
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM alerts WHERE kind='personal_best'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    writer.shutdown().unwrap();
}
