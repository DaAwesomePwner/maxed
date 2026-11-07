//! Party player history and saved run member projections.

use crate::{
    project::{json, micros},
    queries::{bounded, identifier, row_json, rows},
    StoreError, StoreReader,
};
use maxed_engine::names::display_name;
use maxed_engine::party::{contribution_flags, FlagInput};
use maxed_types::*;
use rusqlite::{params, Connection, Params, Transaction, TransactionBehavior};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;

const MAX_PARTY_BYTES: usize = 1024 * 1024;
const MAX_MEMBERS: usize = 512;
const PLAYERS_PAGE: usize = 100;
const RUNS_PAGE: usize = 20;

pub enum PartyWrite {
    Mark {
        key: String,
        name: String,
        mark: Option<PartyMark>,
        note: Option<String>,
    },
    Clear(ClearPartyHistory),
}

impl PartyWrite {
    pub fn apply(&self, writer: &crate::StoreWriter) -> Result<(), StoreError> {
        match self {
            Self::Mark {
                key,
                name,
                mark,
                note,
            } => writer.set_party_mark(key, name, mark.clone(), note.as_deref()),
            Self::Clear(options) => writer.clear_party(options),
        }
    }
}

/// Apply a Party edit while capture is stopped. The caller holds the live
/// namespace lease. Opening the file does not recover sessions or claim alerts.
pub fn party_write_file(path: &std::path::Path, op: &PartyWrite) -> Result<(), StoreError> {
    let mut conn = crate::migrate::open_writer(path)?;
    match op {
        PartyWrite::Mark {
            key,
            name,
            mark,
            note,
        } => set_mark(&conn, key, name, mark.clone(), note.as_deref()),
        PartyWrite::Clear(options) => clear(&mut conn, options),
    }
}

#[derive(Default)]
struct ReadBudget {
    used: usize,
}

impl ReadBudget {
    fn row<T: DeserializeOwned>(
        &mut self,
        row: &rusqlite::Row<'_>,
        column: usize,
    ) -> Result<T, StoreError> {
        row_json(row, column, &mut self.used, MAX_PARTY_BYTES)
    }

    fn rows<T: DeserializeOwned>(
        &mut self,
        conn: &Connection,
        sql: &str,
        args: impl Params,
        limit: usize,
    ) -> Result<Vec<T>, StoreError> {
        rows(conn, sql, args, limit, &mut self.used, MAX_PARTY_BYTES)
    }

    fn one<T: DeserializeOwned>(
        &mut self,
        conn: &Connection,
        sql: &str,
        args: impl Params,
    ) -> Result<Option<T>, StoreError> {
        Ok(self.rows(conn, sql, args, 1)?.pop())
    }
}

// Keep aggregation in SQL; a rejoining player can have multiple member rows per run.
// Detail reads budget profile bytes before decoding; list reads extract only class and maxed.
const PLAYER_ROWS: &str = "WITH players AS (
    SELECT p.*,
        (SELECT count(DISTINCT run_id) FROM party_run_members WHERE key=p.key) AS runs_together,
        (SELECT count(DISTINCT run_id) FROM party_run_members WHERE key=p.key AND died<>0) AS deaths,
        (SELECT count(DISTINCT run_id) FROM party_run_members WHERE key=p.key AND left_early<>0) AS left_early,
        (SELECT avg(share) FROM (
            SELECT sum(share) AS share FROM party_run_members WHERE key=p.key GROUP BY run_id
        )) AS avg_share,
        (SELECT m.class_id FROM party_run_members m JOIN runs r ON r.id=m.run_id
            WHERE m.key=p.key ORDER BY r.started DESC,r.id DESC,m.player_id LIMIT 1) AS last_class_id
    FROM party_players p
)";

const PLAYER_PAYLOAD: &str =
    "json_object('key',key,'name',name,'last_class_id',last_class_id,'last_maxed',NULL,
    'runs_together',runs_together,'deaths',deaths,'left_early',left_early,'avg_share',avg_share,
    'last_seen',CAST(last_seen AS TEXT),'last_map',last_map,'mark',mark,'note',note)";

