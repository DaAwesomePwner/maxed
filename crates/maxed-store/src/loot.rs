use crate::{
    project::micros,
    queries::{bounded, identifier, range, row_json},
    StoreError, StoreReader,
};
use maxed_types::*;
use rusqlite::{params, params_from_iter, types::Value};

const MAX_LOOT_BYTES: usize = 1024 * 1024;

fn validate_page(limit: u16, cursor: Option<&DropCursor>) -> Result<(), StoreError> {
    if !(1..=MAX_LOOT_PAGE).contains(&limit) {
        return Err(StoreError::InvalidEvent);
    }
    if let Some(cursor) = cursor {
        identifier(&cursor.id)?;
        micros(cursor.at)?;
    }
    Ok(())
}

fn drop_predicates(filter: &DropFilter, args: &mut Vec<Value>) -> Result<Vec<String>, StoreError> {
    let mut predicates = vec!["1=1".to_owned()];
    if let Some(rank) = filter.min_rank {
        args.push(rank.into());
        predicates.push(format!("d.bag_rank>=?{}", args.len()));
    }
    match &filter.place {
        Some(LootPlace::Dungeon { name }) => {
            if name.is_empty() || name.len() > 256 {
                return Err(StoreError::InvalidEvent);
            }
            args.push(name.clone().into());
            predicates.push(format!("r.dungeon=?{}", args.len()));
        }
        Some(LootPlace::Outside) => predicates.push("d.run_id IS NULL".into()),
        None => {}
    }
    if let Some(item_type) = filter.item_type {
        args.push(item_type.into());
        predicates.push(format!(
            "EXISTS(SELECT 1 FROM drop_items i WHERE i.drop_id=d.id AND i.item_type=?{})",
            args.len()
        ));
    }
    if filter.enchanted_only {
        predicates.push("EXISTS(SELECT 1 FROM drop_items i WHERE i.drop_id=d.id AND json_extract(i.json,'$.enchant.state')='recorded' AND json_extract(i.json,'$.enchant.applied')>0)".into());
    }
    if let Some(time) = &filter.range {
        let (start, end) = range(time)?;
        args.push(start.into());
        predicates.push(format!("d.at>=?{}", args.len()));
        args.push(end.into());
        predicates.push(format!("d.at<?{}", args.len()));
    }
    Ok(predicates)
}

impl StoreReader {
    /// At most nine aggregate rows, read from one consistent snapshot.
    pub fn drop_summary(
        &self,
        request: DropSummaryRequest,
        tiers: &[(u16, Option<String>)],
    ) -> Result<DropSummary, StoreError> {
        if request.days.len() > 8 {
            return Err(StoreError::InvalidEvent);
        }
        if tiers.len() > 65_536
            || tiers
                .iter()
                .any(|(_, tier)| tier.as_ref().is_some_and(|s| s.len() > 16))
        {
            return Err(StoreError::Limit);
        }
        let unique: std::collections::BTreeMap<_, _> = tiers.iter().cloned().collect();
        let tiers = serde_json::to_string(&unique.into_iter().collect::<Vec<_>>())?;
        for day in &request.days {
            range(day)?;
        }
        let tx = self.conn.unchecked_transaction()?;
        let totals = self.drop_totals(&request.filter, &tiers, request.include_items)?;
        let mut days = Vec::with_capacity(request.days.len());
        for day in request.days {
            let mut filter = request.filter.clone();
            let intersection = match &filter.range {
                Some(time) => TimeRange {
                    start: day.start.max(time.start),
                    end: day.end.min(time.end),
                },
                None => day,
            };
            if intersection.start >= intersection.end {
                days.push(DropTotals {
                    items_counted: request.include_items,
                    ut_items: request.include_items.then_some(0),
                    st_items: request.include_items.then_some(0),
                    enchanted_items: request.include_items.then_some(0),
                    runs_ended: (!matches!(filter.place, Some(LootPlace::Outside))).then_some(0),
                    ..Default::default()
                });
            } else {
                filter.range = Some(intersection);
                days.push(self.drop_totals(&filter, &tiers, request.include_items)?);
            }
        }
        let result = bounded(DropSummary { totals, days }, MAX_LOOT_BYTES)?;
        tx.commit()?;
        Ok(result)
    }

