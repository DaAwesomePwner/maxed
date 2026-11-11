use maxed_engine::character::{calculate, input_revision, CharacterInput};
use maxed_types::*;

fn fixture(att: i32, dex: i32, min: i32, max: i32) -> (CharacterView, Catalog) {
    let mut character = maxed_types::demo::demo_snapshot().character.unwrap();
    character.stats = vec![
        StatView {
            stat_id: 20,
            value: att,
        },
        StatView {
            stat_id: 28,
            value: dex,
        },
        StatView {
            stat_id: 29,
            value: 0,
        },
    ];
    character.equipment = vec![EquipmentView {
        slot: 0,
        item_type: Some(1),
        observed_empty: Some(false),
        enchant: EnchantView {
            state: EnchantState::RecordedEmpty,
            slots: Some(0),
            applied: Some(0),
            ids: vec![],
        },
    }];
    let mut catalog = maxed_types::demo::demo_catalog();
    catalog.client_fingerprint = "a".repeat(64);
    let mut item = catalog.items.remove(0);
    item.object_type = 1;
    item.projectiles = vec![ProjectileDef {
        index: 0,
        min_damage: min,
        max_damage: max,
        armor_piercing: false,
        lifetime_millis: None,
    }];
    item.weapon_firing = Some(WeaponFiringDef {
        subattack_projectile_indices: None,
        support: CatalogSupport::Supported,
        omitted_projectile_indices: vec![],
        reasons: vec![],
        groups: vec![WeaponAttackGroup {
            group_id: "normal".into(),
            projectile_index: 0,
            count: 1,
            rate_factor: 1.0,
            schedule: WeaponSchedule::Simultaneous,
            reasons: vec![],
        }],
    });
    catalog.items = vec![item];
    (character, catalog)
}

fn run(character: &CharacterView, catalog: Option<&Catalog>, defense: u16) -> CharacterCalculation {
    calculate(
        CharacterInput {
            character,
            stream_id: "synthetic-stream",
            capture: &CaptureStatus {
                state: CaptureState::Live,
                ..Default::default()
            },
        },
        catalog,
        &CharacterCalculationRequest {
            defense,
            comparison_item_types: vec![],
            expected_revision: input_revision("synthetic-stream", character, catalog),
        },
    )
}

fn firing(catalog: &mut Catalog) -> &mut WeaponFiringDef {
    catalog.items[0].weapon_firing.as_mut().unwrap()
}

fn assert_value(value: &CharacterValue, expected: f64) {
    assert_eq!(
        value.completeness,
        CalculationCompleteness::Complete,
        "{value:?}"
    );
    assert!(
        (value.value.unwrap() - expected).abs() < 1e-10,
        "{value:?} != {expected}"
    );
}

fn unavailable(value: &CharacterValue) {
    assert_eq!(value.value, None, "{value:?}");
    assert_eq!(value.completeness, CalculationCompleteness::Unavailable);
    assert!(!value.reasons.is_empty());
}

#[test]
fn synthetic_vector_uses_totals_without_double_counting_breakdown() {
    let (mut character, catalog) = fixture(75, 75, 100, 100);
    let result = run(&character, Some(&catalog), 50);
    let weapon = &result.weapons[0];
    assert_value(&weapon.hit_mean, 150.0);
    assert_value(&weapon.attacks_per_sec, 8.0);
    assert_value(&weapon.projectiles_per_sec, 8.0);
    assert_value(&weapon.dps_total, 1200.0);
    assert_eq!(weapon.hit_range.min, Some(150));
    assert_eq!(weapon.hit_range.max, Some(150));
    assert_eq!(result.formula_version, "character-v1");
    character.stat_panel = Some(StatPanel {
        lines: vec![StatLine {
            stat_id: 20,
            total: Some(75),
            base: Some(999),
            gear: Some(999),
            exalt: Some(123),
            cap: None,
            at_cap: None,
            boost: None,
            other: None,
            current: None,
            split_evidence: None,
        }],
        conditions: vec![],
        breakdown_provisional: true,
    });
    assert_eq!(result, run(&character, Some(&catalog), 50));
}

#[test]
fn weak_precedes_damaging_and_products_truncate() {
    let (mut character, catalog) = fixture(75, 75, 101, 101);
    for (mask, hit, dps) in [(4 | 0x40000, 50.0, 400.0), (0x40000, 252.0, 2016.0)] {
        character.stats[2].value = mask;
        let result = run(&character, Some(&catalog), 0);
        assert_value(&result.weapons[0].hit_mean, hit);
        assert_value(&result.weapons[0].dps_total, dps);
    }
}

