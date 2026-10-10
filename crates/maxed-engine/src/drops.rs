use crate::{parse_enchant, Engine};
use maxed_types::*;

pub(crate) struct Bag {
    pub view: DropView,
    pub born_tick: u64,
    pub observed_at: Timestamp,
    pub pos: (f32, f32),
}

// Recognized bag IDs, including excluded bags.
pub(crate) fn known_bag(t: u16) -> bool {
    matches!(t,1280|1283|1286..=1289|1291|1292|1294..=1296|1708..=1710|1722..=1728)
}
fn captured_bag(t: u16) -> bool {
    known_bag(t) && !matches!(t, 1280 | 1283 | 1286 | 1709 | 1710)
}

impl Engine {
    pub(crate) fn observe_bag(&mut self, id: i32, events: &mut Vec<EngineEvent>) {
        let Some(e) = self.entities.get(&id) else {
            return;
        };
        let Some(ty) = e.object_type else {
            return;
        };
        if e.bag_retired {
            return;
        }
        let def = self.catalog.bags.iter().find(|b| b.object_type == ty);
        if matches!(self.area_kind, AreaKind::Nexus | AreaKind::OtherHub)
            || matches!(ty, 1280 | 1283 | 1286 | 1709 | 1710)
            || def.is_some_and(|b| matches!(b.base_type, 1280 | 1286))
            || !def.map(|b| b.capture).unwrap_or_else(|| captured_bag(ty))
        {
            return;
        }
        let key = (id, e.generation);
        let pos = e.pos;
        if !self.drops.contains_key(&key) {
            if self.drops.len() >= self.config.max_active_bags {
                if let Some(old) = self
                    .drops
                    .iter()
                    .min_by_key(|(_, b)| b.view.at)
                    .map(|(k, _)| *k)
                {
                    if let Some(entity) = self
                        .entities
                        .get_mut(&old.0)
                        .filter(|e| e.generation == old.1)
                    {
                        entity.bag_retired = true;
                    }
                    if let Some(mut b) = self.drops.remove(&old) {
                        b.view.issues.push(CoverageIssue::ResourceLimit);
                        events.push(EngineEvent::DropUpdated(b.view));
                    }
                    self.release_entity(old);
                }
                self.mark(CoverageIssue::ResourceLimit, events);
            }
            let white = ty == 1292
                || ty == 1296
                || self
                    .catalog
                    .bags
                    .iter()
                    .find(|b| b.object_type == ty)
                    .is_some_and(|b| b.is_white);
            let rank = if white {
                Some(255)
            } else {
                self.catalog
                    .bags
                    .iter()
                    .find(|b| b.object_type == ty)
                    .and_then(|b| b.rank)
                    .filter(|r| *r < 255)
            };
            let view = DropView {
                capture_scope: Some(CaptureScope::RealmShark),
                id: format!(
                    "{}:bag:{}:{id}:{}",
                    self.config.stream_id, self.map_ordinal, key.1
                ),
                session_id: self.session_id(),
                run_id: self.run.as_ref().map(|r| r.view.id.clone()),
                at: crate::clock::millis(self.now),
                bag_type: ty,
                items: vec![],
                enchant: None,
                contents: vec![],
                source_enemy: None,
                source_estimated: false,
                source_reason: Some("source-not-observed".into()),
                contents_known: false,
                bag_rank: rank,
                issues: {
                    let mut issues: Vec<_> = self
                        .recent_issues
                        .iter()
                        .filter(|(_, at)| {
                            self.now.0.saturating_sub(at.0) <= crate::engine::DROP_COVERAGE_WINDOW
                        })
                        .map(|(issue, _)| issue.clone())
                        .collect();
                    if !issues.contains(&CoverageIssue::MissingStats) {
                        issues.push(CoverageIssue::MissingStats);
                    }
                    issues
                },
            };
            // D-loot: immediate identification, then same-ID enrichment rather than delayed publication.
            events.push(EngineEvent::BagDropped(view.clone()));
            self.drops.insert(
                key,
                Bag {
                    view,
                    born_tick: self.loot_tick,
                    observed_at: self.now,
                    pos,
                },
            );
        }
        self.protect_entity(id);
        self.enrich_contents(key, events);
    }
    fn enrich_contents(&mut self, key: (i32, u32), events: &mut Vec<EngineEvent>) {
        let Some(e) = self.entities.get(&key.0).filter(|e| e.generation == key.1) else {
            return;
        };
        let Some(b) = self.drops.get_mut(&key) else {
            return;
        };
        let mut next = b.view.clone();
        next.contents_known = (8..16).all(|s| {
            e.stat(s)
                .is_some_and(|n| n == -1 || u16::try_from(n).is_ok())
        });
        for slot in 0..8 {
            let Some(value) = e.stat(8 + slot) else {
                continue;
            };
            // Invalid/absent stats do not establish removal.
            if value != -1 && u16::try_from(value).is_err() {
                continue;
            }
            let current = u16::try_from(value).ok();
            for item in next.contents.iter_mut().filter(|item| item.slot == slot) {
                if Some(item.item_type) != current && item.taken_observed_at.is_none() {
                    item.taken_observed_at = Some(crate::clock::millis(self.now));
                }
            }
            if let Some(item_type) = current {
                let enchant = e
                    .enchants
                    .get(usize::from(slot))
                    .cloned()
                    .unwrap_or_else(|| parse_enchant(None));
                if let Some(item) = next
                    .contents
                    .iter_mut()
                    .find(|item| item.slot == slot && item.item_type == item_type)
                {
                    item.enchant = enchant;
                } else {
                    next.contents.push(DropItem {
                        slot,
                        item_type,
                        quantity: 1,
                        enchant,
                        taken_observed_at: None,
                    });
                }
            }
        }
        next.items = next.contents.iter().map(|c| c.item_type).collect();
        if next.contents_known {
            next.issues.retain(|i| *i != CoverageIssue::MissingStats);
        }
        let summaries: Vec<_> = next
            .contents
            .iter()
            .filter(|c| {
                matches!(
                    c.enchant.state,
                    EnchantState::Recorded | EnchantState::RecordedEmpty
                )
            })
            .collect();
        next.enchant = if summaries.len() == 1 {
            summaries[0]
                .enchant
                .slots
                .and_then(|n| {
                    ["Common", "Uncommon", "Rare", "Legendary", "Divine"].get(usize::from(n))
                })
                .map(|s| s.to_string())
        } else {
            None
        };
        if next != b.view {
            b.view = next.clone();
            events.push(EngineEvent::DropUpdated(next));
        }
    }
    pub(crate) fn enrich_loot(&mut self, events: &mut Vec<EngineEvent>) {
        self.loot_tick += 1;
        let mut processed = false;
        for b in self
            .drops
            .values_mut()
            .filter(|b| self.loot_tick == b.born_tick + 2)
        {
            processed = true;
            // TomatoData.LootAttributionManager.findAttributionForBag: closest
            // removed hit target, then exact dialogue window. These are heuristics.
            let mut candidates: Vec<_> = self
                .removed_enemies
                .iter()
                .filter_map(|(ty, p)| {
                    let d = f64::from(b.pos.0 - p.0).powi(2) + f64::from(b.pos.1 - p.1).powi(2);
                    d.is_finite().then_some((*ty, d))
                })
                .collect();
            candidates.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let old = b.view.clone();
            if let Some((ty, d)) = candidates.first() {
                if candidates.get(1).is_some_and(|(_, next)| next == d) {
                    b.view.source_reason = Some("ambiguous-nearby-removed-targets".into());
                } else {
                    b.view.source_enemy = Some(*ty);
                    b.view.source_estimated = true;
                    b.view.source_reason = Some("nearest-removed-hit-target".into());
                }
            } else if let Some((ty, remaining, rule)) = self.loot_window {
                if remaining > 0 {
                    b.view.source_enemy = Some(ty);
                    b.view.source_estimated = true;
                    b.view.source_reason = Some(rule.into());
                }
            }
            if b.view != old {
                events.push(EngineEvent::DropUpdated(b.view.clone()));
            }
        }
        self.attribute_kill_loot(events);
        // TomatoData.lootTick clears killedEntitys every tick, even when the
        // toggled bag queue is empty. Thus a same-UPDATE removal has expired
        // before its new bag is evaluated two ticks later (see D-loot).
        self.removed_enemies.clear();
        self.notices
            .loot_kills
            .retain(|_, kill| self.loot_tick <= kill.tick.saturating_add(3));
        if processed {
            self.loot_window = self.loot_window.and_then(|(ty, remaining, rule)| {
                (remaining > 1).then_some((ty, remaining.saturating_sub(1), rule))
            });
        }
    }
    pub(crate) fn attribute_kill_loot(&mut self, events: &mut Vec<EngineEvent>) {
        // Delayed confirmation may arrive after the bag's ordinary two-tick pass.
        for b in self.drops.values_mut().filter(|b| {
            self.loot_tick >= b.born_tick + 2
                && b.view.source_reason.as_deref() == Some("source-not-observed")
        }) {
            let candidates: Vec<_> = self
                .notices
                .loot_kills
                .values()
                .filter(|kill| {
                    (b.born_tick == kill.tick || b.born_tick == kill.tick.saturating_add(1))
                        && f64::from(b.pos.0 - kill.pos.0).powi(2)
                            + f64::from(b.pos.1 - kill.pos.1).powi(2)
                            <= 2.25
                })
                .collect();
            if let [kill] = candidates.as_slice() {
                b.view.source_enemy = Some(kill.object_type);
                b.view.source_estimated = true;
                b.view.source_reason = Some("killed-target-same-tick".into());
                events.push(EngineEvent::DropUpdated(b.view.clone()));
            } else if candidates.len() > 1 {
                // Expiring evidence must not turn an ambiguous bag into a match.
                b.view.source_reason = Some("ambiguous-nearby-killed-targets".into());
                events.push(EngineEvent::DropUpdated(b.view.clone()));
            }
        }
    }
    pub(crate) fn flush_bags(&mut self, events: &mut Vec<EngineEvent>) {
        for (key, bag) in std::mem::take(&mut self.drops) {
            events.push(EngineEvent::DropUpdated(bag.view));
            if let Some(e) = self
                .entities
                .get_mut(&key.0)
                .filter(|e| e.generation == key.1)
            {
                e.bag_retired = true;
            }
            self.release_entity(key);
        }
    }
    pub(crate) fn loot_dialogue(&mut self, npc: &str, text: &str) {
        // TomatoData.LootAttributionManager.handleTextPacket. Map reset provides
        // the instance/seed guard. HM/TR remain reasons, not invented object types.
        let found=match (npc,text) {
            ("#Kitsune Umi","This fully concludes the Moonlight Festival!")=>Some((20493,"dialogue-kitsune")),
            ("#Dancer Miko","Thank you all for coming tonight.")=>Some((20451,"dialogue-miko")),
            ("#Umi, Goddess of Revelry","This fully concludes the Moonlight Festival.")=>Some((20493,"dialogue-revelry-tr-variant-unverified")),
            ("#Void Entity","You fools... You can never truly defeat me! I am in all of you! I AM all of you!")=>Some((45076,"dialogue-void")),
            ("#The Bridge Sentinel","I tried to protect you... I have failed.")=>Some((29003,"dialogue-sentinel")),
            ("#Valen the Unbreakable","I see now... my strength could not have held against this growing power.")=>Some((29003,"dialogue-valen-hm-variant-unverified")),
            ("#Twilight Archmage","Wait, there's still time! I JUST NEED MORE POWER! WAIT!")=>Some((29021,"dialogue-archmage")),
            ("#Nox the Wild Shadow","Unworthy as you are to know what hides beyond, I've had... an epiphany. So in case you've failed to realize...")=>Some((29021,"dialogue-nox-hm-variant-unverified")),
            ("#The Accursed King","...do you truly think your end will be any different?")=>Some((29039,"dialogue-king")),
            ("#King Azamoth","This fate is mine to bear... not hers.")=>Some((29039,"dialogue-azamoth-hm-variant-unverified")),
            _=>None,
        };
        if let Some((ty, reason)) = found {
            self.loot_window = Some((ty, 2, reason));
        }
    }
}
