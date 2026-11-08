use crate::ReconstructionReason as Reason;
use crate::{damage_with_defense, parse_enchant, Engine};
use maxed_protocol::Packet;
use maxed_types::*;

#[derive(Clone)]
pub(crate) struct Shot {
    pub number: u64,
    pub at: Timestamp,
    pub owner_generation: u32,
    pub item: Option<u16>,
    pub kind: SourceKind,
    pub damage: Option<i32>,
    pub piercing: bool,
    pub scaling: Option<ScalingDef>,
    pub scaling_stat: Option<i32>,
    pub scaling_item: Option<u16>,
    pub unavailable: Reason,
    pub assumptions: Vec<Reason>,
}

fn kind(slot: Option<u8>, default: SourceKind) -> SourceKind {
    // RealmShark DamageSource.forSlot.
    match slot {
        None | Some(0) => default,
        Some(1 | 2 | 3 | 8 | 17 | 24) => SourceKind::Weapon,
        Some(6 | 7 | 9 | 14) => SourceKind::ItemEffect,
        _ => SourceKind::Ability,
    }
}

impl Engine {
    pub(crate) fn expire_shots(&mut self) {
        while let Some(&(at, _, owner, bullet)) = self.shot_expiry.first() {
            if self.now.0.saturating_sub(at.0) <= 5_000_000 {
                break;
            }
            self.remove_shot((owner, bullet));
            self.remember_removed((owner, bullet), Reason::ShotExpired);
        }
        while let Some(&(at, fight, pair)) = self.pair_expiry.first() {
            if self.now.0.saturating_sub(at.0) <= 5_000_000 {
                break;
            }
            self.pair_expiry.pop_first();
            if let Some(f) = self.fights.get_mut(&fight) {
                f.settle_pair(pair);
                self.dirty_fights.insert(fight);
            }
        }
    }
    fn remove_shot(&mut self, key: (i32, u16)) {
        if let Some(s) = self.shots.remove(&key) {
            self.shot_expiry.remove(&(s.at, s.number, key.0, key.1));
        }
    }
    fn remember_removed(&mut self, key: (i32, u16), reason: Reason) {
        // Bounded diagnostic tombstones distinguish expiry/removal from a shot
        // never seen. They cannot restore damage or source evidence.
        self.forget_removed(key);
        if self.removed_shots.len() >= self.config.max_shots {
            if let Some((_, oldest)) = self.removed_shot_order.pop_first() {
                self.removed_shots.remove(&oldest);
            }
        }
        self.removed_shot_number += 1;
        self.removed_shots
            .insert(key, (self.removed_shot_number, reason));
        self.removed_shot_order
            .insert((self.removed_shot_number, key));
    }
    fn forget_removed(&mut self, key: (i32, u16)) {
        if let Some((number, _)) = self.removed_shots.remove(&key) {
            self.removed_shot_order.remove(&(number, key));
        }
    }
    pub(crate) fn remove_owner_shots(&mut self, owner: i32) {
        let keys: Vec<_> = self
            .shots
            .range((owner, 0)..=(owner, u16::MAX))
            .map(|(k, _)| *k)
            .collect();
        for key in keys {
            self.remove_shot(key);
            self.remember_removed(key, Reason::OwnerLeftView);
        }
    }

