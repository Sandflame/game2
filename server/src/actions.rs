//! Using abilities: checking targets and timing, starting casts, running
//! queued actions, and applying damage.

use bevy::prelude::*;
use shared::combat::{
    AbilityDef, ActionState, Health, Reject, Started, TargetKind, in_range, potency_damage,
};
use shared::components::{Faction, HitRadius, Hotbar, Motion, PlayerId};
use shared::gamedata::GameData;
use shared::movement::yaw_from_direction;
use shared::protocol::{Link, ServerEvent};

use crate::characters::{DummyReset, PlayerInput};

/// Damage waiting to be applied at the end of this tick.
#[derive(Resource, Default, Debug)]
pub struct PendingHits(pub Vec<Hit>);

#[derive(Debug, Clone)]
pub struct Hit {
    pub source: Entity,
    pub target: Entity,
    pub ability: String,
}

/// Read-only view of everything that can be targeted.
pub type Targets<'w, 's> = Query<
    'w,
    's,
    (
        &'static Motion,
        &'static HitRadius,
        &'static Health,
        &'static Faction,
    ),
>;

/// Characters that can use abilities.
pub type Actors<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        Option<&'static PlayerId>,
        &'static Hotbar,
        &'static Health,
        &'static Faction,
        &'static Motion,
        &'static mut ActionState,
        &'static mut PlayerInput,
    ),
>;

/// Check that `target` is a valid, living, in-range target for `ability`.
pub fn validate_target(
    ability: &AbilityDef,
    user: Entity,
    user_motion: &Motion,
    user_faction: Faction,
    target: Option<Entity>,
    targets: &Targets,
) -> Result<Option<Entity>, Reject> {
    match ability.target {
        TargetKind::Myself => Ok(None),
        TargetKind::Enemy => {
            let target = target.ok_or(Reject::NoTarget)?;
            let (motion, radius, health, faction) =
                targets.get(target).map_err(|_| Reject::InvalidTarget)?;
            if target == user || !user_faction.can_harm(*faction) {
                return Err(Reject::InvalidTarget);
            }
            if health.is_dead() {
                return Err(Reject::TargetDead);
            }
            if !in_range(
                user_motion.0.position,
                motion.0.position,
                radius.0,
                ability.range,
            ) {
                return Err(Reject::OutOfRange);
            }
            Ok(Some(target))
        }
    }
}

/// Everything needed to act on a request, bundled to keep signatures short.
pub struct UseContext<'a> {
    pub data: &'a GameData,
    pub now: f64,
    pub link: &'a mut Link,
    pub hits: &'a mut PendingHits,
}

/// A player pressed a hotbar slot.
pub fn request_ability(
    ctx: &mut UseContext,
    actor: Entity,
    slot: usize,
    target: Option<Entity>,
    actors: &mut Actors,
    targets: &Targets,
) {
    let Ok((_, player, hotbar, health, faction, motion, mut actions, mut input)) =
        actors.get_mut(actor)
    else {
        return;
    };
    let player = player.copied();
    let reject = |ctx: &mut UseContext, reason| {
        if let Some(player) = player {
            ctx.link
                .to_client
                .push(ServerEvent::Rejected { player, reason });
        }
    };
    if health.is_dead() {
        return reject(ctx, Reject::Dead);
    }
    let Some(ability_id) = hotbar.0.get(slot).cloned().flatten() else {
        return reject(ctx, Reject::NotOnHotbar);
    };
    let Some(ability) = ctx.data.abilities.get(&ability_id) else {
        return reject(ctx, Reject::UnknownAbility);
    };
    let target = match validate_target(ability, actor, motion, *faction, target, targets) {
        Ok(target) => target,
        Err(reason) => return reject(ctx, reason),
    };
    match actions.check(ability, ctx.now) {
        Ok(()) => {
            let face = face_towards(motion, target, targets);
            start(ctx, actor, ability, target, &mut actions);
            input.face_once = face.or(input.face_once);
        }
        Err(_) => match actions.try_queue(ability, target, ctx.now, &ctx.data.config.combat) {
            Ok(()) => {
                if let Some(player) = player {
                    ctx.link.to_client.push(ServerEvent::Queued {
                        player,
                        ability: ability.id.clone(),
                    });
                }
            }
            Err(reason) => reject(ctx, reason),
        },
    }
}