#[test]
fn exclusive_rolls_apply_defense_after_each_truncated_product() {
    let (character, mut catalog) = fixture(0, 0, 19, 23);
    let result = run(&character, Some(&catalog), 9);
    let weapon = &result.weapons[0];
    // Rolls 19,20,21,22 -> adjusted 9,10,10,11 -> hits 0,1,1,2.
    assert_value(&weapon.hit_mean, 1.0);
    assert_value(&weapon.dps_total, 1.5);
    assert_eq!(
        (weapon.hit_range.min, weapon.hit_range.max),
        (Some(0), Some(2))
    );
    assert_eq!(
        (weapon.catalog_range.min, weapon.catalog_range.max),
        (Some(19), Some(23))
    );
    catalog.items[0].projectiles[0].armor_piercing = true;
    let result = run(&character, Some(&catalog), 9);
    assert_value(&result.weapons[0].hit_mean, 10.0);
    assert_value(&result.weapons[0].dps_total, 15.0);
}

#[test]
fn defense_floor_boundaries_and_zero_are_exact() {
    let (character, mut catalog) = fixture(25, 0, 9, 12);
    let result = run(&character, Some(&catalog), 10);
    assert_value(&result.weapons[0].hit_mean, 2.0 / 3.0); // hits 0,1,1
    assert_value(&result.weapons[0].dps_total, 1.0);
    catalog.items[0].projectiles[0].min_damage = 0;
    catalog.items[0].projectiles[0].max_damage = 0;
    assert_value(
        &run(&character, Some(&catalog), 10).weapons[0].dps_total,
        0.0,
    );
}

#[test]
fn mixed_simultaneous_groups_keep_separate_rates_and_sum_dps() {
    let (character, mut catalog) = fixture(25, 75, 10, 10);
    catalog.items[0].projectiles.push(ProjectileDef {
        index: 1,
        min_damage: 20,
        max_damage: 20,
        armor_piercing: true,
        lifetime_millis: None,
    });
    let groups = &mut firing(&mut catalog).groups;
    groups[0].count = 2;
    groups[0].rate_factor = 0.5;
    groups.push(WeaponAttackGroup {
        group_id: "second".into(),
        projectile_index: 1,
        count: 1,
        rate_factor: 0.25,
        schedule: WeaponSchedule::Simultaneous,
        reasons: vec![],
    });
    let result = run(&character, Some(&catalog), 9);
    let weapon = &result.weapons[0];
    assert_value(&weapon.groups[0].attacks_per_sec, 4.0);
    assert_value(&weapon.groups[0].projectiles_per_sec, 8.0);
    assert_value(&weapon.groups[0].dps, 8.0);
    assert_value(&weapon.groups[1].attacks_per_sec, 2.0);
    assert_value(&weapon.groups[1].dps, 40.0);
    assert_value(&weapon.dps_total, 48.0);
    unavailable(&weapon.attacks_per_sec);
    unavailable(&weapon.hit_mean);
}

#[test]
fn missing_stats_block_only_dependent_values() {
    for missing in [20, 28, 29] {
        let (mut character, catalog) = fixture(75, 75, 100, 100);
        character.stats.retain(|s| s.stat_id != missing);
        let result = run(&character, Some(&catalog), 50);
        let weapon = &result.weapons[0];
        unavailable(&weapon.dps_total);
        if missing == 28 {
            assert_value(&weapon.hit_mean, 150.0);
            unavailable(&weapon.attacks_per_sec);
        } else {
            assert_value(&weapon.attacks_per_sec, 8.0);
            unavailable(&weapon.hit_mean);
        }
        assert_eq!(weapon.catalog_range.min, Some(100));
    }
}

#[test]
fn unsupported_schedule_preserves_damage_and_explains_missing_rate() {
    let (character, mut catalog) = fixture(75, 75, 100, 100);
    firing(&mut catalog).groups[0].schedule = WeaponSchedule::Unsupported;
    firing(&mut catalog).groups[0].reasons = vec!["charged_schedule".into()];
    let result = run(&character, Some(&catalog), 50);
    let weapon = &result.weapons[0];
    assert_value(&weapon.hit_mean, 150.0);
    unavailable(&weapon.attacks_per_sec);
    assert!(weapon
        .attacks_per_sec
        .reasons
        .iter()
        .any(|r| r == "charged_schedule"));
    unavailable(&weapon.dps_total);
}

#[test]
fn partial_and_missing_projectiles_never_claim_full_total() {
    let (character, catalog) = fixture(75, 75, 100, 100);
    for variant in 0..4 {
        let mut catalog = catalog.clone();
        match variant {
            0 => firing(&mut catalog).support = CatalogSupport::Partial,
            1 => firing(&mut catalog).omitted_projectile_indices.push(1),
            _ => {
                let mut group = firing(&mut catalog).groups[0].clone();
                group.group_id = "missing".into();
                if variant == 2 {
                    group.projectile_index = 1;
                } else {
                    group.schedule = WeaponSchedule::Unsupported;
                }
                firing(&mut catalog).groups.push(group);
            }
        }
        let result = run(&character, Some(&catalog), 50);
        let weapon = &result.weapons[0];
        assert_eq!(weapon.dps_subtotal.value, Some(1200.0));
        assert_eq!(
            weapon.dps_subtotal.completeness,
            CalculationCompleteness::Partial
        );
        assert!(!weapon.dps_subtotal.reasons.is_empty());
        unavailable(&weapon.dps_total);
        if variant == 2 {
            assert_eq!(weapon.groups[1].armor_piercing, None);
        }
    }
}