    fn drop_totals(
        &self,
        filter: &DropFilter,
        tiers: &str,
        include_items: bool,
    ) -> Result<DropTotals, StoreError> {
        let mut args = Vec::new();
        let predicates = drop_predicates(filter, &mut args)?.join(" AND ");
        let runs_ended = if matches!(filter.place, Some(LootPlace::Outside)) {
            "NULL".to_owned()
        } else {
            let mut ended = vec!["r.ended IS NOT NULL".to_owned()];
            if let Some(LootPlace::Dungeon { name }) = &filter.place {
                args.push(name.clone().into());
                ended.push(format!("r.dungeon=?{}", args.len()));
            }
            if let Some(time) = &filter.range {
                let (start, end) = range(time)?;
                args.push(start.into());
                ended.push(format!("r.ended>=?{}", args.len()));
                args.push(end.into());
                ended.push(format!("r.ended<?{}", args.len()));
            }
            format!(
                "(SELECT count(*) FROM runs r WHERE {})",
                ended.join(" AND ")
            )
        };
        args.push(include_items.into());
        let include_slot = format!("?{}", args.len());
        args.push(MAX_SUMMARY_ITEM_BAGS.into());
        let cap_slot = format!("?{}", args.len());
        args.push(tiers.to_owned().into());
        let sql = include_str!("drop_summary.sql")
            .replace("{predicates}", &predicates)
            .replace("{runs_ended}", &runs_ended)
            .replace("{include_items}", &include_slot)
            .replace("{item_bag_cap}", &cap_slot)
            .replace("{tiers}", &format!("?{}", args.len()));
        let json: Option<String> = self
            .conn
            .query_row(&sql, params_from_iter(args), |row| row.get(0))?;
        bounded(
            serde_json::from_str(&json.ok_or(StoreError::Limit)?)?,
            MAX_LOOT_BYTES,
        )
    }

    pub fn loot_stats(&self, filter: LootStatsFilter) -> Result<LootStats, StoreError> {
        let (start, end) = match filter.range {
            Some(time) => {
                let (start, end) = range(&time)?;
                (Some(start), Some(end))
            }
            None => (None, None),
        };
        let json: Option<String> =
            self.conn
                .query_row(include_str!("loot_stats.sql"), params![start, end], |row| {
                    row.get(0)
                })?;
        bounded(
            serde_json::from_str(&json.ok_or(StoreError::Limit)?)?,
            MAX_LOOT_BYTES,
        )
    }

    /// `tiers` contains catalog (object_type, tier) pairs, as in `scope_loot`.
    pub fn collection(
        &self,
        request: CollectionRequest,
        tiers: &[(u16, Option<String>)],
    ) -> Result<Collection, StoreError> {
        if request.chase.len() > MAX_CHASE_ITEMS {
            return Err(StoreError::InvalidEvent);
        }
        for dungeon in request.dungeon.iter().chain(
            request
                .chase
                .iter()
                .filter_map(|item| item.dungeon.as_ref()),
        ) {
            if dungeon.is_empty() || dungeon.len() > 256 {
                return Err(StoreError::InvalidEvent);
            }
        }
        for item in &request.chase {
            micros(item.pinned_at)?;
        }
        // Match scope_loot's metadata bounds and one-tier-per-type normalization.
        if tiers.len() > 65_536
            || tiers
                .iter()
                .any(|(_, tier)| tier.as_ref().is_some_and(|s| s.len() > 16))
        {
            return Err(StoreError::Limit);
        }
        let unique: std::collections::BTreeMap<_, _> = tiers.iter().cloned().collect();
        let tiers = serde_json::to_string(&unique.into_iter().collect::<Vec<_>>())?;
        let json: Option<String> = self.conn.query_row(
            include_str!("loot_collection.sql"),
            params![
                request.dungeon,
                serde_json::to_string(&request.chase)?,
                tiers
            ],
            |row| row.get(0),
        )?;
        bounded(
            serde_json::from_str(&json.ok_or(StoreError::Limit)?)?,
            MAX_LOOT_BYTES,
        )
    }

