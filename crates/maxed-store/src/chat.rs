//! Chat projection, Logs readers and retention. Errors never carry message text.
use crate::{
    project::{json, matching_run, micros},
    queries::{bounded, identifier, range, row_json},
    StoreError, StoreReader,
};
use maxed_types::*;
use rusqlite::{
    params, params_from_iter, types::Value, Connection, OptionalExtension, Transaction,
};
use std::path::Path;

fn validate(m: &ChatMessageView) -> Result<(), StoreError> {
    let id_ok = |id: &str| !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control);
    if !id_ok(&m.id)
        || m.text.len() > MAX_CHAT_TEXT_BYTES
        || m.sender.len() > 256
        || m.counterpart.as_ref().is_some_and(|c| c.len() > 256)
        || m.map.as_ref().is_some_and(|s| s.len() > 256)
    {
        return Err(StoreError::Limit);
    }
    Ok(())
}

pub(crate) fn project(
    tx: &Transaction<'_>,
    m: &ChatMessageView,
    store: bool,
) -> Result<(), StoreError> {
    validate(m)?;
    if !store {
        return Ok(());
    }
    matching_run(tx, m.run_id.as_deref(), &m.session_id)?;
    tx.execute(
        "INSERT INTO chat_messages VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
         ON CONFLICT(id) DO UPDATE SET session_id=excluded.session_id,run_id=excluded.run_id,at=excluded.at,
           channel=excluded.channel,direction=excluded.direction,sender_kind=excluded.sender_kind,
           sender=excluded.sender,counterpart=excluded.counterpart,is_local=excluded.is_local,
           text=excluded.text,json=excluded.json
         WHERE chat_messages.json<>excluded.json",
        params![
            m.id,
            m.session_id,
            m.run_id,
            micros(m.at)?,
            m.channel.tag(),
            m.direction.map(WhisperDirection::tag),
            m.sender_kind.tag(),
            m.sender,
            m.counterpart,
            m.is_local,
            m.text,
            json(m)?
        ],
    )?;
    Ok(())
}

const MAX_LOGS_BYTES: usize = 1024 * 1024;

/// Hide lists copied from settings by the runtime; validated as ChatSettings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HiddenLists {
    pub senders: Vec<String>,
    pub keywords: Vec<String>,
}

