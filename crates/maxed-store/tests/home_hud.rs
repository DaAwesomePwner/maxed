use crate::common::account_scope_fixture::ACCOUNT;
use crate::common::stats::{character, database, fame, session, Stream};

use maxed_types::*;
use rusqlite::Connection;

const SECOND: u64 = 1_000_000;

#[test]
fn home_hud_and_scope_loot_preserve_catalog_limits() {
    let (_dir, path, db) = database();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    db.execute("INSERT OR IGNORE INTO identity_spans SELECT 'fixture-'||id,1,id,started,last_traffic,'synthetic-fixture-owner',?1 FROM play_sessions", [ACCOUNT]).unwrap();
    for (tiers, potions) in [
        (vec![(0, None); 65_537], vec![]),
        (vec![(0, Some("a".repeat(17)))], vec![]),
        (vec![(0, Some("é".repeat(9)))], vec![]),
        (vec![], vec![(0, PotionStat::Life); 65_537]),
    ] {
        assert!(matches!(
            reader.home_hud(scope(), Some(ACCOUNT), &tiers, &potions),
            Err(maxed_store::StoreError::Limit)
        ));
        if potions.is_empty() {
            assert!(matches!(
                reader.scope_loot(scope(), Some(ACCOUNT), None, 1, &tiers),
                Err(maxed_store::StoreError::Limit)
            ));
        }
    }
    let tiers = vec![(0, Some("é".repeat(8))); 65_536];
    let potions = vec![(0, PotionStat::Life); 65_536];
    reader
        .home_hud(scope(), Some(ACCOUNT), &tiers, &potions)
        .unwrap();
    reader
        .scope_loot(scope(), Some(ACCOUNT), None, 1, &tiers)
        .unwrap();
}

#[test]
fn home_hud_party_average_plan_searches_scoped_fights() {
    let (_dir, _path, mut db) = database();
    seed(
        &mut db,
        vec![EngineEvent::FightUpdated(fight(
            "scoped",
            0,
            10,
            Some(1000.0),
        ))],
    );
    let sql = &crate::common::account_sql::with_accounts(include_str!("../src/home_hud.sql"))
        .replace(
            "/*home_scope*/",
            &crate::common::account_sql::with_accounts(include_str!("../src/home_scope.sql")),
        )
        .replace(
            "/*party_average_scoped*/",
            include_str!("../src/party_average_scoped.sql"),
        );
    for kind in [0, 1] {
        let mut statement = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan = statement
            .query_map(
                rusqlite::params![
                    kind,
                    "synthetic-session",
                    0,
                    600_000_000_i64,
                    ACCOUNT,
                    "[]",
                    "[]"
                ],
                |row| row.get::<_, String>(3),
            )
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");
        eprintln!("Home HUD kind={kind}:\n{plan}");
        assert!(
            plan.lines()
                .any(|line| line.contains("SEARCH fight_players") && line.contains("fight_id=?")),
            "{plan}"
        );
        assert!(!plan.contains("SCAN fight_players"), "{plan}");
    }
}

fn scope() -> HomeScope {
    HomeScope::Session {
        id: "synthetic-session".into(),
    }
}

fn today(start: u64, end: u64) -> HomeScope {
    HomeScope::Today {
        range: TimeRange {
            start: Timestamp(start),
            end: Timestamp(end),
        },
    }
}

fn fight(id: &str, started: u64, seconds: u64, dps: Option<f64>) -> FightView {
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.id = id.into();
    fight.session_id = "synthetic-session".into();
    fight.run_id = None;
    fight.owner_character_id = Some("synthetic-alpha".into());
    fight.started = Timestamp(started);
    fight.first_hit = Some(Timestamp(started + SECOND));
    fight.last_hit = Some(Timestamp(started + (seconds + 1) * SECOND));
    // The idle time before/after damage must not contribute to the weight.
    fight.ended = Some(Timestamp(started + (seconds + 20) * SECOND));
    fight.kill = None;
    fight.issues.clear();
    fight.players.truncate(1);
    fight.players[0].is_local = true;
    fight.players[0].dps = dps;
    fight.players[0].rank = Some(2);
    fight
}

fn seed(db: &mut Connection, events: Vec<EngineEvent>) {
    let mut stream = Stream::new("synthetic-hud");
    stream.apply(
        db,
        vec![
            session("synthetic-session", 0, 600 * SECOND, None),
            character("synthetic-alpha", "Synthetic Alpha", 0),
            character("synthetic-beta", "Synthetic Beta", 0),
        ],
    );
    stream.apply(db, events);
}