#[test]
fn unknown_catalog_firing_and_weapon_are_not_zero() {
    let (mut character, mut catalog) = fixture(75, 75, 100, 100);
    unavailable(&run(&character, None, 0).weapons[0].dps_total);
    catalog.items[0].weapon_firing = None;
    unavailable(&run(&character, Some(&catalog), 0).weapons[0].dps_total);
    character.equipment[0].item_type = None;
    character.equipment[0].observed_empty = Some(true);
    unavailable(&run(&character, Some(&catalog), 0).weapons[0].dps_total);
}

#[test]
fn invalid_stats_and_damage_bounds_are_rejected() {
    for (att, dex, min, max) in [
        (-1, 75, 100, 100),
        (75, -1, 100, 100),
        (75, 75, -1, 100),
        (75, 75, 101, 100),
        (i32::MAX, 75, 100, 100),
        (75, 75, i32::MAX, i32::MAX),
    ] {
        let (character, catalog) = fixture(att, dex, min, max);
        unavailable(&run(&character, Some(&catalog), 0).weapons[0].dps_total);
    }
}

#[test]
fn invalid_rates_counts_and_references_are_rejected() {
    let (character, catalog) = fixture(75, 75, 100, 100);
    for rate in [f64::NAN, f64::INFINITY, -1.0, 0.0, f64::MAX] {
        let mut catalog = catalog.clone();
        firing(&mut catalog).groups[0].rate_factor = rate;
        unavailable(&run(&character, Some(&catalog), 0).weapons[0].dps_total);
    }
    for variant in 0..4 {
        let mut catalog = catalog.clone();
        match variant {
            0 => firing(&mut catalog).groups[0].count = 0,
            1 => firing(&mut catalog).groups[0].projectile_index = -1,
            2 => {
                let duplicate = catalog.items[0].projectiles[0].clone();
                catalog.items[0].projectiles.push(duplicate);
            }
            _ => {
                let duplicate = firing(&mut catalog).groups[0].clone();
                firing(&mut catalog).groups.push(duplicate);
            }
        }
        unavailable(&run(&character, Some(&catalog), 0).weapons[0].dps_total);
    }
}

#[test]
fn large_constant_float_plateau_is_exact_without_enumeration() {
    let (character, catalog) = fixture(0, 0, 1_000_000_000, 1_000_000_030);
    assert_value(
        &run(&character, Some(&catalog), 0).weapons[0].hit_mean,
        500_000_000.0,
    );
}

#[test]
fn hostile_range_exhaustion_is_unavailable_not_approximate() {
    let (character, catalog) = fixture(25, 75, 0, 1_000_000_000);
    let result = run(&character, Some(&catalog), 0);
    let weapon = &result.weapons[0];
    unavailable(&weapon.hit_mean);
    assert!(weapon
        .hit_mean
        .reasons
        .iter()
        .any(|r| r == "calculation_work_limit"));
    unavailable(&weapon.dps_subtotal);
    unavailable(&weapon.dps_total);
    assert_eq!(weapon.hit_range.min, Some(0));
}

#[test]
fn tiny_domains_match_independent_exhaustive_oracle() {
    for (att, mask, multiplier) in [
        (0, 0, 0.5_f32),
        (25, 0, 1.0),
        (75, 0, 2.0),
        (75, 0x40000, 2.5),
        (75, 4 | 0x40000, 0.5),
    ] {
        for min in 0..24 {
            for defense in [0, 1, 9, 10, 25] {
                for piercing in [false, true] {
                    let (mut character, mut catalog) = fixture(att, 75, min, min + 7);
                    character.stats[2].value = mask;
                    catalog.items[0].projectiles[0].armor_piercing = piercing;
                    let hits: Vec<i32> = (min..min + 7)
                        .map(|x| {
                            let d = (x as f32 * multiplier).trunc() as i32;
                            if piercing {
                                d
                            } else {
                                (d / 10).max(d - defense)
                            }
                        })
                        .collect();
                    let result = run(&character, Some(&catalog), defense as u16);
                    let weapon = &result.weapons[0];
                    assert_value(&weapon.hit_mean, f64::from(hits.iter().sum::<i32>()) / 7.0);
                    assert_eq!(weapon.hit_range.min, hits.first().copied());
                    assert_eq!(weapon.hit_range.max, hits.last().copied());
                }
            }
        }
    }
}

#[test]
fn defense_zero_to_ten_thousand_never_increases_dps() {
    for piercing in [false, true] {
        let (character, mut catalog) = fixture(75, 75, 95, 105);
        catalog.items[0].projectiles[0].armor_piercing = piercing;
        let mut previous = f64::INFINITY;
        for defense in 0..=10_000 {
            let dps = run(&character, Some(&catalog), defense).weapons[0]
                .dps_total
                .value
                .unwrap();
            assert!(dps <= previous);
            previous = dps;
        }
    }
}

