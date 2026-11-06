//! Current-scope observations; history and annotations remain owned by the store.

use super::history::{HistoryCaller, HistoryKey};
use maxed_engine::party::{contribution_flags, evaluate, item_tier, FlagInput, ItemTier};
use maxed_engine::PROFILE_LIMIT;
use maxed_types::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

pub(crate) fn realmeye_url(name: &str) -> Option<String> {
    ((1..=10).contains(&name.len()) && name.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .then(|| format!("https://www.realmeye.com/player/{name}"))
}

pub(crate) fn open_realmeye(name: &str) -> Result<(), String> {
    let url = realmeye_url(name).ok_or("RealmEye requires a name of 1 to 10 letters.")?;
    spawn_realmeye(realmeye_command(&url, std::env::var_os("SystemRoot")))
}

fn realmeye_command(url: &str, system_root: Option<std::ffi::OsString>) -> std::process::Command {
    let root = system_root
        .map(std::path::PathBuf::from)
        .filter(|root| root.is_absolute())
        .unwrap_or_else(|| r"C:\Windows".into());
    let mut command = std::process::Command::new(root.join("explorer.exe"));
    command.arg(url);
    command
}

fn spawn_realmeye(mut command: std::process::Command) -> Result<(), String> {
    // Explorer can exit with status 1 after a successful handoff. Only spawning
    // matters; dropping Child neither waits for nor checks the browser status.
    command
        .spawn()
        .map(drop)
        .map_err(|_| "RealmEye could not be opened in your browser.".into())
}

pub(super) use maxed_store::PartyWrite;

pub(super) struct PartyWriteRequest {
    pub operation: PartyWrite,
    pub reply: crossbeam_channel::Sender<Result<(), String>>,
}

pub(super) const WRITE_FAILED: &str = "Party history could not be updated.";

#[derive(Clone)]
pub(crate) struct PartyRoster {
    dirty: bool,
    scope_id: Option<String>,
    run_id: Option<String>,
    map: Option<String>,
    area_kind: AreaKind,
    members: BTreeMap<String, Member>,
    bosses: BTreeMap<String, BossFight>,
    pick: Option<String>,
}

pub(crate) struct PartyCatalog {
    pub(super) catalog: Arc<Catalog>,
    tiers: BTreeMap<u16, ItemTier>,
}

impl PartyCatalog {
    pub(super) fn new(catalog: Arc<Catalog>) -> Self {
        let tiers = catalog
            .items
            .iter()
            .map(|item| (item.object_type, item_tier(Some(item))))
            .collect();
        Self { catalog, tiers }
    }

    fn tier(&self, id: u16) -> ItemTier {
        self.tiers.get(&id).copied().unwrap_or(ItemTier::Unknown)
    }
}

#[derive(Clone)]
struct Member {
    profile: PlayerProfileView,
    joined: Option<Timestamp>,
    present: Option<bool>,
    out_of_view_since: Option<Timestamp>,
    departed_at: Option<Timestamp>,
}

#[derive(Clone)]
struct BossFight {
    started: Timestamp,
    ended: bool,
    damage: BTreeMap<String, f64>,
}

pub(crate) struct RosterContext<'a> {
    pub live: bool,
    pub totals: Option<&'a RunTotals>,
    pub dungeon: Option<&'a str>,
    pub settings: &'a PartySettings,
    pub catalog: Option<&'a PartyCatalog>,
    pub annotations: &'a [PartyAnnotation],
}

impl Default for PartyRoster {
    fn default() -> Self {
        Self {
            dirty: true,
            scope_id: None,
            run_id: None,
            map: None,
            area_kind: AreaKind::Unknown,
            members: BTreeMap::new(),
            bosses: BTreeMap::new(),
            pick: None,
        }
    }
}

impl Member {
    fn new(profile: PlayerProfileView, joined: Option<Timestamp>) -> Self {
        Self {
            profile,
            joined,
            present: None,
            out_of_view_since: None,
            departed_at: None,
        }
    }
}

fn profile_key(profile: &PlayerProfileView) -> Option<String> {
    profile
        .name
        .as_deref()
        .map(name_key)
        .filter(|key| !key.is_empty())
}

impl PartyRoster {
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn take_changed(&mut self) -> Option<crate::bridge::PartyChanged> {
        std::mem::take(&mut self.dirty).then(|| crate::bridge::PartyChanged {
            scope_id: self.scope_id.clone(),
        })
    }

