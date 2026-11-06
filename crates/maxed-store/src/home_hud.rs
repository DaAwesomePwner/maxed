//! Session/Today HUD aggregates in one read snapshot, independent of loot pagination.
use crate::{
    home::with_home_scope, party_average::with_scoped_party_average, project::micros,
    stats::unsigned, StoreError, StoreReader,
};
use maxed_types::{fame_per_hour, HomeHud, HomeScope, PotionStat};
use rusqlite::params;
use std::collections::BTreeMap;

static HOME_HUD_SQL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    with_scoped_party_average(&with_home_scope(include_str!("home_hud.sql")))
});

impl StoreReader {
    pub fn home_hud(
        &self,
        scope: HomeScope,
        tiers: &[(u16, Option<String>)],
        potions: &[(u16, PotionStat)],
    ) -> Result<HomeHud, StoreError> {
        crate::validate_scope_loot(&scope, None, 1)?;
        let (kind, id, lo, hi) = match &scope {
            HomeScope::Session { id } => (0, Some(id.as_str()), None, None),
            HomeScope::Today { range } => (
                1,
                None,
                Some(micros(range.start)?),
                Some(micros(range.end)?),
            ),
        };
        crate::validate_loot_catalog(tiers, potions)?;
        // Like scope_loot, one catalog classification per item type prevents duplicate counts.
        let tiers: BTreeMap<_, _> = tiers.iter().cloned().collect();
        let potions: BTreeMap<_, _> = potions.iter().copied().collect();
        let tiers = serde_json::to_string(&tiers.into_iter().collect::<Vec<_>>())?;
        let potions = serde_json::to_string(&potions.into_iter().collect::<Vec<_>>())?;
        let (json, fame, elapsed): (String, Option<i64>, i64) = self.conn.query_row(
            HOME_HUD_SQL.as_str(),
            params![kind, id, lo, hi, tiers, potions],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let mut hud: HomeHud = serde_json::from_str(&json)?;
        let elapsed = unsigned(elapsed)?;
        hud.fame_per_hour = fame.and_then(|fame| fame_per_hour(fame, elapsed));
        Ok(hud)
    }
}
