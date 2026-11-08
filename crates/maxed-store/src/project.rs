use crate::{migrate::digest, receipts, receipts::ReceiptDigest, StoreError};
use maxed_types::*;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;
use std::{collections::BTreeSet, io::Write};

pub(crate) const MAX_EVENT_BYTES: usize = 8 * 1024 * 1024;

pub(crate) fn micros(at: Timestamp) -> Result<i64, StoreError> {
    i64::try_from(at.0).map_err(|_| StoreError::InvalidEvent)
}

fn optional(at: Option<Timestamp>) -> Result<Option<i64>, StoreError> {
    at.map(micros).transpose()
}

pub(crate) fn json<T: Serialize>(value: &T) -> Result<String, StoreError> {
    Ok(serde_json::to_string(value)?)
}

pub(crate) fn tag<T: Serialize>(value: &T) -> Result<String, StoreError> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Object(map) if map.len() == 1 => {
            Ok(map.keys().next().cloned().ok_or(StoreError::InvalidEvent)?)
        }
        _ => Err(StoreError::InvalidEvent),
    }
}

struct Limited(Vec<u8>);
impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_EVENT_BYTES {
            return Err(std::io::Error::other("event limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Canonical sorted-key JSON, matching the CLI's SHA-256 value hashing.
pub(crate) fn validate_event(event: &EventEnvelope) -> Result<(usize, ReceiptDigest), StoreError> {
    if event.stream_id.is_empty() || event.sequence.0 .0 == 0 {
        return Err(StoreError::InvalidEvent);
    }
    let mut bytes = Limited(Vec::new());
    serde_json::to_writer(&mut bytes, event)?;
    let size = bytes.0.len();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes.0)?;
    fn visit(value: &serde_json::Value) -> Result<(), StoreError> {
        match value {
            serde_json::Value::Object(map) => {
                for (key, value) in map {
                    if matches!(
                        key.as_str(),
                        "at" | "sequence"
                            | "started"
                            | "ended"
                            | "last_traffic"
                            | "observed_at"
                            | "taken_observed_at"
                            | "completed_at"
                            | "died_at"
                            | "first_hit"
                            | "last_hit"
                            | "from"
                            | "until"
                    ) && !value.is_null()
                    {
                        let n = value
                            .as_str()
                            .and_then(|s| s.parse::<u64>().ok())
                            .ok_or(StoreError::InvalidEvent)?;
                        micros(Timestamp(n))?;
                    }
                    visit(value)?;
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    visit(value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    visit(&value)?;
    value.sort_all_objects();
    Ok((size, receipts::digest(&serde_json::to_vec(&value)?)))
}

pub fn apply_batch(conn: &mut Connection, events: &[EventEnvelope]) -> Result<(), StoreError> {
    apply_refs(conn, events.iter())
}

/// Integration-test seam: apply events with explicit alert/chat contexts.
#[doc(hidden)]
pub fn apply_contexts_for_tests(
    conn: &mut Connection,
    events: &[(EventEnvelope, crate::AlertContext)],
) -> Result<(), StoreError> {
    apply_contexts(conn, events.iter().map(|(e, c)| (e, c)))
}

pub(crate) fn apply_refs<'a>(
    conn: &mut Connection,
    events: impl IntoIterator<Item = &'a EventEnvelope>,
) -> Result<(), StoreError> {
    let context = crate::AlertContext::default();
    apply_contexts(conn, events.into_iter().map(|e| (e, &context)))
}

pub(crate) fn apply_contexts<'a>(
    conn: &mut Connection,
    events: impl IntoIterator<Item = (&'a EventEnvelope, &'a crate::AlertContext)>,
) -> Result<(), StoreError> {
    apply_prepared_contexts(conn, events.into_iter().map(|(e, c)| (e, c, None)))
}

/// The writer supplies a digest only for an immutable envelope already validated
/// at submission. Direct batch callers still validate inside the transaction.
pub(crate) fn apply_prepared_contexts<'a>(
    conn: &mut Connection,
    events: impl IntoIterator<
        Item = (
            &'a EventEnvelope,
            &'a crate::AlertContext,
            Option<&'a ReceiptDigest>,
        ),
    >,
) -> Result<(), StoreError> {
    #[cfg(test)]
    fault::check(fault::Point::Begin)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    if conn.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))? != 1 {
        return Err(StoreError::Io);
    }
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let tables = receipts::Tables::read(&tx)?;
    let mut party_runs = BTreeSet::new();
    for (event, context, prepared_hash) in events {
        context.validate()?;
        let hash = match prepared_hash {
            Some(hash) => *hash,
            None => validate_event(event)?.1,
        };
        let sequence = micros(event.sequence.0)?;
        if let Some(old) = tables.lookup(&tx, &event.stream_id, sequence)? {
            if old != hash {
                return Err(StoreError::InvalidEvent);
            }
            continue;
        }
        let (stream_key, head) = tables.intern_stream(&tx, &event.stream_id)?;
        let head = head.unwrap_or(0);
        if head.checked_add(1) != Some(sequence) {
            return Err(StoreError::SequenceGap);
        }
        if head == 0
            && !matches!(
                event.event,
                EngineEvent::SessionUpdated(_) | EngineEvent::CaptureStateChanged(_)
            )
        {
            return Err(StoreError::InvalidEvent);
        }
        let finalized = crate::alerts::was_finalized(&tx, &event.event)?;
        #[cfg(test)]
        fault::check(fault::Point::BeforeProjection)?;
        project(&tx, event, context, &mut party_runs)?;
        crate::alerts::project(&tx, event, context, finalized)?;
        tx.prepare_cached(
            "INSERT INTO event_receipts_v2(stream_key,sequence,digest) VALUES(?1,?2,?3)",
        )?
        .execute(params![stream_key, sequence, hash.as_slice()])?;
        receipts::advance_head(&tx, stream_key, sequence)?;
    }
    // Map close can publish RunEnded before final fights and profiles. Project
    // all of them before rebuilding each affected ended run once.
    for run_id in party_runs {
        let ended: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE id=?1 AND ended IS NOT NULL)",
            [&run_id],
            |r| r.get(0),
        )?;
        if ended {
            crate::party::rebuild_run_members(&tx, &run_id)?;
        }
    }
    #[cfg(test)]
    fault::check(fault::Point::Commit)?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn resolve(conn: &Connection, id: &str) -> Result<String, StoreError> {
    // Merges flatten existing aliases; this is a single lookup, never an unbounded chain.
    Ok(conn
        .query_row(
            "SELECT verified_id FROM character_aliases WHERE provisional_id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_else(|| id.to_owned()))
}

fn stream_session(tx: &Transaction<'_>, stream: &str) -> Result<Option<String>, StoreError> {
    Ok(tx
        .query_row(
            "SELECT session_id FROM stream_sessions WHERE stream_id=?1",
            [stream],
            |r| r.get(0),
        )
        .optional()?)
}

fn project(
    tx: &Transaction<'_>,
    envelope: &EventEnvelope,
    context: &crate::AlertContext,
    party_runs: &mut BTreeSet<String>,
) -> Result<(), StoreError> {
    match &envelope.event {
        EngineEvent::InstanceEvent(event) => crate::instance::project(tx, event)?,
        EngineEvent::SessionUpdated(session) => {
            let mut session = session.clone();
            tx.execute("UPDATE identity_spans SET ended=max(started,?3) WHERE stream_id=?1 AND session_id IS NOT ?2 AND ended IS NULL", params![envelope.stream_id,session.id,micros(session.started)?])?;
            let previous: Option<String> = tx
                .query_row(
                    "SELECT json FROM play_sessions WHERE id=?1",
                    [&session.id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(previous) = previous {
                let previous: PlaySessionView = serde_json::from_str(&previous)?;
                for issue in previous.issues {
                    if !session.issues.contains(&issue) {
                        session.issues.push(issue);
                    }
                }
            }
            tx.execute("INSERT INTO play_sessions VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET started=excluded.started,last_traffic=excluded.last_traffic,ended=excluded.ended,json=excluded.json", params![session.id,micros(session.started)?,micros(session.last_traffic)?,optional(session.ended)?,json(&session)?])?;
            tx.execute("INSERT INTO stream_sessions VALUES(?1,?2) ON CONFLICT(stream_id) DO UPDATE SET session_id=excluded.session_id", params![envelope.stream_id,session.id])?;
            if let Some(ended) = session.ended {
                tx.execute("UPDATE identity_spans SET ended=max(started,?2) WHERE session_id=?1 AND ended IS NULL", params![session.id,micros(ended)?])?;
                crate::visits::close_session(tx, &session.id, ended)?;
            }
        }
        EngineEvent::CharacterUpdated(character) => {
            let mut character = character.clone();
            character.id = resolve(tx, &character.id)?;
            let old_account: Option<String> = tx
                .query_row(
                    "SELECT account_id FROM characters WHERE id=?1",
                    [&character.id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            if let Some(old) = old_account {
                character.account_id = Some(old);
            }
            if let Some(account) = &character.account_id {
                crate::accounts::register(tx, account)?;
            }
            let learned_identity: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM characters WHERE id=?1 AND
                    ((json_extract(json,'$.name') IS NULL AND ?2 IS NOT NULL)
                    OR (json_extract(json,'$.class_id') IS NULL AND ?3 IS NOT NULL)))",
                params![character.id, character.name, character.class_id],
                |row| row.get(0),
            )?;
            tx.execute("INSERT INTO characters(id,observed_at,json,account_id) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET observed_at=excluded.observed_at,json=excluded.json,account_id=coalesce(characters.account_id,excluded.account_id) WHERE excluded.observed_at>=characters.observed_at", params![character.id,micros(character.observed_at)?,json(&character)?,character.account_id])?;
            // Membership is monotonic evidence independent of snapshot freshness.
            tx.execute("UPDATE characters SET account_id=?2,json=json_set(json,'$.account_id',?2) WHERE id=?1 AND account_id IS NULL AND ?2 IS NOT NULL", params![character.id,character.account_id])?;
            if learned_identity {
                backfill_run_identity(tx, &character.id)?;
            }
            crate::visits::set_character(tx, &envelope.stream_id, &character.id)?;
        }
        EngineEvent::RunStarted(run) => {
            write_run(tx, run)?;
            crate::visits::link_run(tx, &envelope.stream_id, run)?;
            run_character(tx, &envelope.stream_id, run)?;
        }
        EngineEvent::RunUpdated(run) | EngineEvent::RunEnded(run) => {
            write_run(tx, run)?;
            if matches!(envelope.event, EngineEvent::RunEnded(_)) {
                party_runs.insert(run.id.clone());
            }
            run_character(tx, &envelope.stream_id, run)?;
        }
        EngineEvent::FightStarted(fight)
        | EngineEvent::FightUpdated(fight)
        | EngineEvent::FightEnded(fight) => {
            let previous_run: Option<String> = tx
                .query_row("SELECT run_id FROM fights WHERE id=?1", [&fight.id], |r| {
                    r.get(0)
                })
                .optional()?
                .flatten();
            write_fight(tx, fight)?;
            if let Some(run_id) = previous_run {
                party_runs.insert(run_id);
            }
            if let Some(run_id) = &fight.run_id {
                party_runs.insert(run_id.clone());
            }
        }
        EngineEvent::BagDropped(drop) | EngineEvent::DropUpdated(drop) => write_drop(tx, drop)?,
        EngineEvent::CharacterMerged {
            provisional_id,
            verified_id,
        } => merge(tx, provisional_id, verified_id)?,
        EngineEvent::FameChanged {
            at,
            character_id,
            fame,
        } => {
            let character = resolve(tx, character_id)?;
            tx.execute(
                "INSERT INTO fame_samples VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    envelope.stream_id,
                    micros(envelope.sequence.0)?,
                    character,
                    micros(*at)?,
                    fame,
                    stream_session(tx, &envelope.stream_id)?
                ],
            )?;
            crate::visits::set_character(tx, &envelope.stream_id, &character)?;
        }
        EngineEvent::PlayerDied {
            at,
            character_id,
            run_id,
        } => {
            let character = character_id
                .as_deref()
                .map(|id| resolve(tx, id))
                .transpose()?;
            tx.execute(
                "INSERT INTO deaths VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    envelope.stream_id,
                    micros(envelope.sequence.0)?,
                    character,
                    run_id,
                    micros(*at)?,
                    match run_id {
                        Some(run) => Some(
                            tx.query_row("SELECT session_id FROM runs WHERE id=?1", [run], |r| {
                                r.get::<_, String>(0)
                            })
                            .optional()?
                            .ok_or(StoreError::InvalidEvent)?
                        ),
                        None => stream_session(tx, &envelope.stream_id)?,
                    }
                ],
            )?;
            if let Some(run) = run_id {
                timeline(
                    tx,
                    run,
                    "death",
                    *at,
                    TimelineKind::Death,
                    Some(run),
                    "Observed death packet",
                )?;
            }
        }
        EngineEvent::AlertFired(alert) => {
            if alert.details.is_some() {
                crate::alerts::alert_json(alert)?;
            }
            tx.execute("INSERT INTO alerts VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET run_id=excluded.run_id,at=excluded.at,kind=excluded.kind,json=excluded.json WHERE NOT EXISTS(SELECT 1 FROM alert_occurrences WHERE alert_id=excluded.id)", params![alert.id,alert.run_id,micros(alert.at)?,tag(&alert.kind)?,json(alert)?])?;
        }
        EngineEvent::Chat(message) => {
            crate::chat::project(tx, message, context.chat.is_none_or(|c| c.store))?
        }
        EngineEvent::MapEntered {
            realm_instance_key,
            at,
            name,
            kind,
        } => crate::visits::enter(
            tx,
            envelope,
            stream_session(tx, &envelope.stream_id)?,
            realm_instance_key.as_ref(),
            *at,
            name,
            kind,
        )?,
        EngineEvent::PlayerObserved(profile) => {
            crate::players::write_profile(tx, profile)?;
            let map: Option<String> = if let Some(run_id) = &profile.run_id {
                tx.query_row("SELECT dungeon FROM runs WHERE id=?1", [run_id], |r| {
                    r.get(0)
                })
                .optional()?
            } else {
                // Scopes have no visit foreign key. Use the indexed session and
                // observation interval: buffered players can precede MapEntered,
                // and a final profile can arrive after its map has closed.
                // started<=last_seen includes buffered players seen at visit start.
                // For old-map profiles this is safe only because the engine's
                // close_map calls end_scope before MapEntered: the final
                // PlayerObserved is projected before the next visit exists.
                tx.query_row(
                    "SELECT v.map FROM map_visits v JOIN stream_sessions s ON s.session_id=v.session_id
                     WHERE s.stream_id=?1 AND v.stream_id=?1 AND v.started<=?2
                         AND (v.ended IS NULL OR v.ended>?3)
                     ORDER BY v.started DESC,v.id DESC LIMIT 1",
                    params![
                        envelope.stream_id,
                        micros(profile.last_seen)?,
                        micros(profile.first_seen)?
                    ],
                    |r| r.get(0),
                )
                .optional()?
            };
            crate::party::write_party_player(tx, profile, map.as_deref())?;
            if let Some(run_id) = &profile.run_id {
                party_runs.insert(run_id.clone());
            }
        }
        EngineEvent::ScopeTimelineUpdated(update) => crate::players::write_timeline(tx, update)?,
        EngineEvent::LocalIdentityChanged {
            at,
            character_id,
            account_id,
        } => {
            crate::accounts::identity(
                tx,
                &envelope.stream_id,
                micros(envelope.sequence.0)?,
                stream_session(tx, &envelope.stream_id)?.as_deref(),
                micros(*at)?,
                character_id.as_deref(),
                account_id.as_deref(),
            )?;
        }
        EngineEvent::CaptureStateChanged(_) => {}
    }
    Ok(())
}

// Entry gear/stats and already-known identity remain historical. Late identity
// observations only fill holes, using the accepted (possibly newer) character row.
fn backfill_run_identity(tx: &Transaction<'_>, id: &str) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE runs SET entry_character_json=json_set(entry_character_json,
            '$.name',coalesce(json_extract(entry_character_json,'$.name'),
                (SELECT json_extract(json,'$.name') FROM characters WHERE id=?1)),
            '$.class_id',coalesce(json_extract(entry_character_json,'$.class_id'),
                (SELECT json_extract(json,'$.class_id') FROM characters WHERE id=?1)))
        WHERE character_id=?1 AND entry_character_json IS NOT NULL
            AND ((json_extract(entry_character_json,'$.name') IS NULL
                AND (SELECT json_extract(json,'$.name') FROM characters WHERE id=?1) IS NOT NULL)
            OR (json_extract(entry_character_json,'$.class_id') IS NULL
                AND (SELECT json_extract(json,'$.class_id') FROM characters WHERE id=?1) IS NOT NULL))",
        [id],
    )?;
    Ok(())
}

fn run_character(tx: &Transaction<'_>, stream: &str, run: &RunView) -> Result<(), StoreError> {
    if let Some(id) = &run.character_id {
        crate::visits::set_character(tx, stream, &resolve(tx, id)?)?;
    }
    Ok(())
}

fn write_run(tx: &Transaction<'_>, input: &RunView) -> Result<(), StoreError> {
    let mut run = input.clone();
    run.character_id = run
        .character_id
        .as_deref()
        .map(|id| resolve(tx, id))
        .transpose()?;
    let entry: Option<String> = match &run.character_id {
        Some(id) => tx
            .query_row("SELECT json FROM characters WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?,
        None => None,
    };
    tx.execute("INSERT INTO runs VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12) ON CONFLICT(id) DO UPDATE SET session_id=excluded.session_id,character_id=excluded.character_id,dungeon=excluded.dungeon,started=excluded.started,ended=excluded.ended,completed_at=excluded.completed_at,died_at=excluded.died_at,outcome=excluded.outcome,complete_capture=excluded.complete_capture,entry_character_json=CASE WHEN runs.character_id IS excluded.character_id THEN runs.entry_character_json ELSE excluded.entry_character_json END,json=excluded.json", params![run.id,run.session_id,run.character_id,run.dungeon,micros(run.started)?,optional(run.ended)?,optional(run.completed_at)?,optional(run.died_at)?,run.outcome.as_ref().map(tag).transpose()?,run.complete_capture,entry,json(&run)?])?;
    tx.execute("DELETE FROM run_evidence WHERE run_id=?1", [&run.id])?;
    for (i, evidence) in run.evidence.iter().enumerate() {
        tx.execute(
            "INSERT INTO run_evidence VALUES(?1,?2,?3,?4,?5)",
            params![
                run.id,
                i as i64,
                micros(evidence.at)?,
                tag(&evidence.evidence)?,
                json(evidence)?
            ],
        )?;
    }
    timeline(
        tx,
        &run.id,
        "entry",
        run.started,
        TimelineKind::Entered,
        Some(&run.id),
        "Entered observed area",
    )?;
    if let Some(at) = run.completed_at {
        let explanation = if run
            .evidence
            .iter()
            .any(|e| matches!(e.evidence, Evidence::Victory))
        {
            "Observed Victory"
        } else if run
            .evidence
            .iter()
            .any(|e| matches!(e.evidence, Evidence::CompletionDialogue { .. }))
        {
            "Observed completion dialogue rule"
        } else if run
            .evidence
            .iter()
            .any(|e| matches!(e.evidence, Evidence::CompletionCounter { .. }))
        {
            "Observed completion counter revision"
        } else {
            "Completion timestamp recorded without supporting evidence"
        };
        timeline(
            tx,
            &run.id,
            "completion",
            at,
            TimelineKind::Completion,
            Some(&run.id),
            explanation,
        )?;
    } else {
        remove_timeline(tx, &run.id, "completion")?;
    }
    if let Some(at) = run.died_at {
        timeline(
            tx,
            &run.id,
            "death",
            at,
            TimelineKind::Death,
            Some(&run.id),
            "Observed death packet",
        )?;
    }
    // AppClosed is a capture issue, not an observed exit.
    if let Some(e) = run
        .evidence
        .iter()
        .find(|e| matches!(e.evidence, Evidence::ObservedExit))
    {
        timeline(
            tx,
            &run.id,
            "exit",
            e.at,
            TimelineKind::Exit,
            Some(&run.id),
            "Observed area exit",
        )?;
    } else {
        remove_timeline(tx, &run.id, "exit")?;
    }
    if !run.complete_capture
        || !run.issues.is_empty()
        || run
            .evidence
            .iter()
            .any(|e| matches!(e.evidence, Evidence::AppClosed | Evidence::CaptureGap))
    {
        timeline(
            tx,
            &run.id,
            "coverage",
            run.ended.unwrap_or(run.started),
            TimelineKind::CaptureIssue,
            Some(&run.id),
            "Capture is partial; inspect recorded evidence and coverage issues",
        )?;
    } else {
        remove_timeline(tx, &run.id, "coverage")?;
    }
    Ok(())
}

pub(crate) fn matching_run(
    tx: &Transaction<'_>,
    run_id: Option<&str>,
    session: &str,
) -> Result<(), StoreError> {
    if let Some(id) = run_id {
        let actual: String = tx
            .query_row("SELECT session_id FROM runs WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or(StoreError::InvalidEvent)?;
        if actual != session {
            return Err(StoreError::InvalidEvent);
        }
    }
    Ok(())
}

pub(crate) fn fight_summary(fight: &FightView) -> Result<RunFightSummary, StoreError> {
    let mut locals = fight.players.iter().filter(|p| p.is_local);
    let local = locals
        .next()
        .filter(|_| locals.next().is_none())
        .filter(|_| !fight.issues.contains(&CoverageIssue::MissingIdentity));
    Ok(RunFightSummary {
        id: fight.id.clone(),
        target_type: fight.target_type,
        target_name: None,
        target_kind: fight.target_kind.clone(),
        boss_type: fight.boss_type,
        ordinal: 0,
        started: fight.started,
        ended: fight.ended,
        first_hit: fight.first_hit,
        last_hit: fight.last_hit,
        total_damage: fight.total_damage,
        local_damage: local.map(|p| p.damage),
        local_dps: local.and_then(|p| p.dps),
        local_rank: local.and_then(|p| p.rank),
        party: None,
        player_count: u32::try_from(fight.players.len()).map_err(|_| StoreError::InvalidEvent)?,
        partial: !fight.complete_capture || !fight.issues.is_empty(),
        estimated: fight
            .players
            .iter()
            .any(|p| p.sources.iter().any(|s| s.damage_uncertain())),
    })
}

pub(crate) fn write_fight_summary(
    tx: &Transaction<'_>,
    fight: &FightView,
) -> Result<(), StoreError> {
    tx.execute("INSERT INTO fight_summaries VALUES(?1,?2,?3,?4) ON CONFLICT(fight_id) DO UPDATE SET run_id=excluded.run_id,started=excluded.started,json=excluded.json",
        params![fight.id, fight.run_id, micros(fight.started)?, crate::instance::summary_json(fight)?])?;
    Ok(())
}

fn write_fight(tx: &Transaction<'_>, fight: &FightView) -> Result<(), StoreError> {
    crate::accounts::owner(tx, "fight", &fight.id, fight.owner_character_id.as_deref())?;
    let player_ids: Vec<_> = fight.players.iter().map(|p| p.player_id.as_str()).collect();
    if player_ids
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>()
        .len()
        != player_ids.len()
    {
        return Err(StoreError::InvalidEvent);
    }
    matching_run(tx, fight.run_id.as_deref(), &fight.session_id)?;
    let mut locals = fight.players.iter().filter(|p| p.is_local);
    let local_dps = locals
        .next()
        .and_then(|p| p.dps)
        .filter(|_| locals.next().is_none());
    tx.execute("INSERT INTO fights VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET run_id=excluded.run_id,session_id=excluded.session_id,started=excluded.started,ended=excluded.ended,boss_type=excluded.boss_type,local_dps=excluded.local_dps,complete_capture=excluded.complete_capture,json=excluded.json", params![fight.id,fight.run_id,fight.session_id,micros(fight.started)?,optional(fight.ended)?,fight.boss_type,local_dps,fight.complete_capture,json(fight)?])?;
    write_fight_summary(tx, fight)?;
    crate::instance::kill_timeline(tx, fight)?;
    // Remove children before their players; retained source indices are upserted below.
    tx.prepare_cached("DELETE FROM damage_sources WHERE fight_id=?1 AND player_id NOT IN (SELECT value FROM json_each(?2))")?
        .execute(params![fight.id, json(&player_ids)?])?;
    // Retain unchanged observations: rebuilding every player's contribution on
    // a local-only update needlessly churns the run aggregates and their indexes.
    tx.execute("DELETE FROM fight_players WHERE fight_id=?1 AND player_id NOT IN (SELECT value FROM json_each(?2))",
        params![fight.id, json(&player_ids)?])?;
    // Share depends on the fight total, not this player. Ignore legacy stored shares
    // in comparisons too, so the first revision does not churn unchanged rows.
    for player in &fight.players {
        tx.execute(
            "INSERT INTO fight_players VALUES(?1,?2,?3,?4,?5,?6,json_remove(?7,'$.share'))
            ON CONFLICT(fight_id,player_id) DO UPDATE SET object_id=excluded.object_id,
                damage=excluded.damage,dps=excluded.dps,rank=excluded.rank,json=excluded.json
            WHERE json_remove(fight_players.json,'$.share')<>excluded.json OR fight_players.object_id IS NOT excluded.object_id
                OR fight_players.damage IS NOT excluded.damage OR fight_players.dps IS NOT excluded.dps
                OR fight_players.rank IS NOT excluded.rank",
            params![
                fight.id,
                player.player_id,
                player.object_id,
                player.damage,
                player.dps,
                player.rank,
                json(player)?
            ],
        )?;
        tx.prepare_cached(
            "DELETE FROM damage_sources WHERE fight_id=?1 AND player_id=?2 AND source_index>=?3",
        )?
        .execute(params![
            fight.id,
            player.player_id,
            player.sources.len() as i64
        ])?;
        for (i, source) in player.sources.iter().enumerate() {
            tx.prepare_cached(
                "INSERT INTO damage_sources VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
                 ON CONFLICT(fight_id,player_id,source_index) DO UPDATE SET
                    item_type=excluded.item_type,kind=excluded.kind,damage=excluded.damage,
                    hits=excluded.hits,json=excluded.json
                 WHERE damage_sources.json IS NOT excluded.json
                    OR damage_sources.item_type IS NOT excluded.item_type
                    OR damage_sources.kind IS NOT excluded.kind
                    OR damage_sources.damage IS NOT excluded.damage
                    OR damage_sources.hits IS NOT excluded.hits",
            )?
            .execute(params![
                fight.id,
                player.player_id,
                i as i64,
                source.item_type,
                tag(&source.kind)?,
                source.damage,
                source.hits,
                json(source)?
            ])?;
        }
    }
    // Remove a stale timeline link if a full projection changes association.
    tx.execute(
        "DELETE FROM timeline WHERE kind='fight' AND related_id=?1",
        [&fight.id],
    )?;
    if let Some(run) = &fight.run_id {
        timeline(
            tx,
            run,
            &format!("fight:{}", fight.id),
            fight.started,
            TimelineKind::Fight,
            Some(&fight.id),
            "Observed target damage; defeat is not inferred",
        )?;
    }
    Ok(())
}

fn write_drop(tx: &Transaction<'_>, input: &DropView) -> Result<(), StoreError> {
    crate::accounts::owner(tx, "drop", &input.id, input.owner_character_id.as_deref())?;
    let mut drop = input.clone();
    let mut keys = std::collections::BTreeSet::new();
    if drop
        .contents
        .iter()
        .any(|item| item.slot >= 8 || !keys.insert((item.slot, item.item_type)))
    {
        return Err(StoreError::InvalidEvent);
    }
    let previous: Option<String> = tx
        .query_row("SELECT json FROM drops WHERE id=?1", [&drop.id], |r| {
            r.get(0)
        })
        .optional()?;
    if let Some(previous) = previous {
        let previous: DropView = serde_json::from_str(&previous)?;
        drop.at = previous.at;
        let incoming = std::mem::take(&mut drop.contents);
        drop.contents = previous.contents;
        for mut item in incoming {
            if let Some(old) = drop
                .contents
                .iter_mut()
                .find(|old| old.slot == item.slot && old.item_type == item.item_type)
            {
                item.taken_observed_at = old.taken_observed_at.or(item.taken_observed_at);
                *old = item;
            } else {
                drop.contents.push(item);
            }
        }
    }
    drop.items = drop.contents.iter().map(|item| item.item_type).collect();
    matching_run(tx, drop.run_id.as_deref(), &drop.session_id)?;
    if drop.contents.iter().any(|item| item.quantity == 0) {
        return Err(StoreError::InvalidEvent);
    }
    tx.execute("INSERT INTO drops(id,session_id,run_id,at,bag_type,bag_rank,json,source_type) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET session_id=excluded.session_id,run_id=excluded.run_id,at=excluded.at,bag_type=excluded.bag_type,bag_rank=excluded.bag_rank,json=excluded.json,source_type=excluded.source_type", params![drop.id,drop.session_id,drop.run_id,micros(drop.at)?,drop.bag_type,drop.bag_rank,json(&drop)?,drop.source_enemy])?;
    for item in &drop.contents {
        tx.execute(
            "INSERT INTO drop_items VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(drop_id,slot,item_type) DO UPDATE SET quantity=excluded.quantity,json=excluded.json,taken_observed_at=excluded.taken_observed_at",
            params![
                drop.id,
                item.slot,
                item.item_type,
                item.quantity,
                json(item)?,
                optional(item.taken_observed_at)?
            ],
        )?;
    }
    tx.execute(
        "DELETE FROM timeline WHERE kind='drop' AND related_id=?1",
        [&drop.id],
    )?;
    if let Some(run) = &drop.run_id {
        timeline(
            tx,
            run,
            &format!("drop:{}", drop.id),
            drop.at,
            TimelineKind::Drop,
            Some(&drop.id),
            if drop.contents_known {
                "Observed bag contents; pickup is unknown"
            } else {
                "Observed bag; contents and pickup are unknown"
            },
        )?;
    }
    Ok(())
}

fn fact_id(run: &str, fact: &str) -> Result<String, StoreError> {
    Ok(digest(json(&(run, fact))?.as_bytes()))
}

fn remove_timeline(tx: &Transaction<'_>, run: &str, fact: &str) -> Result<(), StoreError> {
    tx.execute("DELETE FROM timeline WHERE id=?1", [fact_id(run, fact)?])?;
    Ok(())
}

pub(crate) fn timeline(
    tx: &Transaction<'_>,
    run: &str,
    fact: &str,
    at: Timestamp,
    kind: TimelineKind,
    related: Option<&str>,
    explanation: &str,
) -> Result<(), StoreError> {
    tx.execute("INSERT INTO timeline VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET at=excluded.at,kind=excluded.kind,related_id=excluded.related_id,explanation=excluded.explanation", params![fact_id(run,fact)?,run,micros(at)?,tag(&kind)?,related,explanation])?;
    Ok(())
}

fn merge(tx: &Transaction<'_>, provisional: &str, verified: &str) -> Result<(), StoreError> {
    let from = resolve(tx, provisional)?;
    let to = resolve(tx, verified)?;
    if from == to {
        return Ok(());
    }
    let present: i64 = tx.query_row(
        "SELECT count(*) FROM characters WHERE id IN (?1,?2)",
        params![from, to],
        |r| r.get(0),
    )?;
    if present != 2 {
        return Err(StoreError::InvalidEvent);
    }
    let accounts: Vec<Option<String>> = [from.as_str(), to.as_str()]
        .iter()
        .map(|id| {
            tx.query_row("SELECT account_id FROM characters WHERE id=?1", [id], |r| {
                r.get(0)
            })
        })
        .collect::<Result<_, _>>()?;
    if accounts[0].is_some() && accounts[1].is_some() && accounts[0] != accounts[1] {
        return Err(StoreError::InvalidEvent);
    }
    tx.execute(
        "UPDATE characters SET account_id=coalesce(account_id,?2) WHERE id=?1",
        params![to, accounts[0]],
    )?;
    tx.execute(
        "UPDATE observation_characters SET character_id=?2 WHERE character_id=?1",
        params![from, to],
    )?;
    tx.execute("UPDATE identity_spans SET character_id=?2,account_id=coalesce(account_id,(SELECT account_id FROM characters WHERE id=?2)) WHERE character_id=?1", params![from,to])?;
    // Preserve known historical names, gear, and stats; fill only missing identity.
    tx.execute("UPDATE runs SET character_id=?2,json=json_set(json,'$.character_id',?2),entry_character_json=CASE WHEN entry_character_json IS NULL THEN NULL ELSE json_set(entry_character_json,'$.id',?2) END WHERE character_id=?1", params![from,to])?;
    backfill_run_identity(tx, &to)?;
    tx.execute(
        "UPDATE fame_samples SET character_id=?2 WHERE character_id=?1",
        params![from, to],
    )?;
    tx.execute(
        "UPDATE deaths SET character_id=?2 WHERE character_id=?1",
        params![from, to],
    )?;
    crate::visits::merge(tx, &from, &to)?;
    tx.execute(
        "UPDATE OR REPLACE player_profiles SET player_id=?2,json=json_set(json,'$.player_id',?2) WHERE player_id=?1",
        params![from, to],
    )?;
    for table in ["player_gear_log", "player_timelines"] {
        tx.execute(
            &format!("UPDATE OR REPLACE {table} SET player_id=?2 WHERE player_id=?1"),
            params![from, to],
        )?;
    }
    tx.execute(
        "UPDATE character_aliases SET verified_id=?2 WHERE verified_id=?1",
        params![from, to],
    )?;
    tx.execute("INSERT INTO character_aliases VALUES(?1,?2) ON CONFLICT(provisional_id) DO UPDATE SET verified_id=excluded.verified_id", params![from,to])?;
    Ok(())
}

/// Test-only errors at the transaction boundaries. Returning before commit
/// exercises SQLite rollback and the writer's actual retained-batch path.
#[cfg(test)]
pub(crate) mod fault {
    use crate::StoreError;
    use std::cell::Cell;
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum Point {
        Begin,
        BeforeProjection,
        AfterComparison,
        Commit,
    }
    thread_local! { static NEXT: Cell<Option<(Point, i32)>> = const { Cell::new(None) }; }
    pub fn arm(point: Point, code: i32) {
        NEXT.set(Some((point, code)));
    }
    pub fn check(point: Point) -> Result<(), StoreError> {
        if let Some((expected, code)) = NEXT.get() {
            if expected == point {
                NEXT.set(None);
                return Err(
                    rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None).into(),
                );
            }
        }
        Ok(())
    }
}