    fn scope(&mut self, scope_id: &str, run_id: &Option<String>) {
        if self.scope_id.as_deref() != Some(scope_id) {
            self.dirty = true;
            self.members.clear();
            self.bosses.clear();
            self.pick = None;
            self.scope_id = Some(scope_id.into());
        }
        // Map metadata arrives separately in MapEntered and survives scope adoption.
        if self.run_id != *run_id {
            self.dirty = true;
            self.run_id.clone_from(run_id);
        }
    }

    fn accepts(&self, player_id: &str) -> bool {
        self.members.contains_key(player_id) || self.members.len() < PROFILE_LIMIT
    }

    pub fn observe_profile(&mut self, profile: PlayerProfileView) {
        self.scope(&profile.scope_id, &profile.run_id);
        if !self.accepts(&profile.player_id) {
            return;
        }
        self.dirty |= self
            .members
            .get(&profile.player_id)
            .is_none_or(|member| member.profile != profile);
        match self.members.entry(profile.player_id.clone()) {
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let member = entry.get_mut();
                member.joined = Some(profile.first_seen);
                member.profile = profile;
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                let joined = Some(profile.first_seen);
                entry.insert(Member::new(profile, joined));
            }
        }
    }

    pub fn observe_presence(&mut self, presence: &PlayerPresence) {
        self.dirty = true;
        self.scope(&presence.scope_id, &presence.run_id);
        if !self.accepts(&presence.player_id) {
            return;
        }
        let member = self
            .members
            .entry(presence.player_id.clone())
            .or_insert_with(|| {
                // Presence can precede the throttled profile. Keep its identity,
                // but do not infer a name, gear or a profile's first_seen.
                Member::new(
                    PlayerProfileView {
                        scope_id: presence.scope_id.clone(),
                        run_id: presence.run_id.clone(),
                        player_id: presence.player_id.clone(),
                        object_id: presence.object_id,
                        is_local: false,
                        name: None,
                        class_id: None,
                        level: None,
                        stars: None,
                        fame: None,
                        skin_id: None,
                        guild: None,
                        stats: vec![],
                        maxed: None,
                        maxed_stats: vec![],
                        equipment: vec![],
                        first_seen: presence.at,
                        last_seen: presence.at,
                    },
                    None,
                )
            });
        member.present = Some(presence.present);
        member.out_of_view_since = if presence.present {
            None
        } else {
            member.out_of_view_since.or(Some(presence.at))
        };
    }

    pub fn apply(&mut self, event: &EngineEvent) {
        if matches!(
            event,
            EngineEvent::CaptureStateChanged(_)
                | EngineEvent::MapEntered { .. }
                | EngineEvent::CharacterMerged { .. }
                | EngineEvent::InstanceEvent(_)
                | EngineEvent::FightStarted(_)
                | EngineEvent::FightUpdated(_)
                | EngineEvent::FightEnded(_)
                | EngineEvent::RunStarted(_)
                | EngineEvent::RunUpdated(_)
                | EngineEvent::RunEnded(_)
        ) {
            self.dirty = true;
        }
        match event {
            EngineEvent::MapEntered { name, kind, .. } => {
                *self = Self {
                    map: Some(name.clone()),
                    area_kind: kind.clone(),
                    ..Self::default()
                };
            }
            EngineEvent::PlayerObserved(profile) => {
                self.observe_profile(profile.clone());
            }
            EngineEvent::CharacterMerged {
                provisional_id,
                verified_id,
            } => {
                if let Some(mut member) = self.members.remove(provisional_id) {
                    member.profile.player_id.clone_from(verified_id);
                    if let Some(existing) = self.members.get_mut(verified_id) {
                        if member.profile.last_seen > existing.profile.last_seen {
                            existing.profile = member.profile;
                        }
                        existing.joined = existing.joined.into_iter().chain(member.joined).min();
                        existing.present = match (existing.present, member.present) {
                            (Some(true), _) | (_, Some(true)) => Some(true),
                            (present, other) => present.or(other),
                        };
                        existing.out_of_view_since = if existing.present == Some(true) {
                            None
                        } else {
                            existing
                                .out_of_view_since
                                .into_iter()
                                .chain(member.out_of_view_since)
                                .max()
                        };
                        existing.departed_at = existing
                            .departed_at
                            .into_iter()
                            .chain(member.departed_at)
                            .max();
                    } else {
                        self.members.insert(verified_id.clone(), member);
                    }
                }
                for fight in self.bosses.values_mut() {
                    if let Some(damage) = fight.damage.remove(provisional_id) {
                        *fight.damage.entry(verified_id.clone()).or_default() += damage;
                    }
                }
            }
            EngineEvent::InstanceEvent(InstanceEventView {
                kind: InstanceEventKind::PlayerLeft { object_id, .. },
                at,
                ..
            }) => {
                for member in self.members.values_mut() {
                    if member.profile.object_id == *object_id {
                        member.departed_at = Some(*at);
                    }
                }
            }
            EngineEvent::FightStarted(fight)
            | EngineEvent::FightUpdated(fight)
            | EngineEvent::FightEnded(fight)
                if fight.boss_type.is_some()
                    || matches!(
                        fight.target_kind,
                        Some(TargetKind::Boss | TargetKind::EventBoss)
                    ) =>
            {
                let boss = self
                    .bosses
                    .entry(fight.id.clone())
                    .or_insert_with(|| BossFight {
                        started: fight.started,
                        ended: false,
                        damage: BTreeMap::new(),
                    });
                boss.started = fight.started;
                boss.ended = fight.ended.is_some() || matches!(event, EngineEvent::FightEnded(_));
                let ids: BTreeSet<_> = fight
                    .players
                    .iter()
                    .map(|player| player.player_id.as_str())
                    .collect();
                boss.damage.retain(|id, _| ids.contains(id.as_str()));
                for player in &fight.players {
                    if let Some(damage) = boss.damage.get_mut(&player.player_id) {
                        *damage = player.damage;
                    } else {
                        boss.damage.insert(player.player_id.clone(), player.damage);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn pick(&mut self, preset_id: Option<String>) {
        if self.pick != preset_id {
            self.pick = preset_id;
            self.dirty = true;
        }
    }

    pub fn keys(&self) -> Vec<String> {
        self.members
            .values()
            .filter_map(|member| profile_key(&member.profile))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn view(&self, ctx: &RosterContext<'_>) -> PartyView {
        let resolved = resolve_preset(ctx.settings, self.pick.as_deref(), ctx.dungeon);
        let totals: BTreeMap<_, _> = ctx
            .totals
            .into_iter()
            .flat_map(|totals| &totals.players)
            .map(|player| (player.player_id.as_str(), player))
            .collect();
        let annotations: BTreeMap<_, _> = ctx
            .annotations
            .iter()
            .map(|annotation| (annotation.key.as_str(), annotation))
            .collect();
        let mut boss_damage = BTreeMap::<&str, f64>::new();
        for fight in self.bosses.values() {
            for (id, damage) in &fight.damage {
                *boss_damage.entry(id.as_str()).or_default() += damage;
            }
        }
        let boss_total: f64 = boss_damage.values().sum();
        let last_boss_started = self.bosses.values().map(|fight| fight.started).max();
        let boss_fight_ended = self.bosses.values().any(|fight| fight.ended);
        // The boolean separates nameless player IDs from named account keys.
        let mut groups = BTreeMap::<(bool, String), Vec<&Member>>::new();
        for (id, member) in &self.members {
            let key =
                profile_key(&member.profile).map_or_else(|| (false, id.clone()), |key| (true, key));
            groups.entry(key).or_default().push(member);
        }
        let mut members = Vec::with_capacity(groups.len());
        for group in groups.values() {
            let latest = group
                .iter()
                .max_by_key(|member| member.profile.last_seen)
                .expect("nonempty group");
            let profile = &latest.profile;
            let key = profile_key(profile);
            let annotation = key.as_deref().and_then(|key| annotations.get(key));
            let mut damage = None;
            let mut share = None;
            let mut boss = 0.0;
            let mut missing_total = false;
            for member in group {
                let id = member.profile.player_id.as_str();
                if let Some(total) = totals.get(id) {
                    *damage.get_or_insert(0.0) += total.damage;
                    if let Some(value) = total.share {
                        *share.get_or_insert(0.0) += value;
                    }
                } else {
                    missing_total = true;
                }
                boss += boss_damage.get(id).copied().unwrap_or(0.0);
            }
            let truncated = missing_total
                && ctx.totals.is_some_and(|totals| {
                    totals.players.len() >= maxed_engine::RUN_TOTAL_PLAYER_LIMIT
                });
            if truncated {
                damage = None;
                share = None;
            }
            let boss_share = (boss_total > 0.0).then_some(boss / boss_total);
            let present = if group.iter().any(|member| member.present == Some(true)) {
                Some(true)
            } else if group.iter().any(|member| member.present == Some(false)) {
                Some(false)
            } else {
                None
            };
            let departed_at = group.iter().filter_map(|member| member.departed_at).max();
            let flags = contribution_flags(&FlagInput {
                in_dungeon: self.area_kind == AreaKind::Dungeon,
                joined: if truncated {
                    None
                } else {
                    group.iter().filter_map(|member| member.joined).min()
                },
                departed: departed_at.is_some(),
                damage: damage.unwrap_or(0.0),
                share,
                boss_share,
                last_boss_started,
                boss_fight_ended,
            });
            let parse = resolved.as_ref().and_then(|(preset, _)| {
                ctx.catalog
                    .map(|catalog| evaluate(preset, profile, |id| catalog.tier(id)))
            });
            members.push(PartyMember {
                player_id: profile.player_id.clone(),
                name_key: key,
                name: profile.name.clone(),
                class_id: profile.class_id,
                level: profile.level,
                stars: profile.stars,
                guild: profile.guild.clone(),
                maxed: profile.maxed,
                equipment: profile.equipment.clone(),
                is_local: group.iter().any(|member| member.profile.is_local),
                present,
                out_of_view_since: if present == Some(true) {
                    None
                } else {
                    group
                        .iter()
                        .filter_map(|member| member.out_of_view_since)
                        .max()
                },
                departed_at,
                parse,
                damage,
                share,
                boss_share,
                flags,
                mark: annotation.and_then(|annotation| annotation.mark.clone()),
                runs_together: annotation.map_or(0, |annotation| annotation.runs_together),
            });
        }
        members.sort_by(|a, b| {
            parse_order(a)
                .cmp(&parse_order(b))
                .then_with(|| {
                    b.damage
                        .partial_cmp(&a.damage)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.player_id.cmp(&b.player_id))
        });
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
        for member in &members {
            counts.in_view += u32::from(member.present == Some(true));
            match member.parse.as_ref().map(|parse| &parse.result) {
                Some(ParseResult::Pass) => counts.pass += 1,
                Some(ParseResult::Fail) => counts.fail += 1,
                Some(ParseResult::Check) => counts.check += 1,
                Some(ParseResult::NotSeen) => counts.not_seen += 1,
                None => {}
            }
            counts.no_damage_seen +=
                u32::from(member.flags.contains(&ContributionFlag::NoDamageSeen));
            counts.left_early += u32::from(member.flags.contains(&ContributionFlag::LeftEarly));
            if let Some(class) = member.class_id {
                *classes.entry(class).or_default() += 1;
            }
        }
        let mut classes: Vec<_> = classes
            .into_iter()
            .map(|(class_id, count)| ClassCount { class_id, count })
            .collect();
        classes.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.class_id.cmp(&b.class_id))
        });
        PartyView {
            scope_id: self.scope_id.clone(),
            run_id: self.run_id.clone(),
            map: self.map.clone(),
            area_kind: self.area_kind.clone(),
            live: ctx.live,
            preset: resolved.map(|(preset, source)| ResolvedPreset {
                id: preset.id.clone(),
                name: preset.name.clone(),
                source,
            }),
            counts,
            classes,
            members,
            assets_available: ctx.catalog.is_some(),
            gear_recorded: true,
        }
    }
}

impl super::CaptureRuntime {
    fn party_settings(&self) -> Result<PartySettings, String> {
        let settings = self
            .shared
            .alerts
            .lock()
            .map_err(|_| "Party settings unavailable")?
            .settings
            .clone()
            .ok_or("Party settings unavailable")?;
        settings.get().map(|settings| settings.party)
    }

    pub async fn get_party_live(self: &Arc<Self>) -> Result<HistoryReply<PartyView>, String> {
        let runtime = self.clone();
        // Lifecycle transitions hold session/alerts while retiring history jobs.
        // Finish this blocking prelude before acquiring a history read lease.
        let (settings, catalog, snapshot, roster, live) =
            tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
                let settings = runtime.party_settings()?;
                let catalog = runtime
                    .shared
                    .views
                    .party_catalog
                    .lock()
                    .map_err(|_| "Party unavailable")?
                    .clone();
                let snapshot = runtime.snapshot();
                let roster = runtime
                    .shared
                    .views
                    .party
                    .lock()
                    .map_err(|_| "Party unavailable")?
                    .clone();
                let live = runtime.is_running()
                    && runtime.status().state == CaptureState::Live
                    && runtime
                        .shared
                        .alerts
                        .lock()
                        .map_err(|_| "Party unavailable")?
                        .namespace
                        == super::Namespace::Live;
                Ok((settings, catalog, snapshot, roster, live))
            })
            .await
            .map_err(|_| "Party worker failed".to_string())??;
        let keys = roster.keys();
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::PartyAnnotations(keys.clone()),
            move |job| {
                let annotations = runtime
                    .read_history(job, Vec::new, |reader| reader.party_annotations(&keys))?;
                Ok(roster.view(&RosterContext {
                    live,
                    totals: snapshot.run_totals.as_ref(),
                    dungeon: snapshot
                        .run
                        .as_ref()
                        .map(|run| run.dungeon.as_str())
                        .or(snapshot.map.as_deref()),
                    settings: &settings,
                    catalog: catalog.as_deref(),
                    annotations: &annotations,
                }))
            },
        )
        .await
    }

    pub async fn get_party_run(
        self: &Arc<Self>,
        run_id: String,
    ) -> Result<HistoryReply<Option<PartyView>>, String> {
        let runtime = self.clone();
        let (settings, catalog) =
            tauri::async_runtime::spawn_blocking(move || -> Result<_, String> {
                let settings = runtime.party_settings()?;
                let catalog = runtime
                    .shared
                    .views
                    .party_catalog
                    .lock()
                    .map_err(|_| "Party unavailable")?
                    .clone();
                Ok((settings, catalog))
            })
            .await
            .map_err(|_| "Party worker failed".to_string())??;
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::PartyRun(run_id.clone()),
            move |job| {
                let mut view =
                    runtime.read_history(job, || None, |reader| reader.get_party_run(&run_id))?;
                if let Some(view) = &mut view {
                    parse_saved_party(view, &settings, catalog.as_deref());
                }
                Ok(view)
            },
        )
        .await
    }

    pub async fn get_party_players(
        self: &Arc<Self>,
        query: String,
        filter: PartyPlayerFilter,
        sort: PartyPlayerSort,
        cursor: Option<PartyPlayersCursor>,
    ) -> Result<HistoryReply<PartyPlayersPage>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::PartyPlayers(query.clone(), filter.clone(), sort.clone(), cursor.clone()),
            move |job| {
                runtime.read_history(
                    job,
                    || PartyPlayersPage {
                        rows: vec![],
                        next: None,
                    },
                    |reader| reader.list_party_players(&query, filter, sort, cursor.as_ref()),
                )
            },
        )
        .await
    }

    pub async fn get_party_player(
        self: &Arc<Self>,
        key: String,
        cursor: Option<PartyRunsCursor>,
    ) -> Result<HistoryReply<Option<PartyPlayerView>>, String> {
        let runtime = self.clone();
        self.submit_history(
            HistoryCaller::MainWindow,
            HistoryKey::PartyPlayer(key.clone(), cursor.clone()),
            move |job| {
                runtime.read_history(
                    job,
                    || None,
                    |reader| reader.get_party_player(&key, cursor.as_ref()),
                )
            },
        )
        .await
    }

    pub fn set_party_preset_pick(&self, preset_id: Option<String>) -> Result<(), String> {
        let settings = self.party_settings()?;
        if preset_id
            .as_ref()
            .is_some_and(|id| !settings.presets.iter().any(|preset| &preset.id == id))
        {
            return Err("Party preset is unavailable.".into());
        }
        self.shared
            .views
            .party
            .lock()
            .map_err(|_| "Party unavailable")?
            .pick(preset_id);
        self.publish_party_changed()
    }

    pub(super) fn publish_party_changed(&self) -> Result<(), String> {
        let publisher = self
            .publisher
            .lock()
            .map_err(|_| "Party unavailable")?
            .clone();
        if let Some(publisher) = publisher {
            let notice = self
                .shared
                .views
                .party
                .lock()
                .map_err(|_| "Party unavailable")?
                .take_changed();
            if let Some(notice) = notice {
                publisher.publish(crate::bridge::Publication::PartyChanged(notice));
            }
        }
        Ok(())
    }

    pub fn set_party_mark(
        &self,
        key: String,
        name: String,
        mark: Option<PartyMark>,
        note: Option<String>,
    ) -> Result<(), String> {
        self.write_party_with_timeout(
            PartyWrite::Mark {
                key,
                name,
                mark,
                note,
            },
            std::time::Duration::from_secs(60),
        )
    }

    pub fn clear_party_history(&self, options: ClearPartyHistory) -> Result<(), String> {
        self.write_party_with_timeout(
            PartyWrite::Clear(options),
            std::time::Duration::from_secs(60),
        )
    }

    pub(super) fn write_party_with_timeout(
        &self,
        operation: PartyWrite,
        timeout: std::time::Duration,
    ) -> Result<(), String> {
        let alerts = self.shared.alerts.lock().map_err(|_| WRITE_FAILED)?;
        if alerts.namespace != super::Namespace::Live {
            return Err("Party history cannot be changed during preview or replay.".into());
        }
        drop(alerts);
        let path = self.shared.home.live_database().ok_or(WRITE_FAILED)?;
        let root = path.parent().ok_or(WRITE_FAILED)?;
        match super::storage::namespace_lease(root, super::Namespace::Live) {
            Ok(_lease) => {
                if let Some(request) = self
                    .shared
                    .views
                    .party_write
                    .lock()
                    .map_err(|_| WRITE_FAILED)?
                    .take()
                {
                    let _ = request.reply.send(Err(WRITE_FAILED.into()));
                }
                maxed_store::party_write_file(&path, &operation).map_err(|_| WRITE_FAILED)?;
            }
            Err(_) => {
                if !self.is_running() {
                    return Err(WRITE_FAILED.into());
                }
                let (send, recv) = crossbeam_channel::bounded(1);
                let mut pending = self
                    .shared
                    .views
                    .party_write
                    .lock()
                    .map_err(|_| WRITE_FAILED)?;
                if pending.is_some() {
                    return Err(WRITE_FAILED.into());
                }
                *pending = Some(PartyWriteRequest {
                    operation,
                    reply: send.clone(),
                });
                drop(pending);
                match recv.recv_timeout(timeout) {
                    Ok(result) => result?,
                    Err(_) => {
                        let mut pending = self
                            .shared
                            .views
                            .party_write
                            .lock()
                            .unwrap_or_else(|p| p.into_inner());
                        if pending
                            .as_ref()
                            .is_some_and(|request| request.reply.same_channel(&send))
                        {
                            pending.take();
                            return Err(WRITE_FAILED.into());
                        }
                        drop(pending);
                        // Once claimed, return its actual result, as chat does.
                        drop(send);
                        recv.recv().map_err(|_| WRITE_FAILED)??;
                    }
                }
            }
        }
        self.shared
            .views
            .party
            .lock()
            .map_err(|_| WRITE_FAILED)?
            .mark_dirty();
        self.publish_party_changed()
    }

    pub fn update_party_settings(&self, settings: PartySettings) -> Result<(), String> {
        let alerts = self
            .shared
            .alerts
            .lock()
            .map_err(|_| "Party settings unavailable")?;
        if alerts.namespace != super::Namespace::Live {
            return Err("Party settings cannot be changed during preview or replay.".into());
        }
        alerts
            .settings
            .as_ref()
            .ok_or("Party settings unavailable")?
            .put_party(settings)?;
        drop(alerts);
        self.shared
            .views
            .party
            .lock()
            .map_err(|_| "Party unavailable")?
            .mark_dirty();
        self.publish_party_changed()
    }
}