#[test]
fn stat_113_is_irrelevant_and_regeneration_and_exalt_remain_unavailable() {
    let (mut character, catalog) = fixture(75, 75, 100, 100);
    let before = run(&character, Some(&catalog), 50);
    for value in [1000, 1200, -1] {
        character.stats.retain(|s| s.stat_id != 113);
        character.stats.push(StatView {
            stat_id: 113,
            value,
        });
        assert_eq!(before, run(&character, Some(&catalog), 50));
    }
    assert_eq!(before.exalt_damage.status, ExaltDamageStatus::Deferred);
    assert_eq!(before.exalt_damage.value, None);
    for value in [
        &before.regeneration.wis,
        &before.regeneration.hp_subtotal,
        &before.regeneration.mp_subtotal,
        &before.regeneration.hp_total,
        &before.regeneration.mp_total,
        &before.regeneration.base_vit,
        &before.regeneration.base_mp,
    ] {
        unavailable(value);
    }
}

#[test]
fn revision_tracks_relevant_inputs_but_not_refreshes_or_unrelated_stats() {
    let (character, catalog) = fixture(75, 75, 100, 100);
    let revision = input_revision("stream", &character, Some(&catalog));
    assert_eq!(revision.validate(), Ok(()));
    let mut refreshed = character.clone();
    refreshed.observed_at = Timestamp(99999);
    refreshed.calculation_revision = Some(revision.clone());
    refreshed.fame = Some(999);
    refreshed.stats.push(StatView {
        stat_id: 113,
        value: 1234,
    });
    refreshed.stats.reverse();
    assert_eq!(
        revision,
        input_revision("stream", &refreshed, Some(&catalog))
    );
    for variant in 0..11 {
        let mut changed = character.clone();
        match variant {
            0 => changed.stats[0].value += 1,
            1 => changed.stats[1].value += 1,
            2 => changed.stats[2].value = 4,
            3 => changed.equipment[0].enchant.ids.push(1),
            4 => changed.equipment[0].observed_empty = None,
            5 => changed.id.push('x'),
            6 => changed.class_id = Some(99),
            7 => changed.identity_verified = !changed.identity_verified,
            8 => changed.died_at = Some(Timestamp(1)),
            9 => changed.issues.push(CoverageIssue::StreamGap),
            _ => changed.equipment[0].item_type = Some(2),
        }
        assert_ne!(
            revision,
            input_revision("stream", &changed, Some(&catalog)),
            "variant {variant}"
        );
    }
    for stat_id in [0, 3, 27] {
        let mut changed = character.clone();
        changed.stats.push(StatView {
            stat_id,
            value: 100,
        });
        assert_ne!(revision, input_revision("stream", &changed, Some(&catalog)));
    }
    let mut changed_catalog = catalog.clone();
    changed_catalog.client_fingerprint = "b".repeat(64);
    assert_ne!(
        revision,
        input_revision("stream", &character, Some(&changed_catalog))
    );
    assert_ne!(
        revision,
        input_revision("other", &character, Some(&catalog))
    );
    assert_ne!(revision, input_revision("stream", &character, None));
}

#[test]
fn comparisons_share_inputs_and_calculation_never_mutates_sources() {
    let (character, mut catalog) = fixture(75, 75, 100, 100);
    let mut comparison = catalog.items[0].clone();
    comparison.object_type = 2;
    comparison.projectiles[0].min_damage = 50;
    comparison.projectiles[0].max_damage = 50;
    catalog.items.push(comparison);
    let capture = CaptureStatus::default();
    let options = CharacterCalculationRequest {
        defense: 50,
        comparison_item_types: vec![2, 999],
        expected_revision: input_revision("stream", &character, Some(&catalog)),
    };
    let before = (
        character.clone(),
        catalog.clone(),
        capture.clone(),
        options.clone(),
    );
    let result = calculate(
        CharacterInput {
            character: &character,
            stream_id: "stream",
            capture: &capture,
        },
        Some(&catalog),
        &options,
    );
    assert_eq!(before, (character, catalog, capture, options));
    assert_eq!(result.freshness, CharacterFreshness::LastSeen);
    assert_eq!(result.weapons.len(), 3);
    assert_value(&result.weapons[0].dps_total, 1200.0);
    assert_value(&result.weapons[1].dps_total, 400.0);
    unavailable(&result.weapons[2].dps_total);
}

#[test]
fn freshness_coverage_and_exclusions_are_explicit() {
    let (mut character, catalog) = fixture(75, 75, 100, 100);
    character.issues.push(CoverageIssue::StreamGap);
    let result = run(&character, Some(&catalog), 50);
    assert_eq!(result.freshness, CharacterFreshness::Live);
    assert_eq!(result.issues, character.issues);
    for reason in [
        "berserk",
        "dazed",
        "stunned",
        "firing_restrictions",
        "special_effects",
        "weapon_enchant_damage",
        "weapon_enchant_rate",
        "exalt_damage",
        "crucible",
        "blood_ritual",
        "enemy_conditions",
        "misses",
        "range",
        "repeat_hits",
        "downtime",
    ] {
        assert!(
            result.weapons[0]
                .dps_total
                .exclusions
                .iter()
                .any(|e| e == reason),
            "{reason}"
        );
    }
    character.died_at = Some(Timestamp(5));
    assert_eq!(
        run(&character, Some(&catalog), 50).freshness,
        CharacterFreshness::Died
    );
}

