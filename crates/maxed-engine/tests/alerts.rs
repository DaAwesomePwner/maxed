use maxed_engine::{alerts::*, FLAWLESS_UNSUPPORTED};
use maxed_types::*;

fn candidate() -> (FightView, CharacterView) {
    let mut fight = demo::demo_snapshot().fights.remove(0);
    let mut character = demo::demo_run_story().character.unwrap();
    character.identity_verified = true;
    fight.complete_capture = true;
    fight.issues.clear();
    fight.ended = Some(Timestamp(10_000_000));
    fight.end_reason = Some(FightEnd::MapChanged);
    fight.first_hit = Some(Timestamp(1_000_000));
    fight.last_hit = Some(Timestamp(2_000_000));
    fight.boss_type = Some(42);
    fight.players.retain(|p| p.is_local);
    fight.players[0].player_id = character.id.clone();
    fight.players[0].damage = 100.;
    fight.players[0].dps = Some(100.);
    for source in &mut fight.players[0].sources {
        source.estimated = false;
    }
    (fight, character)
}

#[test]
fn pb_requires_final_complete_unique_verified_self() {
    let (fight, character) = candidate();
    assert_eq!(
        personal_best_candidate(&fight, &character).unwrap().dps,
        100.
    );
    let variations: Vec<fn(&mut FightView)> = vec![
        (|f| f.ended = None),
        (|f| f.pending_local_reconciliation = true),
        (|f| f.end_reason = None),
        (|f| f.end_reason = Some(FightEnd::CaptureStopped)),
        (|f| f.complete_capture = false),
        (|f| f.boss_type = None),
        (|f| f.issues.push(CoverageIssue::StreamGap)),
        (|f| f.players.clear()),
        (|f| f.players.push(f.players[0].clone())),
        (|f| f.players[0].player_id = "another".into()),
        (|f| f.players[0].sources[0].estimated = true),
        (|f| f.first_hit = f.last_hit),
        (|f| f.first_hit = Some(Timestamp(3_000_000))),
        (|f| f.last_hit = None),
        (|f| f.players[0].dps = None),
    ];
    for vary in variations {
        let mut f = fight.clone();
        vary(&mut f);
        assert!(personal_best_candidate(&f, &character).is_none());
    }
    for value in [f64::NAN, f64::INFINITY, 0., -1.] {
        let mut f = fight.clone();
        f.players[0].dps = Some(value);
        assert!(personal_best_candidate(&f, &character).is_none());
        let mut f = fight.clone();
        f.players[0].damage = value;
        assert!(personal_best_candidate(&f, &character).is_none());
    }
    let mut c = character.clone();
    c.identity_verified = false;
    assert!(personal_best_candidate(&fight, &c).is_none());
    let mut f = fight.clone();
    let mut remote = f.players[0].clone();
    remote.is_local = false;
    remote.sources[0].estimated = true;
    f.players.push(remote);
    assert!(personal_best_candidate(&f, &character).is_some());
}

#[test]
fn pb_compares_before_update_and_first_is_silent() {
    let (fight, character) = candidate();
    let mut c = personal_best_candidate(&fight, &character).unwrap();
    assert!(!beats_previous(&c, None));
    assert!(!beats_previous(&c, Some(100.)));
    c.dps = 99.;
    assert!(!beats_previous(&c, Some(100.)));
    c.dps = 100.01;
    assert!(beats_previous(&c, Some(100.)));
}

#[test]
fn flawless_remains_unsupported() {
    assert_eq!(
        FLAWLESS_UNSUPPORTED,
        "Flawless is unavailable: no approved captured-evidence definition exists."
    );
}

#[test]
fn white_requires_rank_255_even_without_contents() {
    let mut drop = demo::demo_run_story().drops.remove(0);
    drop.contents_known = false;
    for rank in [None, Some(0), Some(254)] {
        drop.bag_rank = rank;
        assert!(!is_white_bag(&drop));
    }
    drop.bag_rank = Some(255);
    assert!(is_white_bag(&drop));
}

#[test]
fn pb_accepts_killed_end_reason() {
    let (mut f, c) = candidate();
    let before = personal_best_candidate(&f, &c).unwrap();
    f.end_reason = Some(FightEnd::Killed);
    assert_eq!(personal_best_candidate(&f, &c).unwrap().dps, before.dps);
}
#[test]
fn pb_killed_requires_complete_capture() {
    let (mut f, c) = candidate();
    f.end_reason = Some(FightEnd::Killed);
    f.complete_capture = false;
    assert!(personal_best_candidate(&f, &c).is_none());
}
fn with_kill() -> (FightView, CharacterView) {
    let (mut f, c) = candidate();
    f.kill = Some(KillView {
        at: Timestamp(2_000_000),
        evidence: KillEvidence::VictorySoleBoss,
        provisional: true,
    });
    (f, c)
}
#[test]
fn pb_rejects_capture_stopped_even_with_kill_view() {
    let (mut f, c) = with_kill();
    f.end_reason = Some(FightEnd::CaptureStopped);
    assert!(personal_best_candidate(&f, &c).is_none());
}
#[test]
fn pb_kill_view_without_ended_is_not_candidate() {
    let (mut f, c) = with_kill();
    f.ended = None;
    assert!(personal_best_candidate(&f, &c).is_none());
}
#[test]
fn pb_provisional_kill_does_not_change_eligibility() {
    let (mut f, c) = with_kill();
    f.end_reason = Some(FightEnd::Killed);
    assert!(personal_best_candidate(&f, &c).is_some());
    f.kill.as_mut().unwrap().provisional = false;
    assert!(personal_best_candidate(&f, &c).is_some());
}
#[test]
fn pb_baseline_key_unchanged_by_target_kind() {
    let (mut f, c) = candidate();
    let boss = personal_best_candidate(&f, &c).unwrap().boss_type;
    f.target_kind = Some(TargetKind::EventBoss);
    assert_eq!(personal_best_candidate(&f, &c).unwrap().boss_type, boss);
    f.boss_type = None;
    assert!(personal_best_candidate(&f, &c).is_none());
}
