use crate::{character_stats::CompletionCounts, clock, math::Rng, world::Entity};
use maxed_protocol::{DecodedPacket, Diagnostic, Packet, PacketDiagnostic};
use maxed_types::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use thiserror::Error;

pub(crate) mod instance;
pub(crate) mod kills;
pub(crate) mod notices;
mod publish;
pub(crate) mod series;
pub(crate) mod snapshot_series;
pub use snapshot_series::SNAPSHOT_SERIES_LIMIT;
pub(crate) mod stat_panel;
#[cfg(test)]
mod tests;
pub(crate) mod totals;
pub use totals::{run_totals, RUN_TOTAL_PLAYER_LIMIT};

// Two seconds covers adjacent bag/update observations without tainting old loot.
pub(crate) const DROP_COVERAGE_WINDOW: u64 = 2_000_000;
const PACKET_DIAGNOSTIC_LIMIT: usize = 64;

// Only packets consumed by world/identity, combat, loot and map/run reducers.
fn record_packet(id: u8) -> bool {
    matches!(
        id,
        10 | 12 | 25 | 30 | 42 | 44 | 45 | 46 | 49 | 67 | 75 | 82 | 92 | 101 | 105 | 169
    )
}

#[derive(Clone)]
pub struct EngineConfig {
    pub stream_id: String,
    pub identity_key: [u8; 32],
    pub max_entities: usize,
    pub max_shots: usize,
    pub max_active_fights: usize,
    pub max_active_bags: usize,
}
impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            stream_id: "engine".into(),
            identity_key: [0; 32],
            max_entities: 100_000,
            max_shots: 50_000,
            max_active_fights: 2048,
            max_active_bags: 1024,
        }
    }
}
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("Invalid catalog")]
    InvalidCatalog,
    #[error("Invalid engine configuration")]
    InvalidConfig,
}
pub(crate) struct Local {
    pub object: i32,
    pub number: i32,
    pub provisional: String,
    pub materialized: bool,
    pub counts: Option<CompletionCounts>,
    pub received: Timestamp,
    pub area: Option<u64>,
}
pub struct Engine {
    pub(crate) config: EngineConfig,
    pub(crate) catalog: Arc<Catalog>,
    pub(crate) crucible_definitions: crate::crucible::Definitions,
    pub(crate) object_index: BTreeMap<u16, usize>,
    pub(crate) object_name_index: BTreeMap<String, Option<u16>>,
    pub(crate) character_dirty: bool,
    pub(crate) class_index: BTreeMap<u16, usize>,
    #[cfg(test)]
    pub(crate) character_rebuilds: usize,
    #[cfg(test)]
    pub(crate) target_refreshes: usize,
    pub(crate) target_dirty: BTreeSet<crate::fights::FightKey>,
    pub(crate) refreshed_quest: Option<crate::fights::FightKey>,
    pub(crate) item_index: BTreeMap<u16, usize>,
    pub(crate) enchant_index: BTreeMap<i32, usize>,
    pub(crate) reconstruction: crate::reconstruction::ReconstructionCoverage,
    pub(crate) removed_shots: BTreeMap<(i32, u16), (u64, crate::ReconstructionReason)>,
    pub(crate) removed_shot_order: BTreeSet<(u64, (i32, u16))>,
    pub(crate) removed_shot_number: u64,
    pub(crate) now: Timestamp,
    pub(crate) tick: Option<Timestamp>,
    pub(crate) rng: Option<Rng>,
    pub(crate) rng_valid: bool,
    pub(crate) capture: CaptureStatus,
    pub(crate) sequence: Sequence,
    pub(crate) session: Option<PlaySessionView>,
    pub(crate) session_number: u64,
    pub(crate) chat_emitted: u64,
    session_published: Timestamp,
    pub(crate) connection: Option<ConnId>,
    expected_reconnect: Option<ConnId>,
    pub(crate) map_interrupted: bool,
    pub(crate) retired_connections: BTreeSet<u64>,
    pub(crate) map: Option<String>,
    pub(crate) area_kind: AreaKind,
    pub(crate) map_ordinal: u64,
    pub(crate) map_last_traffic: Timestamp,
    pub(crate) generation: u32,
    pub(crate) entity_epoch: u32,
    pub(crate) entities: BTreeMap<i32, Entity>,
    pub(crate) local: Option<Local>,
    pub(crate) create_number: u64,
    pub(crate) death_seen: bool,
    pub(crate) hp_buffer: crate::runs::HpBuffer,
    pub(crate) character: Option<CharacterView>,
    pub(crate) last_fame: BTreeMap<String, i32>,
    pub(crate) issues: Vec<CoverageIssue>,
    pub(crate) recent_issues: Vec<(CoverageIssue, Timestamp)>,
    ignored_unverified_packets: u64,
    pub(crate) run: Option<crate::runs::Run>,
    pub(crate) scope: Option<crate::players::Scope>,
    pub(crate) roster_presence: Vec<crate::RosterPresence>,
    pub(crate) preceding: Option<crate::runs::Preceding>,
    pub(crate) fights: BTreeMap<(i32, u32), crate::fights::Fight>,
    pub(crate) retired_targets: BTreeSet<(i32, u32)>,
    pub(crate) dropped_fight_targets: BTreeSet<(i32, u32)>,
    pub(crate) dropped_fight_targets_overflow: bool,
    pub(crate) retired_entities: BTreeMap<i32, crate::world::RetiredEntity>,
    pub(crate) shots: BTreeMap<(i32, u16), crate::combat::Shot>,
    pub(crate) shot_number: u64,
    pub(crate) shot_expiry: BTreeSet<(Timestamp, u64, i32, u16)>,
    pub(crate) pair_expiry: BTreeSet<(Timestamp, crate::fights::FightKey, crate::fights::PairKey)>,
    pub(crate) dirty_fights: BTreeSet<crate::fights::FightKey>,
    pub(crate) fight_published: Timestamp,
    pub(crate) snapshot_series_cache: Arc<std::sync::Mutex<snapshot_series::SnapshotSeriesCache>>,
    pub(crate) run_totals_cache: std::sync::Arc<std::sync::Mutex<totals::RunTotalsCache>>,
    pub(crate) summons: BTreeMap<i32, BTreeSet<i32>>,
    pub(crate) referenced_entities: BTreeSet<i32>,
    pub(crate) inactive_entities: BTreeSet<(Timestamp, i32)>,
    pub(crate) drops: BTreeMap<(i32, u32), crate::drops::Bag>,
    pub(crate) loot_tick: u64,
    pub(crate) loot_window: Option<(u16, u64, &'static str)>,
    pub(crate) removed_enemies: Vec<(u16, (f32, f32))>,
    pub(crate) instance: instance::Instance,
    pub(crate) notices: notices::Notices,
    pub(crate) kill_detector: kills::Kills,
    pub(crate) observed_kills: BTreeMap<kills::TargetKey, kills::Kill>,
}

impl Engine {
    pub fn new(config: EngineConfig, catalog: Arc<Catalog>) -> Result<Self, EngineError> {
        if config.stream_id.is_empty()
            || config.stream_id.len() > 256
            || config.max_entities == 0
            || config.max_entities > 100_000
            || config.max_shots == 0
            || config.max_shots > 50_000
            || config.max_active_fights == 0
            || config.max_active_fights > 2048
            || config.max_active_bags == 0
            || config.max_active_bags > 1024
        {
            return Err(EngineError::InvalidConfig);
        }
        validate_catalog(&catalog)?;
        let object_index = catalog
            .objects
            .iter()
            .enumerate()
            .map(|(i, o)| (o.object_type, i))
            .collect();
        let mut object_name_index = BTreeMap::new();
        for object in &catalog.objects {
            object_name_index
                .entry(object.name.to_lowercase())
                .and_modify(|matched| {
                    if *matched != Some(object.object_type) {
                        *matched = None;
                    }
                })
                .or_insert(Some(object.object_type));
        }
        let class_index = catalog
            .classes
            .iter()
            .enumerate()
            .map(|(i, c)| (c.object_type, i))
            .collect();
        let item_index = catalog
            .items
            .iter()
            .enumerate()
            .map(|(i, o)| (o.object_type, i))
            .collect();
        let enchant_index = catalog
            .enchants
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        Ok(Self {
            config,
            catalog,
            crucible_definitions: Default::default(),
            object_index,
            object_name_index,
            item_index,
            class_index,
            character_dirty: false,
            #[cfg(test)]
            character_rebuilds: 0,
            #[cfg(test)]
            target_refreshes: 0,
            target_dirty: BTreeSet::new(),
            refreshed_quest: None,
            enchant_index,
            reconstruction: Default::default(),
            removed_shots: BTreeMap::new(),
            removed_shot_order: BTreeSet::new(),
            removed_shot_number: 0,
            now: Timestamp(0),
            tick: None,
            rng: None,
            rng_valid: false,
            capture: CaptureStatus::default(),
            sequence: Sequence(Timestamp(0)),
            session: None,
            session_number: 0,
            chat_emitted: 0,
            session_published: Timestamp(0),
            connection: None,
            expected_reconnect: None,
            map_interrupted: false,
            retired_connections: BTreeSet::new(),
            map: None,
            area_kind: AreaKind::Unknown,
            map_ordinal: 0,
            map_last_traffic: Timestamp(0),
            generation: 0,
            entity_epoch: 0,
            entities: BTreeMap::new(),
            local: None,
            character: None,
            last_fame: BTreeMap::new(),
            create_number: 0,
            death_seen: false,
            hp_buffer: Default::default(),
            issues: vec![],
            recent_issues: vec![],
            ignored_unverified_packets: 0,
            run: None,
            scope: None,
            roster_presence: vec![],
            preceding: None,
            fights: BTreeMap::new(),
            retired_targets: BTreeSet::new(),
            dropped_fight_targets: BTreeSet::new(),
            dropped_fight_targets_overflow: false,
            retired_entities: BTreeMap::new(),
            shots: BTreeMap::new(),
            shot_number: 0,
            shot_expiry: BTreeSet::new(),
            pair_expiry: BTreeSet::new(),
            dirty_fights: BTreeSet::new(),
            fight_published: Timestamp(0),
            run_totals_cache: Default::default(),
            snapshot_series_cache: Default::default(),
            summons: BTreeMap::new(),
            referenced_entities: BTreeSet::new(),
            inactive_entities: BTreeSet::new(),
            drops: BTreeMap::new(),
            loot_tick: 0,
            loot_window: None,
            removed_enemies: vec![],
            instance: Default::default(),
            notices: Default::default(),
            kill_detector: Default::default(),
            observed_kills: BTreeMap::new(),
        })
    }
    pub fn set_watermark(&mut self, sequence: Sequence) {
        self.sequence = sequence;
    }
    /// Lifetime counters for verified ENEMYHIT reconstruction attempts. Counts
    /// survive map boundaries and finish; values and keys contain no identities.
    pub fn reconstruction_coverage(&self) -> BTreeMap<&'static str, u64> {
        self.reconstruction.counts()
    }
    /// Lifetime count of packets rejected for unverified cipher alignment.
    /// Stale connections are discarded separately and do not count here.
    pub fn ignored_unverified_packets(&self) -> u64 {
        self.ignored_unverified_packets
    }
    /// Cheap presentation scheduling hint; does not project or change fights.
    pub fn has_active_fight(&self) -> bool {
        self.fights.values().any(|fight| fight.base.ended.is_none())
    }

