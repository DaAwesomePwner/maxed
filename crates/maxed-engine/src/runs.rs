use crate::{clock, CompletionCounts, Engine};
use maxed_protocol::defs::MapInfo;
use maxed_types::*;

pub(crate) struct Run {
    pub view: RunView,
    pub baseline: Option<CompletionCounts>,
    pub index: Option<u16>,
    pub death_create: Option<u64>,
    pub fame_samples: Option<(u64, i32, Option<i32>)>,
}
pub(crate) struct Preceding {
    pub run: Run,
    pub last_seen: Timestamp,
    pub next_area: u64,
    pub valid: bool,
}

impl Engine {
    pub(crate) fn enter_map(&mut self, p: MapInfo, events: &mut Vec<EngineEvent>) {
        let carry = self
            .local
            .as_ref()
            .is_some_and(|l| l.area.is_none() || !l.materialized);
        let pending = if carry { self.local.take() } else { None };
        let buffered_entity = pending
            .as_ref()
            .filter(|l| l.area.is_none())
            .and_then(|l| self.entities.remove(&l.object).map(|e| (l.object, e)));
        self.close_map(FightEnd::MapChanged, true, events);
        self.clear_world();
        self.issues.clear();
        self.map_ordinal += 1;
        if let Some(previous) = &mut self.preceding {
            if previous.next_area != self.map_ordinal {
                previous.valid = false;
            }
        }
        self.local = pending;
        if let Some((id, mut entity)) = buffered_entity {
            entity.generation = self.entity_epoch;
            self.entities.insert(id, entity);
        }
        if let Some(local) = &mut self.local {
            local.area = Some(self.map_ordinal);
        }
        // ParseDungeon.canonicalMapName: canonical name wins; display is alias-only fallback.
        let def = self
            .catalog
            .dungeons
            .iter()
            .find(|d| d.name == p.name)
            .or_else(|| {
                self.catalog
                    .dungeons
                    .iter()
                    .find(|d| d.aliases.contains(&p.name))
            })
            .or_else(|| {
                self.catalog
                    .dungeons
                    .iter()
                    .find(|d| d.aliases.contains(&p.displayName))
            })
            .cloned();
        self.area_kind = def
            .as_ref()
            .map(|d| d.kind.clone())
            .unwrap_or(AreaKind::Unknown);
        let realm_instance_key = (self.area_kind == AreaKind::Realm)
            .then(|| serde_json::to_string(&(&p.name, &p.realmName, p.seed, p.gameOpenedTime)).ok())
            .flatten();
        self.instance.enter(&p, &self.area_kind, self.now);
        self.map = Some(def.as_ref().map(|d| d.name.clone()).unwrap_or(p.name));
        self.rng = Some(crate::math::Rng(i64::from(p.seed)));
        self.rng_valid = true;
        events.push(EngineEvent::MapEntered {
            realm_instance_key,
            at: clock::millis(self.now),
            name: self.map.clone().unwrap_or_default(),
            kind: self.area_kind.clone(),
        });
        if self.area_kind == AreaKind::Dungeon {
            let view = RunView {
                context: self.instance.context.as_ref().map(Into::into),
                id: self.id("run", self.map_ordinal),
                session_id: self.session_id(),
                character_id: None,
                dungeon: self.map.clone().unwrap_or_default(),
                area_kind: AreaKind::Dungeon,
                started: clock::millis(self.now),
                ended: None,
                completed_at: None,
                died_at: None,
                outcome: None,
                evidence: vec![],
                party_size: None,
                party_size_estimated: true,
                complete_capture: true,
                issues: vec![],
            };
            events.push(EngineEvent::RunStarted(view.clone()));
            self.run = Some(Run {
                view,
                baseline: None,
                index: def.and_then(|d| d.completion_index),
                death_create: None,
                fame_samples: None,
            });
        }
        self.refresh_character(events);
    }
    pub(crate) fn update_party(&mut self, events: &mut Vec<EngineEvent>) {
        let count = self
            .entities
            .iter()
            .filter(|(id, e)| e.present && self.is_player(**id))
            .count() as u32;
        if let Some(r) = &mut self.run {
            if r.view.ended.is_none() && count > 0 && r.view.party_size.is_none_or(|n| count > n) {
                r.view.party_size = Some(count);
                events.push(EngineEvent::RunUpdated(r.view.clone()));
            }
        }
    }
    pub(crate) fn complete(&mut self, evidence: Evidence, events: &mut Vec<EngineEvent>) {
        let Some(r) = &mut self.run else {
            return;
        };
        // D-outcome: preserve facts and promote priority, never overwrite a death-first outcome.
        // A DEATH freezes identity. Later completion in that same visit remains a
        // fact, while a new CREATE_SUCCESS makes further signals unrelated.
        if r.death_create.is_some_and(|n| n != self.create_number)
            || r.view.ended.is_some() && r.view.died_at.is_none()
        {
            return;
        }
        if r.view.evidence.iter().any(|e| e.evidence == evidence) {
            return;
        }
        r.view.evidence.push(EvidenceAt {
            at: clock::millis(self.now),
            evidence,
        });
        if r.view.completed_at.is_none() {
            r.view.completed_at = Some(clock::millis(self.now));
        }
        if r.view.died_at.is_none() {
            r.view.outcome = Some(Outcome::Cleared);
        }
        order_evidence(&mut r.view);
        events.push(EngineEvent::RunUpdated(r.view.clone()));
    }
    pub(crate) fn die(&mut self, events: &mut Vec<EngineEvent>) {
        if self.death_seen
            || self.character.as_ref().is_some_and(|c| c.died_at.is_some())
            || self.run.as_ref().is_some_and(|r| r.view.died_at.is_some())
        {
            return;
        }
        self.death_seen = true;
        let at = clock::millis(self.now);
        if let Some(c) = &mut self.character {
            c.died_at = Some(at);
            c.observed_at = at;
            events.push(EngineEvent::CharacterUpdated(c.clone()));
        }
        let character_id = self.character.as_ref().map(|c| c.id.clone());
        let run_id = self.run.as_ref().map(|r| r.view.id.clone());
        events.push(EngineEvent::PlayerDied {
            at,
            character_id,
            run_id,
        });
        if let Some(r) = &mut self.run {
            r.death_create = Some(self.create_number);
            r.view.died_at = Some(at);
            r.view.ended = Some(at);
            if r.view.completed_at.is_none() {
                r.view.outcome = Some(Outcome::Died);
            }
            r.view.evidence.push(EvidenceAt {
                at,
                evidence: Evidence::DeathPacket,
            });
            events.push(EngineEvent::RunEnded(r.view.clone()));
        }
        self.notices.departures.retain(|_, (_, row)| !row.is_local);
        self.expire_departures(events);
        for (key, f) in &mut self.fights {
            if let Some(p) = f.base.players.iter_mut().find(|p| p.is_local) {
                p.deaths = p.deaths.saturating_add(1);
                f.changed();
                self.dirty_fights.insert(*key);
            }
        }
        if let Some(p) = &mut self.preceding {
            p.valid = false;
        }
    }
    pub(crate) fn end_run(
        &mut self,
        clean: bool,
        reason: Option<&FightEnd>,
        events: &mut Vec<EngineEvent>,
    ) {
        let clean = clean && !self.map_interrupted;
        if let Some(r) = &mut self.run {
            if r.view.ended.is_some() {
                return;
            }
            let at = clock::millis(self.now);
            r.view.ended = Some(at);
            if clean {
                r.view.evidence.push(EvidenceAt {
                    at,
                    evidence: Evidence::ObservedExit,
                });
            }
            if reason == Some(&FightEnd::AppClosed) {
                r.view.evidence.push(EvidenceAt {
                    at,
                    evidence: Evidence::AppClosed,
                });
            }
            if !clean {
                r.view.complete_capture = false;
            }
            if r.view.outcome.is_none() {
                r.view.outcome = Some(if clean && r.view.complete_capture {
                    Outcome::Left
                } else {
                    Outcome::Unknown
                });
            }
            events.push(EngineEvent::RunEnded(r.view.clone()));
        }
        self.expire_departures(events);
    }
    pub(crate) fn close_map(
        &mut self,
        reason: FightEnd,
        clean: bool,
        events: &mut Vec<EngineEvent>,
    ) {
        self.end_run(clean, Some(&reason), events);
        let targets: Vec<_> = self.fights.keys().copied().collect();
        for target in targets {
            self.retire_fight(target, reason.clone(), events);
        }
        self.flush_bags(events);
        if let Some(run) = self.run.take() {
            self.preceding = Some(Preceding {
                last_seen: self.map_last_traffic,
                next_area: self.map_ordinal + 1,
                valid: clean && run.view.complete_capture && run.view.died_at.is_none(),
                run,
            });
        }
    }
    pub(crate) fn accept_baseline(&mut self, events: &mut Vec<EngineEvent>) {
        let Some(c) = self.character.as_ref().filter(|c| c.identity_verified) else {
            return;
        };
        let Some(local) = self.local.as_ref() else {
            return;
        };
        let Some(counts) = local.counts.clone() else {
            return;
        };
        if self.map.is_none() || local.area != Some(self.map_ordinal) {
            return;
        }
        if let Some(p) = &mut self.preceding {
            let delay = self.now.0.saturating_sub(p.last_seen.0);
            let entry = self
                .run
                .as_ref()
                .map(|r| r.view.started)
                .unwrap_or(clock::millis(local.received));
            if p.valid
                && p.next_area == self.map_ordinal
                && p.run.view.character_id.as_ref() == Some(&c.id)
                && entry.0.saturating_sub(p.last_seen.0) <= 30_000_000
                && delay <= 30_000_000
            {
                if let Some(index) = p.run.index.map(usize::from) {
                    let before = p
                        .run
                        .baseline
                        .as_ref()
                        .and_then(|b| b.by_index.get(index))
                        .copied()
                        .flatten();
                    let after = counts.by_index.get(index).copied().flatten();
                    if let (Some(before), Some(after)) = (before, after) {
                        if crate::valid_counter_step(before, after, delay) {
                            // D-counter: receipt AND next-area entry must meet the deadline.
                            let v = &mut p.run.view;
                            v.evidence.push(EvidenceAt {
                                at: clock::millis(self.now),
                                evidence: Evidence::CompletionCounter {
                                    before,
                                    after,
                                    delay_micros: delay as u32,
                                },
                            });
                            if v.completed_at.is_none() {
                                v.completed_at = Some(clock::millis(self.now));
                            }
                            v.outcome = Some(Outcome::Cleared);
                            order_evidence(v);
                            events.push(EngineEvent::RunUpdated(v.clone()));
                        }
                    }
                }
            }
            p.valid = false;
        }
        if let Some(r) = &mut self.run {
            if r.baseline.is_none() {
                r.baseline = Some(counts);
            }
        }
    }
}

fn order_evidence(view: &mut RunView) {
    view.evidence.sort_by_key(|e| match e.evidence {
        Evidence::Victory => 0,
        Evidence::CompletionDialogue { .. } => 1,
        Evidence::CompletionCounter { .. } => 2,
        _ => 3,
    });
}
