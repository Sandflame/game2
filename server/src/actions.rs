//! Using abilities: checking targets and timing, starting casts (placing
//! ground markers for telegraphed attacks), and running queued actions.
//! When an ability goes off, its effects are handed to `effects.rs`.

use bevy::prelude::*;
use shared::abilities::{AbilityDef, TargetKind};
use shared::combat::{ActionState, Health, Reject, Started, in_range};
use shared::components::{Faction, HitRadius, Hotbar, Motion, PlayerId, Zone};
use shared::gamedata::GameData;
use shared::movement::yaw_from_direction;
use shared::protocol::{Link, ServerEvent};
use shared::telegraphs::{Placement, Telegraph};

use crate::characters::{Defeated, PlayerInput};
use crate::classes::FlameChange;
use crate::effects::{PendingEffects, Resolution};

/// Read-only view of everything that can be targeted.
pub type Targets<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Motion,
        &'static HitRadius,
        &'static Health,
        &'static Faction,
        &'static Zone,
    ),
>;

/// Characters that can use abilities (players and enemies).
pub type Actors<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static PlayerId>,
        Option<&'static Hotbar>,
        &'static Faction,
        &'static Motion,
        &'static Zone,
        &'static mut ActionState,
        Option<&'static mut PlayerInput>,
        Has<Defeated>,
        Has<FlameChange>,
    ),
>;

/// The user's side of a target check.
pub struct User<'a> {
    pub entity: Entity,
    pub motion: &'a Motion,
    pub faction: Faction,
    pub zone: &'a Zone,
}

/// Work out who an ability lands on, or why it can't be used.
/// `Myself` abilities, and `Ally` abilities without a friendly target,
/// land on the user.
pub fn validate_target(
    ability: &AbilityDef,
    user: &User,
    target: Option<Entity>,
    targets: &Targets,
) -> Result<Entity, Reject> {
    let in_reach = |motion: &Motion, radius: &HitRadius| {
        in_range(
            user.motion.0.position,
            motion.0.position,
            radius.0,
            ability.range,
        )
    };
    // Only characters in the same zone can be targeted.
    let get = |t: Entity| targets.get(t).ok().filter(|(.., zone)| *zone == user.zone);
    match ability.target {
        TargetKind::Myself => Ok(user.entity),
        TargetKind::Ally => {
            let friendly = target.and_then(|t| {
                let (_, motion, radius, health, faction, _) = get(t)?;
                (!user.faction.can_harm(*faction) && !health.is_dead())
                    .then_some((t, motion, radius))
            });
            match friendly {
                Some((t, motion, radius)) if t != user.entity => {
                    if in_reach(motion, radius) {
                        Ok(t)
                    } else {
                        Err(Reject::OutOfRange)
                    }
                }
                _ => Ok(user.entity),
            }
        }
        TargetKind::DefeatedAlly => {
            let target = target.ok_or(Reject::NoTarget)?;
            let (_, motion, radius, health, faction, _) =
                get(target).ok_or(Reject::InvalidTarget)?;
            if user.faction.can_harm(*faction) || !health.is_dead() {
                return Err(Reject::InvalidTarget);
            }
            if !in_reach(motion, radius) {
                return Err(Reject::OutOfRange);
            }
            Ok(target)
        }
        TargetKind::Enemy => {
            let target = target.ok_or(Reject::NoTarget)?;
            let (_, motion, radius, health, faction, _) =
                get(target).ok_or(Reject::InvalidTarget)?;
            if target == user.entity || !user.faction.can_harm(*faction) {
                return Err(Reject::InvalidTarget);
            }
            if health.is_dead() {
                return Err(Reject::TargetDead);
            }
            if !in_reach(motion, radius) {
                return Err(Reject::OutOfRange);
            }
            Ok(target)
        }
    }
}

/// Everything needed to act on a request, bundled to keep signatures short.
pub struct UseContext<'a> {
    pub data: &'a GameData,
    pub now: f64,
    pub link: &'a mut Link,
    pub effects: &'a mut PendingEffects,
}

