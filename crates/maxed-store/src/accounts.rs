//! Account pseudonyms only; identity keys never enter storage. Mutating APIs must
//! run on the single writer's connection (or while that writer is stopped).
use crate::{project, StoreError, StoreReader};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

/// Counts unique, exact candidate IDs; duplicates do not inflate counts.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AttachCounts {
    pub attached: usize,
    pub already_attached: usize,
    pub conflicting: usize,
    pub unmatched: usize,
}

pub fn current_account(conn: &Connection) -> Result<Option<String>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT account_id FROM current_account WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?)
}

impl StoreReader {
    pub fn current_account(&self) -> Result<Option<String>, StoreError> {
        current_account(&self.conn)
    }
    /// Inclusive persisted sweep prefix; None means no completed batch.
    pub fn recovery_progress(&self, account: &str) -> Result<Option<u16>, StoreError> {
        recovery_progress(&self.conn, account)
    }

    /// Resolve the pointer and perform a scoped read in one SQLite snapshot.
    /// `verified` overrides the pointer only for a verified native identity.
    ///
    /// The callback MUST NOT start another transaction (including calling this
    /// wrapper recursively). SQLite rejects nested BEGINs; an error rolls back
    /// this snapshot. Use single-statement readers or readers that explicitly
    /// reuse an existing transaction, such as `dungeon_profile`. In particular,
    /// `get_run`, `list_runs` and recap readers cannot be called here. A SAVEPOINT
    /// outside those readers would not make their nested BEGIN valid either.
    /// The account_scope_with_account_transaction_constraint test guards this contract.
    pub fn with_account<T>(
        &self,
        verified: Option<&str>,
        read: impl FnOnce(&Self, Option<&str>) -> Result<T, StoreError>,
    ) -> Result<T, StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        let account = match verified {
            Some(account) => Some(account.to_owned()),
            None => self.current_account()?,
        };
        let result = read(self, account.as_deref())?;
        tx.commit()?;
        Ok(result)
    }
}

/// Attach exact persisted IDs only, atomically. Never resolve candidates through
/// aliases or infer membership from session or character number.
pub fn attach_legacy_characters(
    conn: &mut Connection,
    account_id: &str,
    candidate_ids: &[String],
) -> Result<AttachCounts, StoreError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    register(&tx, account_id)?;
    let mut counts = AttachCounts::default();
    {
        let mut get = tx.prepare("SELECT account_id FROM characters WHERE id=?1")?;
        let mut set = tx.prepare(
            "UPDATE characters SET account_id=?2,
                json=CASE WHEN json_valid(json) THEN json_set(json,'$.account_id',?2) ELSE json END
                WHERE id=?1 AND account_id IS NULL",
        )?;
        for id in candidate_ids
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
        {
            match get
                .query_row([id], |r| r.get::<_, Option<String>>(0))
                .optional()?
            {
                None => counts.unmatched += 1,
                Some(None) => {
                    counts.attached += set.execute(params![id, account_id])?;
                }
                Some(Some(old)) if old == account_id => counts.already_attached += 1,
                Some(Some(_)) => counts.conflicting += 1,
            }
        }
    }
    tx.commit()?;
    Ok(counts)
}

/// Inclusive completed character-number prefix; None means no sweep progress.
pub fn recovery_progress(conn: &Connection, account_id: &str) -> Result<Option<u16>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT completed_through FROM account_recovery WHERE account_id=?1",
            [account_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// Advance only after the corresponding attach batch commits. A crash between
/// attach and progress merely repeats the idempotent batch. 65535 means complete.
pub fn record_recovery_progress(
    conn: &mut Connection,
    account_id: &str,
    completed_through: u16,
) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    register(&tx, account_id)?;
    tx.execute("INSERT INTO account_recovery VALUES(?1,?2) ON CONFLICT(account_id) DO UPDATE SET completed_through=max(completed_through,excluded.completed_through)", params![account_id,completed_through])?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn register(tx: &Transaction<'_>, account: &str) -> Result<(), StoreError> {
    if !account
        .strip_prefix("account-")
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(StoreError::InvalidEvent);
    }
    tx.execute("INSERT OR IGNORE INTO accounts VALUES(?1)", [account])?;
    Ok(())
}

pub(crate) fn owner(
    tx: &Transaction<'_>,
    kind: &str,
    id: &str,
    character: Option<&str>,
) -> Result<(), StoreError> {
    // Rows predating 017 have no first-event ownership evidence. A revision
    // cannot retroactively claim them for the currently observed character.
    let table = if kind == "fight" { "fights" } else { "drops" };
    let existing: bool = tx.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
        [id],
        |r| r.get(0),
    )?;
    if existing {
        // Preserve absent pre-017 ownership for legacy visit fallback. Modern
        // first-event ownership (including NULL) is already stored and immutable.
        return Ok(());
    }
    let character = character.map(|id| project::resolve(tx, id)).transpose()?;
    tx.execute(
        "INSERT OR IGNORE INTO observation_characters VALUES(?1,?2,?3)",
        params![kind, id, character],
    )?;
    Ok(())
}

pub(crate) fn identity(
    tx: &Transaction<'_>,
    stream: &str,
    sequence: i64,
    session: Option<&str>,
    at: i64,
    character: Option<&str>,
    account: Option<&str>,
) -> Result<(), StoreError> {
    let mut character = character.map(|id| project::resolve(tx, id)).transpose()?;
    let mut account = account;
    if let (Some(character_id), Some(account_id)) = (&character, account) {
        let old: Option<String> = tx
            .query_row(
                "SELECT account_id FROM characters WHERE id=?1",
                [character_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if old.as_deref().is_some_and(|old| old != account_id) {
            // This period is contradictory evidence. Clear both fields so a
            // later alias merge cannot accidentally attribute the unknown span.
            character = None;
            account = None;
        } else {
            register(tx, account_id)?;
            tx.execute("INSERT INTO current_account VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET account_id=excluded.account_id", [account_id])?;
        }
    }
    if character.is_none() {
        crate::visits::close_stream(tx, stream, at)?;
    }
    tx.execute(
        "UPDATE identity_spans SET ended=max(started,?2) WHERE stream_id=?1 AND ended IS NULL",
        params![stream, at],
    )?;
    tx.execute(
        "INSERT INTO identity_spans VALUES(?1,?2,?3,?4,NULL,?5,?6)",
        params![
            stream,
            sequence,
            session,
            at,
            character,
            account.filter(|_| character.is_some())
        ],
    )?;
    Ok(())
}