fn player_record(
    row: &rusqlite::Row<'_>,
    budget: &mut ReadBudget,
) -> Result<PartyPlayerView, StoreError> {
    let mut player: PartyPlayerRow = budget.row(row, 0)?;
    let profile: Option<PlayerProfileView> = if row.get::<_, Option<i64>>(2)?.is_some() {
        Some(budget.row(row, 2)?)
    } else {
        None
    };
    if let Some(profile) = &profile {
        player.last_class_id = profile.class_id;
        player.last_maxed = profile.maxed;
    }
    Ok(PartyPlayerView {
        row: player,
        guild: profile.as_ref().and_then(|p| p.guild.clone()),
        first_seen: timestamp(row.get(4)?)?,
        last_profile: profile,
        runs: vec![],
        next: None,
    })
}

fn timestamp(value: i64) -> Result<Timestamp, StoreError> {
    Ok(Timestamp(
        u64::try_from(value).map_err(|_| StoreError::InvalidEvent)?,
    ))
}

fn annotations(
    conn: &Connection,
    keys: &[String],
    budget: &mut ReadBudget,
) -> Result<Vec<PartyAnnotation>, StoreError> {
    if keys.len() > MAX_MEMBERS {
        return Err(StoreError::Limit);
    }
    bounded(keys, MAX_PARTY_BYTES)?;
    budget.rows(conn,
        "SELECT length(CAST(payload AS BLOB)),payload FROM (
            SELECT json_object('key',key,'mark',mark,'note',note,'runs_together',
                (SELECT count(DISTINCT run_id) FROM party_run_members m WHERE m.key=p.key)) AS payload
            FROM party_players p WHERE key IN (SELECT value FROM json_each(?1)) ORDER BY key)",
        [serde_json::to_string(keys)?], MAX_MEMBERS)
}

impl StoreReader {
    pub fn party_annotations(&self, keys: &[String]) -> Result<Vec<PartyAnnotation>, StoreError> {
        bounded(
            annotations(&self.conn, keys, &mut ReadBudget::default())?,
            MAX_PARTY_BYTES,
        )
    }

    pub fn list_party_players(
        &self,
        query: &str,
        filter: PartyPlayerFilter,
        sort: PartyPlayerSort,
        cursor: Option<&PartyPlayersCursor>,
    ) -> Result<PartyPlayersPage, StoreError> {
        bounded(query, MAX_PARTY_BYTES)?;
        let sort_value = cursor
            .map(|cursor| {
                identifier(&cursor.key)?;
                let bits = cursor
                    .sort_value
                    .parse::<u64>()
                    .map_err(|_| StoreError::InvalidEvent)?;
                let value = f64::from_bits(bits);
                if !value.is_finite() || bits.to_string() != cursor.sort_value {
                    return Err(StoreError::InvalidEvent);
                }
                Ok(value)
            })
            .transpose()?;
        let sort = match sort {
            // Compare the same f64 representation encoded in the cursor to preserve ties.
            PartyPlayerSort::LastSeen => "CAST(last_seen AS REAL)",
            PartyPlayerSort::RunsTogether => "runs_together",
            PartyPlayerSort::Deaths => "deaths",
            PartyPlayerSort::LeftEarly => "left_early",
            PartyPlayerSort::AvgShare => "coalesce(avg_share,-1.0)",
        };
        let filter = match filter {
            PartyPlayerFilter::All => "1=1",
            PartyPlayerFilter::Watched => "mark='watched'",
            PartyPlayerFilter::Banned => "mark='banned'",
            PartyPlayerFilter::WithNotes => "note IS NOT NULL AND note<>''",
        };
        let query = format!(
            "%{}%",
            query
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let tx = self.conn.unchecked_transaction()?;
        let page = {
            let sql = format!(
                "{PLAYER_ROWS}, payloads AS (
                    SELECT *,json_set({PLAYER_PAYLOAD},
                        '$.last_class_id',CASE WHEN last_profile_json IS NULL THEN last_class_id
                            ELSE json_extract(last_profile_json,'$.class_id') END,
                        '$.last_maxed',json_extract(last_profile_json,'$.maxed')) AS payload FROM players
                ) SELECT length(CAST(payload AS BLOB)),payload,{sort} AS sort_value FROM payloads
                WHERE name LIKE ?1 ESCAPE '\\' COLLATE NOCASE AND ({filter})
                    AND (?2 IS NULL OR {sort}<?2 OR ({sort}=?2 AND key>?3))
                ORDER BY {sort} DESC,key ASC LIMIT 101"
            );
            let mut stmt = tx.prepare(&sql)?;
            let mut query = stmt.query(params![query, sort_value, cursor.map(|c| &c.key)])?;
            let mut budget = ReadBudget::default();
            let mut page = PartyPlayersPage {
                rows: vec![],
                next: None,
            };
            let mut last = None;
            while let Some(row) = query.next()? {
                if page.rows.len() == PLAYERS_PAGE {
                    page.next = last;
                    break;
                }
                let player: PartyPlayerRow = budget.row(row, 0)?;
                last = Some(PartyPlayersCursor {
                    sort_value: row.get::<_, f64>(2)?.to_bits().to_string(),
                    key: player.key.clone(),
                });
                page.rows.push(player);
            }
            bounded(page, MAX_PARTY_BYTES)?
        };
        tx.commit()?;
        Ok(page)
    }