impl UseContext<'_> {
    fn reject(&mut self, player: Option<PlayerId>, reason: Reject) {
        if let Some(player) = player {
            self.link
                .to_client
                .push(ServerEvent::Rejected { player, reason });
        }
    }
}

/// A player pressed a hotbar slot.
pub fn request_slot(
    ctx: &mut UseContext,
    actor: Entity,
    slot: usize,
    target: Option<Entity>,
    actors: &mut Actors,
    targets: &Targets,
) {
    let Ok((player, hotbar, ..)) = actors.get(actor) else {
        return;
    };
    let player = player.copied();
    match hotbar.and_then(|h| h.0.get(slot).cloned().flatten()) {
        Some(ability) => use_ability(ctx, actor, &ability, target, actors, targets),
        None => ctx.reject(player, Reject::NotOnHotbar),
    }
}

/// Can this character start an ability right now (for enemies whose
/// timelines wait until they are free)?
pub fn can_act_now(ctx: &UseContext, actor: Entity, ability_id: &str, actors: &Actors) -> bool {
    let (Ok((.., actions, _, defeated, _)), Some(ability)) =
        (actors.get(actor), ctx.data.abilities.get(ability_id))
    else {
        return false;
    };
    !defeated && actions.check(ability, ctx.now).is_ok()
}

/// Try to use an ability now, or queue it if it is nearly ready.
pub fn use_ability(
    ctx: &mut UseContext,
    actor: Entity,
    ability_id: &str,
    target: Option<Entity>,
    actors: &mut Actors,
    targets: &Targets,
) {
    let Ok((player, _, faction, motion, zone, mut actions, input, defeated, changing_flame)) =
        actors.get_mut(actor)
    else {
        return;
    };
    let player = player.copied();
    if defeated {
        return ctx.reject(player, Reject::Dead);
    }
    if changing_flame {
        return ctx.reject(player, Reject::Busy);
    }
    let Some(ability) = ctx.data.abilities.get(ability_id) else {
        return ctx.reject(player, Reject::UnknownAbility);
    };
    let user = User {
        entity: actor,
        motion,
        faction: *faction,
        zone,
    };
    let target = match validate_target(ability, &user, target, targets) {
        Ok(target) => target,
        Err(reason) => return ctx.reject(player, reason),
    };
    match actions.check(ability, ctx.now) {
        Ok(()) => {
            let face = (ability.target == TargetKind::Enemy)
                .then(|| face_towards(motion, target, targets))
                .flatten();
            start(ctx, &user, ability, target, &mut actions, targets);
            if let Some(mut input) = input {
                input.face_once = face.or(input.face_once);
            }
        }
        Err(_) => {
            match actions.try_queue(ability, Some(target), ctx.now, &ctx.data.config.combat) {
                Ok(()) => {
                    if let Some(player) = player {
                        ctx.link.to_client.push(ServerEvent::Queued {
                            player,
                            ability: ability.id.clone(),
                        });
                    }
                }
                Err(reason) => ctx.reject(player, reason),
            }
        }
    }
}

fn start(
    ctx: &mut UseContext,
    user: &User,
    ability: &AbilityDef,
    target: Entity,
    actions: &mut ActionState,
    targets: &Targets,
) {
    let started = actions.begin(ability, Some(target), ctx.now, &ctx.data.config.combat);
    ctx.link.to_client.push(ServerEvent::AbilityUsed {
        user: user.entity,
        ability: ability.id.clone(),
        target: Some(target),
    });
    match started {
        Started::Instant { combo } => ctx.effects.resolutions.push(Resolution {
            source: user.entity,
            ability: ability.id.clone(),
            target,
            combo,
            markers: Vec::new(),
        }),
        Started::Casting => {
            if let Some(cast) = &actions.cast {
                place_telegraphs(ctx, user, ability, target, cast.ends, targets);
            }
        }
    }
}

