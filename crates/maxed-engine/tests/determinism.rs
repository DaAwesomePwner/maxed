mod common;
use common::*;
use maxed_types::*;
#[test]
fn complete_synthetic_story_is_byte_deterministic() {
    fn story() -> Vec<u8> {
        let mut e = engine();
        let mut events = vec![];
        for (at, p) in [
            (0, map("The Void")),
            (1000, create(1, 1)),
            (2000, update(vec![player(), target()], vec![])),
            (1_000_000, tick(vec![])),
            (1_001_000, damage(1, 1, 100)),
            (3_000_000, tick(vec![])),
            (3_001_000, damage(1, 2, 200)),
            (
                3_002_000,
                update(vec![object(4, 1292, vec![stat(8, 200)])], vec![]),
            ),
            (4_000_000, victory()),
            (5_000_000, map("Nexus")),
        ] {
            events.extend(apply(&mut e, at, p));
        }
        let fight = events
            .iter()
            .find_map(|e| {
                if let EngineEvent::FightEnded(f) = e {
                    Some(f)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(fight.total_damage, 300.);
        assert_eq!(fight.players[0].dps, None);
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, EngineEvent::BagDropped(_)))
                .count(),
            1
        );
        assert!(events
            .iter()
            .any(|e| matches!(e,EngineEvent::RunEnded(r) if r.outcome==Some(Outcome::Cleared))));
        serde_json::to_vec(&events).unwrap()
    }
    assert_eq!(story(), story());
}
