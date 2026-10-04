//! Making abilities' effects land: damage, healing, shields, statuses,
//! taunts and raises — plus ground markers going off, damage/healing over
//! time ticks, and threat. Effects only reach characters in the caster's zone.

use bevy::ecs::query::QueryData;
use bevy::prelude::*;
use shared::abilities::{Centre, Effect, Recipients};
use shared::classes::Stats;
use shared::combat::{Health, horizontal_distance};
use shared::components::{Faction, HitRadius, Motion, Zone};
use shared::formulas::{healing, incoming_damage, outgoing_damage};
use shared::gamedata::GameData;
use shared::protocol::{Link, ServerEvent};
use shared::statuses::{ActiveStatus, Modifiers, Statuses, Tick, TickAmount};
use shared::telegraphs::Telegraph;
use shared::threat::ThreatTable;

use crate::CombatRng;
use crate::characters::{CombatClock, Defeated};
use crate::enemies::ResetWhenIdle;

/// Healing makes enemies a little angry at the healer: this fraction of
/// the healing counts as threat (a rule, not a tunable number per class).
const HEALING_THREAT: f32 = 0.5;

/// An ability that went off this tick and still has to land.
#[derive(Debug, Clone)]
pub struct Resolution {
    pub source: Entity,
    pub ability: String,
    pub target: Entity,
    pub combo: bool,
    /// Ground markers placed by this cast (they go off now).
    pub markers: Vec<Entity>,
}

/// Work handed from one step of the tick to the next.
#[derive(Resource, Default, Debug)]
pub struct PendingEffects {
    pub resolutions: Vec<Resolution>,
    /// Ground markers to put down (with the zone they belong to).
    pub new_markers: Vec<(Telegraph, Zone)>,
}

/// A living character that effects can land on.
#[derive(QueryData)]
#[query_data(mutable)]
pub struct LivingData {
    entity: Entity,
    motion: &'static Motion,
    faction: &'static Faction,
    zone: &'static Zone,
    hit: &'static HitRadius,
    health: &'static mut Health,
    statuses: &'static mut Statuses,
    stats: &'static Stats,
    threat: Option<&'static mut ThreatTable>,
    clock: Option<&'static mut CombatClock>,
    reset: Option<&'static mut ResetWhenIdle>,
}

pub type Living<'w, 's> = Query<'w, 's, LivingData, Without<Defeated>>;

/// What we need to know about whoever caused an effect.
#[derive(Clone)]
struct SourceInfo {
    position: Vec3,
    faction: Faction,
    zone: Zone,
    power: f32,
    threat_multiplier: f32,
    crit_chance: f32,
    modifiers: Modifiers,
}

fn source_info(living: &Living, data: &GameData, entity: Entity) -> Option<SourceInfo> {
    let s = living.get(entity).ok()?;
    Some(SourceInfo {
        position: s.motion.0.position,
        faction: *s.faction,
        zone: s.zone.clone(),
        power: s.stats.power,
        threat_multiplier: s.stats.threat_multiplier,
        crit_chance: s.stats.crit_chance,
        modifiers: s.statuses.modifiers(|id| data.statuses.get(id)),
    })
}