    pub fn get_party_player(
        &self,
        key: &str,
        cursor: Option<&PartyRunsCursor>,
    ) -> Result<Option<PartyPlayerView>, StoreError> {
        identifier(key)?;
        let started = cursor
            .map(|c| {
                identifier(&c.run_id)?;
                micros(c.started)
            })
            .transpose()?;
        let tx = self.conn.unchecked_transaction()?;
        let mut budget = ReadBudget::default();
        let mut player = {
            let mut stmt = tx.prepare(&format!("{PLAYER_ROWS}, payloads AS (
                SELECT *,{PLAYER_PAYLOAD} AS payload FROM players
            ) SELECT length(CAST(payload AS BLOB)),payload,length(CAST(last_profile_json AS BLOB)),last_profile_json,first_seen
                FROM payloads WHERE key=?1"))?;
            let mut query = stmt.query([key])?;
            let Some(row) = query.next()? else {
                return Ok(None);
            };
            player_record(row, &mut budget)?
        };
        {
            let mut stmt = tx.prepare("SELECT length(CAST(payload AS BLOB)),payload FROM (
                SELECT json_object('run_id',r.id,'dungeon',r.dungeon,'started',CAST(r.started AS TEXT),'outcome',r.outcome,
                    'share',sum(m.share),'died',json(CASE WHEN max(m.died)<>0 THEN 'true' ELSE 'false' END),
                    'left_early',json(CASE WHEN max(m.left_early)<>0 THEN 'true' ELSE 'false' END)) AS payload
                FROM party_run_members m JOIN runs r ON r.id=m.run_id WHERE m.key=?1
                    AND (?2 IS NULL OR (r.started,r.id)<(?2,?3))
                GROUP BY r.id ORDER BY r.started DESC,r.id DESC LIMIT 21)")?;
            let mut query = stmt.query(params![key, started, cursor.map(|c| &c.run_id)])?;
            while let Some(row) = query.next()? {
                if player.runs.len() == RUNS_PAGE {
                    player.next = player.runs.last().map(|r| PartyRunsCursor {
                        started: r.started,
                        run_id: r.run_id.clone(),
                    });
                    break;
                }
                player.runs.push(budget.row(row, 0)?);
            }
        }
        let player = bounded(player, MAX_PARTY_BYTES)?;
        tx.commit()?;
        Ok(Some(player))
    }
}

pub(crate) fn set_mark(
    conn: &Connection,
    key: &str,
    name: &str,
    mark: Option<PartyMark>,
    note: Option<&str>,
) -> Result<(), StoreError> {
    identifier(key)?;
    identifier(name)?;
    if key != name_key(name) || note.is_some_and(|n| n.chars().count() > 500) {
        return Err(StoreError::InvalidEvent);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_micros()).ok())
        .ok_or(StoreError::InvalidEvent)?;
    let mark = mark.map(|m| match m {
        PartyMark::Watched => "watched",
        PartyMark::Banned => "banned",
    });
    conn.execute("INSERT INTO party_players(key,name,first_seen,last_seen,updated,mark,note)
        VALUES(?1,?2,?3,?3,?3,?4,?5) ON CONFLICT(key) DO UPDATE SET mark=excluded.mark,note=excluded.note,updated=excluded.updated",
        params![key, display_name(name), now, mark, note])?;
    Ok(())
}