#[test]
fn invalid_requests_and_stale_revision_cannot_return_current_totals() {
    let (character, catalog) = fixture(75, 75, 100, 100);
    for variant in 0..3 {
        let mut options = CharacterCalculationRequest {
            defense: 50,
            comparison_item_types: vec![],
            expected_revision: input_revision("stream", &character, Some(&catalog)),
        };
        match variant {
            0 => options.defense = 10_001,
            1 => options.comparison_item_types = vec![1, 2, 3],
            _ => options.expected_revision.stream_id = "stale".into(),
        }
        let result = calculate(
            CharacterInput {
                character: &character,
                stream_id: "stream",
                capture: &CaptureStatus::default(),
            },
            Some(&catalog),
            &options,
        );
        unavailable(&result.weapons[0].dps_total);
    }
}

#[test]
fn combat_f32_rounding_precedes_truncation() {
    // f32 (27 * .02) = .5399999619; f32 (50 * M) = 26.99999809.
    // An f64 rewrite would incorrectly produce the integer 27 here.
    let (character, catalog) = fixture(2, 0, 50, 50);
    assert_value(
        &run(&character, Some(&catalog), 0).weapons[0].hit_mean,
        26.0,
    );
}

#[test]
fn collection_work_limits_and_unsupported_metadata_are_explicit() {
    let (character, catalog) = fixture(75, 75, 100, 100);
    for variant in 0..3 {
        let mut catalog = catalog.clone();
        match variant {
            0 => firing(&mut catalog).groups.clear(),
            1 => firing(&mut catalog).support = CatalogSupport::Unsupported,
            _ => {
                let group = firing(&mut catalog).groups[0].clone();
                firing(&mut catalog).groups = (0..257)
                    .map(|index| WeaponAttackGroup {
                        group_id: format!("group_{index}"),
                        ..group.clone()
                    })
                    .collect();
            }
        }
        unavailable(&run(&character, Some(&catalog), 0).weapons[0].dps_total);
    }
}

#[test]
fn unsupported_firing_keeps_independent_known_damage() {
    let (character, mut catalog) = fixture(75, 75, 100, 100);
    firing(&mut catalog).support = CatalogSupport::Unsupported;
    let result = run(&character, Some(&catalog), 50);
    assert_value(&result.weapons[0].hit_mean, 150.0);
    unavailable(&result.weapons[0].attacks_per_sec);
    unavailable(&result.weapons[0].dps_total);
}

#[test]
fn work_exhaustion_discards_accumulated_weapon_subtotal() {
    let (character, mut catalog) = fixture(25, 75, 100, 100);
    catalog.items[0].projectiles.push(ProjectileDef {
        index: 1,
        min_damage: 0,
        max_damage: 1_000_000_000,
        armor_piercing: true,
        lifetime_millis: None,
    });
    let mut group = firing(&mut catalog).groups[0].clone();
    group.group_id = "exhausting".into();
    group.projectile_index = 1;
    firing(&mut catalog).groups.push(group);
    let result = run(&character, Some(&catalog), 0);
    assert_value(&result.weapons[0].groups[0].dps, 800.0);
    unavailable(&result.weapons[0].dps_subtotal);
    assert!(result.weapons[0]
        .dps_subtotal
        .reasons
        .iter()
        .any(|r| r == "calculation_work_limit"));
}

fn regen_effect(flat: Option<f64>, fraction: Option<f64>) -> RegenEffect {
    RegenEffect {
        flat_per_sec: flat,
        fraction_of_max_per_sec: fraction,
        source_units: fraction
            .map(|_| "fraction_of_max_per_second".into())
            .into_iter()
            .chain(flat.map(|_| "units_per_second".into()))
            .collect(),
    }
}

fn regen_fixture() -> (CharacterView, Catalog) {
    let (mut character, mut catalog) = fixture(75, 75, 100, 100);
    character.stats.extend([
        StatView {
            stat_id: 27,
            value: 50,
        },
        StatView {
            stat_id: 0,
            value: 800,
        },
        StatView {
            stat_id: 3,
            value: 300,
        },
    ]);
    character.equipment[0].enchant = EnchantView {
        state: EnchantState::Recorded,
        slots: Some(1),
        applied: Some(1),
        ids: vec![10],
    };
    for slot in 1..4 {
        character.equipment.push(EquipmentView {
            slot,
            item_type: None,
            observed_empty: Some(true),
            enchant: EnchantView {
                state: EnchantState::Missing,
                slots: None,
                applied: None,
                ids: vec![],
            },
        });
    }
    catalog.enchants = vec![EnchantDef {
        id: 10,
        name: "Synthetic enchant".into(),
        description: "Ignored display text".into(),
        stat_damage_multiplier: None,
        weapon_damage: None,
        regeneration: Some(EnchantRegenDef {
            support: CatalogSupport::Supported,
            hp: vec![regen_effect(Some(2.0), Some(0.01))],
            mp: vec![regen_effect(Some(1.0), Some(0.02))],
            reasons: vec![],
            source_units: vec![
                "fraction_of_max_per_second".into(),
                "units_per_second".into(),
            ],
        }),
    }];
    (character, catalog)
}

