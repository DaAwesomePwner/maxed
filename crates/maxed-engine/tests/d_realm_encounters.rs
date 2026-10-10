mod common;
mod d_support;
use d_support::*;
use maxed_assets::{build_catalog, ExtractedAssets, TextAsset};
use maxed_engine::Engine;
use maxed_types::*;
use std::sync::Arc;

fn encounter_engine(ty: u16) -> Engine {
    let extracted = build_catalog(
        ExtractedAssets {
            english: Default::default(),
            texts: vec![TextAsset {
                name: "synthetic-encounters".into(),
                bytes: include_bytes!("../../../tests/fixtures/scrubbed/realm-encounters.xml")
                    .to_vec(),
            }],
            textures: vec![],
            sprite_mapping: vec![],
        },
        "a".repeat(64),
    )
    .unwrap();
    let mut c = catalog();
    c.objects.extend(extracted.objects);
    let mut e = Engine::new(config(), Arc::new(c)).unwrap();
    apply(&mut e, 0, map("Realm"));
    apply(&mut e, 1000, create(1, 1));
    apply(
        &mut e,
        2000,
        update(
            vec![
                player(),
                object(2, ty, vec![stat(0, 125000), stat(1, 125000)]),
            ],
            vec![],
        ),
    );
    apply(&mut e, 3000, damage(1, 1, 100));
    e
}

#[test]
fn realm_encounters_are_boss_tier_without_a_quest_arrow() {
    for ty in [22146, 45104, 34565, 47930, 24005] {
        let e = encounter_engine(ty);
        assert_eq!(
            e.snapshot().fights[0].target_kind,
            Some(TargetKind::EventBoss),
            "type {ty}"
        );
    }
    for ty in [0x8706, 0x8709, 0x8707, 0x5dca, 0x5dc6, 0x5dc7, 0xbb3b, 11] {
        let e = encounter_engine(ty);
        assert_eq!(
            e.snapshot().fights[0].target_kind,
            Some(TargetKind::Ordinary),
            "type {ty}"
        );
    }
}

#[test]
fn internal_event_keys_resolve_observed_variants_and_link_unique_fights() {
    for (ty, key) in [
        (22146, "LH_Lost_Sentry"),
        (45104, "LH_Lost_Sentry"),
        (47930, "Big_Barnacle"),
        (34565, "Maze_Minotaur"),
        (24005, "Legion_General"),
    ] {
        let mut e = encounter_engine(ty);
        let rows = instances(apply(&mut e, 4000, oryx_killed(key, "SynthBravo")));
        assert_eq!(rows.len(), 1);
        assert!(
            matches!(rows[0].kind, InstanceEventKind::RealmEventKilled {object_type: Some(t), ..} if t == ty)
        );
        let fight = &e.snapshot().fights[0];
        let kill = fight
            .kill
            .as_ref()
            .expect("announcement linked to active fight");
        assert_eq!(kill.evidence, KillEvidence::RealmAnnouncement);
        assert!(kill.provisional);
        assert!(fight.ended.is_none());
    }
}

#[test]
fn aliases_preserve_ambiguity_and_do_not_guess_unknown_prefixes() {
    let mut e = encounter_engine(22146);
    let rows = instances(apply(
        &mut e,
        4000,
        oryx_killed("Unknown_LH_Lost_Sentry", "SynthBravo"),
    ));
    assert!(matches!(
        rows[0].kind,
        InstanceEventKind::RealmEventKilled {
            object_type: None,
            ..
        }
    ));
    assert!(e.snapshot().fights[0].kill.is_none());
    apply(&mut e, 5000, update(vec![object(3, 45104, vec![])], vec![]));
    let maxed_protocol::Packet::Damage(mut packet) = damage(1, 2, 100) else {
        unreachable!()
    };
    packet.targetId = 3;
    apply(&mut e, 6000, maxed_protocol::Packet::Damage(packet));
    apply(&mut e, 7000, oryx_killed("LH_Lost_Sentry", "SynthBravo"));
    assert_eq!(e.snapshot().fights.len(), 2);
    assert!(e.snapshot().fights.iter().all(|f| f.kill.is_none()));
}

#[test]
fn unobserved_announcements_prefer_the_exact_display_name_over_a_legacy_alias() {
    // Neither Lord of the Lost Lands variant is observed; the legacy object's
    // XML id matches the key but its display name does not.
    let mut e = encounter_engine(22146);
    let rows = instances(apply(
        &mut e,
        4000,
        oryx_killed("Lord_of_the_Lost_Lands", "SynthBravo"),
    ));
    assert!(matches!(
        rows[0].kind,
        InstanceEventKind::RealmEventKilled {
            object_type: Some(0x55ec),
            ..
        }
    ));
}

#[test]
fn alias_death_announcements_confirm_observed_target_removal() {
    for (ty, key) in [(22146, "LH_Lost_Sentry"), (47930, "Big_Barnacle")] {
        let mut e = encounter_engine(ty);
        apply(&mut e, 4000, tick(vec![status(2, vec![stat(1, 1)])]));
        apply(&mut e, 5000, update(vec![], vec![2]));
        let maxed_protocol::Packet::Text(mut p) = oryx_killed(key, "SyntheticKiller") else {
            unreachable!()
        };
        p.text = format!(r#"{{"k":"stringlist.{key}.death.0"}}"#);
        let rows = instances(apply(&mut e, 5000, maxed_protocol::Packet::Text(p)));
        assert!(
            matches!(rows[0].kind, InstanceEventKind::RealmEventKilled { object_type: Some(t), killer: None, .. } if t == ty)
        );
        let f = e.snapshot().fights.remove(0);
        assert_eq!(
            f.kill.unwrap().evidence,
            KillEvidence::AnnouncedDeathRemoved
        );
        assert_eq!(f.ended, Some(Timestamp(5000)));
    }
}
