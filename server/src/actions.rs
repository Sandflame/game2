//! Using abilities: checking targets and timing, starting casts, and
//! running queued actions. When an ability goes off, its effects are
//! handed to `effects.rs` to land.

use bevy::prelude::*;
use shared::abilities::{AbilityDef, TargetKind};
use shared::combat::{ActionState, Health, Reject, Started, in_range};
use shared::components::{Faction, HitRadius, Hotbar, Motion, PlayerId};
use shared::gamedata::GameData;
use shared::movement::yaw_from_direction;
use shared::protocol::{Link, ServerEvent};

use crate::characters::{Defeated, PlayerInput};
use crate::classes::FlameChange;
use crate::effects::{PendingEffects, Resolution};

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

/// Characters that can use abilities (players and enemies).
pub type Actors<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static PlayerId>,
        Option<&'static Hotbar>,
        &'static Faction,
        &'static Motion,
        &'static mut ActionState,
        Option<&'static mut PlayerInput>,
        Has<Defeated>,
        Has<FlameChange>,
    ),
>;

/// Work out who an ability lands on, or why it can't be used.
/// `Myself` abilities, and `Ally` abilities without a friendly target,
/// land on the user.
pub fn validate_target(
    ability: &AbilityDef,
    user: Entity,
    user_motion: &Motion,
    user_faction: Faction,
    target: Option<Entity>,
    targets: &Targets,
) -> Result<Entity, Reject> {
    let in_reach = |motion: &Motion, radius: &HitRadius| {
        in_range(
            user_motion.0.position,
            motion.0.position,
            radius.0,
            ability.range,
        )
    };
    match ability.target {
        TargetKind::Myself => Ok(user),
        TargetKind::Ally => {
            let friendly = target.and_then(|t| {
                let (motion, radius, health, faction) = targets.get(t).ok()?;
                (!user_faction.can_harm(*faction) && !health.is_dead())
                    .then_some((t, motion, radius))
            });
            match friendly {
                Some((t, motion, radius)) if t != user => {
                    if in_reach(motion, radius) {
                        Ok(t)
                    } else {
                        Err(Reject::OutOfRange)
                    }
                }
                _ => Ok(user),
            }
        }
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

/// Try to use an ability now, or queue it if it is nearly ready.
pub fn use_ability(
    ctx: &mut UseContext,
    actor: Entity,
    ability_id: &str,
    target: Option<Entity>,
    actors: &mut Actors,
    targets: &Targets,
) {
    let Ok((player, _, faction, motion, mut actions, input, defeated, changing_flame)) =
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
    let target = match validate_target(ability, actor, motion, *faction, target, targets) {
        Ok(target) => target,
        Err(reason) => return ctx.reject(player, reason),
    };
    match actions.check(ability, ctx.now) {
        Ok(()) => {
            let face = (ability.target == TargetKind::Enemy)
                .then(|| face_towards(motion, target, targets))
                .flatten();
            start(ctx, actor, ability, target, &mut actions);
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
    actor: Entity,
    ability: &AbilityDef,
    target: Entity,
    actions: &mut ActionState,
) {
    let started = actions.begin(ability, Some(target), ctx.now, &ctx.data.config.combat);
    ctx.link.to_client.push(ServerEvent::AbilityUsed {
        user: actor,
        ability: ability.id.clone(),
        target: Some(target),
    });
    if let Started::Instant { combo } = started {
        ctx.effects.0.push(Resolution {
            source: actor,
            ability: ability.id.clone(),
            target,
            combo,
        });
    }
}

/// The yaw that faces from `motion` towards `target`.
pub fn face_towards(motion: &Motion, target: Entity, targets: &Targets) -> Option<f32> {
    let (target_motion, ..) = targets.get(target).ok()?;
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
        let Ok((_, _, _, _, mut actions, _, defeated, _)) = actors.get_mut(entity) else {
            continue;
        };
        if defeated {
            continue;
        }
        if let Some(cast) = actions.finish_cast(now)
            && let Some(target) = cast.target
            && targets.get(target).is_ok_and(|(_, _, h, _)| !h.is_dead())
        {
            ctx.effects.0.push(Resolution {
                source: entity,
                ability: cast.ability,
                target,
                combo: cast.combo,
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