/// Who an effect lands on, in groups: one group per ground marker for
/// `InTelegraph` (shared damage is split within a group), otherwise one.
fn recipients(
    to: Recipients,
    resolution: &Resolution,
    info: &SourceInfo,
    living: &Living,
    markers: &Query<&Telegraph>,
    grace: f32,
) -> Vec<Vec<Entity>> {
    let same_zone = |e: Entity| living.get(e).ok().filter(|c| *c.zone == info.zone);
    let position_of = |e: Entity| same_zone(e).map(|c| c.motion.0.position);
    let opponents = || {
        living.iter().filter(|c| {
            *c.zone == info.zone && info.faction.can_harm(*c.faction) && !c.health.is_dead()
        })
    };
    let around = |centre: Centre, radius: f32, hostile: bool| -> Vec<Entity> {
        let centre = match centre {
            Centre::Me => Some(info.position),
            Centre::Target => position_of(resolution.target),
        };
        let Some(centre) = centre else {
            return Vec::new();
        };
        living
            .iter()
            .filter(|c| {
                let side_ok = if hostile {
                    info.faction.can_harm(*c.faction)
                } else {
                    c.entity == resolution.source || !info.faction.can_harm(*c.faction)
                };
                side_ok
                    && *c.zone == info.zone
                    && !c.health.is_dead()
                    && horizontal_distance(centre, c.motion.0.position) - c.hit.0 <= radius
            })
            .map(|c| c.entity)
            .collect()
    };
    match to {
        Recipients::Target => vec![
            position_of(resolution.target)
                .map(|_| resolution.target)
                .into_iter()
                .collect(),
        ],
        Recipients::Myself => vec![vec![resolution.source]],
        Recipients::EnemiesAround { centre, radius } => vec![around(centre, radius, true)],
        Recipients::AlliesAround { centre, radius } => vec![around(centre, radius, false)],
        Recipients::InTelegraph => resolution
            .markers
            .iter()
            .filter_map(|m| markers.get(*m).ok())
            .map(|marker| {
                opponents()
                    .filter(|c| marker.covers(c.motion.0.position, grace))
                    .map(|c| c.entity)
                    .collect()
            })
            .collect(),
    }
}

/// Damage that has already had the dealer's buffs applied lands on a target.
fn land_damage(
    living: &mut Living,
    data: &GameData,
    link: &mut Link,
    now: f64,
    source: Entity,
    target: Entity,
    raw: u32,
    crit: bool,
    threat_multiplier: f32,
    cause: &str,
    tick: bool,
) -> bool {
    let Ok(mut c) = living.get_mut(target) else {
        return false;
    };
    if c.health.is_dead() {
        return false;
    }
    let modifiers = c.statuses.modifiers(|id| data.statuses.get(id));
    let mitigated = incoming_damage(raw, modifiers, c.stats.guard);
    let (absorbed, through) = c.statuses.absorb(mitigated);
    c.health.damage(through);
    if let Some(threat) = c.threat.as_mut() {
        threat.add(source, through.max(1) as f32 * threat_multiplier);
    }
    if let Some(clock) = c.clock.as_mut() {
        clock.mark(now);
    }
    if let Some(reset) = c.reset.as_mut() {
        reset.last_hit = now;
    }
    link.to_client.push(ServerEvent::Damage {
        source,
        target,
        amount: through,
        absorbed,
        crit,
        cause: cause.to_owned(),
        tick,
    });
    true
}

/// Put down ground markers for casts that started this tick.
pub fn spawn_telegraphs(mut commands: Commands, mut pending: ResMut<PendingEffects>) {
    for (telegraph, zone) in pending.new_markers.drain(..) {
        commands.spawn((telegraph, zone));
    }
}

/// Stack and spread markers follow their player until they go off.
pub fn follow_telegraphs(
    mut commands: Commands,
    mut markers: Query<(Entity, &mut Telegraph)>,
    characters: Query<(&Motion, Has<Defeated>)>,
) {
    for (entity, mut marker) in &mut markers {
        let Some(follow) = marker.follow else {
            continue;
        };
        match characters.get(follow) {
            Ok((motion, false)) => marker.origin = motion.0.position,
            // Whoever it was on fell or left: the marker fizzles.
            _ => commands.entity(entity).despawn(),
        }
    }
}

