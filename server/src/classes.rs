//! Changing the flame in your lantern: switching class.

use bevy::prelude::*;
use shared::classes::CurrentClass;
use shared::combat::{ActionState, Reject};
use shared::components::{Hotbar, PlayerId};
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

/// Finished flame changes switch the character's class: new stats, new
/// hotbar, timers and statuses reset.
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
            &mut Hotbar,
            &mut ActionState,
            &mut Statuses,
        ),
        Without<Defeated>,
    >,
) {
    let now = time.elapsed_secs_f64();
    for (entity, change, mut current, mut hotbar, mut actions, mut statuses) in &mut changing {
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
            spec: class.default_spec.clone(),
        };
        hotbar.0 = data.hotbar(class, &class.default_spec);
        actions.reset();
        statuses.0.clear();
        link.to_client.push(ServerEvent::ClassChanged {
            user: entity,
            class: change.class.clone(),
        });
    }
}