    /// Latest damage timestamp among open fights, in capture microseconds.
    /// A scheduling hint only: does not project or change fight state.
    pub fn latest_open_fight_hit(&self) -> Option<Timestamp> {
        self.fights
            .values()
            .filter(|fight| fight.base.ended.is_none())
            .filter_map(|fight| fight.base.last_hit)
            .max()
    }

    fn live_fight_priority(fight: &crate::fights::Fight) -> bool {
        fight.base.is_current_quest
            || matches!(
                fight.base.target_kind,
                Some(
                    TargetKind::Boss
                        | TargetKind::EventBoss
                        | TargetKind::Miniboss
                        | TargetKind::Quest
                )
            )
    }

    fn live_fight_visible(&self, fight: &crate::fights::Fight) -> bool {
        Self::live_fight_priority(fight)
            || fight
                .latest_local_hit()
                .is_some_and(|at| self.now.0.saturating_sub(at.0) < 10_000_000)
    }

    /// Presentation projection; domain exports continue to use `snapshot`.
    pub fn live_snapshot(&self) -> LiveSnapshot {
        let selected = self.live_snapshot_series_selection();
        let fights = self
            .fights
            .iter()
            .filter(|(_, f)| self.live_fight_visible(f))
            .map(|(&key, f)| {
                let mut view = f.project_snapshot(selected.contains(key));
                crate::compact_live_fight(&mut view);
                view
            })
            .collect();
        let mut snapshot = self.snapshot_fields(fights, self.snapshot_run_totals());
        snapshot.fights.sort_by(|a, b| a.id.cmp(&b.id));
        snapshot.drops.sort_by(|a, b| a.id.cmp(&b.id));
        snapshot
    }
    pub fn snapshot(&self) -> LiveSnapshot {
        let selected = self.snapshot_series_selection();
        let fights: Vec<_> = self
            .fights
            .iter()
            .map(|(&key, f)| f.project_snapshot(selected.contains(key)))
            .collect();
        let run_totals = self.snapshot_run_totals();
        self.snapshot_fields(fights, run_totals)
    }
    fn snapshot_fields(
        &self,
        fights: Vec<FightView>,
        run_totals: Option<RunTotals>,
    ) -> LiveSnapshot {
        LiveSnapshot {
            revision: None,
            instance: self.instance.context.as_ref().map(|context| InstanceLive {
                context: context.into(),
                population: self.instance.population(),
                population_peak: self.instance.population_peak,
                population_provisional: true,
                quest_target: self.instance.quest.as_ref().map(|q| QuestTarget {
                    object_id: q.object_id,
                    object_type: q.object_type,
                    since: q.since,
                }),
                realm_score: self.instance.realm_score.as_ref().map(|s| RealmScore {
                    current: s.current,
                    max: s.max,
                }),
                visit_fame_delta: self
                    .local
                    .as_ref()
                    .filter(|l| l.area == Some(self.map_ordinal))
                    .and_then(|_| {
                        self.instance.visit_fame_delta(
                            self.create_number,
                            self.character.as_ref().and_then(|c| c.fame),
                        )
                    }),
                events_dropped: self.notices.dropped,
            }),
            run_totals,
            at: self.now,
            stream_id: self.config.stream_id.clone(),
            sequence: self.sequence,
            capture: self.capture.clone(),
            session: self.session.clone(),
            character: self.character.clone(),
            map: self.map.clone(),
            area_kind: self.area_kind.clone(),
            run: self.run.as_ref().map(|r| r.view.clone()),
            fights,
            drops: self.drops.values().map(|b| b.view.clone()).collect(),
            issues: self.issues.clone(),
        }
    }
    pub(crate) fn id(&self, kind: &str, number: u64) -> String {
        format!("{}:{kind}:{number}", self.config.stream_id)
    }
    pub(crate) fn session_id(&self) -> String {
        self.session
            .as_ref()
            .map(|s| s.id.clone())
            .unwrap_or_else(|| self.id("session", self.session_number))
    }
    pub(crate) fn mark(&mut self, issue: CoverageIssue, events: &mut Vec<EngineEvent>) {
        if issue == CoverageIssue::StreamGap {
            self.rng_valid = false;
        }
        if !self.issues.contains(&issue) {
            self.issues.push(issue.clone());
        }
        self.recent_issues
            .retain(|(_, at)| self.now.0.saturating_sub(at.0) <= DROP_COVERAGE_WINDOW);
        if let Some((_, at)) = self.recent_issues.iter_mut().find(|(i, _)| i == &issue) {
            *at = self.now;
        } else {
            self.recent_issues.push((issue.clone(), self.now));
        }
        if let Some(r) = self.run.as_mut().filter(|r| r.view.ended.is_none()) {
            if !r.view.issues.contains(&issue) {
                r.view.issues.push(issue.clone());
                r.view.complete_capture = false;
                if matches!(
                    issue,
                    CoverageIssue::DecodeFailure
                        | CoverageIssue::StreamGap
                        | CoverageIssue::Midstream
                ) {
                    r.view.evidence.push(EvidenceAt {
                        at: clock::millis(self.now),
                        evidence: Evidence::CaptureGap,
                    });
                }
                events.push(EngineEvent::RunUpdated(r.view.clone()));
            }
        }
        for (key, f) in &mut self.fights {
            if f.base.ended.is_none() && !f.base.issues.contains(&issue) {
                f.changed();
                f.base.issues.push(issue.clone());
                f.base.complete_capture = false;
                self.dirty_fights.insert(*key);
            }
        }
        for b in self.drops.values_mut() {
            if self.now.0.saturating_sub(b.observed_at.0) <= DROP_COVERAGE_WINDOW
                && !b.view.issues.contains(&issue)
            {
                b.view.issues.push(issue.clone());
                events.push(EngineEvent::DropUpdated(b.view.clone()));
            }
        }
        if let Some(p) = &mut self.preceding {
            p.valid = false;
        }
    }
    pub fn advance_to(&mut self, at: Timestamp) -> Vec<EngineEvent> {
        self.advance_clock(at, true)
    }
    /// Live inactivity is measured by the adapter's monotonic arrival clock,
    /// independently of capture timestamp corrections. Replay timers may use
    /// `advance_to` when event time itself is the intended idle clock.
    pub fn advance_live_to(
        &mut self,
        at: Timestamp,
        quiet: std::time::Duration,
    ) -> Vec<EngineEvent> {
        let mut events = self.advance_clock(at, false);
        if quiet >= std::time::Duration::from_secs(1800) {
            self.expire_session(&mut events);
        }
        events
    }
    fn advance_clock(&mut self, at: Timestamp, check_idle: bool) -> Vec<EngineEvent> {
        self.roster_presence.clear();
        let mut events = vec![];
        let (time, regression) = clock::monotonic(self.now.0, at.0);
        self.now = Timestamp(time);
        if regression {
            self.mark(CoverageIssue::ClockRegression, &mut events);
        }
        self.kill_detector.expire_announcements(self.now);
        self.expire_departures(&mut events);
        self.expire_shots();
        self.prune_entities(&mut events);
        if self.now.0.saturating_sub(self.fight_published.0) >= 250_000 {
            for key in std::mem::take(&mut self.dirty_fights) {
                if let Some(f) = self.fights.get(&key) {
                    events.push(EngineEvent::FightUpdated(f.project()));
                }
            }
            self.fight_published = self.now;
            self.drain_fight_timelines();
            self.flush_profiles(&mut events);
        }
        self.flush_timeline(false, &mut events);
        if check_idle
            && self.session.as_ref().is_some_and(|s| {
                s.ended.is_none() && self.now >= crate::idle_deadline(s.last_traffic)
            })
        {
            self.expire_session(&mut events);
        }
        events
    }
    fn expire_session(&mut self, events: &mut Vec<EngineEvent>) {
        if self.session.as_ref().is_some_and(|s| s.ended.is_none()) {
            self.close_map(FightEnd::SessionIdle, false, events);
            if let Some(s) = &mut self.session {
                s.ended = Some(s.last_traffic);
                events.push(EngineEvent::SessionUpdated(s.clone()));
            }
            self.lose_identity(events);
            self.clear_world();
            self.map = None;
            self.area_kind = AreaKind::Unknown;
            self.preceding = None;
        }
    }
    pub fn observe_traffic(&mut self, at: Timestamp) -> Vec<EngineEvent> {
        self.observe_traffic_clock(at, true)
    }
    /// Live callers check arrival-clock inactivity with advance_live_to before
    /// observing returning traffic. Replay callers use observe_traffic, whose
    /// input timestamps supply both event time and the inactivity time base.
    pub fn observe_live_traffic(&mut self, at: Timestamp) -> Vec<EngineEvent> {
        self.observe_traffic_clock(at, false)
    }
    fn observe_traffic_clock(&mut self, at: Timestamp, check_idle: bool) -> Vec<EngineEvent> {
        let mut events = self.advance_clock(at, check_idle);
        let started = self.session.as_ref().is_none_or(|s| s.ended.is_some());
        if started {
            if self.session.is_some() {
                self.issues.clear();
            }
            self.session_number += 1;
            self.session = Some(PlaySessionView {
                issues: vec![],
                id: self.id("session", self.session_number),
                started: self.now,
                last_traffic: self.now,
                ended: None,
            });
        } else if let Some(s) = &mut self.session {
            s.last_traffic = self.now;
        }
        if let Some(s) = &self.session {
            if started || s.last_traffic.0.saturating_sub(self.session_published.0) >= 30_000_000 {
                self.session_published = s.last_traffic;
                events.push(EngineEvent::SessionUpdated(s.clone()));
            }
        }
        events
    }
    pub fn capture_changed(&mut self, mut status: CaptureStatus) -> Vec<EngineEvent> {
        self.roster_presence.clear();
        // Native readiness updates do not own the engine's packet counters.
        status.packet_diagnostics = std::mem::take(&mut self.capture.packet_diagnostics);
        self.capture = status.clone();
        vec![EngineEvent::CaptureStateChanged(status)]
    }
    pub fn diagnostic(
        &mut self,
        conn: ConnId,
        _dir: Direction,
        at: Timestamp,
        kind: Diagnostic,
    ) -> Vec<EngineEvent> {
        self.roster_presence.clear();
        if self.connection.is_some_and(|c| conn.0 < c.0)
            || self.retired_connections.contains(&conn.0)
        {
            return vec![];
        }
        let mut events = self.advance_clock(at, false);
        match kind {
            Diagnostic::Packet {
                id,
                problem,
                alignment_verified,
            } => {
                if matches!(
                    problem,
                    PacketDiagnostic::UnknownId
                        | PacketDiagnostic::BadFields
                        | PacketDiagnostic::TrailingBytes
                ) {
                    self.count_packet_diagnostic(id, problem);
                }
                if alignment_verified
                    && matches!(
                        problem,
                        PacketDiagnostic::BadFields | PacketDiagnostic::TrailingBytes
                    )
                    && record_packet(id)
                {
                    if id == 30 {
                        self.rng_valid = false;
                    }
                    self.mark(CoverageIssue::DecodeFailure, &mut events);
                }
            }
            Diagnostic::Recovered => {
                // A decoder search match is only a candidate, not D-trust proof.
                self.capture.state = CaptureState::Recovering;
                self.capture.message = "Verifying capture alignment".into();
            }
            Diagnostic::Recovering => {
                self.capture.state = CaptureState::Recovering;
                self.capture.message = "Recovering capture".into();
                self.mark(CoverageIssue::StreamGap, &mut events);
            }
            Diagnostic::UnknownId => {
                self.capture.unknown_packets = self.capture.unknown_packets.saturating_add(1);
            }
            Diagnostic::BadFrame
            | Diagnostic::BadFields
            | Diagnostic::TrailingBytes
            | Diagnostic::WrongDirection => {
                self.capture.decode_failures = self.capture.decode_failures.saturating_add(1);
            }
            Diagnostic::Closed => {
                if self.connection == Some(conn)
                    && self.expected_reconnect != Some(conn)
                    && !self.map_interrupted
                    && self.run.as_ref().is_some_and(|r| {
                        r.view.ended.is_none()
                            && r.view.died_at.is_none()
                            && r.view.completed_at.is_none()
                    })
                {
                    self.map_interrupted = true;
                    self.mark(CoverageIssue::StreamGap, &mut events);
                    // A prior gap must not hide this distinct interruption time.
                    if let Some(r) = &mut self.run {
                        let evidence = EvidenceAt {
                            at: clock::millis(self.now),
                            evidence: Evidence::CaptureGap,
                        };
                        if !r.view.evidence.contains(&evidence) {
                            r.view.evidence.push(evidence);
                            events.push(EngineEvent::RunUpdated(r.view.clone()));
                        }
                    }
                }
                return events;
            }
            Diagnostic::Cancelled => return events,
        }
        events.push(EngineEvent::CaptureStateChanged(self.capture.clone()));
        events
    }
    fn count_packet_diagnostic(&mut self, id: u8, problem: PacketDiagnostic) {
        let counts = &mut self.capture.packet_diagnostics;
        let index = match counts.binary_search_by_key(&id, |c| c.packet_id) {
            Ok(index) => index,
            Err(index) if counts.len() < PACKET_DIAGNOSTIC_LIMIT => {
                counts.insert(
                    index,
                    PacketDiagnosticCount {
                        packet_id: id,
                        unknown_packets: 0,
                        field_failures: 0,
                    },
                );
                index
            }
            Err(_) => return,
        };
        let count = if problem == PacketDiagnostic::UnknownId {
            &mut counts[index].unknown_packets
        } else {
            &mut counts[index].field_failures
        };
        *count = count.saturating_add(1);
    }
    pub fn finish(&mut self, at: Timestamp, reason: FightEnd) -> Vec<EngineEvent> {
        let mut events = self.advance_to(at);
        self.close_map(reason, false, &mut events);
        if let Some(s) = &mut self.session {
            if s.ended.is_none() {
                s.ended = Some(s.last_traffic);
                events.push(EngineEvent::SessionUpdated(s.clone()));
            }
        }
        self.lose_identity(&mut events);
        self.clear_world();
        self.map = None;
        self.area_kind = AreaKind::Unknown;
        self.preceding = None;
        events
    }
    pub(crate) fn clear_world(&mut self) {
        self.map_interrupted = false;
        self.expected_reconnect = None;
        self.clear_entities();
        self.death_seen = false;
        self.local = None;
        self.character = None;
        self.shots.clear();
        self.removed_shots.clear();
        self.removed_shot_order.clear();
        self.removed_shot_number = 0;
        self.shot_expiry.clear();
        self.rng = None;
        self.rng_valid = false;
        self.tick = None;
        self.drops.clear();
        self.recent_issues.clear();
        self.loot_window = None;
        self.removed_enemies.clear();
        self.loot_tick = 0;
        self.retired_targets.clear();
        self.dropped_fight_targets.clear();
        self.dropped_fight_targets_overflow = false;
        self.pair_expiry.clear();
        self.dirty_fights.clear();
    }
    pub fn apply(&mut self, packet: DecodedPacket) -> Vec<EngineEvent> {
        self.apply_inner(packet, false)
    }
    /// Apply a packet from a frame the adapter already passed to observe_traffic
    /// (replay) or observe_live_traffic (monotonic-arrival live capture).
    /// The packet still needs verified alignment; observing raw traffic grants
    /// no permission to consume unverified gameplay evidence.
    pub fn apply_observed(&mut self, packet: DecodedPacket) -> Vec<EngineEvent> {
        self.apply_inner(packet, true)
    }
    fn apply_inner(&mut self, packet: DecodedPacket, observed: bool) -> Vec<EngineEvent> {
        self.roster_presence.clear();
        if self.connection.is_some_and(|c| packet.conn.0 < c.0)
            || self.retired_connections.contains(&packet.conn.0)
        {
            return vec![];
        }
        if !packet.alignment_verified {
            self.ignored_unverified_packets = self.ignored_unverified_packets.saturating_add(1);
            let mut events = vec![];
            // Do not advance timers or accept connection/map/identity evidence
            // from this packet. Only uncertainty and its counter may change.
            if !matches!(packet.packet, Packet::Unknown { .. }) {
                self.mark(CoverageIssue::Midstream, &mut events);
            }
            self.capture.message = format!(
                "Ignored {} packets with unverified cipher alignment",
                self.ignored_unverified_packets
            );
            events.push(EngineEvent::CaptureStateChanged(self.capture.clone()));
            return events;
        }
        let changed_connection = self.connection.is_some_and(|c| c != packet.conn);
        let mut events = if observed {
            vec![]
        } else {
            self.observe_traffic(packet.at)
        };
        if self.connection != Some(packet.conn) {
            if self.connection.is_some()
                && !matches!(packet.packet, Packet::MapInfo(_) | Packet::CreateSuccess(_))
            {
                return events;
            }
            if let Some(old) = self.connection.replace(packet.conn) {
                self.expected_reconnect = None;
                self.retired_connections.insert(old.0);
                // ConnIds are adapter-local; after the bounded retirement table fills,
                // unseen connections still must begin with MAPINFO/CREATE_SUCCESS.
                while self.retired_connections.len() > 1024 {
                    self.retired_connections.pop_first();
                }
                self.shots.clear();
                self.removed_shots.clear();
                self.removed_shot_order.clear();
                self.removed_shot_number = 0;
                self.shot_expiry.clear();
                self.local = None;
                self.clear_entities();
                self.lose_identity(&mut events);
            }
        }
        if self.capture.state == CaptureState::Recovering
            && !matches!(packet.packet, Packet::Unknown { .. })
        {
            self.capture.state = CaptureState::Live;
            self.capture.message = "Capture recovered".into();
            events.push(EngineEvent::CaptureStateChanged(self.capture.clone()));
        }
        match packet.packet {
            Packet::CrucibleResponse(p) => self.crucible_definitions.learn(&p.crucibleJsons),
            Packet::MapInfo(p) => self.enter_map(p, &mut events),
            Packet::CreateSuccess(p) => {
                let replacing = self.local.is_some();
                if replacing {
                    self.forget_entity(p.objectId, &mut events);
                    self.lose_identity(&mut events);
                }
                self.create_number += 1;
                self.death_seen = false;
                self.hp_buffer = Default::default();
                self.local = Some(Local {
                    object: p.objectId,
                    number: p.charId,
                    provisional: format!(
                        "{}:provisional:{}:{}",
                        self.config.stream_id, self.map_ordinal, self.create_number
                    ),
                    materialized: false,
                    counts: crate::decode_completion_counts(&p.str).ok(),
                    received: self.now,
                    area: if replacing || changed_connection || self.map.is_none() {
                        None
                    } else {
                        Some(self.map_ordinal)
                    },
                });
                self.refresh_character(&mut events);
            }
            Packet::Update(p) => {
                let mut roster_joins = Vec::new();
                for o in p.newObjects {
                    let id = o.status.objectId;
                    if let Some(l) = &mut self.local {
                        if l.object == o.status.objectId {
                            l.materialized = true;
                        }
                    }
                    self.merge_entity(o.status, Some(o.objectType), &mut events);
                    if self.is_player(id) && self.entities.get(&id).is_some_and(|e| e.present) {
                        roster_joins.push(id);
                        self.notices
                            .departures
                            .retain(|(object, _), _| *object != id);
                    } else {
                        self.instance.roster.remove(&id);
                    }
                }
                self.instance.roster_update(roster_joins, &p.drops);
                for id in p.drops {
                    if let Some(e) = self.entities.get(&id) {
                        let loot = e.object_type.map(|object_type| notices::LootKill {
                            tick: self.loot_tick,
                            object_type,
                            pos: e.pos,
                        });
                        let kill = self
                            .kill_detector
                            .removed((id, e.generation), self.now, loot);
                        self.observe_kill(kill, &mut events);
                    }
                    if self
                        .entities
                        .get(&id)
                        .is_some_and(|e| self.fights.contains_key(&(id, e.generation)))
                    {
                        if let Some(e) = self.entities.get(&id) {
                            if let Some(t) = e.object_type {
                                if self.removed_enemies.len() < self.config.max_entities {
                                    self.removed_enemies.push((t, e.pos));
                                }
                            }
                        }
                    }
                    self.hold_departure(id);
                    self.remove_entity(id, &mut events);
                }
                self.update_party(&mut events);
            }
            Packet::NewTick(p) => {
                self.tick = Some(clock::millis(self.now));
                for s in p.status {
                    self.merge_entity(s, None, &mut events);
                }
                self.update_party(&mut events);
                self.enrich_loot(&mut events);
            }
            Packet::QuestObjId(p) => {
                self.instance.quest_update(p.objectId, self.now);
                let generation = self.entities.get(&p.objectId).map(|e| e.generation);
                self.instance.quest_generation = generation;
                if let Some(ty) = self.entities.get(&p.objectId).and_then(|e| e.object_type) {
                    self.instance.resolve_quest(p.objectId, ty);
                }
                let kill = self
                    .kill_detector
                    .quest_id(p.objectId, generation, self.now);
                self.observe_kill(kill, &mut events);
            }
            Packet::RealmScoreUpdate(p) => self.instance.score_update(p.score),
            Packet::Notification(p) if p.data.effect == Some("Victory") => {
                let mut detector = std::mem::take(&mut self.kill_detector);
                let kill = detector.victory(self.now, &self.active_targets());
                self.kill_detector = detector;
                self.observe_kill(kill, &mut events);
                self.complete(Evidence::Victory, &mut events)
            }
            Packet::Notification(p)
                if matches!(p.data.effect, Some("PlayerDeath" | "PortalOpened")) =>
            {
                if let Some(notice) = p.data.message.as_deref().and_then(notices::parse) {
                    if matches!(
                        (&notice, p.data.effect),
                        (notices::Notice::PlayerDeath { .. }, Some("PlayerDeath"))
                            | (notices::Notice::PortalOpened { .. }, Some("PortalOpened"))
                    ) {
                        self.notices
                            .push(notice, self.now, Some(p.data.pictureType));
                    }
                }
            }
            Packet::Text(p) => {
                if p.recipient.is_empty() {
                    if p.name == "#Oryx the Mad God" {
                        if let Some(notice @ notices::Notice::RealmEventKilled { .. }) =
                            notices::parse(&p.text)
                        {
                            if let notices::Notice::RealmEventKilled { boss_key, .. } = &notice {
                                let mut detector = std::mem::take(&mut self.kill_detector);
                                let name = self
                                    .realm_event_object(boss_key)
                                    .map_or(boss_key.as_str(), |o| o.name.as_str());
                                let kill = detector.oryx(name, self.now, &self.active_targets());
                                self.kill_detector = detector;
                                self.observe_kill(kill, &mut events);
                            }
                            self.notices.push(notice, self.now, None);
                        }
                    }
                    if let Some(rule) = self
                        .map
                        .as_deref()
                        .and_then(|m| crate::completion_rule(m, &p.name, &p.text))
                    {
                        self.complete(
                            Evidence::CompletionDialogue { rule: rule.into() },
                            &mut events,
                        );
                    }
                    self.loot_dialogue(&p.name, &p.text);
                }
                self.emit_chat(&p, &mut events);
            }
            Packet::Death(p) => self.die(p, &mut events),
            Packet::Escape(_) => self.end_run(true, None, &mut events),
            Packet::Reconnect(_) => self.expected_reconnect = Some(packet.conn),
            Packet::Unknown { .. } => {}
            p => self.apply_combat(&p, &mut events),
        }
        self.publish_notices(&mut events);
        self.refresh_targets();
        if self
            .local
            .as_ref()
            .is_none_or(|l| l.area == Some(self.map_ordinal))
        {
            self.map_last_traffic = self.now;
        }
        events
    }
    fn active_targets(&self) -> Vec<kills::ActiveTarget<'_>> {
        self.fights
            .iter()
            .map(|(&key, f)| kills::ActiveTarget {
                key,
                boss: f.base.boss_type.is_some(),
                last_hit: f.base.last_hit,
                name: f
                    .base
                    .target_type
                    .and_then(|ty| self.object_index.get(&ty))
                    .map(|&i| self.catalog.objects[i].name.as_str()),
            })
            .collect()
    }
    pub(crate) fn observe_kill(
        &mut self,
        kill: Option<kills::Kill>,
        events: &mut Vec<EngineEvent>,
    ) {
        if let Some(kill) = kill {
            if let Some(f) = self.fights.get_mut(&kill.target) {
                if f.base
                    .kill
                    .as_ref()
                    .is_some_and(|old| !old.provisional || kill.provisional)
                {
                    return;
                }
                f.changed();
                if !kill.provisional {
                    f.base.ended = Some(clock::millis(kill.at));
                    f.base.end_reason = Some(FightEnd::Killed);
                }
                f.base.kill = Some(KillView {
                    at: clock::millis(kill.at),
                    provisional: kill.provisional,
                    evidence: match kill.evidence {
                        kills::KillEvidence::VictoryQuestTarget => KillEvidence::VictoryQuestTarget,
                        kills::KillEvidence::VictorySoleBoss => KillEvidence::VictorySoleBoss,
                        kills::KillEvidence::HpZeroRemoved => KillEvidence::HpZeroRemoved,
                        kills::KillEvidence::RealmAnnouncement => KillEvidence::RealmAnnouncement,
                        kills::KillEvidence::AnnouncedDeathRemoved => {
                            KillEvidence::AnnouncedDeathRemoved
                        }
                    },
                });
                let loot = self.kill_detector.removed_loot(kill.target).or_else(|| {
                    self.entities
                        .get(&kill.target.0)
                        .filter(|e| e.generation == kill.target.1)
                        .and_then(|e| {
                            e.object_type.map(|object_type| notices::LootKill {
                                tick: self.loot_tick,
                                object_type,
                                pos: e.pos,
                            })
                        })
                });
                if let Some(loot) = loot {
                    self.notices.loot_kills.insert(kill.target, loot);
                }
                self.dirty_fights.insert(kill.target);
                events.push(EngineEvent::FightUpdated(f.project()));
                // The confirmation immediately publishes the entire dirty view.
                self.dirty_fights.remove(&kill.target);
                if !kill.provisional {
                    self.attribute_kill_loot(events);
                }
                self.observed_kills.insert(kill.target, kill);
            }
        }
    }
}

