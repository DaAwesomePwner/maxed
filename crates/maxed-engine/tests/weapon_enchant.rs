//! D-weapon-enchant: weapon shots scale projectile bounds by the equipped weapon's
//! enchant damage multipliers before the roll. Synthetic names and IDs only.
use crate::common::*;
use maxed_engine::Engine;
use maxed_protocol::Packet;
use maxed_types::*;
use std::sync::Arc;

// Six-bit enchant codes for weapon slot 0 (header 0,2,4 + little-endian i16 IDs).
const E11: &str = "AAIECwA=";
const E11_12: &str = "AAIECwAMAA==";
const E11_END: &str = "AAIECwD9_w==";
const E11_16: &str = "AAIECwAQAA==";
const E13: &str = "AAIEDQA=";
const E14: &str = "AAIEDgA=";
const E15: &str = "AAIEDwA=";
const E16: &str = "AAIEEAA=";
const E17: &str = "AAIEEQA=";
const E19: &str = "AAIEEwA=";
const E20: &str = "AAIEFAA=";
const E0: &str = "AAIEAAA=";

fn supported(m: f64) -> Option<EnchantWeaponDamageDef> {
    Some(EnchantWeaponDamageDef {
        support: CatalogSupport::Supported,
        min_multiplier: Some(m),
        max_multiplier: Some(m),
        reasons: vec![],
    })
}

fn enchant(id: i32, weapon_damage: Option<EnchantWeaponDamageDef>) -> EnchantDef {
    EnchantDef {
        id,
        name: "Synthetic".into(),
        description: String::new(),
        stat_damage_multiplier: Some(1.0),
        regeneration: None,
        weapon_damage,
    }
}

fn weapon(object_type: u16, min_damage: i32, max_damage: i32) -> ItemDef {
    let mut item = catalog().items.remove(0);
    item.object_type = object_type;
    item.projectiles[0].min_damage = min_damage;
    item.projectiles[0].max_damage = max_damage;
    item
}

fn weapon_catalog() -> Catalog {
    let mut c = catalog();
    c.items.extend([
        weapon(210, 135, 135),
        weapon(211, 135, 175),
        weapon(212, 108, 140),
        weapon(213, 10, 11),
    ]);
    c.enchants = vec![
        enchant(11, supported(0.8)),
        enchant(12, supported(1.5)),
        enchant(13, None),
        enchant(
            14,
            Some(EnchantWeaponDamageDef {
                support: CatalogSupport::Unsupported,
                min_multiplier: None,
                max_multiplier: None,
                reasons: vec!["unsupported_subattack".into()],
            }),
        ),
        enchant(15, supported(f64::from(1.05f32))),
        // Valid catalog products whose scaled bounds are unusable: min above max,
        // above i32::MAX, or non-finite. Negative products are rejected by catalog
        // validation; the unit tests in src/weapon_enchant.rs cover that fallback.
        enchant(
            17,
            Some(EnchantWeaponDamageDef {
                min_multiplier: Some(2.0),
                max_multiplier: Some(0.5),
                ..supported(1.0).unwrap()
            }),
        ),
        enchant(19, supported(1e10)),
        enchant(20, supported(f64::from(f32::MAX))),
    ];
    c
}

fn shoot_with(bullet: i16, weapon: u16) -> Packet {
    let Packet::PlayerShoot(mut p) = shoot(bullet) else {
        unreachable!()
    };
    p.weaponId = weapon;
    Packet::PlayerShoot(p)
}

/// Equips `weapon` with weapon-slot enchant evidence `code` (None: no stat 80),
/// fires `bullets` shots and returns (fight total, weapon-enchant assumptions, estimated).
fn run(weapon: u16, code: Option<&str>, bullets: i16) -> (f64, u64, bool) {
    run_in(weapon_catalog(), weapon, code, bullets)
}