pub(crate) fn clear(conn: &mut Connection, opts: &ClearPartyHistory) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute("DELETE FROM party_run_members", [])?;
    if opts.marks_and_notes {
        tx.execute("DELETE FROM party_players", [])?;
    } else {
        tx.execute(
            "DELETE FROM party_players WHERE mark IS NULL AND (note IS NULL OR note='')",
            [],
        )?;
        tx.execute(
            "UPDATE party_players SET last_profile_json=NULL,last_scope_id=NULL,last_map=NULL",
            [],
        )?;
    }
    tx.execute(
        "DELETE FROM player_gear_log WHERE EXISTS (SELECT 1 FROM player_profiles p
            WHERE p.scope_id=player_gear_log.scope_id AND p.player_id=player_gear_log.player_id AND p.is_local=0)",
        [],
    )?;
    tx.execute(
        "DELETE FROM player_timelines WHERE json_extract(json,'$.is_local')=0",
        [],
    )?;
    tx.execute("DELETE FROM player_profiles WHERE is_local=0", [])?;
    tx.commit()?;
    Ok(())
}

#[derive(serde::Deserialize)]
struct RunDamage {
    player_id: String,
    name: Option<String>,
    class_id: Option<u16>,
    is_local: bool,
    damage: f64,
}

#[derive(serde::Deserialize)]
struct MemberFacts {
    player_id: String,
    joined: Option<Timestamp>,
    departed_at: Option<Timestamp>,
    left_early: bool,
    damage: f64,
    share: Option<f64>,
    boss_share: Option<f64>,
}

struct SavedMember {
    member: PartyMember,
    joined: Option<Timestamp>,
    departed: bool,
    profile_at: Option<Timestamp>,
}

impl SavedMember {
    fn new(player_id: String) -> Self {
        Self {
            member: PartyMember {
                maxed_gated: false,
                player_id,
                name_key: None,
                name: None,
                class_id: None,
                level: None,
                stars: None,
                guild: None,
                maxed: None,
                equipment: vec![],
                is_local: false,
                present: None,
                out_of_view_since: None,
                departed_at: None,
                parse: None,
                damage: Some(0.),
                share: None,
                boss_share: None,
                flags: vec![],
                mark: None,
                runs_together: 0,
            },
            joined: None,
            departed: false,
            profile_at: None,
        }
    }

    fn profile(&mut self, profile: PlayerProfileView) {
        self.profile_at = Some(profile.last_seen);
        self.joined = Some(profile.first_seen);
        self.member.name = profile.name.or(self.member.name.take());
        self.member.class_id = profile.class_id;
        self.member.level = profile.level;
        self.member.stars = profile.stars;
        self.member.guild = profile.guild;
        self.member.maxed = profile.maxed;
        self.member.maxed_gated = profile.maxed_gated;
        self.member.equipment = profile.equipment;
        self.member.is_local |= profile.is_local;
        self.normalize_maxed();
    }

    fn normalize_maxed(&mut self) {
        if self.member.is_local && !self.member.maxed_gated {
            self.member.maxed = None;
        }
    }

    fn merge(&mut self, mut other: Self) {
        // Keep all display fields and the drill-down player ID from the latest profile.
        if other.profile_at > self.profile_at {
            std::mem::swap(self, &mut other);
        }
        self.member.damage = sum_known(self.member.damage, other.member.damage);
        self.member.share = sum_known(self.member.share, other.member.share);
        self.member.boss_share = sum_known(self.member.boss_share, other.member.boss_share);
        self.member.is_local |= other.member.is_local;
        self.normalize_maxed();
        self.member.departed_at = earliest(self.member.departed_at, other.member.departed_at);
        self.joined = earliest(self.joined, other.joined);
        self.departed |= other.departed;
    }
}

fn sum_known(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a + b),
        (a, b) => a.or(b),
    }
}

fn earliest(a: Option<Timestamp>, b: Option<Timestamp>) -> Option<Timestamp> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

