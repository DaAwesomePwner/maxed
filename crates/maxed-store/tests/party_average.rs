use crate::common::{envelope, story_events};
use maxed_store::{apply_batch, migrate, open_reader};
use maxed_types::*;
use rusqlite::Connection;

fn database(parties: &[&[Option<f64>]]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    let events = story_events();
    apply_batch(&mut db, &events[..3]).unwrap();
    for (index, party) in parties.iter().enumerate() {
        let mut fight = demo::demo_snapshot().fights.remove(0);
        fight.id = format!("synthetic-fight-{index}");
        fight.started = Timestamp(index as u64);
        fight.unattributed_damage = 100_000.0;
        let template = fight.players[0].clone();
        fight.players = party
            .iter()
            .enumerate()
            .map(|(index, dps)| {
                let mut player = template.clone();
                player.player_id = format!("SyntheticPlayer{index}");
                player.object_id = index as i32 + 1;
                player.is_local = index == 0;
                player.dps = *dps;
                player
            })
            .collect();
        apply_batch(
            &mut db,
            &[envelope(index as u64 + 4, EngineEvent::FightStarted(fight))],
        )
        .unwrap();
    }
    (dir, path)
}

#[test]
fn party_average_excludes_local_and_null_dps() {
    let (_dir, path) = database(&[&[Some(5000.0), Some(3000.0), Some(1000.0), None]]);
    let story = open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap();
    let summary = serde_json::to_value(&story.fights[0]).unwrap();
    assert_eq!(summary["party"]["avg_dps"], 2000.0);
    assert_eq!(summary["party"]["players"], 2);
}

#[test]
fn party_average_none_without_other_players() {
    let (_dir, path) = database(&[&[Some(5000.0)]]);
    let story = open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap();
    assert_eq!(story.fights[0].party, None);
}

#[test]
fn run_story_fight_summaries_carry_party_average() {
    let (_dir, path) = database(&[
        &[Some(5000.0), Some(3000.0), Some(1000.0), None],
        &[Some(9000.0), Some(0.0)],
        &[Some(5000.0)],
    ]);
    let story = open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap();
    assert_eq!(story.fights.len(), 3);
    for (index, fight) in story.fights.iter().enumerate() {
        assert_eq!(fight.id, format!("synthetic-fight-{index}"));
        assert_eq!(fight.ordinal, index as u32 + 1);
    }
    let summaries = serde_json::to_value(&story.fights).unwrap();
    assert_eq!(
        summaries[0]["party"],
        serde_json::json!({"avg_dps": 2000.0, "players": 2})
    );
    assert_eq!(
        summaries[1]["party"],
        serde_json::json!({"avg_dps": 0.0, "players": 1})
    );
    assert!(summaries[2]["party"].is_null());
}

#[test]
fn party_average_none_when_other_dps_is_unknown() {
    let (_dir, path) = database(&[&[Some(5000.0), None, None]]);
    let story = open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap();
    assert_eq!(story.fights[0].party, None);
}

#[test]
fn party_average_accepts_legacy_summaries_and_missing_local_flags() {
    let (_dir, path) = database(&[&[Some(5000.0), Some(3000.0), Some(1000.0), None]]);
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE fight_summaries SET json=json_remove(json,'$.party')",
        [],
    )
    .unwrap();
    db.execute("UPDATE fight_players SET json=json_remove(json,'$.is_local') WHERE player_id='SyntheticPlayer2'", []).unwrap();
    let story = open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap();
    assert_eq!(
        story.fights[0].party,
        Some(PartyAverage {
            avg_dps: 2000.0,
            players: 2
        })
    );
}

#[test]
fn party_average_excludes_malformed_other_player_json() {
    let (_dir, path) = database(&[&[Some(5000.0), Some(3000.0), Some(1000.0)]]);
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.execute(
            "UPDATE fight_players SET json='broken' WHERE player_id='SyntheticPlayer1'",
            [],
        )
        .unwrap(),
        1
    );
    let story = open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap();
    assert_eq!(
        story.fights[0].party,
        Some(PartyAverage {
            avg_dps: 1000.0,
            players: 1
        })
    );
}

#[test]
fn run_story_without_fights_has_no_party_rows() {
    let (_dir, path) = database(&[]);
    let story = open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap();
    assert!(story.fights.is_empty());
}
