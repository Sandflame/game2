//! Spawning characters, moving them, and training-dummy upkeep.

use std::collections::HashMap;

use bevy::prelude::*;
use shared::combat::{ActionState, Health};
use shared::components::{
    CharacterName, Faction, HOTBAR_SLOTS, HitRadius, Hotbar, Motion, PlayerId, VisualKey,
};
use shared::gamedata::GameData;
use shared::level::Level;
use shared::movement::{self, MoveInput, MoveState};
use shared::protocol::{Link, ServerEvent};

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

/// Which enemy type this is (file name in `assets/data/enemies/`).
#[derive(Component, Debug, Clone)]
pub struct EnemyKind(pub String);

/// Training dummies heal to full when left alone.
#[derive(Component, Debug, Clone, Copy)]
pub struct DummyReset {
    pub after: f32,
    pub last_hit: f64,
}

pub fn spawn_enemies(mut commands: Commands, data: Res<GameData>, level: Res<Level>) {
    for spawn in &level.spawns {
        // References were checked when the level was loaded.
        let Some(def) = data.enemies.get(&spawn.enemy) else {
            continue;
        };
        let mut entity = commands.spawn((
            EnemyKind(spawn.enemy.clone()),
            CharacterName(def.name.clone()),
            Motion(MoveState {
                yaw: spawn.yaw,
                ..MoveState::spawn_at(spawn.position)
            }),
            Faction::Enemy,
            HitRadius(def.hit_radius),
            Health::full(def.max_health),
            VisualKey(def.visual.clone()),
        ));
        if let Some(after) = def.reset_after {
            entity.insert(DummyReset {
                after,
                last_hit: 0.0,
            });
        }
    }
}

/// Create a player's character, or return the existing one.
pub fn spawn_player(
    commands: &mut Commands,
    index: &mut PlayerIndex,
    link: &mut Link,
    data: &GameData,
    level: &Level,
    player: PlayerId,
    name: &str,
) {
    if let Some(&entity) = index.0.get(&player) {
        link.to_client.push(ServerEvent::Joined { player, entity });
        return;
    }
    let mut hotbar: Vec<Option<String>> = data.player.hotbar.iter().cloned().map(Some).collect();
    hotbar.resize(HOTBAR_SLOTS, None);
    let name = if name.trim().is_empty() {
        "Adventurer"
    } else {
        name.trim()
    };
    let entity = commands
        .spawn((
            player,
            CharacterName(name.to_owned()),
            Motion(MoveState::spawn_at(level.spawn_point)),
            PlayerInput::default(),
            Faction::Player,
            HitRadius(data.player.hit_radius),
            Health::full(data.player.max_health),
            ActionState::default(),
            Hotbar(hotbar),
            VisualKey("player".to_owned()),
        ))
        .id();
    index.0.insert(player, entity);
    link.to_client.push(ServerEvent::Joined { player, entity });
}

pub fn move_characters(
    time: Res<Time>,
    data: Res<GameData>,
    level: Res<Level>,
    mut link: ResMut<Link>,
    mut characters: Query<(
        Entity,
        &mut PlayerInput,
        &mut Motion,
        &mut ActionState,
        &Health,
    )>,
) {
    let now = time.elapsed_secs_f64();
    let dt = time.delta_secs();
    for (entity, mut player_input, mut motion, mut actions, health) in &mut characters {
        let mut input = player_input.input;
        player_input.input.jump = false;
        if health.is_dead() {
            continue;
        }
        let wants_to_move = input.direction != Vec2::ZERO || input.jump;
        if wants_to_move && let Some(cast) = actions.interrupt(now) {
            link.to_client.push(ServerEvent::CastInterrupted {
                user: entity,
                ability: cast.ability,
            });
        }
        if let Some(yaw) = player_input.face_once.take() {
            input.face_yaw = Some(yaw);
        }
        motion.0 = movement::step(motion.0, input, &data.config.movement, &level, dt);
    }
}

pub fn reset_dummies(time: Res<Time>, mut dummies: Query<(&mut Health, &DummyReset)>) {
    let now = time.elapsed_secs_f64();
    for (mut health, reset) in &mut dummies {
        if health.current != health.max && now - reset.last_hit >= f64::from(reset.after) {
            *health = Health::full(health.max);
        }
    }
}