fn validate_catalog(c: &Catalog) -> Result<(), EngineError> {
    let bad = || EngineError::InvalidCatalog;
    if c.enchants.len() > 65536
        || c.enchants.iter().any(|e| {
            e.stat_damage_multiplier
                .is_some_and(|m| !m.is_finite() || m < 0.0 || m > f64::from(f32::MAX))
        })
        || c.enchants
            .iter()
            .map(|e| e.id)
            .collect::<BTreeSet<_>>()
            .len()
            != c.enchants.len()
    {
        return Err(bad());
    }
    if c.format_version != 1
        || c.items.len() > 65536
        || c.objects.len() > 65536
        || c.dungeons.len() > 4096
        || c.classes.len() > 65536
        || c.bags.len() > 65536
    {
        return Err(bad());
    }
    for keys in [
        c.items.iter().map(|i| i.object_type).collect::<Vec<_>>(),
        c.objects.iter().map(|i| i.object_type).collect(),
        c.classes.iter().map(|i| i.object_type).collect(),
        c.bags.iter().map(|i| i.object_type).collect(),
    ] {
        if keys.iter().copied().collect::<BTreeSet<_>>().len() != keys.len() {
            return Err(bad());
        }
    }
    for item in &c.items {
        let mut indexes = BTreeSet::new();
        for p in &item.projectiles {
            if p.index < 0
                || p.min_damage < 0
                || p.max_damage < p.min_damage
                || p.max_damage > 1_000_000
                || !indexes.insert(p.index)
            {
                return Err(bad());
            }
        }
        if let Some(s) = &item.scaling {
            if !s.damage_per_stat.is_finite()
                || s.damage_per_stat < 0.
                || s.damage_per_stat > 100_000.
                || s.minimum < 0
                || s.shots == 0
            {
                return Err(bad());
            }
        }
    }
    if c.crucible_bonuses.iter().any(|c| {
        !c.damage_multiplier.is_finite() || c.damage_multiplier <= 0. || c.damage_multiplier > 100.
    }) {
        return Err(bad());
    }
    Ok(())
}
