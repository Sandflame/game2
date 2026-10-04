//! Player characters: joining, moving, regeneration, defeat and revival.

use std::collections::HashMap;

use bevy::prelude::*;
use shared::classes::CurrentClass;
use shared::combat::Reject;
use shared::combat::{ActionState, Health};
use shared::components::{
    CharacterName, Faction, HitRadius, Hotbar, Motion, PlayerId, VisualKey, Zone,
};
use shared::gamedata::{GameData, Zones};
use shared::movement::{self, MoveInput, MoveState};
use shared::protocol::{Link, ServerEvent};
use shared::statuses::Statuses;
use shared::threat::ThreatTable;

use crate::classes::FlameChange;

/// Finds a player's character from their id.
#[derive(Resource, Default, Debug)]
pub struct PlayerIndex(pub HashMap<PlayerId, Entity>);

/// The latest movement keys a player sent.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct PlayerInput {
    pub input: MoveInput,
    /// Turn to face this way on the next tick (set when using an ability).
    pub face_once: Option<f32>,
}

/// When this character last took part in a fight.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct CombatClock {
    pub last_hostile: Option<f64>,
    /// Fractions of a health point regenerated but not yet added.
    pub regen_carry: f32,
}

impl CombatClock {
    pub fn in_combat(&self, now: f64, timeout: f32) -> bool {
        self.last_hostile
            .is_some_and(|t| now - t < f64::from(timeout))
    }

    pub fn mark(&mut self, now: f64) {
        self.last_hostile = Some(now);
    }
}

/// This character is down. Players get back up after a while.
#[derive(Component, Debug, Clone, Copy)]
pub struct Defeated {
    pub at: f64,
}

/// Create a player's character, or return the existing one.
pub fn spawn_player(
    commands: &mut Commands,
    index: &mut PlayerIndex,
    link: &mut Link,
    data: &GameData,
    zones: &Zones,
    player: PlayerId,
    name: &str,
) {
    if let Some(&entity) = index.0.get(&player) {
        link.to_client.push(ServerEvent::Joined { player, entity });
        return;
    }
    let class_id = data.player.start_class.clone();
    let zone = data.player.start_zone.clone();
    // Both checked when the data was loaded.
    let (Some(class), Some(level)) = (data.classes.get(&class_id), zones.get(&zone)) else {
        return;
    };
    let name = if name.trim().is_empty() {
        "Adventurer"
    } else {
        name.trim()
    };
    let stats = class.stats();
    let entity = commands
        .spawn((
            (
                player,
                CharacterName(name.to_owned()),
                Motion(MoveState::spawn_at(level.spawn_point)),
                Zone(zone),
                PlayerInput::default(),
                Faction::Player,
                HitRadius(data.player.hit_radius),
                VisualKey("player".to_owned()),
            ),
            (
                Health::full(stats.max_health),
                stats,
                ActionState::default(),
                Statuses::default(),
                CombatClock::default(),
                Hotbar(data.hotbar(class, &class.default_spec)),
                CurrentClass {
                    class: class_id,
                    spec: class.default_spec.clone(),
                },
            ),
        ))
        .id();
    index.0.insert(player, entity);
    link.to_client.push(ServerEvent::Joined { player, entity });
}

pub fn move_characters(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    zones: Res<Zones>,
    mut link: ResMut<Link>,
    mut characters: Query<(
        Entity,
        &Zone,
        &mut PlayerInput,
        &mut Motion,
        &mut ActionState,
        Has<Defeated>,
        Option<&FlameChange>,
    )>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs();
    for (entity, zone, mut player_input, mut motion, mut actions, defeated, flame_change) in
        &mut characters
    {
        let Some(level) = zones.get(&zone.0) else {
            continue;
        };
        let mut input = player_input.input;
        player_input.input.jump = false;
        if defeated {
            continue;
        }
        let wants_to_move = input.direction != Vec2::ZERO || input.jump;
        if wants_to_move {
            if let Some(cast) = actions.interrupt(now) {
                link.to_client.push(ServerEvent::CastInterrupted {
                    user: entity,
                    ability: cast.ability,
                });
            }
            if flame_change.is_some() {
                commands.entity(entity).remove::<FlameChange>();
                link.to_client.push(ServerEvent::CastInterrupted {
                    user: entity,
                    ability: "flame_change".to_owned(),
                });
            }
        }
        if let Some(yaw) = player_input.face_once.take() {
            input.face_yaw = Some(yaw);
        }
        motion.0 = movement::step(motion.0, input, &data.config.movement, level, dt);
    }
}