impl StoreReader {
    pub fn get_party_run(&self, run_id: &str) -> Result<Option<PartyView>, StoreError> {
        identifier(run_id)?;
        let tx = self.conn.unchecked_transaction()?;
        let mut budget = ReadBudget::default();
        let Some(run): Option<RunView> = budget.one(
            &tx,
            "SELECT length(CAST(json AS BLOB)),json FROM runs WHERE id=?1",
            [run_id],
        )?
        else {
            return Ok(None);
        };
        let profiles: Vec<PlayerProfileView> = budget.rows(&tx,
            "SELECT length(CAST(json AS BLOB)),json FROM (
                SELECT json,row_number() OVER (PARTITION BY player_id ORDER BY last_seen DESC,scope_id DESC) AS n
                FROM player_profiles WHERE run_id=?1) WHERE n=1", [run_id], MAX_MEMBERS)?;
        let gear_recorded = !profiles.is_empty();
        let damage: Vec<RunDamage> = budget.rows(
            &tx,
            "SELECT length(CAST(payload AS BLOB)),payload FROM (
                SELECT json_object('player_id',player_id,'name',name,'class_id',class_id,
                    'is_local',json(CASE WHEN is_local<>0 THEN 'true' ELSE 'false' END),
                    'damage',damage+damage_correction) AS payload
                FROM pd_run_players WHERE run_id=?1)",
            [run_id],
            MAX_MEMBERS,
        )?;
        let total: f64 = damage.iter().map(|p| p.damage).sum();
        let mut players = BTreeMap::<String, SavedMember>::new();
        for p in damage {
            let mut saved = SavedMember::new(p.player_id.clone());
            saved.member.name = p.name;
            saved.member.class_id = p.class_id;
            saved.member.is_local = p.is_local;
            saved.member.damage = Some(p.damage);
            saved.joined = Some(run.started);
            players.insert(p.player_id, saved);
        }
        for profile in profiles {
            players
                .entry(profile.player_id.clone())
                .or_insert_with(|| SavedMember::new(profile.player_id.clone()))
                .profile(profile);
        }
        let boss_damage: Vec<(String, f64)> = budget.rows(&tx,
            "SELECT length(CAST(payload AS BLOB)),payload FROM (
                SELECT json_array(p.player_id,sum(p.damage)) AS payload
                FROM fight_summaries s JOIN fights f ON f.id=s.fight_id
                JOIN fight_players p ON p.fight_id=f.id WHERE s.run_id=?1
                    AND (json_extract(s.json,'$.target_kind') IN ('boss','event_boss') OR f.boss_type IS NOT NULL)
                GROUP BY p.player_id)", [run_id], MAX_MEMBERS)?;
        let boss_total: f64 = boss_damage.iter().map(|p| p.1).sum();
        let boss_damage: BTreeMap<_, _> = boss_damage.into_iter().collect();
        for (id, p) in &mut players {
            p.member.share = (total > 0.).then(|| p.member.damage.unwrap_or(0.) / total);
            p.member.boss_share =
                (boss_total > 0.).then(|| boss_damage.get(id).copied().unwrap_or(0.) / boss_total);
        }
        let facts: Vec<MemberFacts> = budget.rows(&tx,
            &format!("SELECT length(CAST(payload AS BLOB)),payload FROM (
                SELECT json_object('player_id',player_id,'joined',CAST(min(joined) AS TEXT),'departed_at',CAST(min(departed_at) AS TEXT),
                    'left_early',json(CASE WHEN max(left_early)<>0 THEN 'true' ELSE 'false' END),
                    'damage',sum(damage),'share',sum(share),'boss_share',sum(boss_share)) AS payload
                FROM ({}) GROUP BY player_id)", run_members_sql()), [run_id], MAX_MEMBERS)?;
        for facts in facts {
            if let Some(p) = players.get_mut(&facts.player_id) {
                p.joined = facts.joined;
                p.departed = facts.left_early;
                p.member.departed_at = facts.departed_at;
                p.member.damage = Some(facts.damage);
                p.member.share = facts.share;
                p.member.boss_share = facts.boss_share;
            }
        }
        let (last_boss_started, boss_fight_ended): (Option<i64>, bool) = tx.query_row(
            "SELECT max(s.started),coalesce(max(json_extract(s.json,'$.ended') IS NOT NULL),0)
                FROM fight_summaries s JOIN fights f ON f.id=s.fight_id WHERE s.run_id=?1
                    AND (json_extract(s.json,'$.target_kind') IN ('boss','event_boss') OR f.boss_type IS NOT NULL)",
            [run_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let last_boss_started = last_boss_started.map(timestamp).transpose()?;
        let mut merged = BTreeMap::<(bool, String), SavedMember>::new();
        for mut p in players.into_values() {
            p.member.name = p
                .member
                .name
                .as_deref()
                .map(display_name)
                .map(str::to_owned);
            p.member.name_key = p
                .member
                .name
                .as_deref()
                .map(name_key)
                .filter(|k| !k.is_empty());
            let key = p.member.name_key.as_ref().map_or_else(
                || (false, p.member.player_id.clone()),
                |key| (true, key.clone()),
            );
            if let Some(existing) = merged.get_mut(&key) {
                existing.merge(p);
            } else {
                merged.insert(key, p);
            }
        }
        if merged.len() > MAX_MEMBERS {
            return Err(StoreError::Limit);
        }
        let keys: Vec<_> = merged
            .values()
            .filter_map(|p| p.member.name_key.clone())
            .collect();
        let annotations: BTreeMap<_, _> = annotations(&tx, &keys, &mut budget)?
            .into_iter()
            .map(|a| (a.key.clone(), a))
            .collect();
        let mut counts = PartyCounts {
            in_view: 0,
            pass: 0,
            fail: 0,
            check: 0,
            not_seen: 0,
            no_damage_seen: 0,
            left_early: 0,
        };
        let mut classes = BTreeMap::<u16, u32>::new();
        let mut members = Vec::with_capacity(merged.len());
        for mut p in merged.into_values() {
            p.member.flags = contribution_flags(&FlagInput {
                in_dungeon: true,
                joined: p.joined,
                departed: p.departed,
                damage: p.member.damage.unwrap_or(0.),
                share: p.member.share,
                boss_share: p.member.boss_share,
                last_boss_started,
                boss_fight_ended,
            });
            if let Some(annotation) = p
                .member
                .name_key
                .as_ref()
                .and_then(|key| annotations.get(key))
            {
                p.member.mark = annotation.mark.clone();
                p.member.runs_together = annotation.runs_together;
            }
            counts.no_damage_seen +=
                u32::from(p.member.flags.contains(&ContributionFlag::NoDamageSeen));
            counts.left_early += u32::from(p.member.flags.contains(&ContributionFlag::LeftEarly));
            if let Some(class) = p.member.class_id {
                *classes.entry(class).or_default() += 1;
            }
            members.push(p.member);
        }
        members.sort_by(|a, b| {
            b.damage
                .partial_cmp(&a.damage)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name_key.cmp(&b.name_key))
                .then_with(|| a.player_id.cmp(&b.player_id))
        });
        let mut classes: Vec<_> = classes
            .into_iter()
            .map(|(class_id, count)| ClassCount { class_id, count })
            .collect();
        classes.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.class_id.cmp(&b.class_id))
        });
        let result = bounded(
            PartyView {
                scope_id: Some(run.id.clone()),
                run_id: Some(run.id),
                map: Some(run.dungeon),
                area_kind: run.area_kind,
                live: false,
                preset: None,
                counts,
                classes,
                members,
                assets_available: false,
                gear_recorded,
            },
            MAX_PARTY_BYTES,
        )?;
        tx.commit()?;
        Ok(Some(result))
    }
}