/// Make abilities that went off this tick land.
pub fn resolve_effects(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut link: ResMut<Link>,
    mut pending: ResMut<PendingEffects>,
    mut rng: ResMut<CombatRng>,
    markers: Query<&Telegraph>,
    mut living: Living,
    mut fallen: Query<(&mut Health, &Zone), With<Defeated>>,
) {
    let now = time.elapsed_secs_f64();
    let combat = &data.config.combat;
    let mut healing_threat: Vec<(Entity, Entity, f32)> = Vec::new();

    for resolution in std::mem::take(&mut pending.resolutions) {
        for marker in &resolution.markers {
            commands.entity(*marker).despawn();
        }
        let Some(ability) = data.abilities.get(&resolution.ability) else {
            continue;
        };
        let source = resolution.source;
        let Some(info) = source_info(&living, &data, source) else {
            continue;
        };
        link.to_client.push(ServerEvent::AbilityLanded {
            user: source,
            ability: ability.id.clone(),
            target: resolution.target,
        });
        let mut hostile = false;
        for entry in &ability.effects {
            let groups = recipients(
                entry.to,
                &resolution,
                &info,
                &living,
                &markers,
                combat.marker_grace,
            );
            for group in groups {
                for &recipient in &group {
                    match &entry.effect {
                        Effect::Damage { amount } | Effect::SharedDamage { amount } => {
                            let mut amount = match (&ability.combo, resolution.combo) {
                                (Some(combo), true) => combo.amount,
                                _ => *amount,
                            };
                            if matches!(entry.effect, Effect::SharedDamage { .. }) {
                                amount /= group.len().max(1) as u32;
                            }
                            let crit = rng.0.chance(info.crit_chance);
                            let raw =
                                outgoing_damage(amount, info.power, info.modifiers, crit, combat);
                            hostile |= land_damage(
                                &mut living,
                                &data,
                                &mut link,
                                now,
                                source,
                                recipient,
                                raw,
                                crit,
                                info.threat_multiplier,
                                &ability.id,
                                false,
                            );
                        }
                        Effect::Heal { amount } => {
                            let Ok(mut c) = living.get_mut(recipient) else {
                                continue;
                            };
                            let crit = rng.0.chance(info.crit_chance);
                            let received = c.statuses.modifiers(|id| data.statuses.get(id));
                            let amount = healing(
                                *amount,
                                info.power,
                                info.modifiers,
                                received,
                                crit,
                                combat,
                            );
                            let restored = c.health.heal(amount);
                            healing_threat.push((
                                source,
                                recipient,
                                restored as f32 * info.threat_multiplier,
                            ));
                            link.to_client.push(ServerEvent::Heal {
                                source,
                                target: recipient,
                                amount: restored,
                                crit,
                                cause: ability.id.clone(),
                                tick: false,
                            });
                        }
                        Effect::Shield { amount, status } => {
                            let Some(def) = data.statuses.get(status) else {
                                continue;
                            };
                            let Ok(mut c) = living.get_mut(recipient) else {
                                continue;
                            };
                            let received = c.statuses.modifiers(|id| data.statuses.get(id));
                            let absorb = healing(
                                *amount,
                                info.power,
                                info.modifiers,
                                received,
                                false,
                                combat,
                            );
                            c.statuses.apply(ActiveStatus {
                                id: def.id.clone(),
                                source,
                                applied: now,
                                expires: now + f64::from(def.duration),
                                next_tick: now + f64::from(combat.tick_interval),
                                tick: None,
                                absorb,
                                is_shield: true,
                            });
                        }
                        Effect::ApplyStatus { status } => {
                            let Some(def) = data.statuses.get(status) else {
                                continue;
                            };
                            let tick = def.tick.map(|t| match t {
                                Tick::Damage { amount } => TickAmount::Damage(outgoing_damage(
                                    amount,
                                    info.power,
                                    info.modifiers,
                                    false,
                                    combat,
                                )),
                                Tick::Heal { amount } => TickAmount::Heal(healing(
                                    amount,
                                    info.power,
                                    info.modifiers,
                                    Modifiers::default(),
                                    false,
                                    combat,
                                )),
                            });
                            let Ok(mut c) = living.get_mut(recipient) else {
                                continue;
                            };
                            let harmful = info.faction.can_harm(*c.faction);
                            c.statuses.apply(ActiveStatus {
                                id: def.id.clone(),
                                source,
                                applied: now,
                                expires: now + f64::from(def.duration),
                                next_tick: now + f64::from(combat.tick_interval),
                                tick,
                                absorb: 0,
                                is_shield: false,
                            });
                            if harmful {
                                hostile = true;
                                if let Some(threat) = c.threat.as_mut() {
                                    threat.add(source, 1.0);
                                }
                                if let Some(clock) = c.clock.as_mut() {
                                    clock.mark(now);
                                }
                            }
                        }
                        Effect::Taunt => {
                            if let Ok(mut c) = living.get_mut(recipient)
                                && let Some(threat) = c.threat.as_mut()
                            {
                                threat.taunt(source);
                                hostile = true;
                            }
                        }
                        Effect::Raise { .. } => {}
                    }
                }
            }
            // Raises land on defeated characters, who aren't "living".
            if let Effect::Raise { health_percent } = entry.effect
                && let Ok((mut health, zone)) = fallen.get_mut(resolution.target)
                && *zone == info.zone
            {
                let max = health.max;
                *health = Health {
                    current: ((max as f32 * health_percent / 100.0).round() as u32).clamp(1, max),
                    max,
                };
                commands.entity(resolution.target).remove::<Defeated>();
                link.to_client.push(ServerEvent::Revived {
                    entity: resolution.target,
                });
            }
        }
        if hostile
            && let Ok(mut c) = living.get_mut(source)
            && let Some(clock) = c.clock.as_mut()
        {
            clock.mark(now);
        }
    }

    spread_healing_threat(&mut living, &healing_threat);
}

