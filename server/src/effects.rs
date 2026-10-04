//! Making abilities' effects land: damage, healing, shields, statuses and
//! taunts — plus damage/healing over time ticks and threat.

use bevy::prelude::*;
use shared::abilities::{Centre, Effect, Recipients};
use shared::classes::Stats;
use shared::combat::{Health, horizontal_distance};
use shared::components::{Faction, HitRadius, Motion};
use shared::formulas::{healing, incoming_damage, outgoing_damage};
use shared::gamedata::GameData;
use shared::protocol::{Link, ServerEvent};
use shared::statuses::{ActiveStatus, Modifiers, Statuses, Tick, TickAmount};
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
}

#[derive(Resource, Default, Debug)]
pub struct PendingEffects(pub Vec<Resolution>);

/// Every living character that effects can land on.
pub type Living<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Motion,
        &'static Faction,
        &'static HitRadius,
        &'static mut Health,
        &'static mut Statuses,
        &'static Stats,
        Option<&'static mut ThreatTable>,
        Option<&'static mut CombatClock>,
        Option<&'static mut ResetWhenIdle>,
    ),
    Without<Defeated>,
>;

/// What we need to know about whoever caused an effect.
#[derive(Clone, Copy)]
struct SourceInfo {
    position: Vec3,
    faction: Faction,
    power: f32,
    threat_multiplier: f32,
    modifiers: Modifiers,
}

fn source_info(living: &Living, data: &GameData, entity: Entity) -> Option<SourceInfo> {
    let (_, motion, faction, _, _, statuses, stats, ..) = living.get(entity).ok()?;
    Some(SourceInfo {
        position: motion.0.position,
        faction: *faction,
        power: stats.power,
        threat_multiplier: stats.threat_multiplier,
        modifiers: statuses.modifiers(|id| data.statuses.get(id)),
    })
}

/// Who an effect lands on.
fn recipients(
    to: Recipients,
    source: Entity,
    info: SourceInfo,
    target: Entity,
    living: &Living,
) -> Vec<Entity> {
    let position_of = |e: Entity| living.get(e).ok().map(|(_, m, ..)| m.0.position);
    let around = |centre: Centre, radius: f32, hostile: bool| -> Vec<Entity> {
        let centre = match centre {
            Centre::Me => Some(info.position),
            Centre::Target => position_of(target),
        };
        let Some(centre) = centre else {
            return Vec::new();
        };
        living
            .iter()
            .filter(|(e, motion, faction, hit, health, ..)| {
                let side_ok = if hostile {
                    info.faction.can_harm(**faction)
                } else {
                    *e == source || !info.faction.can_harm(**faction)
                };
                side_ok
                    && !health.is_dead()
                    && horizontal_distance(centre, motion.0.position) - hit.0 <= radius
            })
            .map(|(e, ..)| e)
            .collect()
    };
    match to {
        Recipients::Target => position_of(target).map(|_| target).into_iter().collect(),
        Recipients::Myself => vec![source],
        Recipients::EnemiesAround { centre, radius } => around(centre, radius, true),
        Recipients::AlliesAround { centre, radius } => around(centre, radius, false),
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
    let Ok((_, _, _, _, mut health, mut statuses, _, threat, clock, reset)) =
        living.get_mut(target)
    else {
        return false;
    };
    if health.is_dead() {
        return false;
    }
    let mitigated = incoming_damage(raw, statuses.modifiers(|id| data.statuses.get(id)));
    let (absorbed, through) = statuses.absorb(mitigated);
    health.damage(through);
    if let Some(mut threat) = threat {
        threat.add(source, through.max(1) as f32 * threat_multiplier);
    }
    if let Some(mut clock) = clock {
        clock.mark(now);
    }
    if let Some(mut reset) = reset {
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

/// Make abilities that went off this tick land.
pub fn resolve_effects(
    time: Res<Time>,
    data: Res<GameData>,
    mut link: ResMut<Link>,
    mut pending: ResMut<PendingEffects>,
    mut rng: ResMut<CombatRng>,
    mut living: Living,
) {
    let now = time.elapsed_secs_f64();
    let combat = &data.config.combat;
    let mut healing_threat: Vec<(Entity, Entity, f32)> = Vec::new();

    for resolution in std::mem::take(&mut pending.0) {
        let Some(ability) = data.abilities.get(&resolution.ability) else {
            continue;
        };
        let source = resolution.source;
        let Some(info) = source_info(&living, &data, source) else {
            continue;
        };
        let mut hostile = false;
        for entry in &ability.effects {
            for recipient in recipients(entry.to, source, info, resolution.target, &living) {
                match &entry.effect {
                    Effect::Damage { amount } => {
                        let amount = match (&ability.combo, resolution.combo) {
                            (Some(combo), true) => combo.amount,
                            _ => *amount,
                        };
                        let crit = rng.0.chance(combat.crit_chance);
                        let raw = outgoing_damage(amount, info.power, info.modifiers, crit, combat);
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
                        let Ok((_, _, _, _, mut health, statuses, ..)) = living.get_mut(recipient)
                        else {
                            continue;
                        };
                        let crit = rng.0.chance(combat.crit_chance);
                        let received = statuses.modifiers(|id| data.statuses.get(id));
                        let amount =
                            healing(*amount, info.power, info.modifiers, received, crit, combat);
                        let restored = health.heal(amount);
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
                        let Ok((_, _, _, _, _, mut statuses, ..)) = living.get_mut(recipient)
                        else {
                            continue;
                        };
                        let received = statuses.modifiers(|id| data.statuses.get(id));
                        let absorb =
                            healing(*amount, info.power, info.modifiers, received, false, combat);
                        statuses.apply(ActiveStatus {
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
                        let Ok((_, _, faction, _, _, mut statuses, _, threat, clock, _)) =
                            living.get_mut(recipient)
                        else {
                            continue;
                        };
                        let harmful = info.faction.can_harm(*faction);
                        statuses.apply(ActiveStatus {
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
                            if let Some(mut threat) = threat {
                                threat.add(source, 1.0);
                            }
                            if let Some(mut clock) = clock {
                                clock.mark(now);
                            }
                        }
                    }
                    Effect::Taunt => {
                        if let Ok((.., Some(mut threat), _, _)) = living.get_mut(recipient) {
                            threat.taunt(source);
                            hostile = true;
                        }
                    }
                }
            }
        }
        if hostile && let Ok((.., Some(mut clock), _)) = living.get_mut(source) {
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
    for (.., threat, _, _) in living.iter_mut() {
        let Some(mut threat) = threat else {
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
    for (entity, _, _, _, _, mut statuses, ..) in &mut living {
        for tick in statuses.due_ticks(now, interval) {
            due.push((entity, tick));
        }
        statuses.expire(now);
    }
    let mut healing_threat = Vec::new();
    for (entity, tick) in due {
        let multiplier = living
            .get(tick.source)
            .map_or(1.0, |(.., stats, _, _, _)| stats.threat_multiplier);
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
                let Ok((_, _, _, _, mut health, statuses, ..)) = living.get_mut(entity) else {
                    continue;
                };
                let received = statuses
                    .modifiers(|id| data.statuses.get(id))
                    .healing_received;
                let restored = health.heal((amount as f32 * received).round() as u32);
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