pub(crate) fn write_party_player(
    tx: &Transaction<'_>,
    profile: &PlayerProfileView,
    map: Option<&str>,
) -> Result<(), StoreError> {
    if profile.is_local {
        return Ok(());
    }
    let Some(name) = profile.name.as_deref() else {
        return Ok(());
    };
    let key = name_key(name);
    if key.is_empty() {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO party_players(key,name,first_seen,last_seen,last_scope_id,last_map,last_profile_json,updated)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?4)
         ON CONFLICT(key) DO UPDATE SET
             first_seen=min(party_players.first_seen,excluded.first_seen),
             name=CASE WHEN party_players.last_profile_json IS NULL OR excluded.last_seen>=CAST(json_extract(party_players.last_profile_json,'$.last_seen') AS INTEGER) THEN excluded.name ELSE party_players.name END,
             last_seen=max(party_players.last_seen,excluded.last_seen),
             last_scope_id=CASE WHEN party_players.last_profile_json IS NULL OR excluded.last_seen>=CAST(json_extract(party_players.last_profile_json,'$.last_seen') AS INTEGER) THEN excluded.last_scope_id ELSE party_players.last_scope_id END,
             last_map=CASE WHEN party_players.last_profile_json IS NULL OR excluded.last_seen>=CAST(json_extract(party_players.last_profile_json,'$.last_seen') AS INTEGER) THEN coalesce(excluded.last_map,party_players.last_map) ELSE party_players.last_map END,
             last_profile_json=CASE WHEN party_players.last_profile_json IS NULL OR excluded.last_seen>=CAST(json_extract(party_players.last_profile_json,'$.last_seen') AS INTEGER) THEN excluded.last_profile_json ELSE party_players.last_profile_json END,
             updated=max(party_players.updated,excluded.updated)",
        params![key, display_name(name), micros(profile.first_seen)?, micros(profile.last_seen)?,
            profile.scope_id, map, json(profile)?],
    )?;
    Ok(())
}