fn start(
    ctx: &mut UseContext,
    actor: Entity,
    ability: &AbilityDef,
    target: Option<Entity>,
    actions: &mut ActionState,
) {
    let started = actions.begin(ability, target, ctx.now, &ctx.data.config.combat);
    ctx.link.to_client.push(ServerEvent::AbilityUsed {
        user: actor,
        ability: ability.id.clone(),
        target,
    });
    if started == Started::Instant
        && let Some(target) = target
    {
        ctx.hits.0.push(Hit {
            source: actor,
            target,
            ability: ability.id.clone(),
        });
    }
}

/// The yaw that faces from `motion` towards `target`, if there is one.
fn face_towards(motion: &Motion, target: Option<Entity>, targets: &Targets) -> Option<f32> {
    let (target_motion, ..) = targets.get(target?).ok()?;
    let offset = target_motion.0.position - motion.0.position;
    let direction = Vec2::new(offset.x, offset.z);
    (direction.length_squared() > 1e-6).then(|| yaw_from_direction(direction))
}

/// Finish casts that are done and start queued actions that are ready.
pub fn process_actions(
    time: Res<Time>,
    data: Res<GameData>,
    mut link: ResMut<Link>,
    mut hits: ResMut<PendingHits>,
    mut actors: Actors,
    targets: Targets,
) {
    let now = time.elapsed_secs_f64();
    let mut ctx = UseContext {
        data: &data,
        now,
        link: &mut link,
        hits: &mut hits,
    };
    for (actor, player, _, health, faction, motion, mut actions, mut input) in &mut actors {
        if health.is_dead() {
            continue;
        }
        if let Some(cast) = actions.finish_cast(now)
            && let Some(target) = cast.target
            && targets.get(target).is_ok_and(|(_, _, h, _)| !h.is_dead())
        {
            ctx.hits.0.push(Hit {
                source: actor,
                target,
                ability: cast.ability,
            });
        }

        let Some(queued) = actions.queued.clone() else {
            continue;
        };
        let Some(ability) = data.abilities.get(&queued.ability) else {
            actions.queued = None;
            continue;
        };
        if actions.check(ability, now).is_err() {
            continue;
        }
        actions.queued = None;
        match validate_target(ability, actor, motion, *faction, queued.target, &targets) {
            Ok(target) => {
                let face = face_towards(motion, target, &targets);
                start(&mut ctx, actor, ability, target, &mut actions);
                input.face_once = face.or(input.face_once);
            }
            Err(reason) => {
                if let Some(player) = player {
                    ctx.link.to_client.push(ServerEvent::Rejected {
                        player: *player,
                        reason,
                    });
                }
            }
        }
    }
}

/// Apply this tick's damage and report it.
pub fn apply_hits(
    time: Res<Time>,
    data: Res<GameData>,
    mut link: ResMut<Link>,
    mut hits: ResMut<PendingHits>,
    mut targets: Query<(&mut Health, Option<&mut DummyReset>)>,
) {
    let now = time.elapsed_secs_f64();
    for hit in hits.0.drain(..) {
        let Some(ability) = data.abilities.get(&hit.ability) else {
            continue;
        };
        let Ok((mut health, reset)) = targets.get_mut(hit.target) else {
            continue;
        };
        if health.is_dead() {
            continue;
        }
        let amount = potency_damage(ability.potency, &data.config.combat);
        health.damage(amount);
        if let Some(mut reset) = reset {
            reset.last_hit = now;
        }
        link.to_client.push(ServerEvent::Damage {
            source: hit.source,
            target: hit.target,
            amount,
            ability: hit.ability,
        });
    }
}