fn party(fight: &mut FightView, dps: Option<f64>) {
    let mut other = fight.players[0].clone();
    other.player_id = "SyntheticOther".into();
    other.object_id += 1;
    other.is_local = false;
    other.dps = dps;
    fight.players.push(other);
}

fn bag(id: &str, at: u64, rank: Option<u8>, items: &[u16]) -> DropView {
    let mut bag = demo::demo_snapshot().drops.remove(0);
    bag.id = id.into();
    bag.session_id = "synthetic-session".into();
    bag.run_id = None;
    bag.owner_character_id = Some("synthetic-alpha".into());
    bag.at = Timestamp(at);
    bag.bag_rank = rank;
    bag.contents_known = true;
    bag.items = items.to_vec();
    bag.contents = items
        .iter()
        .enumerate()
        .map(|(slot, item_type)| DropItem {
            slot: slot as u8,
            item_type: *item_type,
            quantity: 5, // Count appearances, not stack sizes or observed pickups.
            taken_observed_at: None,
            enchant: EnchantView {
                state: EnchantState::Missing,
                slots: None,
                applied: None,
                ids: vec![],
            },
        })
        .collect();
    bag
}

#[test]
fn home_hud_weights_dps_by_fight_span() {
    let (_dir, path, mut db) = database();
    seed(
        &mut db,
        vec![
            EngineEvent::FightUpdated(fight("short", 0, 10, Some(4000.0))),
            EngineEvent::FightUpdated(fight("long", 100 * SECOND, 30, Some(2000.0))),
        ],
    );
    let hud = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .home_hud(scope(), Some(ACCOUNT), &[], &[])
        .unwrap();
    let combat = hud.combat.unwrap();
    assert_eq!(combat.fights, 2);
    assert_eq!(combat.weighted_dps, 2500.0);
    assert_eq!(combat.weighted_party_dps, None);
    assert_eq!(combat.vs_party, None);
    assert_eq!(combat.avg_rank, Some(2.0));
}

