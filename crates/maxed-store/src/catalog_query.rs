//! Catalog classifications prepared once for history reads.
use crate::{validate_loot_catalog, StoreError};
use maxed_types::{Catalog, PotionStat};

/// Immutable SQL classifications shared by every request for one catalog generation.
#[derive(Clone, Debug)]
pub struct CatalogQueryData(std::sync::Arc<CatalogQueryInner>);
#[derive(Debug)]
struct CatalogQueryInner {
    identity: CatalogQueryIdentity,
    tiers: String,
    potions: String,
    chase: Vec<u16>,
    representatives: Vec<(PotionStat, u16)>,
}

/// Constant-size identity; loot/HUD metadata limits are checked before admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogQueryIdentity {
    generation: u64,
    valid: bool,
}
impl CatalogQueryIdentity {
    pub fn validate(self) -> Result<(), StoreError> {
        if self.valid {
            Ok(())
        } else {
            Err(StoreError::Limit)
        }
    }
}
impl Default for CatalogQueryData {
    fn default() -> Self {
        Self::new(&[], &[])
    }
}
impl From<Vec<(u16, Option<String>)>> for CatalogQueryData {
    fn from(tiers: Vec<(u16, Option<String>)>) -> Self {
        Self::new(&tiers, &[])
    }
}
impl CatalogQueryData {
    pub fn new(tiers: &[(u16, Option<String>)], potions: &[(u16, PotionStat)]) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let generation = NEXT
            .try_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |value| value.checked_add(1),
            )
            .expect("catalog generation exhausted");
        let valid = validate_loot_catalog(tiers, potions).is_ok();
        let mut inner = CatalogQueryInner {
            identity: CatalogQueryIdentity { generation, valid },
            tiers: "[]".into(),
            potions: "[]".into(),
            // Recap historically accepted catalog tiers of any length. Build its
            // membership independently of the limits used by loot/HUD SQL.
            chase: tiers
                .iter()
                .filter_map(|(kind, tier)| {
                    matches!(tier.as_deref(), Some("UT" | "ST")).then_some(*kind)
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            representatives: vec![],
        };
        if valid {
            let tiers: std::collections::BTreeMap<_, _> = tiers.iter().cloned().collect();
            let potions: std::collections::BTreeMap<_, _> = potions.iter().copied().collect();
            for (kind, stat) in &potions {
                if !inner
                    .representatives
                    .iter()
                    .any(|(existing, _)| existing == stat)
                {
                    inner.representatives.push((*stat, *kind));
                }
            }
            inner.tiers = serde_json::to_string(&tiers.into_iter().collect::<Vec<_>>())
                .expect("serializable catalog tiers");
            inner.potions = serde_json::to_string(&potions.into_iter().collect::<Vec<_>>())
                .expect("serializable catalog potions");
        }
        Self(std::sync::Arc::new(inner))
    }
    pub fn from_catalog(catalog: &Catalog) -> Self {
        let mut data = Self::new(
            &catalog
                .items
                .iter()
                .map(|item| (item.object_type, item.tier.clone()))
                .collect::<Vec<_>>(),
            &catalog
                .items
                .iter()
                .filter_map(|item| item.potion_stat.map(|stat| (item.object_type, stat)))
                .collect::<Vec<_>>(),
        );
        let inner = std::sync::Arc::get_mut(&mut data.0).expect("unpublished catalog data");
        for (stat, kind) in &mut inner.representatives {
            let base = match stat {
                PotionStat::Life => "Potion of Life",
                PotionStat::Mana => "Potion of Mana",
                PotionStat::Attack => "Potion of Attack",
                PotionStat::Defense => "Potion of Defense",
                PotionStat::Speed => "Potion of Speed",
                PotionStat::Dexterity => "Potion of Dexterity",
                PotionStat::Vitality => "Potion of Vitality",
                PotionStat::Wisdom => "Potion of Wisdom",
            };
            // Match the old HUD's base-potion search hit. Catalog classification
            // remains authoritative; names only choose the representative sprite.
            if let Some(base_type) = catalog
                .items
                .iter()
                .filter(|item| item.potion_stat == Some(*stat) && item.name == base)
                .map(|item| item.object_type)
                .min()
            {
                *kind = base_type;
            }
        }
        data
    }
    pub fn identity(&self) -> CatalogQueryIdentity {
        self.0.identity
    }
    pub fn shares_instance(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
    pub fn chase_types(&self) -> &[u16] {
        &self.0.chase
    }
    pub(crate) fn tiers_json(&self) -> &str {
        &self.0.tiers
    }
    pub(crate) fn potions_json(&self) -> &str {
        &self.0.potions
    }
    pub fn potion_type(&self, stat: PotionStat) -> Option<u16> {
        self.0
            .representatives
            .iter()
            .find(|(value, _)| *value == stat)
            .map(|(_, kind)| *kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recap_membership_survives_unrelated_tier_limits() {
        let data = CatalogQueryData::new(
            &[
                (1, Some("x".repeat(17))),
                (2, Some("UT".into())),
                (3, Some("ST".into())),
            ],
            &[],
        );
        assert!(data.identity().validate().is_err());
        assert_eq!(data.chase_types(), &[2, 3]);
    }

    #[test]
    fn potion_representative_prefers_base_then_lowest_type() {
        let mut catalog = maxed_types::demo::demo_catalog();
        let template = catalog.items[0].clone();
        catalog.items = [
            (1, "Greater Potion of Life", PotionStat::Life),
            (2, "Potion of Life (SB)", PotionStat::Life),
            (9, "Potion of Life", PotionStat::Life),
            (8, "Potion of Life", PotionStat::Life),
            (4, "Greater Potion of Mana", PotionStat::Mana),
            (3, "Potion of Mana (SB)", PotionStat::Mana),
        ]
        .into_iter()
        .map(|(object_type, name, stat)| {
            let mut item = template.clone();
            item.object_type = object_type;
            item.name = name.into();
            item.potion_stat = Some(stat);
            item
        })
        .collect();
        for _ in 0..2 {
            let data = CatalogQueryData::from_catalog(&catalog);
            assert_eq!(data.potion_type(PotionStat::Life), Some(8));
            assert_eq!(data.potion_type(PotionStat::Mana), Some(3));
            assert_eq!(data.potion_type(PotionStat::Attack), None);
            catalog.items.reverse();
        }
    }

    #[test]
    fn normalized_classifications_are_shared_and_generation_specific() {
        let data = CatalogQueryData::new(
            &[(2, Some("UT".into())), (1, None), (2, Some("ST".into()))],
            &[
                (9, PotionStat::Life),
                (8, PotionStat::Mana),
                (8, PotionStat::Life),
            ],
        );
        assert_eq!(data.tiers_json(), "[[1,null],[2,\"ST\"]]");
        assert_eq!(data.potions_json(), "[[8,\"life\"],[9,\"life\"]]");
        assert_eq!(data.chase_types(), &[2]);
        assert_eq!(data.potion_type(PotionStat::Life), Some(8));
        assert_eq!(data.potion_type(PotionStat::Mana), None);
        let shared = data.clone();
        assert!(data.shares_instance(&shared));
        assert_eq!(data.identity(), shared.identity());
        assert_ne!(data.identity(), CatalogQueryData::default().identity());
        assert!(CatalogQueryData::new(&[(1, Some("x".repeat(17)))], &[])
            .identity()
            .validate()
            .is_err());
        assert!(CatalogQueryData::new(&vec![(1, None); 65_537], &[])
            .identity()
            .validate()
            .is_err());
    }
}