/// Put down the ground markers for a telegraphed cast.
fn place_telegraphs(
    ctx: &mut UseContext,
    user: &User,
    ability: &AbilityDef,
    target: Entity,
    resolves: f64,
    targets: &Targets,
) {
    let Some(def) = ability.telegraph else {
        return;
    };
    let caster_position = user.motion.0.position;
    let position_of = |e: Entity| targets.get(e).ok().map(|(_, m, ..)| m.0.position);
    let target_position = position_of(target).unwrap_or(caster_position);
    let towards_target = {
        let offset = target_position - caster_position;
        let flat = Vec2::new(offset.x, offset.z);
        if flat.length_squared() > 1e-6 {
            yaw_from_direction(flat)
        } else {
            user.motion.0.yaw
        }
    };
    let marker = |origin: Vec3, yaw: f32, follow: Option<Entity>| Telegraph {
        shape: def.shape,
        placement: def.placement,
        origin,
        yaw,
        follow,
        caster: user.entity,
        ability: ability.id.clone(),
        starts: ctx.now,
        resolves,
    };
    let markers: Vec<Telegraph> = match def.placement {
        Placement::Caster => vec![marker(caster_position, user.motion.0.yaw, None)],
        Placement::Target => vec![marker(target_position, 0.0, None)],
        Placement::CasterTowardsTarget => vec![marker(caster_position, towards_target, None)],
        Placement::StackOnTarget => vec![marker(target_position, 0.0, Some(target))],
        // One marker on every living opponent in the same zone.
        Placement::SpreadOnEveryone => targets
            .iter()
            .filter(|(_, _, _, health, faction, zone)| {
                user.faction.can_harm(**faction) && !health.is_dead() && *zone == user.zone
            })
            .map(|(e, m, ..)| marker(m.0.position, 0.0, Some(e)))
            .collect(),
    };
    for telegraph in markers {
        ctx.effects.new_markers.push((telegraph, user.zone.clone()));
    }
}

/// The yaw that faces from `motion` towards `target`.
pub fn face_towards(motion: &Motion, target: Entity, targets: &Targets) -> Option<f32> {
    let (_, target_motion, ..) = targets.get(target).ok()?;
    let offset = target_motion.0.position - motion.0.position;
    let direction = Vec2::new(offset.x, offset.z);
    (direction.length_squared() > 1e-6).then(|| yaw_from_direction(direction))
}

/// Finish casts that are done and start queued actions that are ready.
pub fn process_actions(
    time: Res<Time>,
    data: Res<GameData>,
    mut link: ResMut<Link>,
    mut effects: ResMut<PendingEffects>,
    entities: Query<Entity, With<ActionState>>,
    markers: Query<(Entity, &Telegraph)>,
    mut actors: Actors,
    targets: Targets,
) {
    let now = time.elapsed_secs_f64();
    let mut ctx = UseContext {
        data: &data,
        now,
        link: &mut link,
        effects: &mut effects,
    };
    for entity in &entities {
        let Ok((.., mut actions, _, defeated, _)) = actors.get_mut(entity) else {
            continue;
        };
        if defeated {
            continue;
        }
        if let Some(cast) = actions.finish_cast(now)
            && let Some(target) = cast.target
        {
            let own_markers: Vec<Entity> = markers
                .iter()
                .filter(|(_, m)| m.caster == entity && m.ability == cast.ability)
                .map(|(e, _)| e)
                .collect();
            ctx.effects.resolutions.push(Resolution {
                source: entity,
                ability: cast.ability,
                target,
                combo: cast.combo,
                markers: own_markers,
            });
        }
        let Some(queued) = actions.queued.clone() else {
            continue;
        };
        let ready = data
            .abilities
            .get(&queued.ability)
            .is_some_and(|a| actions.check(a, now).is_ok());
        if ready {
            actions.queued = None;
            use_ability(
                &mut ctx,
                entity,
                &queued.ability,
                queued.target,
                &mut actors,
                &targets,
            );
        }
    }
}