pub(crate) fn rebuild_run_members(tx: &Transaction<'_>, run_id: &str) -> Result<(), StoreError> {
    tx.execute("DELETE FROM party_run_members WHERE run_id=?1", [run_id])?;
    tx.execute(
        &format!(
            "INSERT INTO party_run_members
             (key,run_id,player_id,class_id,joined,departed_at,left_early,died,damage,share,boss_share)
             {}", run_members_sql()
        ),
        [run_id],
    )?;
    // Clear can remove a player's history and profile while their run is open.
    // Restore discoverability from the surviving member evidence, keeping marks
    // and newer observations on any existing row.
    tx.execute(
        "INSERT OR IGNORE INTO party_players(key,name,first_seen,last_seen,last_map,updated)
         WITH observations AS (
             SELECT m.key,r.started,r.dungeon,coalesce(p.name,
                 (SELECT json_extract(json,'$.name') FROM player_profiles
                  WHERE run_id=m.run_id AND player_id=m.player_id ORDER BY last_seen DESC,scope_id DESC LIMIT 1)) AS name
             FROM party_run_members m JOIN runs r ON r.id=m.run_id
             LEFT JOIN pd_run_players p ON p.run_id=m.run_id AND p.player_id=m.player_id
             WHERE m.run_id=?1
         )
         SELECT key,CASE WHEN instr(name, ',')>0 THEN substr(name,1,instr(name, ',')-1) ELSE name END,
             started,started,dungeon,started FROM observations ORDER BY key,name",
        [run_id],
    )?;
    Ok(())
}

// Both reads and rebuilds derive facts from the durable fight/profile/event
// evidence. party_run_members is only the clearable cross-run history index.
fn run_members_sql() -> String {
    format!(
        "WITH damage_members AS ({RUN_MEMBERS_SQL}), profile_members AS ({PROFILE_MEMBERS_SQL})
        SELECT * FROM damage_members UNION ALL SELECT * FROM profile_members"
    )
}

// Profiles without fight contributions still have joined/departure evidence.
const PROFILE_MEMBERS_SQL: &str = "WITH profiles AS (
    SELECT p.*,json_extract(json,'$.name') AS name,json_extract(json,'$.class_id') AS class_id
    FROM player_profiles p WHERE run_id=?1 AND is_local=0
        AND NOT EXISTS (SELECT 1 FROM pd_run_players m
            WHERE m.run_id=p.run_id AND m.player_id=p.player_id
                AND m.is_local=0 AND m.name IS NOT NULL
                AND lower(CASE WHEN instr(m.name, ',') > 0 THEN substr(m.name, 1, instr(m.name, ',') - 1) ELSE m.name END)<>'')
), named_profiles AS (
    SELECT p.*,
        lower(CASE WHEN instr(name, ',') > 0 THEN substr(name, 1, instr(name, ',') - 1) ELSE name END) AS key
    FROM profiles p WHERE name IS NOT NULL
), departures AS (
    SELECT at,json_extract(json,'$.kind.object_id') AS object_id,
        json_extract(json,'$.kind.player_name') AS name
    FROM instance_events WHERE run_id=?1 AND kind='player_left' AND json_valid(json)
), object_departures AS (
    SELECT object_id,min(at) AS at FROM departures GROUP BY object_id
), name_departures AS (
    SELECT lower(CASE WHEN instr(name, ',') > 0 THEN substr(name, 1, instr(name, ',') - 1) ELSE name END) AS key,
        min(at) AS at
    FROM departures e WHERE name IS NOT NULL
        AND NOT EXISTS (SELECT 1 FROM fights f JOIN fight_players p ON p.fight_id=f.id
            WHERE f.run_id=?1 AND p.object_id=e.object_id)
        AND NOT EXISTS (SELECT 1 FROM player_profiles p WHERE p.run_id=?1 AND p.object_id=e.object_id)
    GROUP BY key
), members AS (
    SELECT p.*,coalesce(o.at,n.at) AS departed_at
    FROM named_profiles p
    LEFT JOIN object_departures o ON o.object_id=p.object_id
    LEFT JOIN name_departures n ON n.key=p.key
    WHERE p.key<>''
)
SELECT key,run_id,player_id,class_id,first_seen,departed_at,departed_at IS NOT NULL,0,0,
    (SELECT CASE WHEN sum(damage+damage_correction)>0 THEN 0.0 END
        FROM pd_run_players WHERE run_id=?1),
    (SELECT CASE WHEN sum(p.damage)>0 THEN 0.0 END
        FROM fight_summaries s JOIN fights f ON f.id=s.fight_id
        JOIN fight_players p ON p.fight_id=f.id WHERE s.run_id=?1
            AND (json_extract(s.json,'$.target_kind') IN ('boss','event_boss') OR f.boss_type IS NOT NULL))
