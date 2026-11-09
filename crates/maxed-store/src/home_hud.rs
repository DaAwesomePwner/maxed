//! Session/Today HUD aggregates in one read snapshot, independent of loot pagination.
use crate::CatalogQueryData;
use crate::{
    home::with_home_scope, party_average::with_scoped_party_average, project::micros,
    stats::unsigned, StoreError, StoreReader,
};
use maxed_types::{fame_per_hour, HomeHud, HomeScope, PotionStat};
use rusqlite::params;

static HOME_HUD_SQL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    with_scoped_party_average(&with_home_scope(&crate::account_sql::with_accounts(
        include_str!("home_hud.sql"),
    )))
});

impl StoreReader {
    pub fn home_hud(
        &self,
        scope: HomeScope,
        account: Option<&str>,
        tiers: &[(u16, Option<String>)],
        potions: &[(u16, PotionStat)],
    ) -> Result<HomeHud, StoreError> {
        self.home_hud_catalog(scope, account, &CatalogQueryData::new(tiers, potions))
    }

    pub fn home_hud_catalog(
        &self,
        scope: HomeScope,
        account: Option<&str>,
        catalog: &CatalogQueryData,
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
        catalog.identity().validate()?;
        let tiers = catalog.tiers_json();
        let potions = catalog.potions_json();
        let (json, fame, elapsed): (String, Option<i64>, i64) = self.conn.query_row(
            HOME_HUD_SQL.as_str(),
            params![kind, id, lo, hi, account, tiers, potions],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        let mut hud: HomeHud = serde_json::from_str(&json)?;
        for potion in &mut hud.potions {
            potion.object_type = catalog.potion_type(potion.stat);
        }
        let elapsed = unsigned(elapsed)?;
        hud.fame_per_hour = fame.and_then(|fame| fame_per_hour(fame, elapsed));
        Ok(hud)
    }
}