/// Plain quoted prefix terms. Only alphanumeric runs survive, so no FTS5
/// operator or quote typed by the user reaches MATCH.
pub(crate) fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .take(16)
        .map(|t| format!("\"{t}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

fn page_bounds(limit: u16, cursor: Option<(&Timestamp, &str)>) -> Result<(), StoreError> {
    if !(1..=MAX_LOGS_PAGE).contains(&limit) {
        return Err(StoreError::InvalidEvent);
    }
    if let Some((at, id)) = cursor {
        identifier(id)?;
        micros(*at)?;
    }
    Ok(())
}

fn count_u32(count: i64) -> Result<u32, StoreError> {
    u32::try_from(count).map_err(|_| StoreError::Limit)
}

/// Scope shared by event pages and summaries. Presentation filters are separate.
fn event_scope(
    filter: &EventFilter,
    args: &mut Vec<Value>,
    include_query: bool,
) -> Result<Vec<String>, StoreError> {
    if filter.query.len() > 512
        || filter.query.chars().count() > MAX_CHAT_QUERY_CHARS
        || filter.query.contains('\0')
    {
        return Err(StoreError::InvalidEvent);
    }
    let mut predicates = Vec::new();
    if let Some(run) = &filter.run_id {
        identifier(run)?;
        args.push(run.clone().into());
        predicates.push(format!("e.run_id=?{}", args.len()));
    }
    if let Some(value) = &filter.range {
        let (start, end) = range(value)?;
        args.push(start.into());
        predicates.push(format!("e.at>=?{}", args.len()));
        args.push(end.into());
        predicates.push(format!("e.at<?{}", args.len()));
    }
    if include_query && !filter.query.is_empty() {
        if let Some(query) = fts_query(&filter.query) {
            args.push(query.into());
            predicates.push(format!(
                "e.rowid IN (SELECT rowid FROM event_fts WHERE event_fts MATCH ?{})",
                args.len()
            ));
        }
    }
    Ok(predicates)
}

const EVENT_FTS_PROBE: &str = "SELECT EXISTS(SELECT 1 FROM event_fts WHERE event_fts MATCH ?1)";

/// Integration-test seam: production probe SQL and its sanitized binding.
#[doc(hidden)]
pub fn event_fts_probe_for_tests(query: &str) -> Option<(&'static str, String)> {
    fts_query(query).map(|binding| (EVENT_FTS_PROBE, binding))
}

/// Integration-test seam: the exact SQL and bindings used by event pages.
#[doc(hidden)]
pub fn event_page_sql_for_tests(filter: &EventFilter) -> Result<(String, Vec<Value>), StoreError> {
    event_page_sql(filter)
}

pub(crate) fn event_page_sql(filter: &EventFilter) -> Result<(String, Vec<Value>), StoreError> {
    page_bounds(
        filter.limit,
        filter.cursor.as_ref().map(|c| (&c.at, c.id.as_str())),
    )?;
    if filter.kinds.len() > 4
        || filter
            .dungeon
            .as_ref()
            .is_some_and(|d| d.is_empty() || d.len() > 256)
    {
        return Err(StoreError::InvalidEvent);
    }
    let kinds = if filter.kinds.is_empty() {
        InstanceEventTag::ALL.to_vec()
    } else {
        filter.kinds.clone()
    };
    let mut args: Vec<Value> = kinds.iter().map(|k| k.tag().to_owned().into()).collect();
    let marks = (1..=args.len())
        .map(|n| format!("?{n}"))
        .collect::<Vec<_>>()
        .join(",");
    let mut predicates = vec![format!("e.kind IN ({marks})")];
    predicates.extend(event_scope(filter, &mut args, true)?);
    if let Some(local) = filter.opener_local {
        predicates.push("e.kind='portal_opened'".into());
        args.push(i64::from(local).into());
        predicates.push(format!(
            "json_extract(e.json,'$.kind.opener_is_local')=?{}",
            args.len()
        ));
    }
    if let Some(dungeon) = &filter.dungeon {
        args.push(dungeon.clone().into());
        predicates.push(format!(
            "json_extract(e.json,'$.kind.dungeon')=?{}",
            args.len()
        ));
    }
    if let Some(cursor) = &filter.cursor {
        args.push(micros(cursor.at)?.into());
        let at = args.len();
        args.push(cursor.id.clone().into());
        predicates.push(format!("(e.at,e.id)<(?{at},?{})", args.len()));
    }
    args.push((i64::from(filter.limit) + 1).into());
    let sql = format!(
        "SELECT length(CAST(e.json AS BLOB)),e.json FROM instance_events e WHERE {} ORDER BY e.at DESC,e.id DESC LIMIT ?{}",
        predicates.join(" AND "),
        args.len()
    );
    Ok((sql, args))
}

impl StoreReader {
    pub fn chat_overview(&self, request: ChatOverviewRequest) -> Result<ChatOverview, StoreError> {
        if let Some(run) = &request.run_id {
            identifier(run)?;
        }
        let (source, predicate) = if request.run_id.is_some() {
            ("chat_messages INDEXED BY chat_run", "run_id=?1")
        } else {
            ("chat_messages INDEXED BY chat_channel", "1=1")
        };
        let sql = format!("SELECT channel,count(*),min(at),max(at) FROM {source} WHERE {predicate} GROUP BY channel");
        let mut statement = self.conn.prepare(&sql)?;
        let mut result = statement.query(params_from_iter(request.run_id.iter()))?;
        let mut overview = ChatOverview::default();
        while let Some(row) = result.next()? {
            let count = count_u32(row.get(1)?)?;
            let slot = match row.get::<_, String>(0)?.as_str() {
                "public" => &mut overview.by_channel.public,
                "guild" => &mut overview.by_channel.guild,
                "party" => &mut overview.by_channel.party,
                "whisper" => &mut overview.by_channel.whisper,
                _ => &mut overview.by_channel.unknown,
            };
            *slot = count;
            overview.lines = overview.lines.checked_add(count).ok_or(StoreError::Limit)?;
            let first = Timestamp(row.get::<_, i64>(2)? as u64);
            let last = Timestamp(row.get::<_, i64>(3)? as u64);
            overview.first_at = Some(overview.first_at.map_or(first, |old| old.min(first)));
            overview.latest_at = Some(overview.latest_at.map_or(last, |old| old.max(last)));
        }
        bounded(overview, MAX_LOGS_BYTES)
    }

    pub fn chat_sender(
        &self,
        request: ChatSenderRequest,
    ) -> Result<Option<ChatSenderSummary>, StoreError> {
        if request.name.is_empty()
            || request.name.len() > MAX_HIDDEN_SENDER_BYTES
            || request.name.chars().any(char::is_control)
        {
            return Err(StoreError::Limit);
        }
        // Read one snapshot so a line arriving mid-read can't split the
        // latest message from the counts.
        let tx = self.conn.unchecked_transaction()?;
        let mut statement = tx.prepare("SELECT length(CAST(json AS BLOB)),json FROM chat_messages WHERE sender=?1 ORDER BY at DESC,id DESC LIMIT 1")?;
        let mut latest = statement.query([&request.name])?;
        let Some(row) = latest.next()? else {
            return Ok(None);
        };
        let message: ChatMessageView = row_json(row, 0, &mut 0, MAX_LOGS_BYTES)?;
        let (lines, first): (i64, i64) = tx.query_row(
            "SELECT count(*),min(at) FROM chat_messages WHERE sender=?1",
            [&request.name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut statement =
            tx.prepare("SELECT DISTINCT channel FROM chat_messages WHERE sender=?1")?;
        let channels = statement
            .query_map([&request.name], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let channels = [
            ChatChannel::Public,
            ChatChannel::Guild,
            ChatChannel::Party,
            ChatChannel::Whisper,
            ChatChannel::Unknown,
        ]
        .into_iter()
        .filter(|channel| channels.iter().any(|tag| tag == channel.tag()))
        .collect();
        let whispers: i64 = tx.query_row(
            "SELECT count(*) FROM chat_messages WHERE rowid IN (
                SELECT rowid FROM chat_messages INDEXED BY chat_sender WHERE sender=?1 AND channel='whisper'
                UNION SELECT rowid FROM chat_messages INDEXED BY chat_counterpart WHERE counterpart=?1 AND channel='whisper')",
            [&request.name], |row| row.get(0))?;
        bounded(
            Some(ChatSenderSummary {
                name: message.sender,
                lines: count_u32(lines)?,
                first_at: Timestamp(first as u64),
                last_at: message.at,
                channels,
                stars: message.stars,
                supporter: message.supporter,
                whispers: count_u32(whispers)?,
            }),
            MAX_LOGS_BYTES,
        )
    }

    pub fn whisper_partners(
        &self,
        filter: WhisperPartnerFilter,
        hidden: &HiddenLists,
    ) -> Result<WhisperPartners, StoreError> {
        if !(1..=MAX_WHISPER_PARTNERS).contains(&filter.limit)
            || filter.query.len() > 512
            || filter.query.chars().count() > MAX_CHAT_QUERY_CHARS
            || filter.query.contains('\0')
        {
            return Err(StoreError::InvalidEvent);
        }
        let settings = ChatSettings {
            hidden_senders: hidden.senders.clone(),
            hidden_keywords: hidden.keywords.clone(),
            ..ChatSettings::default()
        };
        if !settings.valid() {
            return Err(StoreError::Limit);
        }
        let mut args: Vec<Value> = vec![];
        let mut message_predicates = vec!["1=1".to_owned()];
        let mut predicates = vec!["partner IS NOT NULL AND length(partner)>0".to_owned()];
        if !filter.show_hidden {
            // Filter message text before window counts and newest-line selection.
            // This uses the same ASCII-folded substring rule as list_chat.
            for keyword in &hidden.keywords {
                args.push(keyword.to_ascii_lowercase().into());
                message_predicates.push(format!("instr(lower(text),?{})=0", args.len()));
            }
            for sender in &hidden.senders {
                args.push(sender.clone().into());
                predicates.push(format!("partner<>?{} COLLATE NOCASE", args.len()));
            }
        }
        if let Some(query) = fts_query(&filter.query) {
            // FTS indexes sender/text, not the outgoing counterpart. Match the
            // partner separately and restrict FTS to text so the local sender
            // cannot make every outgoing conversation match.
            args.push(format!("text : ({query})").into());
            let fts = args.len();
            let escaped = filter
                .query
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            args.push(format!("%{escaped}%").into());
            predicates.push(format!("(partner LIKE ?{} ESCAPE '\\' OR message_rowid IN (SELECT rowid FROM chat_fts WHERE chat_fts MATCH ?{fts}))", args.len()));
        }
        args.push(i64::from(filter.limit).into());
        let sql = include_str!("whisper_partners.sql")
            .replace(
                "/* message predicates */",
                &message_predicates.join(" AND "),
            )
            .replace("/* partner predicates */", &predicates.join(" AND "))
            .replace(":limit", &format!("?{}", args.len()));
        let mut statement = self.conn.prepare(&sql)?;
        let mut result = statement.query(params_from_iter(args))?;
        let mut partners = WhisperPartners::default();
        let mut bytes = 0;
        while let Some(row) = result.next()? {
            partners.people = count_u32(row.get(0)?)?;
            partners.lines = count_u32(row.get(1)?)?;
            let Some(name) = row.get::<_, Option<String>>(2)? else {
                continue;
            };
            let message: ChatMessageView = row_json(row, 4, &mut bytes, MAX_LOGS_BYTES)?;
            partners.rows.push(WhisperPartner {
                name,
                lines: count_u32(row.get(3)?)?,
                last_at: message.at,
                last_text: message.text.chars().take(120).collect(),
                last_direction: message.direction.unwrap_or(WhisperDirection::Unknown),
                stars: message.stars,
            });
        }
        bounded(partners, MAX_LOGS_BYTES)
    }

    pub fn list_chat(
        &self,
        filter: ChatFilter,
        hidden: &HiddenLists,
    ) -> Result<ChatPage, StoreError> {
        page_bounds(
            filter.limit,
            filter.cursor.as_ref().map(|c| (&c.at, c.id.as_str())),
        )?;
        if filter.query.len() > 512
            || filter.query.chars().count() > MAX_CHAT_QUERY_CHARS
            || filter.query.contains('\0')
            || filter
                .sender
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 256)
        {
            return Err(StoreError::InvalidEvent);
        }
        if let Some(run) = &filter.run_id {
            identifier(run)?;
        }
        let mut args = Vec::<Value>::new();
        let mut predicates = vec!["1=1".to_owned()];
        let arg = |args: &mut Vec<Value>, v: Value| {
            args.push(v);
            args.len()
        };
        if let Some(channel) = filter.channel {
            let n = arg(&mut args, channel.tag().to_owned().into());
            predicates.push(format!("c.channel=?{n}"));
        }
        if !filter.show_npc {
            predicates.push("c.sender_kind<>'npc'".into());
        }
        if let Some(sender) = &filter.sender {
            let n = arg(&mut args, sender.clone().into());
            predicates.push(format!("(c.sender=?{n} OR c.counterpart=?{n})"));
        }
        if let Some(run) = &filter.run_id {
            let n = arg(&mut args, run.clone().into());
            predicates.push(format!("c.run_id=?{n}"));
        }
        if let Some(query) = fts_query(&filter.query) {
            let n = arg(&mut args, query.into());
            predicates.push(format!(
                "c.rowid IN (SELECT rowid FROM chat_fts WHERE chat_fts MATCH ?{n})"
            ));
        }
        let mut hidden_terms = Vec::new();
        for sender in &hidden.senders {
            let n = arg(&mut args, sender.clone().into());
            hidden_terms.push(format!("c.sender=?{n}"));
        }
        for keyword in &hidden.keywords {
            let n = arg(&mut args, keyword.to_ascii_lowercase().into());
            hidden_terms.push(format!("instr(lower(c.text),?{n})>0"));
        }
        let hidden_expr = if hidden_terms.is_empty() {
            "0".to_owned()
        } else {
            format!("({})", hidden_terms.join(" OR "))
        };
        if !filter.show_hidden {
            predicates.push(format!("NOT {hidden_expr}"));
        }
        if let Some(cursor) = &filter.cursor {
            let at = arg(&mut args, micros(cursor.at)?.into());
            let id = arg(&mut args, cursor.id.clone().into());
            predicates.push(format!("(c.at,c.id)<(?{at},?{id})"));
        }
        let limit = arg(&mut args, (i64::from(filter.limit) + 1).into());
        let sql = format!(
            "SELECT length(CAST(c.json AS BLOB)),c.json,{hidden_expr} FROM chat_messages c WHERE {} ORDER BY c.at DESC,c.id DESC LIMIT ?{limit}",
            predicates.join(" AND ")
        );
        let mut statement = self.conn.prepare(&sql)?;
        let mut result = statement.query(params_from_iter(args))?;
        let (mut rows, mut bytes, mut more) = (Vec::new(), 0, false);
        while let Some(row) = result.next()? {
            if rows.len() == usize::from(filter.limit) {
                more = true;
                break;
            }
            let message: ChatMessageView = row_json(row, 0, &mut bytes, MAX_LOGS_BYTES)?;
            rows.push(ChatRow {
                message,
                hidden: row.get::<_, bool>(2)?,
            });
        }
        let next = more
            .then(|| rows.last())
            .flatten()
            .map(|r: &ChatRow| ChatCursor {
                at: r.message.at,
                id: r.message.id.clone(),
            });
        bounded(ChatPage { rows, next }, MAX_LOGS_BYTES)
    }

    /// Bounded event pages with quoted token-prefix search across stored names.
    pub fn list_events(&self, filter: EventFilter) -> Result<EventPage, StoreError> {
        let (sql, args) = event_page_sql(&filter)?;
        // The probe skips names absent from the whole store. Otherwise the FTS
        // rowid list filters the kind-index scan; the default four-kind query
        // sorts the matches. A name excluded only by the selected scope still
        // needs the page query. The probe, page and run links read one snapshot.
        let tx = self.conn.unchecked_transaction()?;
        if let Some(query) = fts_query(&filter.query) {
            let found: bool = tx.query_row(EVENT_FTS_PROBE, [query], |row| row.get(0))?;
            if !found {
                return Ok(EventPage {
                    rows: vec![],
                    next: None,
                    entered_runs: vec![],
                });
            }
        }
        let mut statement = tx.prepare(&sql)?;
        let mut result = statement.query(params_from_iter(args))?;
        let (mut rows, mut bytes, mut more) = (Vec::new(), 0, false);
        while let Some(row) = result.next()? {
            if rows.len() == usize::from(filter.limit) {
                more = true;
                break;
            }
            let event = row_json(row, 0, &mut bytes, MAX_LOGS_BYTES)?;
            rows.push(crate::instance::normalize_names(event));
        }
        let next = more
            .then(|| rows.last())
            .flatten()
            .map(|r: &InstanceEventView| EventCursor {
                at: r.at,
                id: r.id.clone(),
            });
        let entered_runs = Self::entered_runs(&tx, &rows)?;
        bounded(
            EventPage {
                rows,
                next,
                entered_runs,
            },
            MAX_LOGS_BYTES,
        )
    }

    /// One bounded query for portal IDs on this page. Later portals are checked
    /// against all stored events, independent of the page's filters.
    /// Portals whose dungeon text is a catalog alias don't match a run.
    fn entered_runs(
        conn: &Connection,
        rows: &[InstanceEventView],
    ) -> Result<Vec<EnteredRun>, StoreError> {
        let ids: Vec<_> = rows
            .iter()
            .filter(|row| {
                matches!(
                    row.kind,
                    InstanceEventKind::PortalOpened {
                        dungeon: Some(_),
                        ..
                    }
                )
            })
            .map(|row| &row.id)
            .collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = (1..=ids.len())
            .map(|n| format!("?{n}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT e.id,r.id,r.dungeon,r.started,r.outcome
            FROM instance_events e JOIN runs r ON r.id=(
                SELECT candidate.id FROM runs candidate
                WHERE candidate.session_id=e.session_id
                  AND candidate.dungeon=json_extract(e.json,'$.kind.dungeon')
                  AND candidate.started>=e.at AND candidate.started<=e.at+min(600000000,9223372036854775807-e.at)
                  AND NOT EXISTS (SELECT 1 FROM instance_events later
                    WHERE later.kind='portal_opened' AND later.session_id=e.session_id
                      AND later.at>e.at AND later.at<candidate.started
                      AND json_extract(later.json,'$.kind.dungeon')=candidate.dungeon)
                ORDER BY candidate.started,candidate.id LIMIT 1)
            WHERE e.id IN ({marks}) ORDER BY e.at DESC,e.id DESC LIMIT 100");
        let mut statement = conn.prepare(&sql)?;
        let result = statement
            .query_map(params_from_iter(ids), |row| {
                Ok(EnteredRun {
                    event_id: row.get(0)?,
                    run_id: row.get(1)?,
                    dungeon: row.get(2)?,
                    started_at: Timestamp(row.get::<_, i64>(3)? as u64),
                    outcome: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        bounded(result, MAX_LOGS_BYTES)
    }

    pub fn event_summary(&self, request: EventSummaryRequest) -> Result<EventSummary, StoreError> {
        let mut args = Vec::new();
        let mut predicates = vec![
            "e.kind IN ('player_death','player_left','portal_opened','realm_event_killed')"
                .to_owned(),
        ];
        predicates.extend(event_scope(&request.filter, &mut args, false)?);
        let scope = predicates.join(" AND ");
        let mut summary = EventSummary::default();
        // Read one snapshot so kind totals and dungeon pops agree during capture.
        let tx = self.conn.unchecked_transaction()?;
        let sql = format!(
            "SELECT e.kind,count(*),min(e.at) FROM instance_events e WHERE {scope} GROUP BY e.kind"
        );
        let mut statement = tx.prepare(&sql)?;
        let mut result = statement.query(params_from_iter(&args))?;
        while let Some(row) = result.next()? {
            let count = count_u32(row.get(1)?)?;
            match row.get::<_, String>(0)?.as_str() {
                "player_death" => summary.by_kind.player_death = count,
                "player_left" => summary.by_kind.player_left = count,
                "portal_opened" => summary.by_kind.portal_opened = count,
                "realm_event_killed" => summary.by_kind.realm_event_killed = count,
                _ => unreachable!(),
            }
            summary.total = summary.total.checked_add(count).ok_or(StoreError::Limit)?;
            let first = Timestamp(row.get::<_, i64>(2)? as u64);
            summary.first_at = Some(summary.first_at.map_or(first, |old| old.min(first)));
        }
        // Materialize per-dungeon aggregates once: ranking and local-pop totals
        // otherwise each scan and decode the entire scoped portal history.
        let sql = include_str!("event_summary_pops.sql").replace("/* scope */", &scope);
        let mut statement = tx.prepare(&sql)?;
        let mut result = statement.query(params_from_iter(&args))?;
        while let Some(row) = result.next()? {
            let dungeon: Option<String> = row.get(1)?;
            let pops = count_u32(row.get(2)?)?;
            let last_at = Timestamp(row.get::<_, i64>(3)? as u64);
            if row.get::<_, i64>(0)? == 0 {
                if let Some(dungeon) = dungeon {
                    summary.dungeons.push(DungeonPops {
                        dungeon,
                        pops,
                        last_at,
                    });
                }
            } else {
                summary.local_pops = count_u32(row.get(4)?)?;
                // Preserve an unknown dungeon on the latest local pop as None.
                summary.last_local_pop = dungeon.map(|dungeon| DungeonPops {
                    dungeon,
                    pops,
                    last_at,
                });
            }
        }
        summary.flagged_since = tx
            .query_row(
                "SELECT at FROM instance_events WHERE kind='portal_opened'
             AND json_extract(json,'$.kind.opener_is_local') IS NOT NULL ORDER BY at,id LIMIT 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|at| Timestamp(at as u64));
        bounded(summary, MAX_LOGS_BYTES)
    }
}

pub const CHAT_PRUNE_BATCH: u32 = 1000;
pub const CHAT_CLEAR_BATCH: u32 = 5000;
const CHAT_ALERT_KINDS: &str = "('whisper','guild_message','party_message')";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatMaintenance {
    Prune { before: Timestamp },
    Clear,
}

/// One bounded batch per table. Returns the largest number deleted from any
/// table; repeat until fewer than the batch size. Chat alerts go with chat, except
/// pending deliveries and their referenced alerts, which are preserved.
pub(crate) fn maintain(conn: &mut Connection, op: ChatMaintenance) -> Result<u64, StoreError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let (cutoff, batch, comparison) = match op {
        ChatMaintenance::Prune { before } => match i64::try_from(before.0) {
            Ok(cutoff) => (cutoff, CHAT_PRUNE_BATCH, "<"),
            // A cutoff above SQLite's range includes its largest stored timestamp.
            Err(_) => (i64::MAX, CHAT_PRUNE_BATCH, "<="),
        },
        ChatMaintenance::Clear => (i64::MAX, CHAT_CLEAR_BATCH, "<="),
    };
    let deleted = tx.execute(
        &format!("DELETE FROM chat_messages WHERE rowid IN (SELECT rowid FROM chat_messages WHERE at{comparison}?1 ORDER BY at LIMIT ?2)"),
        params![cutoff, batch],
    )?;
    let orphan_range = match op {
        ChatMaintenance::Clear => "1".to_owned(),
        ChatMaintenance::Prune { .. } => format!("committed_at{comparison}?1"),
    };
    let occurrences = tx.execute(
        &format!("DELETE FROM alert_occurrences WHERE id IN (
            SELECT id FROM alert_occurrences WHERE kind IN {CHAT_ALERT_KINDS} AND state<>'pending'
            AND ((alert_id IS NULL AND {orphan_range})
                OR alert_id IN (SELECT id FROM alerts WHERE kind IN {CHAT_ALERT_KINDS} AND at{comparison}?1))
            ORDER BY committed_at,id LIMIT ?2)"),
        params![cutoff, batch],
    )?;
    let alerts = tx.execute(
        &format!(
            "DELETE FROM alerts WHERE id IN (
            SELECT id FROM alerts WHERE kind IN {CHAT_ALERT_KINDS} AND at{comparison}?1
            AND id NOT IN (SELECT alert_id FROM alert_occurrences WHERE alert_id IS NOT NULL)
            ORDER BY at,id LIMIT ?2)"
        ),
        params![cutoff, batch],
    )?;
    tx.commit()?;
    Ok(deleted.max(occurrences).max(alerts) as u64)
}

/// For when no writer is open (capture stopped). The caller holds the
/// live namespace lease, as for `upgrade_history`. Demo and replay databases
/// must not be passed to this function.
pub fn maintain_chat_file(path: &Path, op: ChatMaintenance) -> Result<u64, StoreError> {
    let mut conn = crate::migrate::open_writer(path)?;
    maintain(&mut conn, op)
}

#[cfg(test)]
mod tests {
    #[test]
    fn fts_query_keeps_only_alphanumeric_terms() {
        assert_eq!(
            super::fts_query("lost halls"),
            Some("\"lost\"* \"halls\"*".into())
        );
        assert_eq!(super::fts_query("\"*() NEAR"), Some("\"NEAR\"*".into()));
        assert_eq!(super::fts_query("  ,; "), None);
    }
}