    pub fn list_drops(&self, filter: DropFilter) -> Result<DropPage, StoreError> {
        validate_page(filter.limit, filter.cursor.as_ref())?;
        let mut args = Vec::<Value>::new();
        let mut predicates = drop_predicates(&filter, &mut args)?;
        if let Some(cursor) = filter.cursor {
            args.push(micros(cursor.at)?.into());
            let number = args.len();
            args.push(cursor.id.into());
            predicates.push(format!("(d.at,d.id)<(?{number},?{})", args.len()));
        }
        args.push((i64::from(filter.limit) + 1).into());
        let sql = format!(
            "SELECT length(CAST(d.json AS BLOB)),d.json,r.dungeon FROM drops d LEFT JOIN runs r ON r.id=d.run_id WHERE {} ORDER BY d.at DESC,d.id DESC LIMIT ?{}",
            predicates.join(" AND "), args.len()
        );
        let mut statement = self.conn.prepare(&sql)?;
        let mut result = statement.query(params_from_iter(args))?;
        let mut rows = Vec::new();
        let mut last = None;
        let mut more = false;
        let mut bytes = 0;
        while let Some(row) = result.next()? {
            if rows.len() == usize::from(filter.limit) {
                more = true;
                break;
            }
            let drop: DropView = row_json(row, 0, &mut bytes, MAX_LOOT_BYTES)?;
            last = Some(DropCursor {
                at: drop.at,
                id: drop.id.clone(),
            });
            rows.push(LootDropRow {
                drop,
                dungeon: row.get(2)?,
            });
        }
        bounded(
            DropPage {
                rows,
                next: if more { last } else { None },
            },
            MAX_LOOT_BYTES,
        )
    }

    pub fn item_history(&self, request: ItemHistoryRequest) -> Result<ItemHistory, StoreError> {
        validate_page(request.limit, request.cursor.as_ref())?;
        let tx = self.conn.unchecked_transaction()?;
        let totals = tx.query_row(
            "SELECT count(*),count(DISTINCT i.drop_id),count(i.taken_observed_at),min(d.at),max(d.at) FROM drop_items i JOIN drops d INDEXED BY drops_id_at ON d.id=i.drop_id WHERE i.item_type=?1",
            [request.item_type],
            |row| Ok(ItemTotals {
                appearances: row.get(0)?,
                bags: row.get(1)?,
                taken: row.get(2)?,
                first_at: row.get::<_, Option<i64>>(3)?.map(|at| Timestamp(at as u64)),
                last_at: row.get::<_, Option<i64>>(4)?.map(|at| Timestamp(at as u64)),
            }),
        )?;
        let enchant_variants = {
            let mut statement = tx.prepare(
                "WITH enchants AS (
                    SELECT json_extract(json,'$.enchant.state') AS state,
                        CASE WHEN json_extract(json,'$.enchant.state') IN ('missing','invalid') THEN NULL ELSE json_extract(json,'$.enchant.applied') END AS applied,
                        CASE WHEN json_extract(json,'$.enchant.state') IN ('missing','invalid') THEN '[]' ELSE json_extract(json,'$.enchant.ids') END AS ids
                    FROM drop_items WHERE item_type=?1
                ), variants AS (
                    SELECT json_object('state',state,'applied',applied,'ids',json(ids),'count',count(*)) AS json,
                        count(*) AS n,state,applied,ids
                    FROM enchants GROUP BY state,applied,ids
                ) SELECT length(CAST(json AS BLOB)),json FROM variants
                    ORDER BY n DESC,state,applied,ids LIMIT 32",
            )?;
            let mut query = statement.query([request.item_type])?;
            let mut variants = Vec::new();
            let mut bytes = 0;
            while let Some(row) = query.next()? {
                variants.push(row_json::<EnchantVariant>(
                    row,
                    0,
                    &mut bytes,
                    MAX_LOOT_BYTES,
                )?);
            }
            variants
        };
        let by_dungeon = {
            let mut statement = tx.prepare(include_str!("loot_item_places.sql"))?;
            let rows = statement.query_map([request.item_type], |row| {
                Ok(PlaceCount {
                    dungeon: row.get(0)?,
                    count: row.get(1)?,
                    last_at: Timestamp(row.get::<_, i64>(2)? as u64),
                    runs_since: row.get(3)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let by_source = {
            let mut statement = tx.prepare(
                "SELECT d.source_type,json_extract(d.json,'$.source_estimated') AS estimated,count(*) FROM drop_items i JOIN drops d ON d.id=i.drop_id WHERE i.item_type=?1 GROUP BY d.source_type,estimated ORDER BY count(*) DESC,d.source_type,estimated LIMIT 64",
            )?;
            let rows = statement.query_map([request.item_type], |row| {
                Ok(SourceCount {
                    source_enemy: row.get(0)?,
                    estimated: row.get(1)?,
                    count: row.get(2)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let bags = self.list_drops(DropFilter {
            min_rank: None,
            place: None,
            item_type: Some(request.item_type),
            enchanted_only: false,
            range: None,
            cursor: request.cursor,
            limit: request.limit,
        })?;
        let history = bounded(
            ItemHistory {
                item_type: request.item_type,
                totals,
                enchant_variants,
                by_dungeon,
                by_source,
                bags,
            },
            MAX_LOOT_BYTES,
        )?;
        tx.commit()?;
        Ok(history)
    }
}
