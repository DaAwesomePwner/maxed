//! Map visits: time per map, projected from `MapEntered` inside the writer transaction.
use crate::{
    project::{json, micros, tag},
    StoreError,
};
use maxed_types::*;
use rusqlite::{params, Transaction};

/// `ended` never precedes `started`; the JSON copy stays in step with the column.
const CLOSE: &str = "UPDATE map_visits SET ended=max(started,?2),
    json=json_set(json,'$.ended',CAST(max(started,?2) AS TEXT))";

pub(crate) fn enter(
    tx: &Transaction<'_>,
    envelope: &EventEnvelope,
    session: Option<String>,
    realm_instance_key: Option<&String>,
    at: Timestamp,
    name: &str,
    kind: &AreaKind,
) -> Result<(), StoreError> {
    let at_micros = micros(at)?;
    // At most one open visit per stream (unique index map_visits_open).
    let previous: Option<(Option<String>, String)> = {
        let mut statement = tx.prepare(&format!(
            "{CLOSE} WHERE stream_id=?1 AND ended IS NULL RETURNING character_id,session_id"
        ))?;
        let mut rows = statement.query(params![envelope.stream_id, at_micros])?;
        match rows.next()? {
            Some(row) => Some((row.get(0)?, row.get(1)?)),
            None => None,
        }
    };
    let Some(session_id) = session else {
        return Ok(());
    };
    // The engine reports the character only when its view changes, so a map change on
    // the same connection keeps the previous visit's character.
    let character_id = previous
        .filter(|(_, previous_session)| *previous_session == session_id)
        .and_then(|(character, _)| character);
    let view = MapVisitView {
        id: format!("{}:visit:{}", envelope.stream_id, envelope.sequence.0 .0),
        stream_id: envelope.stream_id.clone(),
        session_id,
        character_id,
        run_id: None,
        map: name.to_owned(),
        kind: kind.clone(),
        realm_instance_key: realm_instance_key.cloned(),
        started: at,
        ended: None,
        source: VisitSource::Live,
    };
    tx.execute(
        "INSERT INTO map_visits VALUES(?1,?2,?3,?4,NULL,?5,?6,?7,?8,NULL,'live',?9)",
        params![
            view.id,
            view.stream_id,
            view.session_id,
            view.character_id,
            view.map,
            tag(&view.kind)?,
            view.realm_instance_key,
            at_micros,
            json(&view)?
        ],
    )?;
    Ok(())
}

/// `character` is already resolved through `character_aliases`.
pub(crate) fn set_character(
    tx: &Transaction<'_>,
    stream: &str,
    character: &str,
) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE map_visits SET character_id=?2,json=json_set(json,'$.character_id',?2)
         WHERE stream_id=?1 AND ended IS NULL AND character_id IS NOT ?2",
        params![stream, character],
    )?;
    Ok(())
}

pub(crate) fn link_run(
    tx: &Transaction<'_>,
    stream: &str,
    run: &RunView,
) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE map_visits SET run_id=?2,map=?3,json=json_set(json,'$.run_id',?2,'$.map',?3)
         WHERE stream_id=?1 AND ended IS NULL AND kind='dungeon' AND run_id IS NULL",
        params![stream, run.id, run.dungeon],
    )?;
    Ok(())
}

/// Identity loss ends continuity without erasing the old visit's ownership.
pub(crate) fn close_stream(tx: &Transaction<'_>, stream: &str, at: i64) -> Result<(), StoreError> {
    tx.execute(
        &format!("{CLOSE} WHERE stream_id=?1 AND ended IS NULL"),
        params![stream, at],
    )?;
    Ok(())
}

pub(crate) fn close_session(
    tx: &Transaction<'_>,
    session: &str,
    ended: Timestamp,
) -> Result<(), StoreError> {
    tx.execute(
        &format!("{CLOSE} WHERE session_id=?1 AND ended IS NULL"),
        params![session, micros(ended)?],
    )?;
    Ok(())
}

pub(crate) fn merge(tx: &Transaction<'_>, from: &str, to: &str) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE map_visits SET character_id=?2,json=json_set(json,'$.character_id',?2) WHERE character_id=?1",
        params![from, to],
    )?;
    Ok(())
}