fn parse_saved_party(
    view: &mut PartyView,
    settings: &PartySettings,
    catalog: Option<&PartyCatalog>,
) {
    let resolved = resolve_preset(settings, None, view.map.as_deref());
    view.assets_available = catalog.is_some();
    view.preset = resolved.as_ref().map(|(preset, source)| ResolvedPreset {
        id: preset.id.clone(),
        name: preset.name.clone(),
        source: source.clone(),
    });
    view.counts.pass = 0;
    view.counts.fail = 0;
    view.counts.check = 0;
    view.counts.not_seen = 0;
    for member in &mut view.members {
        member.parse = resolved
            .as_ref()
            .filter(|_| view.gear_recorded && catalog.is_some())
            .map(|(preset, _)| {
                let profile = PlayerProfileView {
                    scope_id: view.scope_id.clone().unwrap_or_default(),
                    run_id: view.run_id.clone(),
                    player_id: member.player_id.clone(),
                    object_id: 0,
                    is_local: member.is_local,
                    name: member.name.clone(),
                    class_id: member.class_id,
                    level: member.level,
                    stars: member.stars,
                    fame: None,
                    skin_id: None,
                    guild: member.guild.clone(),
                    stats: vec![],
                    maxed: member.maxed,
                    maxed_stats: vec![],
                    equipment: member.equipment.clone(),
                    first_seen: Timestamp(0),
                    last_seen: Timestamp(0),
                };
                evaluate(preset, &profile, |item| {
                    catalog.map_or(ItemTier::Unknown, |catalog| catalog.tier(item))
                })
            });
        match member.parse.as_ref().map(|parse| &parse.result) {
            Some(ParseResult::Pass) => view.counts.pass += 1,
            Some(ParseResult::Fail) => view.counts.fail += 1,
            Some(ParseResult::Check) => view.counts.check += 1,
            Some(ParseResult::NotSeen) => view.counts.not_seen += 1,
            None => {}
        }
    }
}

