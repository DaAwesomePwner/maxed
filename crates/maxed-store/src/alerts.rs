//! Durable decisions share the source transaction; claims precede any effect.
use crate::{
    migrate::digest,
    project::{json, micros, tag},
    StoreError, StoreReader,
};
use maxed_engine::alerts::{
    beats_previous, chat_alert_kind, chat_excerpt, is_white_bag, personal_best_candidate,
    PersonalBestCandidate,
};
use maxed_types::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

/// Runtime decisions for one chat event. None means store it, unhidden, no alert.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatContext {
    pub store: bool,
    pub hidden: bool,
    pub alertable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlertContext {
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub chat: Option<ChatContext>,
    pub preferences: AlertPreferences,
    pub bag_window: Option<TimeRange>,
    // Set only for eligible live notices, containing catalog text and no identity.
    #[serde(default)]
    pub key_pop_dungeon: Option<String>,
}
impl Default for AlertContext {
    fn default() -> Self {
        Self {
            live: false,
            chat: None,
            preferences: AlertPreferences::disabled(),
            key_pop_dungeon: None,
            bag_window: None,
        }
    }
}
fn chat_eligible(context: &AlertContext, m: &ChatMessageView) -> Option<AlertKind> {
    let chat = context.chat?;
    (context.live && chat.alertable && !chat.hidden)
        .then(|| chat_alert_kind(m))
        .flatten()
}

impl AlertContext {
    /// Events whose alert decisions consume preferences. Validation still applies
    /// to every context; callers must retain invalid preferences for rejection.
    pub fn uses_preferences(event: &EngineEvent) -> bool {
        match event {
            EngineEvent::BagDropped(_)
            | EngineEvent::DropUpdated(_)
            | EngineEvent::FightEnded(_)
            | EngineEvent::RunEnded(_)
            | EngineEvent::InstanceEvent(_)
            | EngineEvent::Chat(_) => true,
            EngineEvent::FightUpdated(fight) => fight.ended.is_some(),
            EngineEvent::RunUpdated(run) => run.completed_at.is_some(),
            _ => false,
        }
    }

    pub(crate) fn needs_prompt_commit(&self, event: &EngineEvent) -> bool {
        let kind = match event {
            EngineEvent::BagDropped(drop) | EngineEvent::DropUpdated(drop)
                if is_white_bag(drop) =>
            {
                AlertKind::WhiteBag
            }
            // Prior records and character eligibility are resolved in the transaction.
            EngineEvent::FightEnded(_) => AlertKind::PersonalBest,
            EngineEvent::FightUpdated(fight) if fight.ended.is_some() => AlertKind::PersonalBest,
            EngineEvent::RunEnded(_) if self.live => AlertKind::PersonalBest,
            EngineEvent::RunUpdated(run) if self.live && run.completed_at.is_some() => {
                AlertKind::PersonalBest
            }
            EngineEvent::InstanceEvent(view)
                if !view.provisional
                    && matches!(view.kind, InstanceEventKind::PortalOpened { .. })
                    && self
                        .key_pop_dungeon
                        .as_deref()
                        .is_some_and(|d| self.preferences.includes_key_pop_dungeon(d)) =>
            {
                AlertKind::KeyPopped
            }
            EngineEvent::Chat(m) => match chat_eligible(self, m) {
                Some(kind) => kind,
                None => return false,
            },
            _ => return false,
        };
        matches!(
            self.preferences.loudness(&kind),
            AlertLoudness::Toast | AlertLoudness::Banner
        )
    }