fn regen_metadata(catalog: &mut Catalog) -> &mut EnchantRegenDef {
    catalog.enchants[0].regeneration.as_mut().unwrap()
}

fn partial(value: &CharacterValue, expected: f64) {
    assert_eq!(value.value, Some(expected), "{value:?}");
    assert_eq!(value.completeness, CalculationCompleteness::Partial);
    assert!(!value.reasons.is_empty());
}

#[test]
fn regeneration_vectors_preserve_coefficients_and_provenance() {
    let (character, catalog) = regen_fixture();
    let result = run(&character, Some(&catalog), 0).regeneration;
    assert_eq!(result.applied_wis, Some(50));
    assert_value(&result.wis, 6.0);
    assert_eq!(result.wis.evidence, CalculationEvidence::Modeled);
    assert!(result
        .wis
        .reasons
        .iter()
        .any(|r| r == "wis_reference_model"));
    assert_value(&result.hp_total, 10.0);
    assert_value(&result.mp_total, 13.0);
    assert_value(&result.hp_subtotal, 10.0);
    assert_value(&result.mp_subtotal, 13.0);
    assert_eq!(result.effects.len(), 2);
    for (effect, resource, maximum, flat, fraction, total) in [
        (
            &result.effects[0],
            CharacterRegenResource::Hp,
            800,
            2.0,
            0.01,
            10.0,
        ),
        (
            &result.effects[1],
            CharacterRegenResource::Mp,
            300,
            1.0,
            0.02,
            7.0,
        ),
    ] {
        assert_eq!(effect.source_slot, 0);
        assert_eq!(effect.enchant_id, 10);
        assert_eq!(effect.resource, resource);
        assert_eq!(effect.applied_maximum, Some(maximum));
        assert_value(&effect.flat_per_sec, flat);
        assert_value(&effect.fraction_of_max_per_sec, fraction);
        assert_eq!(effect.flat_per_sec.evidence, CalculationEvidence::Catalog);
        assert_eq!(
            effect.source_units,
            catalog.enchants[0]
                .regeneration
                .as_ref()
                .unwrap()
                .source_units
        );
        assert_value(&effect.total, total);
    }
}

#[test]
fn regeneration_sums_each_equipped_effect_once_and_ignores_inventory() {
    let (mut character, mut catalog) = regen_fixture();
    // Match extraction's separate flat and fractional effects.
    regen_metadata(&mut catalog).hp = vec![
        regen_effect(Some(2.0), None),
        regen_effect(None, Some(0.01)),
    ];
    for slot in 1..4 {
        character.equipment[slot] = EquipmentView {
            slot: slot as u8,
            ..character.equipment[0].clone()
        };
    }
    character.equipment.push(EquipmentView {
        slot: 4,
        ..character.equipment[0].clone()
    });
    let before = (character.clone(), catalog.clone());
    let result = run(&character, Some(&catalog), 0).regeneration;
    assert_value(&result.hp_total, 40.0);
    assert_value(&result.mp_total, 34.0);
    assert_eq!(result.effects.len(), 12);
    assert_eq!(before, (character, catalog));
}

#[test]
fn regeneration_missing_maximum_preserves_only_known_flat_parts() {
    let (mut character, mut catalog) = regen_fixture();
    character.stats.retain(|s| s.stat_id != 0);
    let result = run(&character, Some(&catalog), 0).regeneration;
    partial(&result.hp_subtotal, 2.0);
    partial(&result.effects[0].subtotal, 2.0);
    unavailable(&result.effects[0].total);
    unavailable(&result.hp_total);
    assert_value(&result.mp_total, 13.0);
    regen_metadata(&mut catalog).hp = vec![regen_effect(None, Some(0.01))];
    let result = run(&character, Some(&catalog), 0).regeneration;
    unavailable(&result.hp_subtotal);
    unavailable(&result.hp_total);
    unavailable(&result.effects[0].subtotal);
    // A flat-only effect has no dependency on an observed maximum.
    regen_metadata(&mut catalog).hp = vec![regen_effect(Some(2.0), None)];
    assert_value(
        &run(&character, Some(&catalog), 0).regeneration.hp_total,
        2.0,
    );
}