fn parse_order(member: &PartyMember) -> u8 {
    match member.parse.as_ref().map(|parse| &parse.result) {
        Some(ParseResult::Fail) => 0,
        Some(ParseResult::Check) => 1,
        Some(ParseResult::NotSeen) => 2,
        Some(ParseResult::Pass) | None => 3,
    }
}

pub(crate) fn resolve_preset<'a>(
    settings: &'a PartySettings,
    pick: Option<&str>,
    dungeon: Option<&str>,
) -> Option<(&'a RequirementPreset, PresetSource)> {
    let by_id = |id: &str| settings.presets.iter().find(|preset| preset.id == id);
    if let Some(preset) = pick.and_then(by_id) {
        return Some((preset, PresetSource::Picked));
    }
    if let Some(preset) = dungeon.and_then(|dungeon| {
        settings
            .presets
            .iter()
            .find(|preset| preset.auto_apply.iter().any(|name| name == dungeon))
    }) {
        return Some((preset, PresetSource::Auto));
    }
    settings
        .default_preset_id
        .as_deref()
        .and_then(by_id)
        .map(|preset| (preset, PresetSource::Default))
}

#[cfg(test)]
mod realmeye_tests {
    use super::*;

    #[test]
    fn realmeye_uses_system_explorer_and_a_single_validated_url_argument() {
        let url = realmeye_url("SynthAlpha").unwrap();
        for (root, expected) in [
            (None, r"C:\Windows\explorer.exe"),
            (Some("".into()), r"C:\Windows\explorer.exe"),
            (Some("relative".into()), r"C:\Windows\explorer.exe"),
            (Some(r"C:relative".into()), r"C:\Windows\explorer.exe"),
            (Some(r"\Windows".into()), r"C:\Windows\explorer.exe"),
            (
                Some(r"D:\Synthetic Windows".into()),
                r"D:\Synthetic Windows\explorer.exe",
            ),
        ] {
            let command = realmeye_command(&url, root);
            assert_eq!(command.get_program(), std::ffi::OsStr::new(expected));
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                vec![std::ffi::OsStr::new(&url)]
            );
        }
    }

    #[test]
    fn realmeye_spawn_failure_is_a_user_facing_error() {
        let root = tempfile::tempdir().unwrap();
        let command = realmeye_command(
            &realmeye_url("SynthAlpha").unwrap(),
            Some(root.path().as_os_str().into()),
        );
        assert_eq!(
            spawn_realmeye(command).unwrap_err(),
            "RealmEye could not be opened in your browser."
        );
        assert_eq!(
            open_realmeye("../invalid").unwrap_err(),
            "RealmEye requires a name of 1 to 10 letters."
        );
    }
}