    pub(crate) fn validate(&self) -> Result<usize, StoreError> {
        if !self.preferences.valid()
            || self
                .key_pop_dungeon
                .as_deref()
                .is_some_and(|d| !id_valid(d))
        {
            return Err(StoreError::InvalidEvent);
        }
        if let Some(w) = &self.bag_window {
            if w.start >= w.end {
                return Err(StoreError::InvalidEvent);
            }
            micros(w.start)?;
            micros(w.end)?;
        }
        Ok(json(self)?.len())
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ContextualEvent {
    pub event: EventEnvelope,
    pub context: AlertContext,
}
#[derive(Debug)]
pub struct ContextualShutdownError {
    pub error: StoreError,
    pub pending: Vec<ContextualEvent>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct CommittedAlert {
    pub view: AlertView,
    pub source_stream_id: String,
    pub source_sequence: Sequence,
    pub loudness: AlertLoudness,
    pub committed_at: Timestamp,
}

fn identity(parts: &[&str]) -> String {
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }
    digest(&bytes)
}
fn occurrence_id(kind: &AlertKind, source: &str) -> Result<String, StoreError> {
    Ok(identity(&["p5-v1", &tag(kind)?, source]))
}
fn id_valid(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
}
pub(crate) fn alert_json(view: &AlertView) -> Result<String, StoreError> {
    if !id_valid(&view.id)
        || view.run_id.as_deref().is_some_and(|id| !id_valid(id))
        || view.explanation.len() > 2048
    {
        return Err(StoreError::Limit);
    }
    match &view.details {
        Some(AlertDetails::WhiteBag { drop_id, .. }) if !id_valid(drop_id) => {
            return Err(StoreError::Limit)
        }
        Some(AlertDetails::Chat {
            sender, excerpt, ..
        }) if sender.len() > 256 || excerpt.chars().count() > 120 => return Err(StoreError::Limit),
        Some(AlertDetails::PersonalBest {
            fight_id,
            character_id,
            ..
        }) if !id_valid(fight_id) || !id_valid(character_id) => return Err(StoreError::Limit),
        Some(AlertDetails::Death {
            character_id: Some(id),
        }) if !id_valid(id) => return Err(StoreError::Limit),
        _ => {}
    }
    let text = json(view)?;
    if text.len() > 16 * 1024 {
        return Err(StoreError::Limit);
    }
    Ok(text)
}

pub(crate) fn candidate(
    conn: &Connection,
    fight: &FightView,
) -> Result<Option<PersonalBestCandidate>, StoreError> {
    if fight.dps_basis != DpsBasis::PlayerActiveWindowV1 {
        return Ok(None);
    }
    let mut locals = fight.players.iter().filter(|p| p.is_local);
    let Some(local) = locals.next() else {
        return Ok(None);
    };
    if locals.next().is_some() {
        return Ok(None);
    }
    let canonical = crate::project::resolve(conn, &local.player_id)?;
    let text: Option<String> = conn
        .query_row(
            "SELECT json FROM characters WHERE id=?1",
            [&canonical],
            |r| r.get(0),
        )
        .optional()?;
    let Some(text) = text else {
        return Ok(None);
    };
    if text.len() > crate::project::MAX_EVENT_BYTES {
        return Err(StoreError::Limit);
    }
    let mut character: CharacterView = serde_json::from_str(&text)?;
    if character.id != canonical {
        return Err(StoreError::InvalidEvent);
    }
    // The flattened persisted alias proves equivalence. Match the historical
    // player's spelling for the pure rule without cloning a full fight, then
    // key the result by the verified canonical identity.
    character.id = local.player_id.clone();
    Ok(
        personal_best_candidate(fight, &character).map(|mut candidate| {
            candidate.character_id = canonical;
            candidate
        }),
    )
}

fn baseline(conn: &Connection, c: &PersonalBestCandidate) -> Result<Option<f64>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT dps FROM personal_bests WHERE character_id=?1 AND boss_type=?2",
            params![c.character_id, c.boss_type],
            |r| r.get(0),
        )
        .optional()?)
}
fn update_baseline(tx: &Transaction<'_>, c: &PersonalBestCandidate) -> Result<(), StoreError> {
    tx.execute("INSERT INTO personal_bests VALUES(?1,?2,?3,?4,?5) ON CONFLICT(character_id,boss_type) DO UPDATE SET fight_id=excluded.fight_id,dps=excluded.dps,at=excluded.at WHERE excluded.dps>personal_bests.dps", params![c.character_id,c.boss_type,c.fight_id,c.dps,micros(c.at)?])?;
    Ok(())
}

