//! Enemies: spawning from zone data, picking who to attack by threat,
//! using their abilities, and calming down when left alone.

use bevy::prelude::*;
use shared::classes::Stats;
use shared::combat::in_range;
use shared::combat::{ActionState, Health};
use shared::components::PlayerId;
use shared::components::{CharacterName, Faction, HitRadius, Motion, VisualKey, Zone};
use shared::enemy_ai::{approach, ground_distance, notices, stop_distance, too_far_from_home};
use shared::gamedata::{GameData, Zones};
use shared::movement::{self, MoveInput, MoveState};
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

/// Where an enemy belongs: it walks back here after giving up a chase,
/// and gets back up here after being defeated.
#[derive(Component, Debug, Clone, Copy)]
pub struct EnemyHome {
    pub position: Vec3,
    pub yaw: f32,
}

/// How a regular enemy moves and notices players (from its data).
#[derive(Component, Debug, Clone, Copy)]
pub struct Roaming {
    /// Its speed compared with a player's walking speed (0–1).
    pub speed_fraction: f32,
    pub aggro_radius: f32,
    pub leash_radius: f32,
    pub assist_radius: f32,
    /// Its shortest attack range: it chases until this close.
    pub attack_range: f32,
}

/// Walking home after giving up a chase. It ignores everyone until it
/// gets there, then heals to full.
#[derive(Component, Debug, Clone, Copy)]
pub struct Returning;

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
    entity.insert(EnemyHome { position, yaw });
    if def.move_speed > 0.0 || def.aggro_radius > 0.0 {
        let attack_range = def
            .actions
            .iter()
            .filter_map(|a| data.abilities.get(&a.ability))
            .map(|a| a.range)
            .fold(f32::INFINITY, f32::min);
        entity.insert(Roaming {
            speed_fraction: (def.move_speed / data.config.movement.walk_speed).min(1.0),
            aggro_radius: def.aggro_radius,
            leash_radius: def.leash_radius,
            assist_radius: def.assist_radius,
            attack_range: if attack_range.is_finite() {
                attack_range
            } else {
                3.0
            },
        });
    }
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
    mut brains: Query<
        (Entity, &Zone, &mut EnemyBrain, &mut ThreatTable),
        (Without<Defeated>, Without<Returning>),
    >,
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
        let reach = |ability: &str| {
            let range = data.abilities.get(ability).map_or(0.0, |a| a.range);
            match (targets.get(enemy), targets.get(target)) {
                (Ok((_, own, ..)), Ok((_, other, radius, ..))) => {
                    in_range(own.0.position, other.0.position, radius.0, range)
                }
                _ => false,
            }
        };
        for action in &mut brain.actions {
            // The first attack comes one full interval after the fight starts.
            let next = *action.next_at.get_or_insert(now + f64::from(action.every));
            // Out of reach (still chasing): wait instead of wasting the attack.
            if now >= next && reach(&action.ability) {
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

/// Idle enemies notice players who come close, and join friends nearby
/// who are already fighting.
pub fn notice_players(
    mut enemies: Query<
        (Entity, &Zone, &Motion, &Roaming, &mut ThreatTable),
        (Without<Defeated>, Without<Returning>),
    >,
    players: Query<(Entity, &Zone, &Motion), (With<PlayerId>, Without<Defeated>)>,
) {
    // Who each fighting enemy is after, for friends to join in.
    let fighting: Vec<(Entity, Zone, Vec3, Entity)> = enemies
        .iter()
        .filter_map(|(e, zone, motion, _, threat)| {
            threat
                .top()
                .map(|target| (e, zone.clone(), motion.0.position, target))
        })
        .collect();
    for (enemy, zone, motion, roaming, mut threat) in &mut enemies {
        if !threat.is_empty() {
            continue;
        }
        let here = motion.0.position;
        let spotted = players
            .iter()
            .filter(|(_, z, m)| *z == zone && notices(here, m.0.position, roaming.aggro_radius))
            .min_by(|a, b| {
                ground_distance(here, a.2.0.position)
                    .total_cmp(&ground_distance(here, b.2.0.position))
            })
            .map(|(player, ..)| player);
        let helping = fighting
            .iter()
            .find(|(friend, z, position, _)| {
                *friend != enemy
                    && z == zone
                    && roaming.assist_radius > 0.0
                    && ground_distance(here, *position) <= roaming.assist_radius
            })
            .map(|(.., target)| *target);
        if let Some(target) = spotted.or(helping) {
            // A token amount: real threat comes from what players do.
            threat.add(target, 1.0);
        }
    }
}

/// Regular enemies chase whoever they are fighting until in range, and
/// give up and walk home when pulled too far.
pub fn move_enemies(
    data: Res<GameData>,
    zones: Res<Zones>,
    time: Res<Time>,
    mut commands: Commands,
    mut enemies: Query<
        (
            Entity,
            &Zone,
            &EnemyHome,
            &Roaming,
            &ActionState,
            &mut Motion,
            &mut ThreatTable,
            &mut Health,
            &mut Statuses,
            Has<Returning>,
        ),
        Without<Defeated>,
    >,
    targets: Query<(&Motion, &HitRadius), Without<EnemyHome>>,
) {
    let dt = time.delta_secs();
    for (
        enemy,
        zone,
        home,
        roaming,
        actions,
        mut motion,
        mut threat,
        mut health,
        mut statuses,
        returning,
    ) in &mut enemies
    {
        let Some(level) = zones.get(&zone.0) else {
            continue;
        };
        let here = motion.0.position;
        let mut stick = None;
        if returning {
            threat.clear();
            match approach(
                here,
                home.position,
                HOME_REACHED,
                roaming.speed_fraction.max(0.5),
            ) {
                Some(direction) => stick = Some(direction),
                None => {
                    // Home: heal up and wait again.
                    *health = Health::full(health.max);
                    statuses.0.clear();
                    motion.0.yaw = home.yaw;
                    commands.entity(enemy).remove::<Returning>();
                }
            }
        } else if threat.is_empty() && ground_distance(here, home.position) > HOME_REACHED {
            // Nobody left to fight (they fell or left): go home.
            commands.entity(enemy).insert(Returning);
            continue;
        } else if let Some(target) = threat.top() {
            if too_far_from_home(home.position, here, roaming.leash_radius) {
                commands.entity(enemy).insert(Returning);
                threat.clear();
                continue;
            }
            // Stand still while casting so the attack goes where it was aimed.
            if actions.cast.is_none()
                && let Ok((target_motion, radius)) = targets.get(target)
            {
                stick = approach(
                    here,
                    target_motion.0.position,
                    stop_distance(roaming.attack_range, radius.0),
                    roaming.speed_fraction,
                );
            }
        }
        if roaming.speed_fraction <= 0.0 {
            continue;
        }
        if let Some(direction) = stick {
            let input = MoveInput {
                direction,
                ..Default::default()
            };
            motion.0 = movement::step(motion.0, input, &data.config.movement, level, dt);
        }
    }
}

/// How close (metres) an enemy walking home must get to count as home.
const HOME_REACHED: f32 = 0.5;

/// Enemies left alone for a while heal up and forget everyone (training
/// dummies), and defeated enemies get back up at home.
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
        &mut Motion,
        &EnemyHome,
        Has<Defeated>,
        Has<Roaming>,
    )>,
) {
    let now = time.elapsed_secs_f64();
    for (
        entity,
        reset,
        mut health,
        mut threat,
        mut statuses,
        mut actions,
        mut motion,
        home,
        defeated,
        roaming,
    ) in &mut enemies
    {
        // Roaming enemies only use this to get back up after being
        // defeated; while alive they give up by walking home instead.
        let disturbed = if roaming {
            defeated
        } else {
            health.current != health.max || !threat.is_empty() || defeated
        };
        if disturbed && now - reset.last_hit >= f64::from(reset.after) {
            *health = Health::full(health.max);
            threat.clear();
            statuses.0.clear();
            actions.reset();
            // Defeated enemies get back up at home.
            if defeated {
                motion.0 = MoveState {
                    yaw: home.yaw,
                    ..MoveState::spawn_at(home.position)
                };
            }
            commands.entity(entity).remove::<Defeated>();
        }
    }
}