fn run_in(catalog: Catalog, weapon: u16, code: Option<&str>, bullets: i16) -> (f64, u64, bool) {
    let mut e = Engine::new(config(), Arc::new(catalog)).unwrap();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    let mut p = player();
    p.status
        .stats
        .retain(|s| s.statTypeNum != 80 && s.statTypeNum != 8);
    p.status.stats.push(stat(8, i32::from(weapon)));
    if let Some(code) = code {
        p.status.stats.push(string(80, &format!("{code},,,")));
    }
    apply(&mut e, 2000, update(vec![p, target()], vec![]));
    for bullet in 1..=bullets {
        apply(&mut e, 3000, shoot_with(bullet, weapon));
        apply(&mut e, 4000, hit(bullet));
    }
    let counts = e.reconstruction_coverage();
    assert_eq!(counts["attemptedHits"], bullets as u64);
    assert_eq!(counts["reconstructedHits"], bullets as u64);
    assert_eq!(counts["omittedHits"], 0);
    let snapshot = e.snapshot();
    let fight = &snapshot.fights[0];
    (
        fight.total_damage,
        counts["assumedWeaponEnchantDefault"],
        fight.players[0].sources[0].estimated,
    )
}

#[test]
fn known_enchant_products_scale_weapon_damage_and_stay_exact() {
    // Synthetic sword: fixed 100 damage, neutral attack, exaltation and defense.
    assert_eq!(run(200, Some(""), 1), (100., 0, false));
    assert_eq!(run(200, Some(E11), 1), (80., 0, false));
    // Slot-order f32 product: 0.8 * 1.5.
    assert_eq!(run(200, Some(E11_12), 1), (120., 0, false));
    // -3 terminates the applied list.
    assert_eq!(run(200, Some(E11_END), 1), (80., 0, false));
}

#[test]
fn scaled_bounds_truncate_the_f32_product() {
    // 135 * 1.05f32 = 141.75: truncation gives 141, rounding would give 142.
    assert_eq!(run(210, Some(E15), 1), (141., 0, false));
    assert_eq!(run(210, Some(""), 1), (135., 0, false));
}

#[test]
fn bounds_are_scaled_before_the_roll_with_the_same_draw() {
    // 135-175 scaled by 0.8 is 108-140. The same RNG draws must reproduce an
    // unenchanted 108-140 weapon, not 0.8 times an unenchanted 135-175 roll.
    let mut before_roll = vec![];
    let mut after_roll = vec![];
    let mut scaled = vec![];
    for bullets in 1..=8 {
        let total = |w, code| run(w, Some(code), bullets).0;
        let previous = |w, code| {
            if bullets == 1 {
                0.
            } else {
                run(w, Some(code), bullets - 1).0
            }
        };
        scaled.push(total(211, E11) - previous(211, E11));
        before_roll.push(total(212, "") - previous(212, ""));
        let plain = total(211, "") - previous(211, "");
        after_roll.push(f64::from((plain as f32 * 0.8f32) as i32));
    }
    assert_eq!(scaled, before_roll);
    assert_ne!(scaled, after_roll);
    assert!(scaled.iter().all(|d| (108. ..140.).contains(d)));
}

#[test]
fn missing_or_unusable_weapon_enchant_evidence_is_a_counted_neutral_assumption() {
    for (code, label) in [
        (None, "missing stat 80"),
        (Some("!!"), "invalid code"),
        (Some(E13), "legacy catalog metadata"),
        (Some(E14), "unsupported enchant"),
        (Some(E16), "unknown enchant ID"),
        (Some(E11_16), "one unknown among known"),
    ] {
        assert_eq!(run(200, code, 2), (200., 2, true), "{label}");
    }
}

#[test]
fn evidence_for_another_equipped_item_is_not_used() {
    let mut e = Engine::new(config(), Arc::new(weapon_catalog())).unwrap();
    setup(&mut e);
    // Slot 0 now holds a different item with recorded enchant evidence.
    apply(
        &mut e,
        2500,
        tick(vec![status(
            1,
            vec![stat(8, 210), string(80, &format!("{E11},,,"))],
        )]),
    );
    apply(&mut e, 3000, shoot_with(1, 200));
    apply(&mut e, 4000, hit(1));
    assert_eq!(e.snapshot().fights[0].total_damage, 100.);
    assert_eq!(
        e.reconstruction_coverage()["assumedWeaponEnchantDefault"],
        1
    );
    assert!(e.snapshot().fights[0].players[0].sources[0].estimated);
}

