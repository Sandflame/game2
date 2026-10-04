//! Enemies: spawning from zone data, picking who to attack by threat,
//! using their abilities, and calming down when left alone.

use bevy::prelude::*;
use shared::classes::Stats;
use shared::combat::{ActionState, Health};
use shared::components::{CharacterName, Faction, HitRadius, Motion, VisualKey};
use shared::gamedata::GameData;
use shared::level::Level;
use shared::movement::MoveState;
use shared::protocol::Link;
use shared::statuses::Statuses;
use shared::threat::ThreatTable;

use crate::actions::{Actors, Targets, UseContext, use_ability};
use crate::characters::Defeated;
use crate::effects::PendingEffects;
use shared::movement::yaw_from_direction;

/// Which enemy type this is (file name in `assets/data/enemies/`).
#[derive(Component, Debug, Clone)]
pub struct EnemyKind(pub String);

/// Heal to full and forget all threat after this long without being hit.
#[derive(Component, Debug, Clone, Copy)]
pub struct ResetWhenIdle {
    pub after: f32,
    pub last_hit: f64,
}

/// An enemy's simple rotation: each ability is used every so often.
#[derive(Component, Debug, Clone)]
pub struct EnemyBrain {
    pub actions: Vec<BrainAction>,
}

#[derive(Debug, Clone)]
pub struct BrainAction {
    pub ability: String,
    pub every: f32,
    /// When it is next used (`None` until the enemy is in a fight).
    pub next_at: Option<f64>,
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
            Stats {
                max_health: def.max_health,
                power: def.power,
                threat_multiplier: 1.0,
            },
            Statuses::default(),
            ThreatTable::default(),
            ActionState::default(),
            VisualKey(def.visual.clone()),
        ));
        if let Some(after) = def.reset_after {
            entity.insert(ResetWhenIdle {
                after,
                last_hit: 0.0,
            });
        }
        if !def.actions.is_empty() {
            entity.insert(EnemyBrain {
                actions: def
                    .actions
                    .iter()
                    .map(|a| BrainAction {
                        ability: a.ability.clone(),
                        every: a.every,
                        next_at: None,
                    })
                    .collect(),
            });
        }
    }
}

/// Enemies use their abilities on whoever they are angriest at.
pub fn enemy_brains(
    time: Res<Time>,
    data: Res<GameData>,
    mut link: ResMut<Link>,
    mut effects: ResMut<PendingEffects>,
    mut brains: Query<(Entity, &mut EnemyBrain, &mut ThreatTable), Without<Defeated>>,
    alive: Query<(), Without<Defeated>>,
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
    for (enemy, mut brain, mut threat) in &mut brains {
        threat
            .0
            .retain(|who, _| alive.get(*who).is_ok() && targets.get(*who).is_ok());
        let Some(target) = threat.top() else {
            for action in &mut brain.actions {
                action.next_at = None;
            }
            continue;
        };
        for action in &mut brain.actions {
            // The first attack comes one full interval after the fight starts.
            let next = *action.next_at.get_or_insert(now + f64::from(action.every));
            if now >= next {
                action.next_at = Some(now + f64::from(action.every));
                use_ability(
                    &mut ctx,
                    enemy,
                    &action.ability,
                    Some(target),
                    &mut actors,
                    &targets,
                );
            }
        }
    }
}

/// Enemies turn to face whoever they are angriest at.
pub fn face_targets(
    enemies: Query<(Entity, &ThreatTable), (With<EnemyBrain>, Without<Defeated>)>,
    mut motions: Query<&mut Motion>,
) {
    for (enemy, threat) in &enemies {
        let Some(target) = threat.top() else {
            continue;
        };
        let (Ok(own), Ok(other)) = (motions.get(enemy), motions.get(target)) else {
            continue;
        };
        let offset = other.0.position - own.0.position;
        let direction = Vec2::new(offset.x, offset.z);
        if direction.length_squared() > 1e-6
            && let Ok(mut motion) = motions.get_mut(enemy)
        {
            motion.0.yaw = yaw_from_direction(direction);
        }
    }
}

/// Enemies left alone for a while heal up and forget everyone.
pub fn reset_idle_enemies(
    mut commands: Commands,
    time: Res<Time>,
    mut enemies: Query<(
        Entity,
        &ResetWhenIdle,
        &mut Health,
        &mut ThreatTable,
        &mut Statuses,
        &mut ActionState,
        Has<Defeated>,
    )>,
) {
    let now = time.elapsed_secs_f64();
    for (entity, reset, mut health, mut threat, mut statuses, mut actions, defeated) in &mut enemies
    {
        let disturbed = health.current != health.max || !threat.is_empty() || defeated;
        if disturbed && now - reset.last_hit >= f64::from(reset.after) {
            *health = Health::full(health.max);
            threat.clear();
            statuses.0.clear();
            actions.reset();
            commands.entity(entity).remove::<Defeated>();
        }
    }
}