/// Enemies already fighting someone who was healed get angry at the healer.
fn spread_healing_threat(living: &mut Living, healing: &[(Entity, Entity, f32)]) {
    if healing.is_empty() {
        return;
    }
    for mut c in living.iter_mut() {
        let Some(threat) = c.threat.as_mut() else {
            continue;
        };
        for (healer, healed, amount) in healing {
            if threat.0.contains_key(healed) {
                threat.add(*healer, amount * HEALING_THREAT);
            }
        }
    }
}

/// Damage and healing over time, and statuses running out.
pub fn tick_statuses(
    time: Res<Time>,
    data: Res<GameData>,
    mut link: ResMut<Link>,
    mut living: Living,
) {
    let now = time.elapsed_secs_f64();
    let interval = f64::from(data.config.combat.tick_interval);
    let mut due = Vec::new();
    for mut c in &mut living {
        for tick in c.statuses.due_ticks(now, interval) {
            due.push((c.entity, tick));
        }
        c.statuses.expire(now);
    }
    let mut healing_threat = Vec::new();
    for (entity, tick) in due {
        let multiplier = living
            .get(tick.source)
            .map_or(1.0, |c| c.stats.threat_multiplier);
        match tick.amount {
            TickAmount::Damage(amount) => {
                land_damage(
                    &mut living,
                    &data,
                    &mut link,
                    now,
                    tick.source,
                    entity,
                    amount,
                    false,
                    multiplier,
                    &tick.status,
                    true,
                );
            }
            TickAmount::Heal(amount) => {
                let Ok(mut c) = living.get_mut(entity) else {
                    continue;
                };
                let received = c
                    .statuses
                    .modifiers(|id| data.statuses.get(id))
                    .healing_received;
                let restored = c.health.heal((amount as f32 * received).round() as u32);
                healing_threat.push((tick.source, entity, restored as f32 * multiplier));
                link.to_client.push(ServerEvent::Heal {
                    source: tick.source,
                    target: entity,
                    amount: restored,
                    crit: false,
                    cause: tick.status,
                    tick: true,
                });
            }
        }
    }
    spread_healing_threat(&mut living, &healing_threat);
}
