//! Enemies: spawning from zone data, picking who to attack by threat,
//! using their abilities, and calming down when left alone.

use bevy::prelude::*;
use shared::classes::Stats;
use shared::combat::{ActionState, Health};
use shared::components::{CharacterName, Faction, HitRadius, Motion, VisualKey, Zone};
use shared::gamedata::{GameData, Zones};
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

/// Spawn every enemy placed in every zone.
pub fn spawn_enemies(mut commands: Commands, data: Res<GameData>, zones: Res<Zones>) {
    for (zone, level) in &zones.0 {
        for spawn in &level.spawns {
            spawn_enemy(
                &mut commands,
                &data,
                &spawn.enemy,
                zone,
                spawn.position,
                spawn.yaw,
            );
        }
    }
}

/// Spawn one enemy of type `id` (file name in `assets/data/enemies/`).
/// Returns `None` if there is no such enemy (data is checked at load, so
/// this shouldn't happen).
pub fn spawn_enemy(
    commands: &mut Commands,
    data: &GameData,
    id: &str,
    zone: &str,
    position: Vec3,
    yaw: f32,
) -> Option<Entity> {
    let def = data.enemies.get(id)?;
    let mut entity = commands.spawn((
        EnemyKind(id.to_owned()),
        CharacterName(def.name.clone()),
        Motion(MoveState {
            yaw,
            ..MoveState::spawn_at(position)
        }),
        Zone(zone.to_owned()),
        Faction::Enemy,
        HitRadius(def.hit_radius),
        Health::full(def.max_health),
        Stats {
            max_health: def.max_health,
            power: def.power,
            threat_multiplier: 1.0,
            crit_chance: data.config.combat.crit_chance,
            guard: 0.0,
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
    Some(entity.id())
}

/// Enemies use their abilities on whoever they are angriest at.
pub fn enemy_brains(
    time: Res<Time>,
    data: Res<GameData>,
    mut link: ResMut<Link>,
    mut effects: ResMut<PendingEffects>,
    mut brains: Query<(Entity, &Zone, &mut EnemyBrain, &mut ThreatTable), Without<Defeated>>,
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
    for (enemy, zone, mut brain, mut threat) in &mut brains {
        // Forget anyone who fell, vanished or left the zone.
        threat.0.retain(|who, _| {
            alive.get(*who).is_ok() && targets.get(*who).is_ok_and(|(.., z)| z == zone)
        });
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
    enemies: Query<(Entity, &ThreatTable, &ActionState), (With<EnemyBrain>, Without<Defeated>)>,
    mut motions: Query<&mut Motion>,
) {
    for (enemy, threat, actions) in &enemies {
        // Hold still while casting so the attack goes where it was aimed.
        if actions.cast.is_some() {
            continue;
        }
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