/// Characters whose health reached zero are defeated: casts stop, statuses
/// fall off, and enemies stop paying attention to them.
pub fn handle_defeats(
    mut commands: Commands,
    time: Res<Time>,
    mut link: ResMut<Link>,
    mut fallen: Query<(Entity, &Health, &mut ActionState, &mut Statuses), Without<Defeated>>,
    mut enemies: Query<&mut ThreatTable>,
) {
    let now = time.elapsed_secs_f64();
    for (entity, health, mut actions, mut statuses) in &mut fallen {
        if !health.is_dead() {
            continue;
        }
        actions.interrupt(now);
        statuses.0.clear();
        commands
            .entity(entity)
            .insert(Defeated { at: now })
            .remove::<FlameChange>();
        for mut table in &mut enemies {
            table.forget(entity);
        }
        link.to_client.push(ServerEvent::Defeated { entity });
    }
}

/// Defeated players get back up after a while (raising arrives in Milestone 4).
pub fn revive(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    zones: Res<Zones>,
    mut link: ResMut<Link>,
    mut players: Query<(Entity, &Defeated, &Zone, &mut Health), With<PlayerId>>,
) {
    let now = time.elapsed_secs_f64();
    for (entity, defeated, zone, mut health) in &mut players {
        // In trials you have to be raised (or the fight resets).
        let allowed = zones.get(&zone.0).is_some_and(|z| z.revive_in_place);
        if allowed && now - defeated.at >= f64::from(data.config.combat.revive_after) {
            *health = Health::full(health.max);
            commands.entity(entity).remove::<Defeated>();
            link.to_client.push(ServerEvent::Revived { entity });
        }
    }
}

/// A player pressed the interact key: use the portal they are standing in.
pub fn interact(
    commands: &mut Commands,
    link: &mut Link,
    data: &GameData,
    zones: &Zones,
    now: f64,
    player: PlayerId,
    entity: Entity,
    state: (&Zone, &mut Motion, &CombatClock, bool),
) {
    let (zone, motion, clock, defeated) = state;
    let reject = |link: &mut Link, reason| {
        link.to_client
            .push(ServerEvent::Rejected { player, reason })
    };
    let portal = zones
        .get(&zone.0)
        .and_then(|level| level.portal_at(motion.0.position));
    let Some(portal) = portal else {
        return reject(link, Reject::NothingHere);
    };
    if defeated {
        return reject(link, Reject::Dead);
    }
    if clock.in_combat(now, data.config.combat.combat_timeout) {
        return reject(link, Reject::InCombat);
    }
    motion.0 = MoveState::spawn_at(portal.arrive);
    commands.entity(entity).insert(Zone(portal.to.clone()));
    link.to_client.push(ServerEvent::ZoneChanged {
        entity,
        zone: portal.to.clone(),
    });
}

/// Characters who left a zone are forgotten by its enemies.
pub fn forget_absent(
    mut enemies: Query<(&Zone, &mut ThreatTable)>,
    characters: Query<&Zone, Without<ThreatTable>>,
) {
    for (zone, mut table) in &mut enemies {
        table
            .0
            .retain(|who, _| characters.get(*who).map_or(true, |z| z == zone));
    }
}

/// Out of combat, players slowly heal.
pub fn regenerate(
    time: Res<Time>,
    data: Res<GameData>,
    mut players: Query<(&mut Health, &mut CombatClock), Without<Defeated>>,
) {
    let now = time.elapsed_secs_f64();
    let combat = &data.config.combat;
    for (mut health, mut clock) in &mut players {
        if clock.in_combat(now, combat.combat_timeout) || health.current == health.max {
            clock.regen_carry = 0.0;
            continue;
        }
        let gained =
            health.max as f32 * combat.out_of_combat_regen * time.delta_secs() + clock.regen_carry;
        let whole = gained.floor();
        clock.regen_carry = gained - whole;
        health.heal(whole as u32);
    }
}