#[test]
fn home_hud_vs_party_uses_fights_with_both_values() {
    let (_dir, path, mut db) = database();
    let mut first = fight("first", 0, 10, Some(4000.0));
    first.players[0].rank = Some(1);
    party(&mut first, Some(1000.0));
    let mut second = fight("second", 100 * SECOND, 30, Some(2000.0));
    second.players[0].rank = Some(3);
    party(&mut second, Some(3000.0));
    let mut solo = fight("solo", 200 * SECOND, 10, Some(10000.0));
    solo.players[0].rank = None;
    party(&mut solo, None);
    let mut unknown = fight("unknown", 300 * SECOND, 10, None);
    party(&mut unknown, Some(100000.0));
    seed(
        &mut db,
        vec![first, second, solo, unknown]
            .into_iter()
            .map(EngineEvent::FightUpdated)
            .collect(),
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    db.execute("INSERT OR IGNORE INTO identity_spans SELECT 'fixture-'||id,1,id,started,last_traffic,'synthetic-fixture-owner',?1 FROM play_sessions", [ACCOUNT]).unwrap();
    let combat = reader
        .home_hud(scope(), Some(ACCOUNT), &[], &[])
        .unwrap()
        .combat
        .unwrap();
    assert_eq!(combat.fights, 3);
    assert_eq!(combat.weighted_dps, 4000.0);
    assert_eq!(combat.weighted_party_dps, Some(2500.0));
    assert_eq!(combat.vs_party, Some(0.0));
    assert_eq!(combat.avg_rank, Some(2.0));
    db.execute(
        "UPDATE fight_players SET dps=0 WHERE json_extract(json,'$.is_local')=0",
        [],
    )
    .unwrap();
    let combat = reader
        .home_hud(scope(), Some(ACCOUNT), &[], &[])
        .unwrap()
        .combat
        .unwrap();
    assert_eq!(combat.weighted_party_dps, Some(0.0));
    assert_eq!(combat.vs_party, None);
}

#[test]
fn home_hud_counts_whites_ut_st_and_potions_by_stat() {
    let (_dir, path, mut db) = database();
    let mut events: Vec<_> = (1..=14)
        .map(|i| {
            EngineEvent::BagDropped(bag(
                &format!("bag-{i}"),
                u64::from(i) * SECOND,
                Some(255),
                &[i],
            ))
        })
        .collect();
    events.push(EngineEvent::BagDropped(bag(
        "duplicate",
        20 * SECOND,
        Some(254),
        &[1, 1, 99],
    )));
    events.push(EngineEvent::BagDropped(bag(
        "potions-a",
        21 * SECOND,
        None,
        &[101, 102, 103, 104, 105, 106, 107, 108],
    )));
    events.push(EngineEvent::BagDropped(bag(
        "potions-b",
        22 * SECOND,
        None,
        &[103, 109],
    )));
    seed(&mut db, events);
    let mut tiers: Vec<_> = (1..=14)
        .map(|i| (i, Some(if i % 2 == 0 { "ST" } else { "UT" }.into())))
        .collect();
    tiers.extend([(1, Some("UT".into())), (99, Some("T6".into()))]);
    let potions = [
        (101, PotionStat::Life),
        (102, PotionStat::Mana),
        (103, PotionStat::Attack),
        (104, PotionStat::Defense),
        (105, PotionStat::Speed),
        (106, PotionStat::Dexterity),
        (107, PotionStat::Vitality),
        (108, PotionStat::Wisdom),
        (109, PotionStat::Life),
        (101, PotionStat::Life), // Repeated catalog entries must not multiply appearances.
    ];
    let hud = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .home_hud(scope(), Some(ACCOUNT), &tiers, &potions)
        .unwrap();
    assert_eq!(hud.whites, 14);
    assert_eq!(hud.ut_st_count, 14);
    assert_eq!(hud.ut_st, [1, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4]);
    assert_eq!(
        hud.potions
            .iter()
            .map(|p| (p.stat, p.count))
            .collect::<Vec<_>>(),
        [
            (PotionStat::Life, 2),
            (PotionStat::Attack, 2),
            (PotionStat::Mana, 1),
            (PotionStat::Defense, 1),
            (PotionStat::Speed, 1),
            (PotionStat::Dexterity, 1),
            (PotionStat::Vitality, 1),
            (PotionStat::Wisdom, 1),
        ]
    );
}

#[test]
fn home_hud_fame_per_hour_needs_five_minutes() {
    let (_dir, path, mut db) = database();
    seed(
        &mut db,
        vec![
            fame("synthetic-alpha", 100, 0),
            fame("synthetic-alpha", 125, 60 * SECOND),
            fame("synthetic-beta", 200, 120 * SECOND),
            fame("synthetic-beta", 225, 180 * SECOND),
        ],
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    db.execute("INSERT OR IGNORE INTO identity_spans SELECT 'fixture-'||id,1,id,started,last_traffic,'synthetic-fixture-owner',?1 FROM play_sessions", [ACCOUNT]).unwrap();
    for (elapsed, rate) in [(299_999_999, None), (300_000_000, Some(600.0))] {
        db.execute(
            "UPDATE play_sessions SET last_traffic=?1,ended=900000000",
            [elapsed],
        )
        .unwrap();
        db.execute("UPDATE identity_spans SET ended=?1", [elapsed])
            .unwrap();
        assert_eq!(
            reader
                .home_hud(scope(), Some(ACCOUNT), &[], &[])
                .unwrap()
                .fame_per_hour,
            rate
        );
    }
    db.execute("DELETE FROM fame_samples", []).unwrap();
    assert_eq!(
        reader
            .home_hud(scope(), Some(ACCOUNT), &[], &[])
            .unwrap()
            .fame_per_hour,
        None
    );
}

#[test]
fn home_hud_today_scope_sums_sessions() {
    let (_dir, path, mut db) = database();
    let mut first = Stream::new("synthetic-first");
    first.apply(
        &mut db,
        vec![
            session(
                "synthetic-session",
                50 * SECOND,
                350 * SECOND,
                Some(400 * SECOND),
            ),
            character("synthetic-alpha", "Synthetic Alpha", 50 * SECOND),
            character("synthetic-beta", "Synthetic Beta", 50 * SECOND),
            fame("synthetic-alpha", 0, 99 * SECOND),
            fame("synthetic-alpha", 100, 100 * SECOND),
            fame("synthetic-alpha", 125, 300 * SECOND),
            EngineEvent::FightUpdated(fight("included", 100 * SECOND, 10, Some(4000.0))),
            EngineEvent::FightUpdated(fight("before", 99 * SECOND, 10, Some(9000.0))),
            EngineEvent::BagDropped(bag("included", 100 * SECOND, Some(255), &[1])),
            EngineEvent::BagDropped(bag("before", 99 * SECOND, Some(255), &[1])),
        ],
    );
    let mut second = Stream::new("synthetic-second");
    let mut included = fight("other-session", 800 * SECOND, 30, Some(2000.0));
    included.session_id = "synthetic-session-two".into();
    let mut excluded = fight("at-end", 900 * SECOND, 10, Some(9000.0));
    excluded.session_id = included.session_id.clone();
    let mut last_bag = bag("at-end", 900 * SECOND, Some(255), &[1]);
    last_bag.session_id = included.session_id.clone();
    second.apply(
        &mut db,
        vec![
            session("synthetic-session-two", 700 * SECOND, 1000 * SECOND, None),
            fame("synthetic-alpha", 140, 700 * SECOND),
            fame("synthetic-alpha", 160, 899 * SECOND),
            fame("synthetic-alpha", 1000, 900 * SECOND),
            fame("synthetic-beta", 10, 750 * SECOND),
            fame("synthetic-beta", 25, 850 * SECOND),
            EngineEvent::FightUpdated(included),
            EngineEvent::FightUpdated(excluded),
            EngineEvent::BagDropped(last_bag),
        ],
    );
    let mut outside = Stream::new("synthetic-outside");
    outside.apply(
        &mut db,
        vec![
            session("outside-before", 0, 99 * SECOND, None),
            session("outside-after", 900 * SECOND, 1100 * SECOND, None),
        ],
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    db.execute("INSERT OR IGNORE INTO identity_spans SELECT 'fixture-'||id,1,id,started,last_traffic,'synthetic-fixture-owner',?1 FROM play_sessions", [ACCOUNT]).unwrap();
    let day = today(100 * SECOND, 900 * SECOND);
    assert_eq!(
        reader
            .home_summary(day.clone(), Some(ACCOUNT))
            .unwrap()
            .fame_delta,
        Some(75)
    );
    let hud = reader
        .home_hud(day, Some(ACCOUNT), &[(1, Some("UT".into()))], &[])
        .unwrap();
    assert_eq!(hud.fame_per_hour, Some(600.0)); // 75 fame / (250 + 200) seconds.
    assert_eq!(hud.combat.unwrap().weighted_dps, 2500.0);
    assert_eq!((hud.whites, hud.ut_st_count), (1, 1));
    assert_eq!(
        reader
            .home_hud(scope(), Some(ACCOUNT), &[], &[])
            .unwrap()
            .combat
            .unwrap()
            .fights,
        2
    );
}

#[test]
fn home_hud_combat_none_without_eligible_fights() {
    let (_dir, path, mut db) = database();
    seed(&mut db, vec![]);
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    db.execute("INSERT OR IGNORE INTO identity_spans SELECT 'fixture-'||id,1,id,started,last_traffic,'synthetic-fixture-owner',?1 FROM play_sessions", [ACCOUNT]).unwrap();
    let empty = reader.home_hud(scope(), Some(ACCOUNT), &[], &[]).unwrap();
    assert_eq!(empty.combat, None);
    assert_eq!(empty.fame_per_hour, None);
    assert_eq!(empty.whites, 0);
    assert_eq!(empty.ut_st_count, 0);
    assert!(empty.ut_st.is_empty() && empty.potions.is_empty());
    let mut events = vec![session("synthetic-session", 0, 600 * SECOND, None)];
    for i in 0..8 {
        let mut invalid = fight(&format!("invalid-{i}"), 100 * SECOND, 10, Some(4000.0));
        match i {
            0 => invalid.ended = None,
            1 => invalid.players[0].dps = None,
            2 => invalid.first_hit = None,
            3 => invalid.last_hit = invalid.first_hit,
            4 => invalid.last_hit = Some(Timestamp(100 * SECOND)),
            5 => invalid.first_hit = Some(Timestamp(99 * SECOND)),
            6 => invalid.last_hit = Some(Timestamp(140 * SECOND)),
            _ => invalid.ended = Some(Timestamp(99 * SECOND)),
        }
        events.push(EngineEvent::FightUpdated(invalid));
    }
    Stream::new("synthetic-invalid").apply(&mut db, events);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM fight_accounts WHERE account_id=?1",
            [ACCOUNT],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        8
    );
    assert_eq!(
        reader
            .home_hud(scope(), Some(ACCOUNT), &[], &[])
            .unwrap()
            .combat,
        None
    );
    db.execute("UPDATE fights SET json='broken'", []).unwrap();
    db.execute("UPDATE fight_summaries SET json='broken'", [])
        .unwrap();
    assert_eq!(
        reader
            .home_hud(scope(), Some(ACCOUNT), &[], &[])
            .unwrap()
            .combat,
        None
    );
}