// Revisions of the owning fight may lower or invalidate a record. Restore the
// best other eligible stored fight before comparing the revised candidate.
fn restore_other_best(tx: &Transaction<'_>, fight_id: &str) -> Result<(), StoreError> {
    let owners = tx
        .prepare("SELECT character_id,boss_type FROM personal_bests WHERE fight_id=?1")?
        .query_map([fight_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u16>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (character, boss) in owners {
        tx.execute(
            "DELETE FROM personal_bests WHERE character_id=?1 AND boss_type=?2",
            params![character, boss],
        )?;
        let mut statement = tx.prepare("SELECT f.json FROM fights f
            WHERE f.boss_type=?2 AND f.ended IS NOT NULL AND f.id<>?3
              AND json_extract(f.json,'$.dps_basis')='player_active_window_v1'
              AND EXISTS(SELECT 1 FROM fight_players p WHERE p.fight_id=f.id
                AND p.player_id IN (SELECT ?1 UNION ALL SELECT provisional_id FROM character_aliases WHERE verified_id=?1))
            ORDER BY f.id")?;
        let mut rows = statement.query(params![character, boss, fight_id])?;
        while let Some(row) = rows.next()? {
            let text: String = row.get(0)?;
            if text.len() > crate::project::MAX_EVENT_BYTES {
                return Err(StoreError::Limit);
            }
            let fight: FightView = serde_json::from_str(&text)?;
            if let Some(c) = candidate(tx, &fight)?
                .filter(|c| c.character_id == character && c.boss_type == boss)
            {
                update_baseline(tx, &c)?;
            }
        }
    }
    Ok(())
}

/// Inspect before upsert for baseline repair and late identity handling.
pub(crate) fn was_finalized(tx: &Transaction<'_>, event: &EngineEvent) -> Result<bool, StoreError> {
    if let EngineEvent::FightEnded(fight) | EngineEvent::FightUpdated(fight) = event {
        if fight.ended.is_none() {
            return Ok(false);
        }
        return Ok(tx
            .query_row(
                "SELECT ended IS NOT NULL FROM fights WHERE id=?1",
                [&fight.id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false));
    }
    if let EngineEvent::CharacterUpdated(character) = event {
        let old: Option<String> = tx
            .query_row(
                "SELECT json FROM characters WHERE id=?1",
                [&character.id],
                |r| r.get(0),
            )
            .optional()?;
        return old
            .map(|text| {
                serde_json::from_str::<CharacterView>(&text)
                    .map(|c| c.identity_verified)
                    .map_err(StoreError::from)
            })
            .transpose()
            .map(|v| v.unwrap_or(false));
    }
    Ok(false)
}
fn record(
    tx: &Transaction<'_>,
    source: &str,
    event: &EventEnvelope,
    context: &AlertContext,
    mut view: AlertView,
    celebrate: bool,
) -> Result<(), StoreError> {
    view.id = occurrence_id(&view.kind, source)?;
    let loudness = context.preferences.loudness(&view.kind);
    view.presentation = Some(loudness);
    let text = alert_json(&view)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM alert_occurrences WHERE id=?1)",
        [&view.id],
        |r| r.get(0),
    )?;
    if exists {
        return Ok(());
    }
    let log = celebrate && loudness != AlertLoudness::Off;
    if log {
        tx.execute(
            "INSERT INTO alerts VALUES(?1,?2,?3,?4,?5)",
            params![
                view.id,
                view.run_id,
                micros(view.at)?,
                tag(&view.kind)?,
                text
            ],
        )?;
    }
    let pending = log && matches!(loudness, AlertLoudness::Toast | AlertLoudness::Banner);
    tx.execute(
        "INSERT INTO alert_occurrences VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            view.id,
            tag(&view.kind)?,
            source,
            event.stream_id,
            micros(event.sequence.0)?,
            micros(event.at)?,
            tag(&loudness)?,
            if pending { "pending" } else { "suppressed" },
            if log { Some(&view.id) } else { None }
        ],
    )?;
    Ok(())
}

pub(crate) fn project(
    tx: &Transaction<'_>,
    e: &EventEnvelope,
    context: &AlertContext,
    finalized: bool,
) -> Result<(), StoreError> {
    match &e.event {
        EngineEvent::RunEnded(run) | EngineEvent::RunUpdated(run)
            if run.area_kind == AreaKind::Dungeon
                && run.outcome == Some(Outcome::Cleared)
                && run.evidence.iter().any(|e| e.evidence == Evidence::Victory) =>
        {
            let Some(duration) = run
                .completed_at
                .and_then(|end| end.0.checked_sub(run.started.0))
                .filter(|&duration| duration > 0)
            else {
                return Ok(());
            };
            let occurrence = occurrence_id(&AlertKind::PersonalBest, &run.id)?;
            if tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM alert_occurrences WHERE id=?1)",
                [&occurrence],
                |r| r.get::<_, bool>(0),
            )? {
                return Ok(());
            }
            let owner: Option<String> = tx.query_row(
                &crate::account_sql::with_accounts("SELECT /*account:run:?1*/"),
                [&run.id],
                |r| r.get(0),
            )?;
            let Some(owner) = owner else {
                return Ok(());
            };
            // Match Home's account-scoped dungeon best, excluding this run even
            // when its completed RunUpdated projection is already durable.
            let previous: Option<i64> = tx.query_row(
                &crate::account_sql::with_accounts("SELECT min(r.completed_at-r.started) FROM runs r
                 WHERE r.id<>?1 AND r.dungeon=?2 AND r.outcome='cleared'
                   AND /*account:character:r.character_id*/=?3
                   AND r.completed_at>r.started
                   AND CASE WHEN json_valid(r.json) THEN json_extract(r.json,'$.area_kind') END='dungeon'
                   AND r.id IN (SELECT run_id FROM run_evidence WHERE kind IN ('victory','completion_dialogue'))"),
                params![run.id, run.dungeon, owner], |r| r.get(0),
            )?;
            let previous = previous.map(|micros| micros as u64);
            record(
                tx,
                &run.id,
                e,
                context,
                AlertView {
                    id: String::new(),
                    at: run.completed_at.expect("validated completion"),
                    kind: AlertKind::PersonalBest,
                    run_id: Some(run.id.clone()),
                    explanation: "Personal best observed dungeon clear time".into(),
                    presentation: None,
                    details: Some(AlertDetails::RunTimePersonalBest {
                        dungeon: run.dungeon.clone(),
                        duration_micros: duration,
                        previous_micros: previous.unwrap_or(duration),
                    }),
                },
                context.live && previous.is_some_and(|best| duration < best),
            )?;
        }
        EngineEvent::FightEnded(fight) | EngineEvent::FightUpdated(fight)
            if fight.ended.is_some() =>
        {
            if finalized {
                restore_other_best(tx, &fight.id)?;
            }
            if !context.live {
                seed_receipt(tx, AlertKind::PersonalBest, &fight.id)?;
            }
            if let Some(c) = candidate(tx, fight)? {
                let previous = baseline(tx, &c)?;
                let beats = beats_previous(&c, previous);
                #[cfg(test)]
                crate::project::fault::check(crate::project::fault::Point::AfterComparison)?;
                update_baseline(tx, &c)?;
                if context.live && (previous.is_none() || beats) {
                    record(
                        tx,
                        &fight.id,
                        e,
                        context,
                        AlertView {
                            id: String::new(),
                            at: c.at,
                            kind: AlertKind::PersonalBest,
                            run_id: fight.run_id.clone(),
                            explanation: "Personal best observed fight DPS".into(),
                            presentation: None,
                            details: Some(AlertDetails::PersonalBest {
                                fight_id: c.fight_id,
                                character_id: c.character_id,
                                boss_type: c.boss_type,
                                dps: c.dps,
                                previous_dps: previous.unwrap_or(c.dps),
                            }),
                        },
                        beats,
                    )?;
                }
            }
        }
        EngineEvent::BagDropped(drop) | EngineEvent::DropUpdated(drop) if is_white_bag(drop) => {
            let id = occurrence_id(&AlertKind::WhiteBag, &drop.id)?;
            let existing: Option<String> = tx.query_row("SELECT a.json FROM alert_occurrences o JOIN alerts a ON a.id=o.alert_id WHERE o.id=?1", [&id], |r| r.get(0)).optional()?;
            if let Some(text) = existing {
                let mut view: AlertView = serde_json::from_str(&text)?;
                if let Some(AlertDetails::WhiteBag {
                    source_enemy,
                    source_estimated,
                    contents_known,
                    ..
                }) = &mut view.details
                {
                    *source_enemy = drop.source_enemy;
                    *source_estimated = drop.source_estimated;
                    *contents_known = drop.contents_known;
                    tx.execute(
                        "UPDATE alerts SET json=?2 WHERE id=?1",
                        params![id, alert_json(&view)?],
                    )?;
                }
                return Ok(());
            }
            // Read the retained drop projection; its original time survives revisions.
            let at: i64 =
                tx.query_row("SELECT at FROM drops WHERE id=?1", [&drop.id], |r| r.get(0))?;
            let at = Timestamp(u64::try_from(at).map_err(|_| StoreError::InvalidEvent)?);
            let window = context
                .bag_window
                .clone()
                .filter(|w| w.start <= at && at < w.end);
            let ordinal = window.as_ref().map(|w| -> Result<u32, StoreError> {
                // drops.id is the primary key: one indexed row is one distinct
                // observed bag, regardless of rank, items, run or character.
                let count: i64 = tx.query_row("SELECT count(*) FROM drops WHERE bag_type=?1 AND at>=?2 AND at<?3 AND (at<?4 OR (at=?4 AND id<=?5))", params![drop.bag_type,micros(w.start)?,micros(w.end)?,micros(at)?,drop.id], |r| r.get(0))?;
                u32::try_from(count).map_err(|_| StoreError::Limit)
            }).transpose()?;
            record(
                tx,
                &drop.id,
                e,
                context,
                AlertView {
                    id,
                    at,
                    kind: AlertKind::WhiteBag,
                    run_id: drop.run_id.clone(),
                    explanation: "White bag observed".into(),
                    presentation: None,
                    details: Some(AlertDetails::WhiteBag {
                        drop_id: drop.id.clone(),
                        bag_type: drop.bag_type,
                        ordinal,
                        window,
                        source_enemy: drop.source_enemy,
                        source_estimated: drop.source_estimated,
                        contents_known: drop.contents_known,
                    }),
                },
                true,
            )?;
        }
        EngineEvent::InstanceEvent(view)
            if !view.provisional && matches!(view.kind, InstanceEventKind::PortalOpened { .. }) =>
        {
            if let Some(dungeon) = context
                .key_pop_dungeon
                .as_ref()
                .filter(|d| context.preferences.includes_key_pop_dungeon(d))
            {
                record(
                    tx,
                    &view.id,
                    e,
                    context,
                    AlertView {
                        id: String::new(),
                        at: view.at,
                        kind: AlertKind::KeyPopped,
                        run_id: view.run_id.clone(),
                        explanation: format!("{dungeon} key popped"),
                        details: Some(AlertDetails::KeyPopped {
                            dungeon: dungeon.clone(),
                        }),
                        presentation: None,
                    },
                    true,
                )?;
            }
        }
        EngineEvent::Chat(m) => {
            if let Some(kind) = chat_eligible(context, m) {
                let excerpt = chat_excerpt(&m.text);
                let prefix = match m.channel {
                    ChatChannel::Whisper => "Whisper from",
                    ChatChannel::Guild => "Guild message from",
                    _ => "Party message from",
                };
                record(
                    tx,
                    &m.id,
                    e,
                    context,
                    AlertView {
                        id: String::new(),
                        at: m.at,
                        kind,
                        run_id: m.run_id.clone(),
                        explanation: format!("{prefix} {}: {excerpt}", m.sender),
                        details: Some(AlertDetails::Chat {
                            channel: m.channel,
                            sender: m.sender.clone(),
                            excerpt,
                        }),
                        presentation: None,
                    },
                    true,
                )?;
            }
        }
        EngineEvent::CharacterMerged { verified_id, .. }
        | EngineEvent::CharacterUpdated(CharacterView {
            id: verified_id, ..
        }) => {
            // Identity arriving late may improve a baseline, never produce a celebration.
            if finalized
                || matches!(&e.event, EngineEvent::CharacterUpdated(c) if !c.identity_verified)
            {
                return Ok(());
            }
            let mut s = tx.prepare("SELECT DISTINCT f.id,f.json FROM fights f JOIN fight_players p ON p.fight_id=f.id WHERE (p.player_id=?1 OR p.player_id IN (SELECT provisional_id FROM character_aliases WHERE verified_id=?1)) AND f.ended IS NOT NULL AND f.boss_type IS NOT NULL")?;
            let mut rows = s.query([verified_id])?;
            while let Some(row) = rows.next()? {
                let f: FightView = serde_json::from_str(&row.get::<_, String>(1)?)?;
                if let Some(c) = candidate(tx, &f)? {
                    update_baseline(tx, &c)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn seed_receipt(tx: &Transaction<'_>, kind: AlertKind, source: &str) -> Result<(), StoreError> {
    tx.execute("INSERT OR IGNORE INTO alert_occurrences VALUES(?1,?2,?3,NULL,NULL,NULL,'off','suppressed',NULL)", params![occurrence_id(&kind,source)?,tag(&kind)?,source])?;
    Ok(())
}
pub(crate) fn seed(tx: &Transaction<'_>) -> Result<(), StoreError> {
    let mut after = String::new();
    loop {
        let mut statement = tx.prepare("SELECT id,length(CAST(json AS BLOB)),json FROM fights WHERE ended IS NOT NULL AND boss_type IS NOT NULL AND id>?1 ORDER BY id LIMIT 128")?;
        let mut rows = statement.query([&after])?;
        let mut count = 0;
        while let Some(row) = rows.next()? {
            after = row.get(0)?;
            count += 1;
            if row.get::<_, i64>(1)? > crate::project::MAX_EVENT_BYTES as i64 {
                return Err(StoreError::Limit);
            }
            let f: FightView = serde_json::from_str(&row.get::<_, String>(2)?)?;
            if let Some(c) = candidate(tx, &f)? {
                update_baseline(tx, &c)?;
            }
        }
        if count < 128 {
            break;
        }
    }
    let mut s =
        tx.prepare("SELECT fight_id FROM personal_bests ORDER BY character_id,boss_type")?;
    let mut rows = s.query([])?;
    while let Some(row) = rows.next()? {
        seed_receipt(tx, AlertKind::PersonalBest, &row.get::<_, String>(0)?)?;
    }
    let mut s = tx.prepare("SELECT id FROM drops WHERE bag_rank=255 ORDER BY id")?;
    let mut rows = s.query([])?;
    while let Some(row) = rows.next()? {
        seed_receipt(tx, AlertKind::WhiteBag, &row.get::<_, String>(0)?)?;
    }
    let mut s = tx.prepare("SELECT stream_id,sequence FROM deaths ORDER BY stream_id,sequence")?;
    let mut rows = s.query([])?;
    while let Some(row) = rows.next()? {
        seed_receipt(
            tx,
            AlertKind::Death,
            &identity(&[
                &row.get::<_, String>(0)?,
                &row.get::<_, i64>(1)?.to_string(),
            ]),
        )?;
    }
    Ok(())
}

pub(crate) fn claim(
    conn: &mut Connection,
    stream: &str,
    limit: u16,
) -> Result<Vec<CommittedAlert>, StoreError> {
    if !(1..=32).contains(&limit) {
        return Err(StoreError::Limit);
    }
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut result = Vec::new();
    let mut claimed_ids = Vec::new();
    {
        let mut s = tx.prepare("SELECT o.id,a.json,o.source_sequence,o.committed_at,o.loudness FROM alert_occurrences o JOIN alerts a ON a.id=o.alert_id WHERE o.state='pending' AND o.source_stream_id=?1 ORDER BY o.source_sequence,o.id LIMIT ?2")?;
        let mut rows = s.query(params![stream, limit])?;
        while let Some(row) = rows.next()? {
            let view: AlertView = serde_json::from_str(&row.get::<_, String>(1)?)?;
            alert_json(&view)?;
            result.push(CommittedAlert {
                view,
                source_stream_id: stream.into(),
                source_sequence: Sequence(Timestamp(
                    u64::try_from(row.get::<_, i64>(2)?).map_err(|_| StoreError::InvalidEvent)?,
                )),
                committed_at: Timestamp(
                    u64::try_from(row.get::<_, i64>(3)?).map_err(|_| StoreError::InvalidEvent)?,
                ),
                loudness: serde_json::from_value(serde_json::Value::String(row.get(4)?))?,
            });
            claimed_ids.push(row.get::<_, String>(0)?);
        }
    }
    for id in claimed_ids {
        tx.execute(
            "UPDATE alert_occurrences SET state='attempted' WHERE id=?1",
            [id],
        )?;
    }
    tx.commit()?;
    Ok(result)
}

impl StoreReader {
    pub fn alert_summary(&self, request: AlertSummaryRequest) -> Result<AlertSummary, StoreError> {
        let mut predicate = String::from("1=1");
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(range) = request.range {
            let (start, end) = crate::queries::range(&range)?;
            predicate.push_str(" AND a.at>=? AND a.at<?");
            values.extend([start.into(), end.into()]);
        }
        // Read one snapshot. Counts stay on the covering kind/time index; the
        // first alert and latest white use indexed LIMIT 1 lookups.
        let tx = self.conn.unchecked_transaction()?;
        let mut result = AlertSummary::default();
        let mut statement = tx.prepare(&format!(
            "SELECT kind,count(*) FROM alerts a INDEXED BY alerts_kind_at WHERE {predicate} GROUP BY kind LIMIT {}", AlertKind::ALL.len() + 1
        ))?;
        let mut rows = statement.query(rusqlite::params_from_iter(&values))?;
        while let Some(row) = rows.next()? {
            if result.by_kind.len() == AlertKind::ALL.len() {
                return Err(StoreError::Limit);
            }
            let kind: AlertKind = serde_json::from_value(serde_json::Value::String(row.get(0)?))?;
            let count: u32 = row.get(1)?;
            result.total = result.total.checked_add(count).ok_or(StoreError::Limit)?;
            result.by_kind.push(AlertKindCount { kind, count });
        }
        let first: Option<i64> = tx
            .query_row(
                &format!("SELECT a.at FROM alerts a WHERE {predicate} ORDER BY a.at,a.id LIMIT 1"),
                rusqlite::params_from_iter(&values),
                |row| row.get(0),
            )
            .optional()?;
        result.first_at = first
            .map(|at| {
                u64::try_from(at)
                    .map(Timestamp)
                    .map_err(|_| StoreError::InvalidEvent)
            })
            .transpose()?;
        let kinds = AlertKind::ALL;
        result
            .by_kind
            .sort_by_key(|entry| kinds.iter().position(|kind| *kind == entry.kind));
        let mut statement = tx.prepare(&format!(
            "SELECT a.at,a.run_id,r.dungeon,length(CAST(a.run_id AS BLOB)),length(CAST(r.dungeon AS BLOB))
             FROM (SELECT a.at,a.run_id FROM alerts a
                   WHERE a.kind='white_bag' AND {predicate} ORDER BY a.at DESC,a.id DESC LIMIT 1) a
             LEFT JOIN runs r ON r.id=a.run_id"
        ))?;
        let mut rows = statement.query(rusqlite::params_from_iter(&values))?;
        if let Some(row) = rows.next()? {
            let bytes = row
                .get::<_, Option<i64>>(3)?
                .unwrap_or(0)
                .checked_add(row.get::<_, Option<i64>>(4)?.unwrap_or(0))
                .ok_or(StoreError::Limit)?;
            if bytes > 2 * 1024 * 1024 {
                return Err(StoreError::Limit);
            }
            result.last_white = Some(AlertLastWhite {
                at: Timestamp(
                    u64::try_from(row.get::<_, i64>(0)?).map_err(|_| StoreError::InvalidEvent)?,
                ),
                run_id: row.get(1)?,
                dungeon: row.get(2)?,
            });
        }
        crate::queries::bounded(result, 2 * 1024 * 1024)
    }

    pub fn list_alerts(&self, filter: AlertFilter) -> Result<AlertPage, StoreError> {
        let query = filter.query.trim();
        for kinds in [&filter.kinds, &filter.exclude_kinds] {
            if kinds.len() > AlertKind::ALL.len()
                || kinds
                    .iter()
                    .enumerate()
                    .any(|(i, k)| kinds[..i].contains(k))
            {
                return Err(StoreError::Limit);
            }
        }
        if filter.kind.is_some() && !filter.kinds.is_empty() {
            return Err(StoreError::Limit);
        }
        if !(1..=100).contains(&filter.limit)
            || query.len() > 512
            || query.chars().count() > 128
            || query.contains('\0')
            || filter.cursor.as_ref().is_some_and(|c| !id_valid(&c.id))
        {
            return Err(StoreError::Limit);
        }
        let mut sql = String::from("SELECT length(CAST(json AS BLOB)),json FROM alerts WHERE 1=1");
        let mut values: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(kind) = filter.kind {
            sql.push_str(" AND kind=?");
            values.push(tag(&kind)?.into());
        }
        for (kinds, operator) in [(&filter.kinds, "IN"), (&filter.exclude_kinds, "NOT IN")] {
            if !kinds.is_empty() {
                sql.push_str(&format!(
                    " AND kind {operator} ({})",
                    vec!["?"; kinds.len()].join(",")
                ));
                for kind in kinds {
                    values.push(tag(kind)?.into());
                }
            }
        }
        if let Some(range) = filter.range {
            if range.start >= range.end {
                return Err(StoreError::InvalidEvent);
            }
            sql.push_str(" AND at>=? AND at<?");
            values.push(micros(range.start)?.into());
            values.push(micros(range.end)?.into());
        }
        if let Some(cursor) = filter.cursor {
            sql.push_str(" AND (at,id)<(?,?)");
            values.push(micros(cursor.at)?.into());
            values.push(cursor.id.into());
        }
        if !query.is_empty() {
            sql.push_str(" AND json_extract(json,'$.explanation') LIKE ? ESCAPE '\\'");
            let escaped = query
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            values.push(format!("%{escaped}%").into());
        }
        sql.push_str(" ORDER BY at DESC,id DESC LIMIT ?");
        values.push((i64::from(filter.limit) + 1).into());
        let tx = self.conn.unchecked_transaction()?;
        let mut s = tx.prepare(&sql)?;
        let mut rows = s.query(rusqlite::params_from_iter(values))?;
        let mut page = AlertPage {
            rows: vec![],
            next: None,
        };
        let mut bytes = 0usize;
        while let Some(row) = rows.next()? {
            let size = usize::try_from(row.get::<_, i64>(0)?).map_err(|_| StoreError::Limit)?;
            bytes = bytes.checked_add(size).ok_or(StoreError::Limit)?;
            if bytes > 2 * 1024 * 1024 {
                return Err(StoreError::Limit);
            }
            if page.rows.len() == usize::from(filter.limit) {
                page.next = page.rows.last().map(|v| AlertCursor {
                    at: v.at,
                    id: v.id.clone(),
                });
                break;
            }
            page.rows
                .push(serde_json::from_str(&row.get::<_, String>(1)?)?);
        }
        crate::queries::bounded(page, 2 * 1024 * 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{apply_contexts, fault};

    #[test]
    fn injected_alert_transaction_boundaries_roll_back_every_projection() {
        let snapshot = demo::demo_snapshot();
        let mut character = snapshot.character.unwrap();
        character.identity_verified = true;
        let mut fight = snapshot.fights[0].clone();
        fight.run_id = None;
        fight.complete_capture = true;
        fight.issues.clear();
        fight.boss_type = Some(42);
        fight.ended = Some(Timestamp(10_000_000));
        fight.end_reason = Some(FightEnd::MapChanged);
        fight.first_hit = Some(Timestamp(1_000_000));
        fight.last_hit = Some(Timestamp(2_000_000));
        fight.players.retain(|p| p.is_local);
        fight.players[0].player_id = character.id.clone();
        fight.players[0].dps = Some(100.);
        for source in &mut fight.players[0].sources {
            source.estimated = false;
        }
        let envelope = |n, event| EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "fault-alert".into(),
            sequence: Sequence(Timestamp(n)),
            at: Timestamp(8_000_000),
            event,
        };
        let context = AlertContext {
            chat: None,
            live: true,
            preferences: AlertPreferences::default(),
            key_pop_dungeon: None,
            bag_window: None,
        };
        for point in [
            fault::Point::BeforeProjection,
            fault::Point::AfterComparison,
            fault::Point::Commit,
        ] {
            let mut db = Connection::open_in_memory().unwrap();
            crate::migrate(&mut db).unwrap();
            let events = [
                envelope(
                    1,
                    EngineEvent::SessionUpdated(snapshot.session.clone().unwrap()),
                ),
                envelope(2, EngineEvent::CharacterUpdated(character.clone())),
                envelope(3, EngineEvent::FightEnded(fight.clone())),
            ];
            apply_contexts(&mut db, events.iter().map(|e| (e, &context))).unwrap();
            let mut winner = fight.clone();
            winner.id = "winner".into();
            winner.players[0].dps = Some(200.);
            let event = envelope(4, EngineEvent::FightEnded(winner));
            fault::arm(point, rusqlite::ffi::SQLITE_FULL);
            assert!(apply_contexts(&mut db, [(&event, &context)]).is_err());
            assert_eq!(
                db.query_row("SELECT count(*) FROM fights", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                db.query_row("SELECT dps FROM personal_bests", [], |r| r.get::<_, f64>(0))
                    .unwrap(),
                100.
            );
            assert_eq!(
                db.query_row("SELECT count(*) FROM alert_occurrences", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
            assert_eq!(
                db.query_row("SELECT count(*) FROM alerts", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert!(claim(&mut db, "fault-alert", 32).unwrap().is_empty());
            apply_contexts(&mut db, [(&event, &context)]).unwrap();
            let committed = claim(&mut db, "fault-alert", 32).unwrap();
            assert_eq!(committed.len(), 1);
            assert_eq!(committed[0].committed_at, event.at);
        }
    }
}

#[cfg(test)]
mod performance_tests {
    use crate::alerts_workload::{self as workload, cases, planned_sql, ORDINARY, STRESS};
    use std::cell::RefCell;

    #[test]
    fn alerts_pages_have_one_select_without_n_plus_one() {
        thread_local! { static SQL: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) }; }
        fn capture(event: rusqlite::trace::TraceEvent<'_>) {
            if let rusqlite::trace::TraceEvent::Stmt(s, _) = event {
                let sql = s.sql().trim_start().to_ascii_uppercase();
                // Count every non-transaction statement, including future CTEs
                // or pragmas, so a hydration query cannot evade the gate.
                if !["BEGIN", "COMMIT", "ROLLBACK"]
                    .iter()
                    .any(|p| sql.starts_with(p))
                {
                    SQL.with(|v| {
                        v.borrow_mut()
                            .push((s.sql().into_owned(), s.expanded_sql().unwrap()))
                    });
                }
            }
        }
        // Query tracing needs the retained log cardinality, not a second 4M-fight build.
        let mut fixture = workload::query_fixture(ORDINARY);
        for n in [ORDINARY, STRESS] {
            if n == STRESS {
                fixture.add_alerts(n);
            }
            let reader = fixture.validate_pages();
            reader.conn.trace_v2(
                rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
                Some(capture),
            );
            for (label, filter) in cases(n) {
                let expected = planned_sql(&filter);
                reader.list_alerts(filter).unwrap();
                let statements = SQL.with(|s| std::mem::take(&mut *s.borrow_mut()));
                assert_eq!(
                    statements
                        .iter()
                        .map(|(_, expanded)| expanded.clone())
                        .collect::<Vec<_>>(),
                    vec![expected.clone()],
                    "{label}: constant one SELECT; no hydration queries"
                );
                if label.ends_with("search") {
                    // This is the pre-multi-kind parameterized SQL, byte for byte.
                    assert_eq!(statements[0].0, "SELECT length(CAST(json AS BLOB)),json FROM alerts WHERE 1=1 AND json_extract(json,'$.explanation') LIKE ? ESCAPE '\\' ORDER BY at DESC,id DESC LIMIT ?");
                    eprintln!("{label}: emitted SQL: {}", statements[0].0);
                }
                fixture.assert_plan(label, &expected);
            }
        }
    }

    #[test]
    fn alert_retirement_uses_keyed_baseline_lookups() {
        use maxed_types::EngineEvent;
        thread_local! { static SQL: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }
        fn capture(event: rusqlite::trace::TraceEvent<'_>) {
            if let rusqlite::trace::TraceEvent::Stmt(s, _) = event {
                let sql = s.sql().trim_start().to_ascii_uppercase();
                if !["BEGIN", "COMMIT", "ROLLBACK"]
                    .iter()
                    .any(|p| sql.starts_with(p))
                {
                    SQL.with(|v| v.borrow_mut().push(s.expanded_sql().unwrap()));
                }
            }
        }
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::migrate(&mut db).unwrap();
        let mut seq = workload::burst_setup(&mut db);
        let context = crate::AlertContext {
            chat: None,
            live: false,
            preferences: Default::default(),
            key_pop_dungeon: None,
            bag_window: None,
        };
        let mut baseline_reads = 0;
        for i in 0..400 {
            seq += 1;
            let e = workload::envelope(
                seq,
                EngineEvent::FightEnded(workload::burst_fight(i, false)),
            );
            db.trace_v2(
                rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
                Some(capture),
            );
            crate::project::apply_contexts(&mut db, [(&e, &context)]).unwrap();
            // EXPLAIN is test instrumentation, not a production projection read.
            db.trace_v2(rusqlite::trace::TraceEventCodes::empty(), None);
            let statements = SQL.with(|s| std::mem::take(&mut *s.borrow_mut()));
            let reads: Vec<_> = statements
                .iter()
                .filter(|sql| db.prepare(sql).unwrap().readonly())
                .collect();
            let baseline_count = reads
                .iter()
                .filter(|sql| sql.to_ascii_uppercase().contains("FROM PERSONAL_BESTS"))
                .count();
            assert_eq!(baseline_count, usize::from(i < 20));
            baseline_reads += baseline_count;
            for sql in reads {
                let plan = db
                    .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                    .unwrap()
                    .query_map([], |r| r.get::<_, String>(3))
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap()
                    .join("; ");
                // EXISTS has a synthetic constant-row node; it does not scan
                // retained data. Every actual table/index read must avoid SCAN.
                assert!(
                    plan.split("; ")
                        .all(|step| { step == "SCAN CONSTANT ROW" || !step.contains("SCAN") }),
                    "{sql}: {plan}"
                );
                let normalized = sql.to_ascii_uppercase();
                if normalized.contains("FROM FIGHTS") || normalized.contains("FROM PERSONAL_BESTS")
                {
                    assert!(plan.contains("SEARCH"), "{sql}: {plan}");
                }
                if i == 0 {
                    eprintln!("retirement: {sql}\n{plan}");
                }
            }
        }
        assert_eq!(baseline_reads, 20);
    }
}

#[cfg(test)]
mod revision_query_tests {
    use super::*;
    use crate::alerts_workload as workload;
    use std::cell::RefCell;

    thread_local! { static SQL: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }
    fn capture(event: rusqlite::trace::TraceEvent<'_>) {
        if let rusqlite::trace::TraceEvent::Stmt(statement, _) = event {
            SQL.with(|sql| sql.borrow_mut().push(statement.expanded_sql().unwrap()));
        }
    }
    fn captured(db: &Connection) -> Vec<String> {
        db.trace_v2(rusqlite::trace::TraceEventCodes::empty(), None);
        SQL.with(|sql| std::mem::take(&mut *sql.borrow_mut()))
    }
    fn trace(db: &Connection) {
        SQL.with(|sql| sql.borrow_mut().clear());
        db.trace_v2(
            rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
            Some(capture),
        );
    }

    #[test]
    fn pb_revision_repairs_with_indexed_boss_and_identity_lookups() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::migrate(&mut db).unwrap();
        let mut seq = workload::burst_setup(&mut db);
        let mut fight = workload::eligible_fight();
        fight.id = "revision-record".into();
        fight.players[0].dps = Some(200.);
        seq += 1;
        crate::apply_batch(
            &mut db,
            &[workload::envelope(
                seq,
                EngineEvent::FightEnded(fight.clone()),
            )],
        )
        .unwrap();
        fight.players[0].dps = Some(50.);
        seq += 1;
        trace(&db);
        crate::apply_batch(
            &mut db,
            &[workload::envelope(seq, EngineEvent::FightUpdated(fight))],
        )
        .unwrap();
        let statements = captured(&db);
        assert_eq!(
            db.query_row(
                "SELECT dps FROM personal_bests WHERE character_id='c000' AND boss_type=0",
                [],
                |r| r.get::<_, f64>(0)
            )
            .unwrap(),
            100.
        );
        let mut repaired = false;
        for sql in statements
            .iter()
            .filter(|s| s.trim_start().starts_with("SELECT"))
        {
            let plan = db
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map([], |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(
                plan.iter()
                    .all(|step| step == "SCAN CONSTANT ROW" || !step.contains("SCAN")),
                "{sql}: {plan:?}"
            );
            if sql.contains("f.json") {
                repaired = true;
                assert!(
                    sql.contains("json_extract(f.json,'$.dps_basis')='player_active_window_v1'")
                );
                assert!(
                    plan.iter()
                        .any(|step| step.contains("fights_final_boss_key (boss_type=?)")),
                    "{plan:?}"
                );
                assert!(
                    plan.iter().any(|step| step.contains("fight_players")
                        && step.contains("fight_id=? AND player_id=?")),
                    "{plan:?}"
                );
            }
        }
        assert!(repaired);
    }

    #[test]
    fn run_pb_republish_skips_the_history_minimum_query() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::migrate(&mut db).unwrap();
        let mut seq = workload::burst_setup(&mut db);
        let mut run = demo::demo_snapshot().run.unwrap();
        let account = format!("account-{}", "a".repeat(64));
        db.execute("INSERT OR IGNORE INTO accounts VALUES(?1)", [&account])
            .unwrap();
        db.execute(
            "UPDATE characters SET account_id=?1 WHERE id='c000'",
            [&account],
        )
        .unwrap();
        run.id = "victory-query".into();
        run.character_id = Some("c000".into());
        run.ended = None;
        run.completed_at = Some(Timestamp(run.started.0 + 3_000_000));
        run.outcome = Some(Outcome::Cleared);
        run.evidence = vec![EvidenceAt {
            at: run.completed_at.unwrap(),
            evidence: Evidence::Victory,
        }];
        let context = AlertContext {
            live: true,
            ..Default::default()
        };
        for repeats in [false, true] {
            seq += 1;
            let event = workload::envelope(seq, EngineEvent::RunUpdated(run.clone()));
            trace(&db);
            crate::project::apply_contexts(&mut db, [(&event, &context)]).unwrap();
            let statements = captured(&db);
            let comparisons = statements
                .iter()
                .filter(|sql| sql.contains("SELECT min(r.completed_at-r.started)"))
                .count();
            assert_eq!(comparisons, usize::from(!repeats));
        }
    }
}