FROM members";

/// Select members for run ID `?1`, in `party_run_members` column order:
/// key, run_id, player_id, class_id, joined, departed_at, left_early, died,
/// damage, share, boss_share. Object-ID departures take precedence over name
/// fallback. Local and unnamed players still contribute to damage denominators.
/// Migration 014 keeps a frozen all-run copy of this query.
pub const RUN_MEMBERS_SQL: &str = "WITH run_players AS (
    SELECT p.*,r.started,
        lower(CASE WHEN instr(name, ',') > 0 THEN substr(name, 1, instr(name, ',') - 1) ELSE name END) AS key
    FROM pd_run_players p JOIN runs r ON r.id=p.run_id WHERE p.run_id=?1
), run_totals AS (
    SELECT run_id,sum(damage+damage_correction) AS damage FROM run_players GROUP BY run_id
), boss_damage AS (
    SELECT s.run_id,p.player_id,sum(p.damage) AS damage
    FROM fight_summaries s JOIN fights f ON f.id=s.fight_id
    JOIN fight_players p ON p.fight_id=f.id JOIN run_totals t ON t.run_id=s.run_id
    WHERE json_extract(s.json,'$.target_kind') IN ('boss','event_boss') OR f.boss_type IS NOT NULL
    GROUP BY s.run_id,p.player_id
), boss_totals AS (
    SELECT run_id,sum(damage) AS damage FROM boss_damage GROUP BY run_id
), departures AS (
    SELECT e.run_id,e.at,json_extract(e.json,'$.kind.object_id') AS object_id,
        json_extract(e.json,'$.kind.player_name') AS name
    FROM instance_events e JOIN run_totals t ON t.run_id=e.run_id
    WHERE e.kind='player_left' AND json_valid(e.json)
), run_objects AS (
    SELECT f.run_id,p.player_id,p.object_id
    FROM fights f JOIN fight_players p ON p.fight_id=f.id JOIN run_totals t ON t.run_id=f.run_id
    UNION
    SELECT p.run_id,p.player_id,p.object_id FROM player_profiles p JOIN run_totals t ON t.run_id=p.run_id
), object_departures AS (
    SELECT p.run_id,p.player_id,min(e.at) AS at
    FROM departures e JOIN run_objects p ON p.run_id=e.run_id AND p.object_id=e.object_id
    GROUP BY p.run_id,p.player_id
), name_departures AS (
    SELECT run_id,
        lower(CASE WHEN instr(name, ',') > 0 THEN substr(name, 1, instr(name, ',') - 1) ELSE name END) AS key,
        min(at) AS at
    FROM departures e WHERE name IS NOT NULL
        AND NOT EXISTS (SELECT 1 FROM run_objects p WHERE p.run_id=e.run_id AND p.object_id=e.object_id)
    GROUP BY run_id,key
), members AS (
    SELECT p.*,coalesce(o.at,n.at) AS departed_at
    FROM run_players p
    LEFT JOIN object_departures o ON o.run_id=p.run_id AND o.player_id=p.player_id
    LEFT JOIN name_departures n ON n.run_id=p.run_id AND n.key=p.key
    WHERE p.is_local=0 AND p.name IS NOT NULL AND p.key<>''
)
SELECT m.key,m.run_id,m.player_id,m.class_id,m.started AS joined,m.departed_at,
    m.departed_at IS NOT NULL AS left_early,m.deaths>0 AS died,m.damage+m.damage_correction AS damage,
    (m.damage+m.damage_correction)/nullif(t.damage,0) AS share,
    coalesce(b.damage,0)/nullif(bt.damage,0) AS boss_share
FROM members m JOIN run_totals t ON t.run_id=m.run_id
LEFT JOIN boss_damage b ON b.run_id=m.run_id AND b.player_id=m.player_id
LEFT JOIN boss_totals bt ON bt.run_id=m.run_id";
