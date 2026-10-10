mod common;
mod d_support;
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
    assert!(p.breakdown_provisional);
    let line = p.lines.iter().find(|s| s.stat_id == 20).unwrap();
    assert_eq!(
        (line.base, line.cap, line.gear, line.other, line.exalt),
        (Some(70), Some(75), Some(5), Some(5), None)
    );
    assert_eq!(line.at_cap, Some(false));
    assert!(p.conditions.iter().any(|c| c.name == "Weak" && c.confirmed));
    apply(&mut e, 4000, tick(vec![status(1, vec![stat(11, 999)])]));
    let p = e.snapshot().character.unwrap().stat_panel.unwrap();
    assert!(p
        .lines
        .iter()
        .all(|s| s.gear.is_none() && s.other.is_none()));
}