    pub(crate) fn resolve_owner(&self, owner: i32) -> Option<i32> {
        if self.is_player(owner) {
            return Some(owner);
        }
        let (id, generation) = self.entities.get(&owner)?.summoner?;
        (self
            .entities
            .get(&id)
            .is_some_and(|p| p.generation == generation && p.present)
            && self.is_player(id))
        .then_some(id)
    }
    pub(crate) fn source(
        &self,
        owner: i32,
        shot: Option<&Shot>,
        damage: f64,
        mut estimated: bool,
    ) -> SourceDamage {
        let resolved = self.resolve_owner(owner);
        let summon = resolved.is_some_and(|id| id != owner);
        let mut item = if summon {
            self.entities.get(&owner).and_then(|e| e.object_type)
        } else {
            shot.and_then(|s| s.item)
        };
        let mut kind = if summon {
            SourceKind::Summon
        } else {
            shot.map(|s| s.kind.clone()).unwrap_or(SourceKind::Other)
        };
        let mut reasons = if estimated {
            vec!["client-projectile-reconstruction".into()]
        } else {
            vec![]
        };
        if !summon
            && shot.is_none()
            && resolved == Some(owner)
            && self.local.as_ref().map(|l| l.object) != Some(owner)
        {
            let weapon = self
                .entities
                .get(&owner)
                .and_then(|e| e.stat(8))
                .and_then(|t| u16::try_from(t).ok())
                .filter(|t| {
                    self.item_index
                        .get(t)
                        .map(|&i| &self.catalog.items[i])
                        .is_some_and(|item| !item.projectiles.is_empty())
                });
            if let Some(weapon) = weapon {
                item = Some(weapon);
                kind = SourceKind::Weapon;
                estimated = true;
                reasons.push(INFERRED_EQUIPPED_WEAPON.into());
            }
        }
        let enchant = resolved
            .and_then(|id| self.entities.get(&id))
            .and_then(|e| {
                let item = item?;
                (0..4)
                    .find(|&slot| e.stat(8 + slot) == Some(i32::from(item)))
                    .and_then(|slot| e.enchants.get(usize::from(slot)))
                    .cloned()
            })
            .unwrap_or_else(|| parse_enchant(None));
        SourceDamage {
            item_type: item,
            ability: kind == SourceKind::Ability,
            kind,
            enchant,
            damage,
            hits: 1,
            biggest_hit: Some(damage),
            estimated,
            reasons,
        }
    }
    fn remember(&mut self, owner: i32, bullet: u16, mut shot: Shot, events: &mut Vec<EngineEvent>) {
        if self.shots.len() >= self.config.max_shots && !self.shots.contains_key(&(owner, bullet)) {
            if let Some(&(_, _, owner, bullet)) = self.shot_expiry.first() {
                self.remove_shot((owner, bullet));
                self.remember_removed((owner, bullet), Reason::ShotEvicted);
            }
            self.mark(CoverageIssue::ResourceLimit, events);
        }
        self.shot_number += 1;
        shot.number = self.shot_number;
        self.remove_shot((owner, bullet));
        self.forget_removed((owner, bullet));
        self.shot_expiry
            .insert((shot.at, shot.number, owner, bullet));
        self.shots.insert((owner, bullet), shot);
    }
    pub(crate) fn apply_combat(&mut self, packet: &Packet, events: &mut Vec<EngineEvent>) {
        match packet {
            Packet::AllyShoot(p) => {
                let item = u16::try_from(p.containerType).ok();
                let def = item
                    .and_then(|t| self.item_index.get(&t))
                    .map(|&i| &self.catalog.items[i]);
                let shot = Shot {
                    number: 0,
                    at: self.now,
                    owner_generation: self
                        .entities
                        .get(&p.ownerId)
                        .map(|e| e.generation)
                        .unwrap_or(0),
                    item,
                    kind: kind(def.and_then(|i| i.slot_type), SourceKind::Weapon),
                    damage: None,
                    piercing: false,
                    scaling: None,
                    scaling_stat: None,
                    scaling_item: None,
                    unavailable: Reason::MissingProjectile,
                    assumptions: vec![],
                };
                self.remember(p.ownerId, p.bulletId, shot, events);
            }
            Packet::PlayerShoot(p) => {
                let Some(owner) = self.local.as_ref().map(|l| l.object) else {
                    self.rng_valid = false;
                    self.mark(CoverageIssue::MissingIdentity, events);
                    return;
                };
                let item = self
                    .item_index
                    .get(&p.weaponId)
                    .map(|&i| self.catalog.items[i].clone());
                let projectile = item.as_ref().and_then(|i| {
                    let subattacks = i
                        .weapon_firing
                        .as_ref()
                        .and_then(|f| f.subattack_projectile_indices.as_ref())
                        .filter(|indices| !indices.is_empty());
                    let index = if let Some(indices) = subattacks {
                        *indices
                            .get(usize::try_from(p.projectileId).ok()?)?
                            .as_ref()?
                    } else if p.projectileId == -1 {
                        0
                    } else {
                        i16::from(p.projectileId)
                    };
                    i.projectiles.iter().find(|pdef| pdef.index == index)
                });
                if projectile.is_none() || self.rng.is_none() {
                    self.rng_valid = false;
                }
                let source_kind =
                    kind(item.as_ref().and_then(|i| i.slot_type), SourceKind::Unknown);
                let mut damage = Err(if projectile.is_none() {
                    Reason::MissingProjectileDefinition
                } else {
                    Reason::MissingSeed
                });
                let mut assumptions = vec![];
                // RealmShark Projectile(RNG,...): exclusive maximum, no roll when equal.
                // Each received PlayerShoot consumes its own roll in packet order, including
                // subattacks. Mapping only selects the bounds; it does not expand a volley
                // or reorder rolls. Missing definitions still invalidate RNG continuity.
                if let (Some(pdef), Some(rng)) = (projectile, self.rng.as_mut()) {
                    let base = if pdef.min_damage == pdef.max_damage {
                        pdef.min_damage
                    } else {
                        pdef.min_damage
                            + (rng.next() % i64::from(pdef.max_damage - pdef.min_damage)) as i32
                    };
                    damage = Ok(base);
                }
                if source_kind == SourceKind::Weapon {
                    damage = damage.and_then(|base| {
                        self.weapon_multiplier(owner).and_then(|(m, defaults)| {
                            assumptions.extend(defaults);
                            finite_damage(base as f32 * m).ok_or(Reason::InvalidDamage)
                        })
                    });
                } else if let Some(scaling) = item.as_ref().and_then(|i| i.scaling.as_ref()) {
                    damage = damage.and_then(|base| {
                        self.scaling_bonus(owner, p.weaponId, scaling, None)
                            .and_then(|(bonus, defaults)| {
                                assumptions.extend(defaults);
                                base.checked_add(bonus).ok_or(Reason::InvalidDamage)
                            })
                    });
                }
                let unavailable = damage
                    .as_ref()
                    .err()
                    .copied()
                    .unwrap_or(Reason::InvalidDamage);
                let shot = Shot {
                    number: 0,
                    at: self.now,
                    owner_generation: self.entities.get(&owner).map(|e| e.generation).unwrap_or(0),
                    item: Some(p.weaponId),
                    kind: source_kind,
                    damage: damage.ok(),
                    piercing: projectile.is_some_and(|p| p.armor_piercing),
                    scaling: None,
                    scaling_stat: None,
                    scaling_item: None,
                    unavailable,
                    assumptions,
                };
                self.remember(owner, p.bulletId as u16, shot, events);
            }
            Packet::ServerPlayerShoot(p) => {
                if !self.ensure_entity(p.ownerId, events) {
                    return;
                }
                if p.summonerId != 0 {
                    self.set_summoner(p.ownerId, p.summonerId);
                }
                let item = u16::try_from(p.containerType).ok();
                let def = item
                    .and_then(|t| self.item_index.get(&t))
                    .map(|&i| &self.catalog.items[i]);
                let scaling = def.and_then(|d| d.scaling.clone());
                let projectile = def.and_then(|d| {
                    d.projectiles
                        .iter()
                        .find(|pdef| pdef.index == i16::from(p.bulletType))
                });
                let resolved = self.resolve_owner(p.ownerId);
                let scaling_stat = scaling.as_ref().and_then(|s| {
                    resolved
                        .and_then(|id| self.entities.get(&id))
                        .and_then(|e| e.stat(s.stat_id))
                });
                let summon =
                    p.ownerId != p.summonerId && p.summonerId != 0 && !self.is_player(p.ownerId);
                let shot = Shot {
                    number: 0,
                    at: self.now,
                    owner_generation: self.entities[&p.ownerId].generation,
                    item: if summon {
                        self.entities[&p.ownerId].object_type.or(item)
                    } else {
                        item
                    },
                    kind: if summon {
                        SourceKind::Summon
                    } else {
                        kind(def.and_then(|d| d.slot_type), SourceKind::ItemEffect)
                    },
                    damage: projectile.map(|_| i32::from(p.damage)),
                    piercing: projectile.is_some_and(|p| p.armor_piercing),
                    scaling,
                    scaling_stat,
                    scaling_item: item,
                    unavailable: Reason::MissingProjectileDefinition,
                    assumptions: vec![],
                };
                // TomatoData.serverPlayerShoot remembers every bullet in the
                // volley. D-owner keeps the unsigned 16-bit identity instead of
                // the legacy modulo-256 projectile-array fallback.
                for offset in 0..p.bulletCount.max(1) as u16 {
                    self.remember(
                        p.ownerId,
                        (p.bulletId as u16).wrapping_add(offset),
                        shot.clone(),
                        events,
                    );
                }
            }
            Packet::Damage(p) => {
                self.report(
                    p.targetId,
                    p.objectId,
                    p.bulletId,
                    true,
                    Ok(i32::from(p.damageAmount)),
                    events,
                );
            }
            Packet::EnemyHit(p) => {
                self.reconstruction.increment(Reason::AttemptedHits);
                let value = self.reconstruct(p.shooterID, p.bulletId as u16, p.targetId);
                let mut assumed_stats = false;
                let value = match value {
                    Ok((amount, assumptions)) => {
                        if !assumptions.is_empty() {
                            assumed_stats = true;
                        }
                        self.reconstruction.increment(Reason::ReconstructedHits);
                        if amount == 0 {
                            self.reconstruction.increment(Reason::ReconstructedZero);
                        }
                        for reason in assumptions {
                            self.reconstruction.increment(reason);
                        }
                        Ok(amount)
                    }
                    Err(reason) => {
                        self.reconstruction.increment(Reason::OmittedHits);
                        self.reconstruction.increment(reason);
                        Err(reason.issue())
                    }
                };
                self.report_with_coverage(
                    p.targetId,
                    p.shooterID,
                    p.bulletId as u16,
                    false,
                    (value, assumed_stats.then_some(CoverageIssue::MissingStats)),
                    events,
                );
            }
            _ => {} // AOE, PLAYERHIT, GROUNDDAMAGE and RECONNECT are not outgoing damage.
        }
    }
    fn weapon_multiplier(&self, owner: i32) -> Result<(f32, Vec<Reason>), Reason> {
        // Entity.java:269-291 has no null-stat defaults. Read all three before
        // Weak, like Java; only the two Crucible channels default to neutral.
        let e = self.entities.get(&owner).ok_or(Reason::MissingIdentity)?;
        let condition = e.stat(29).ok_or(Reason::MissingCondition)?;
        let attack = e.stat(20).ok_or(Reason::MissingAttack)?;
        let exaltation = e.stat(113).ok_or(Reason::MissingExaltation)?;
        if condition & 4 != 0 {
            return Ok((0.5, vec![]));
        }
        if !(0..=100_000).contains(&attack) {
            return Err(Reason::InvalidAttack);
        }
        let mut defaults = vec![];
        let crucible = e.crucible.unwrap_or_else(|| {
            defaults.push(Reason::AssumedCrucibleDefault);
            1.0
        });
        let blood = e.blood_ritual.unwrap_or_else(|| {
            defaults.push(Reason::AssumedBloodRitualDefault);
            1.0
        });
        let mut m = (attack + 25) as f32 * 0.02;
        if condition & 0x40000 != 0 {
            m = (f64::from(m) * 1.25) as f32;
        }
        // Offline.multiplier multiplies the two channels before Java's
        // compound float assignment in Entity.playerStatsMultiplier.
        m = (f64::from(m) * (crucible * blood)) as f32;
        m *= exaltation as f32 / 1000.;
        if !m.is_finite() {
            return Err(Reason::InvalidDamage);
        }
        Ok((m, defaults))
    }
    fn scaling_bonus(
        &self,
        owner: i32,
        item: u16,
        s: &ScalingDef,
        fallback: Option<i32>,
    ) -> Result<(i32, Vec<Reason>), Reason> {
        let e = self.entities.get(&owner).ok_or(Reason::MissingIdentity)?;
        // Java returns zero bonus if the scaling stat is missing. Server proc
        // callers prefer current stats, then their shot-time snapshot.
        let Some(stat) = e.stat(s.stat_id).or(fallback) else {
            return Ok((0, vec![Reason::AssumedScalingStatDefault]));
        };
        if stat <= s.minimum {
            return Ok((0, vec![]));
        }
        let mut defaults = vec![];
        let mut multiplier = 1.0f32;
        if e.stat(9) == Some(i32::from(item)) {
            match e.enchants.get(1) {
                None
                | Some(EnchantView {
                    state: EnchantState::Missing,
                    ..
                }) => {
                    defaults.push(Reason::AssumedAbilityEnchantDefault);
                }
                Some(v) if v.state == EnchantState::Invalid => {
                    // ParseEnchants.java:715-732 returns neutral Totals for
                    // malformed codes; retain the invalid source evidence.
                    defaults.push(Reason::AssumedAbilityEnchantDefault);
                }
                Some(v) => {
                    for id in v
                        .ids
                        .iter()
                        .take_while(|&&id| id != -3)
                        .filter(|&&id| id > 0)
                    {
                        let def = self
                            .enchant_index
                            .get(id)
                            .map(|&i| &self.catalog.enchants[i])
                            .ok_or(Reason::UnknownEnchantMultiplier)?;
                        multiplier *= def
                            .stat_damage_multiplier
                            .ok_or(Reason::MissingEnchantMetadata)?
                            as f32;
                    }
                }
            }
        } else if e.stat(9).is_none() {
            defaults.push(Reason::AssumedAbilityEnchantDefault);
        }
        let base = (i64::from(stat) - i64::from(s.minimum)) as f32 * s.damage_per_stat;
        let bonus = finite_damage(base * multiplier).ok_or(Reason::InvalidDamage)?;
        Ok((bonus, defaults))
    }
    fn reconstruct(
        &self,
        owner: i32,
        bullet: u16,
        target: i32,
    ) -> Result<(i32, Vec<Reason>), Reason> {
        if self.local.is_none() {
            return Err(Reason::MissingIdentity);
        }
        let shot = self.shots.get(&(owner, bullet)).ok_or_else(|| {
            self.removed_shots
                .get(&(owner, bullet))
                .map(|(_, reason)| *reason)
                .unwrap_or(Reason::MissingProjectile)
        })?;
        let e = self.entities.get(&owner).ok_or(Reason::MissingIdentity)?;
        if shot.owner_generation != e.generation {
            return Err(Reason::StaleOwnerGeneration);
        }
        if self.now.0.saturating_sub(shot.at.0) > 5_000_000 {
            return Err(Reason::ShotExpired);
        }
        let mut amount = shot.damage.ok_or(shot.unavailable)?;
        let mut assumptions = shot.assumptions.clone();
        if !self.rng_valid {
            assumptions.push(Reason::UntrustedRng);
        }
        if let Some(s) = &shot.scaling {
            let owner = self.resolve_owner(owner).ok_or(Reason::MissingIdentity)?;
            let (bonus, defaults) = self.scaling_bonus(
                owner,
                shot.scaling_item
                    .ok_or(Reason::MissingProjectileDefinition)?,
                s,
                shot.scaling_stat,
            )?;
            amount = amount.checked_add(bonus).ok_or(Reason::InvalidDamage)?;
            assumptions.extend(defaults);
        }
        let target = self.entities.get(&target).ok_or(Reason::MissingTarget)?;
        // Typed UPDATE supplies the known-zero secondary condition baseline.
        // Other absent combat stats still require an uncertain numeric fallback.
        let [defense, condition, secondary_condition] = [21, 29, 96].map(|id| {
            target.stat(id).unwrap_or_else(|| {
                assumptions.push(Reason::AssumedTargetStatDefault);
                0
            })
        });
        let damage = damage_with_defense(
            amount,
            shot.piercing,
            defense,
            [condition, secondary_condition],
        );
        Ok((damage, assumptions))
    }
}
fn finite_damage(value: f32) -> Option<i32> {
    (value.is_finite() && (0.0..i32::MAX as f32).contains(&value)).then_some(value as i32)
}