#[test]
fn regeneration_known_empty_and_complete_no_effect_metadata_establish_zero() {
    let (mut character, mut catalog) = regen_fixture();
    regen_metadata(&mut catalog).hp.clear();
    regen_metadata(&mut catalog).mp.clear();
    let result = run(&character, Some(&catalog), 0).regeneration;
    assert_value(&result.hp_total, 0.0);
    assert_value(&result.mp_total, 6.0);
    for ids in [vec![], vec![-1, -2, -3]] {
        character.equipment[0].enchant = EnchantView {
            state: EnchantState::RecordedEmpty,
            slots: Some(1),
            applied: Some(0),
            ids,
        };
        let result = run(&character, Some(&catalog), 0).regeneration;
        assert_value(&result.hp_total, 0.0);
        assert_value(&result.mp_total, 6.0);
    }
    character.equipment[0].observed_empty = Some(true);
    character.equipment[0].item_type = None;
    character.equipment[0].enchant.state = EnchantState::Missing;
    let result = run(&character, None, 0).regeneration;
    assert_value(&result.hp_total, 0.0);
    assert_value(&result.mp_total, 6.0);
}

#[test]
fn regeneration_unknown_equipment_or_enchants_never_establish_zero() {
    let (character, catalog) = regen_fixture();
    for variant in 0..13 {
        let mut character = character.clone();
        let mut catalog = catalog.clone();
        match variant {
            0 => character.equipment[0].enchant.state = EnchantState::Missing,
            1 => character.equipment[0].enchant.state = EnchantState::Invalid,
            2 => character.equipment[0].enchant.ids = vec![999],
            3 => catalog.enchants[0].regeneration = None,
            4 => regen_metadata(&mut catalog).support = CatalogSupport::Unsupported,
            5 => character.equipment[0].item_type = Some(999),
            6 => character.equipment.clear(),
            7 => character.equipment[0].observed_empty = Some(true),
            8 => {
                character.equipment[0].item_type = None;
                character.equipment[0].observed_empty = None;
            }
            9 => character.equipment.push(character.equipment[0].clone()),
            10 => catalog.enchants.push(catalog.enchants[0].clone()),
            11 => {
                character.equipment[0].enchant.state = EnchantState::RecordedEmpty;
            }
            _ => {
                character.equipment[0].enchant.ids = vec![-4];
            }
        }
        let result = run(&character, Some(&catalog), 0).regeneration;
        unavailable(&result.hp_total);
        unavailable(&result.mp_total);
        partial(&result.mp_subtotal, 6.0);
        assert_value(&result.wis, 6.0);
    }
}

#[test]
fn regeneration_catalog_and_wis_are_independent() {
    let (mut character, catalog) = regen_fixture();
    let result = run(&character, None, 0).regeneration;
    assert_value(&result.wis, 6.0);
    partial(&result.mp_subtotal, 6.0);
    unavailable(&result.hp_total);
    unavailable(&result.mp_total);
    for variant in 0..3 {
        character.stats.retain(|s| s.stat_id != 27);
        if variant > 0 {
            character.stats.push(StatView {
                stat_id: 27,
                value: -1,
            });
        }
        if variant == 2 {
            character.stats.push(StatView {
                stat_id: 27,
                value: 50,
            });
        }
        let result = run(&character, Some(&catalog), 0).regeneration;
        unavailable(&result.wis);
        partial(&result.mp_subtotal, 7.0);
        unavailable(&result.mp_total);
        assert_value(&result.hp_total, 10.0);
    }
}

#[test]
fn regeneration_partial_metadata_retains_supported_contributions() {
    let (character, mut catalog) = regen_fixture();
    regen_metadata(&mut catalog).support = CatalogSupport::Partial;
    regen_metadata(&mut catalog)
        .reasons
        .push("unsupported_regen_effect".into());
    let result = run(&character, Some(&catalog), 0).regeneration;
    partial(&result.hp_subtotal, 10.0);
    partial(&result.mp_subtotal, 13.0);
    unavailable(&result.hp_total);
    unavailable(&result.mp_total);
    assert!(result
        .hp_total
        .reasons
        .iter()
        .any(|r| r == "unsupported_regen_effect"));
}

#[test]
fn regeneration_rejects_invalid_coefficients_and_checked_overflow() {
    let (character, catalog) = regen_fixture();
    for coefficient in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.01] {
        for fractional in [false, true] {
            let mut catalog = catalog.clone();
            let effect = &mut regen_metadata(&mut catalog).hp[0];
            if fractional {
                effect.fraction_of_max_per_sec = Some(coefficient);
            } else {
                effect.flat_per_sec = Some(coefficient);
            }
            let result = run(&character, Some(&catalog), 0).regeneration;
            unavailable(&result.hp_total);
            unavailable(&result.effects[0].total);
            assert_value(&result.mp_total, 13.0);
        }
    }
    for effects in [
        vec![regen_effect(None, None)],
        vec![regen_effect(Some(2.0), Some(f64::MAX))],
        vec![
            regen_effect(Some(f64::MAX), None),
            regen_effect(Some(f64::MAX), None),
        ],
    ] {
        let mut catalog = catalog.clone();
        regen_metadata(&mut catalog).hp = effects;
        let result = run(&character, Some(&catalog), 0).regeneration;
        unavailable(&result.hp_total);
        unavailable(&result.hp_subtotal);
        assert_value(&result.mp_total, 13.0);
    }
}