#[test]
fn ability_shots_ignore_weapon_enchant_evidence() {
    let mut e = Engine::new(config(), Arc::new(weapon_catalog())).unwrap();
    setup(&mut e);
    apply(
        &mut e,
        2500,
        tick(vec![status(1, vec![string(80, &format!("{E14},,,"))])]),
    );
    apply(&mut e, 3000, shoot_with(1, 201));
    apply(&mut e, 4000, hit(1));
    // Ability: 50 base + (60 - 50) * 2 scaling, armor piercing.
    assert_eq!(e.snapshot().fights[0].total_damage, 70.);
    assert_eq!(
        e.reconstruction_coverage()["assumedWeaponEnchantDefault"],
        0
    );
}

/// Fires one shot per `(weapon, code)`, re-equipping slot 0 with that weapon and
/// weapon-slot enchant evidence before each shot. Returns each hit's damage.
fn shots(sequence: &[(u16, &str)]) -> Vec<f64> {
    let mut e = Engine::new(config(), Arc::new(weapon_catalog())).unwrap();
    setup(&mut e);
    let mut damages = vec![];
    let mut total = 0.;
    for (bullet, &(weapon, code)) in (1..).zip(sequence) {
        apply(
            &mut e,
            2500,
            tick(vec![status(
                1,
                vec![
                    stat(8, i32::from(weapon)),
                    string(80, &format!("{code},,,")),
                ],
            )]),
        );
        apply(&mut e, 3000, shoot_with(bullet, weapon));
        apply(&mut e, 4000, hit(bullet));
        let now = e.snapshot().fights[0].total_damage;
        damages.push(now - total);
        total = now;
    }
    assert_eq!(
        e.reconstruction_coverage()["assumedWeaponEnchantDefault"],
        0
    );
    damages
}

#[test]
fn scaled_bounds_that_become_equal_skip_the_roll_and_shift_later_draws() {
    // 10-11 scaled by 0.8 truncates to 8-8: the shot deals 8 and, as for any
    // equal-bounds projectile, consumes no RNG draw. Unscaled 10-11 still draws
    // (and always deals 10, since the maximum is exclusive).
    let skipped = shots(&[(213, E11), (211, "")]);
    let drawn = shots(&[(213, ""), (211, "")]);
    assert_eq!(skipped[0], 8.);
    assert_eq!(drawn[0], 10.);
    // The next shot therefore takes the first draw after a skipped roll, and the
    // second draw after an unscaled 10-11 roll.
    assert_eq!(skipped[1], shots(&[(211, "")])[0]);
    assert_eq!(drawn[1], shots(&[(211, ""), (211, "")])[1]);
    assert_ne!(skipped[1], drawn[1]);
}

#[test]
fn unusable_scaled_bounds_keep_the_unscaled_roll_as_a_counted_assumption() {
    for (code, label) in [
        (E17, "scaled minimum above scaled maximum"),
        (E19, "bounds above i32::MAX"),
        (E20, "non-finite bounds"),
    ] {
        assert_eq!(run(200, Some(code), 2), (200., 2, true), "{label}");
        // The fallback still draws: each shot matches the unscaled weapon's roll.
        for bullets in 1..=3 {
            let (fallback, defaults, estimated) = run(211, Some(code), bullets);
            assert_eq!((defaults, estimated), (bullets as u64, true), "{label}");
            assert_eq!(fallback, run(211, Some(""), bullets).0, "{label}");
        }
    }
}

#[test]
fn enchant_id_zero_is_an_applied_weapon_enchant() {
    // parse_enchant counts ID 0 as applied (state Recorded), so the weapon path
    // resolves it like any other ID instead of treating it as empty.
    let mut known = weapon_catalog();
    known.enchants.insert(0, enchant(0, supported(0.8)));
    assert_eq!(run_in(known, 200, Some(E0), 1), (80., 0, false));
    // Without a catalog definition it is unknown: neutral, counted and estimated,
    // never a silent known 1.0.
    assert_eq!(run(200, Some(E0), 1), (100., 1, true));
}
