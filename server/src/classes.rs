//! Changing the flame in your lantern: switching class.

use bevy::prelude::*;
use shared::classes::{ChosenSpecs, CurrentClass};
use shared::combat::{ActionState, Reject};
use shared::components::PlayerId;
use shared::gamedata::GameData;
use shared::protocol::{Link, ServerEvent};
use shared::statuses::Statuses;

use crate::characters::{CombatClock, Defeated};

/// A flame change in progress. Moving cancels it.
#[derive(Component, Debug, Clone)]
pub struct FlameChange {
    pub class: String,
    pub ends: f64,
}

/// A player asked to change class.
pub fn request_change(
    commands: &mut Commands,
    link: &mut Link,
    data: &GameData,
    now: f64,
    player: PlayerId,
    entity: Entity,
    class: &str,
    state: (&CurrentClass, &ActionState, &CombatClock, bool),
) {
    let (current, actions, clock, defeated) = state;
    let reject = |link: &mut Link, reason| {
        link.to_client
            .push(ServerEvent::Rejected { player, reason })
    };
    if !data.classes.contains_key(class) {
        return reject(link, Reject::UnknownClass);
    }
    if current.class == class {
        return reject(link, Reject::AlreadyThatClass);
    }
    if defeated {
        return reject(link, Reject::Dead);
    }
    if clock.in_combat(now, data.config.combat.combat_timeout) {
        return reject(link, Reject::InCombat);
    }
    if actions.cast.is_some() {
        return reject(link, Reject::Busy);
    }
    commands.entity(entity).insert(FlameChange {
        class: class.to_owned(),
        ends: now + f64::from(data.config.combat.flame_change_time),
    });
    link.to_client.push(ServerEvent::FlameChangeStarted {
        user: entity,
        class: class.to_owned(),
    });
}

/// Specialization switches asked for this tick.
#[derive(Resource, Default, Debug)]
pub struct PendingSpecs(pub Vec<(PlayerId, Entity, String)>);

/// Carry out specialization switches.
pub fn handle_spec_changes(
    time: Res<Time>,
    data: Res<GameData>,
    mut pending: ResMut<PendingSpecs>,
    mut link: ResMut<Link>,
    mut players: Query<(
        &mut CurrentClass,
        &mut ChosenSpecs,
        &CombatClock,
        &ActionState,
        Has<Defeated>,
    )>,
) {
    let now = time.elapsed_secs_f64();
    for (player, entity, spec) in std::mem::take(&mut pending.0) {
        let Ok((mut current, mut chosen, clock, actions, defeated)) = players.get_mut(entity)
        else {
            continue;
        };
        let casting = actions.cast.is_some();
        request_spec(
            &mut link,
            &data,
            now,
            player,
            entity,
            &spec,
            (&mut current, &mut chosen, clock, defeated, casting),
        );
    }
}

/// A player asked to switch their class's specialization: free and
/// instant, but only out of combat.
pub fn request_spec(
    link: &mut Link,
    data: &GameData,
    now: f64,
    player: PlayerId,
    entity: Entity,
    spec: &str,
    state: (
        &mut CurrentClass,
        &mut ChosenSpecs,
        &CombatClock,
        bool,
        bool,
    ),
) {
    let (current, chosen, clock, defeated, casting) = state;
    let reject = |link: &mut Link, reason| {
        link.to_client
            .push(ServerEvent::Rejected { player, reason })
    };
    let Some(class) = data.classes.get(&current.class) else {
        return reject(link, Reject::UnknownClass);
    };
    if !class.specializations.iter().any(|s| s.id == spec) {
        return reject(link, Reject::UnknownSpec);
    }
    if current.spec == spec {
        return reject(link, Reject::AlreadyThatSpec);
    }
    if defeated {
        return reject(link, Reject::Dead);
    }
    if clock.in_combat(now, data.config.combat.combat_timeout) {
        return reject(link, Reject::InCombat);
    }
    if casting {
        return reject(link, Reject::Busy);
    }
    // The hotbar follows in `progression::refresh_stats`.
    current.spec = spec.to_owned();
    chosen.0.insert(current.class.clone(), spec.to_owned());
    link.to_client.push(ServerEvent::SpecChanged {
        user: entity,
        spec: spec.to_owned(),
    });
}

/// Finished flame changes switch the character's class; timers and
/// statuses reset. (Stats and hotbar follow in `progression::refresh_stats`.)
pub fn finish_flame_changes(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut link: ResMut<Link>,
    mut changing: Query<
        (
            Entity,
            &FlameChange,
            &mut CurrentClass,
            &mut ActionState,
            &mut Statuses,
            &ChosenSpecs,
        ),
        Without<Defeated>,
    >,
) {
    let now = time.elapsed_secs_f64();
    for (entity, change, mut current, mut actions, mut statuses, chosen) in &mut changing {
        if now < change.ends {
            continue;
        }
        commands.entity(entity).remove::<FlameChange>();
        let Some(class) = data.classes.get(&change.class) else {
            continue;
        };
        // Stats follow in `progression::refresh_stats` (class level, gear).
        *current = CurrentClass {
            class: change.class.clone(),
            spec: chosen.spec_of(&change.class, class),
        };
        actions.reset();
        statuses.0.clear();
        link.to_client.push(ServerEvent::ClassChanged {
            user: entity,
            class: change.class.clone(),
        });
    }
}
