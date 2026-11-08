use crate::d_support;
use d_support::*;
#[test]
fn character_stat_panel_uses_known_equipment_and_leaves_exalt_unknown() {
    let mut c = catalog();
    c.items[0].stat_bonuses = Some(vec![maxed_types::StatView {
        stat_id: 20,
        value: 5,
    }]);
    let mut e = maxed_engine::Engine::new(config(), std::sync::Arc::new(c)).unwrap();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        tick(vec![status(
            1,
            vec![
                stat(8, 200),
                stat(9, -1),
                stat(10, -1),
                stat(11, -1),
                stat(20, 80),
                stat(48, 10),
                stat(29, 4),
            ],
        )]),
    );
    let p = e.snapshot().character.unwrap().stat_panel.unwrap();
    // Placement is verified even when some split operands remain unknown.
    assert!(!p.breakdown_provisional);
    let line = p.lines.iter().find(|s| s.stat_id == 20).unwrap();
    assert_eq!(
        (line.base, line.cap, line.gear, line.other, line.exalt),
        (Some(70), Some(75), Some(5), None, None)
    );
    assert_eq!(line.at_cap, None);
    assert!(p.conditions.iter().any(|c| c.name == "Weak" && c.confirmed));
    apply(&mut e, 4000, tick(vec![status(1, vec![stat(11, 999)])]));
    let p = e.snapshot().character.unwrap().stat_panel.unwrap();
    assert!(p
        .lines
        .iter()
        .all(|s| s.gear.is_none() && s.other.is_none()));
}

#[test]
fn observed_levels_survive_sparse_updates_without_becoming_new_observations() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        tick(vec![status(
            1,
            vec![
                stat(105, 2),
                stat(111, 0),
                stat(20, 81),
                stat(48, 6),
                stat(0, 750),
                stat(46, 0),
            ],
        )]),
    );
    for (at, stats) in [(4000, vec![stat(1, 600)]), (5000, vec![stat(20, 80)])] {
        apply(&mut e, at, tick(vec![status(1, stats)]));
        let c = e.snapshot().character.unwrap();
        assert!(c.stats.iter().any(|s| s.stat_id == 105 && s.value == 2));
        let p = c.stat_panel.unwrap();
        let attack = p.lines.iter().find(|s| s.stat_id == 20).unwrap();
        assert_eq!(attack.exalt, Some(2));
        assert_eq!(attack.at_cap, Some(at == 4000));
        assert_eq!(p.lines[0].exalt, Some(0));
        assert_eq!(
            attack.split_evidence.as_ref().unwrap().placement,
            maxed_types::ExaltPlacement::VerifiedInsideBoost
        );
    }
}
