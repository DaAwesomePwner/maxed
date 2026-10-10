mod common;
mod d_support;
use d_support::*;
use maxed_engine::Engine;
use maxed_types::*;
use std::sync::Arc;

fn duplicate_catalog() -> Catalog {
    let mut c = catalog();
    for (ty, event, sprite) in [
        (400, false, "visible:0"),
        (401, true, "invisible:0"),
        (402, true, "visible:1"),
        (403, true, "visible:2"),
    ] {
        let mut o = c.objects[2].clone();
        o.object_type = ty;
        o.name = "Grand Sphinx".into();
        o.event_boss = Some(event);
        o.sprite_key = Some(sprite.into());
        c.objects.push(o);
    }
    c
}

fn announced(e: &mut Engine, key: &str, at: u64) -> Option<u16> {
    let rows = instances(apply(e, at, oryx_killed(key, "SynthBravo")));
    assert_eq!(rows.len(), 1);
    assert!(rows[0].provisional);
    let InstanceEventKind::RealmEventKilled { object_type, .. } = rows[0].kind else {
        panic!("expected realm announcement");
    };
    object_type
}

#[test]
fn duplicate_resolution_is_order_independent_and_unknown_stays_unknown() {
    for reverse in [false, true] {
        let mut c = duplicate_catalog();
        if reverse {
            c.objects.reverse();
        }
        let mut e = Engine::new(config(), Arc::new(c)).unwrap();
        setup(&mut e);
        assert_eq!(announced(&mut e, "Grand_Sphinx", 3000), Some(402));
        assert_eq!(announced(&mut e, "Unknown_Boss", 4000), None);
    }
}

#[test]
fn observed_variant_wins_even_after_removal_and_eviction() {
    let mut e = Engine::new(config(), Arc::new(duplicate_catalog())).unwrap();
    setup(&mut e);
    apply(&mut e, 3000, update(vec![object(9, 400, vec![])], vec![]));
    apply(&mut e, 4000, update(vec![], vec![9]));
    e.advance_to(Timestamp(31_000_000));
    assert_eq!(announced(&mut e, "Grand_Sphinx", 32_000_000), Some(400));
    apply(&mut e, 33_000_000, map("Realm"));
    assert_eq!(announced(&mut e, "Grand_Sphinx", 34_000_000), Some(402));
}

#[test]
fn observed_candidates_still_use_event_sprite_and_type_tiebreakers() {
    let mut e = Engine::new(config(), Arc::new(duplicate_catalog())).unwrap();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(
            vec![object(9, 400, vec![]), object(10, 401, vec![])],
            vec![],
        ),
    );
    assert_eq!(announced(&mut e, "Grand_Sphinx", 4000), Some(401));
    apply(
        &mut e,
        5000,
        update(
            vec![object(11, 403, vec![]), object(12, 402, vec![])],
            vec![],
        ),
    );
    assert_eq!(announced(&mut e, "Grand_Sphinx", 6000), Some(402));
}
