use super::*;

fn fight() -> Fight {
    let mut base = maxed_types::demo::demo_snapshot().fights.remove(0);
    base.issues.clear();
    Fight {
        base,
        was_quest: false,
        pending: BTreeMap::new(),
        series: Default::default(),
        series_budget: Default::default(),
        timeline_out: vec![],
        local_last_hit: None,
        player_windows: BTreeMap::new(),
        snapshot_series_cache: Default::default(),
        totals_cache: Default::default(),
        projection: Default::default(),
        projection_builds: Default::default(),
        series_nanos: Default::default(),
    }
}

#[test]
fn local_window_keeps_exact_span_and_matches_dps_end_cap() {
    for span in [0, 2_999_999, 3_000_000, 19_999_999, 20_000_000] {
        let mut f = fight();
        let first = Timestamp(1_000_123);
        let end = Timestamp(first.0 + span);
        f.base.ended = Some(end);
        f.player_windows.insert(
            f.base.players[0].player_id.clone(),
            (first, Timestamp(end.0 + 999)),
        );
        let projected = f.project();
        assert_eq!(projected.local_active_micros, Some(span));
        assert_eq!(f.project_snapshot(false).local_active_micros, Some(span));
        let expected = (span >= 3_000_000)
            .then(|| dps(projected.players[0].damage, first, end))
            .flatten();
        assert_eq!(projected.players[0].dps, expected);
    }
}

#[test]
fn local_window_includes_pending_activity_and_drops_cancelled_estimate() {
    for server_first in [false, true] {
        let mut f = fight();
        let local = f.base.players[0].clone();
        f.player_windows.insert(
            local.player_id.clone(),
            (Timestamp(1_000_000), Timestamp(1_000_000)),
        );
        f.pending.insert(
            (1, 0, 0, 1),
            Pair {
                estimate: (!server_first).then(|| Contribution {
                    player: Some(local.clone()),
                    source: local.sources[0].clone(),
                    at: Timestamp(21_000_000),
                    active_at: Timestamp(21_000_000),
                }),
                estimates: u32::from(!server_first),
                servers: u32::from(server_first),
                server_damage: None,
                server_activity: server_first.then_some((local.player_id, Timestamp(21_000_000))),
                at: Timestamp(21_000_000),
            },
        );
        assert_eq!(f.project().local_active_micros, Some(20_000_000));
        f.pending.clear();
        f.changed();
        assert_eq!(f.project().local_active_micros, Some(0));
    }
}

#[test]
fn local_window_needs_one_identified_local_with_observed_activity() {
    let mut f = fight();
    assert_eq!(f.project_uncached(false).local_active_micros, None);
    f.player_windows.insert(
        f.base.players[0].player_id.clone(),
        (Timestamp(0), Timestamp(20_000_000)),
    );
    assert_eq!(
        f.project_uncached(false).local_active_micros,
        Some(20_000_000)
    );
    f.base.issues.push(CoverageIssue::MissingIdentity);
    assert_eq!(f.project_uncached(false).local_active_micros, None);
    f.base.issues.clear();
    let mut duplicate_local = f.base.players[0].clone();
    duplicate_local.player_id = "synthetic-second-local".into();
    f.base.players.push(duplicate_local);
    assert_eq!(f.project_uncached(false).local_active_micros, None);
    for player in &mut f.base.players {
        player.is_local = false;
    }
    assert_eq!(f.project_uncached(false).local_active_micros, None);
}