#[test]
fn regeneration_invalid_or_duplicate_maximum_blocks_only_fractional_part() {
    let (character, catalog) = regen_fixture();
    for duplicate in [false, true] {
        let mut character = character.clone();
        if duplicate {
            character.stats.push(StatView {
                stat_id: 0,
                value: 800,
            });
        } else {
            character
                .stats
                .iter_mut()
                .find(|s| s.stat_id == 0)
                .unwrap()
                .value = -1;
        }
        let result = run(&character, Some(&catalog), 0).regeneration;
        partial(&result.hp_subtotal, 2.0);
        unavailable(&result.hp_total);
        assert_eq!(result.effects[0].applied_maximum, None);
    }
}

#[test]
fn regeneration_baseline_exclusions_remain_visible_with_suppressing_conditions() {
    let (mut character, catalog) = regen_fixture();
    let baseline = run(&character, Some(&catalog), 0).regeneration;
    // Quiet (bit 1), Sick (bit 4), Bleeding (bit 15), individually and together.
    for mask in [0, 2, 16, 32768, 2 | 16 | 32768] {
        character
            .stats
            .iter_mut()
            .find(|s| s.stat_id == 29)
            .unwrap()
            .value = mask;
        let result = run(&character, Some(&catalog), 0).regeneration;
        assert_eq!(result, baseline);
        unavailable(&result.base_vit);
        unavailable(&result.base_mp);
        for (value, exclusions) in [
            (
                &result.hp_total,
                vec![
                    "base_vit",
                    "pets",
                    "healing_effects",
                    "combat_state_modifiers",
                ],
            ),
            (
                &result.mp_total,
                vec![
                    "base_mp_regeneration",
                    "pets",
                    "consumables",
                    "ability_costs",
                    "combat_state_modifiers",
                ],
            ),
        ] {
            assert_eq!(value.exclusions, exclusions);
        }
        assert_eq!(result.hp_subtotal.exclusions, result.hp_total.exclusions);
        assert_eq!(result.mp_subtotal.exclusions, result.mp_total.exclusions);
    }
}

#[test]
fn regeneration_invalid_request_or_stale_revision_cannot_return_current_totals() {
    let (character, catalog) = regen_fixture();
    for stale in [false, true] {
        let mut options = CharacterCalculationRequest {
            defense: if stale { 0 } else { 10_001 },
            comparison_item_types: vec![],
            expected_revision: input_revision("stream", &character, Some(&catalog)),
        };
        if stale {
            options.expected_revision.stream_id = "other".into();
        }
        let result = calculate(
            CharacterInput {
                character: &character,
                stream_id: "stream",
                capture: &CaptureStatus::default(),
            },
            Some(&catalog),
            &options,
        )
        .regeneration;
        unavailable(&result.wis);
        unavailable(&result.hp_total);
        unavailable(&result.mp_total);
        unavailable(&result.hp_subtotal);
        unavailable(&result.mp_subtotal);
        assert!(result.effects.iter().all(|e| e.total.value.is_none()));
    }
}

#[test]
fn revision_and_results_depend_only_on_modeled_condition_bits() {
    let (mut character, catalog) = fixture(75, 75, 100, 100);
    let baseline = run(&character, Some(&catalog), 0);
    for bit in 0..32 {
        character
            .stats
            .iter_mut()
            .find(|s| s.stat_id == 29)
            .unwrap()
            .value = 1_i32 << bit;
        let result = run(&character, Some(&catalog), 0);
        if bit == 2 || bit == 18 {
            assert_eq!(result.weapons[0].applied_primary_mask, Some(1_u32 << bit));
            assert_ne!(
                result.weapons[0].applied_primary_mask,
                baseline.weapons[0].applied_primary_mask
            );
            assert_ne!(result.revision.input_hash, baseline.revision.input_hash);
            assert_ne!(result.weapons[0].dps_total, baseline.weapons[0].dps_total);
        } else {
            assert_eq!(
                result.revision, baseline.revision,
                "unused condition bit {bit}"
            );
            assert_eq!(result.weapons[0].dps_total, baseline.weapons[0].dps_total);
            assert_eq!(result.regeneration, baseline.regeneration);
            assert_eq!(
                result.weapons[0].applied_primary_mask,
                baseline.weapons[0].applied_primary_mask
            );
            assert_eq!(result, baseline);
        }
    }
}

#[test]
fn masked_conditions_retain_missing_and_ambiguous_input_detection() {
    let (mut character, catalog) = fixture(75, 75, 100, 100);
    let baseline = run(&character, Some(&catalog), 0);
    character.stats.push(StatView {
        stat_id: 29,
        value: 0x8000,
    });
    let ambiguous = run(&character, Some(&catalog), 0);
    assert_ne!(ambiguous.revision, baseline.revision);
    unavailable(&ambiguous.weapons[0].dps_total);
    character.stats.retain(|s| s.stat_id != 29);
    let missing = run(&character, Some(&catalog), 0);
    assert_ne!(missing.revision, baseline.revision);
    unavailable(&missing.weapons[0].dps_total);
}
