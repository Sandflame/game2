//! Player characters: joining, moving, regeneration, defeat and revival.

use std::collections::HashMap;

use bevy::prelude::*;
use shared::classes::{CurrentClass, Secondaries};
use shared::combat::Reject;
use shared::combat::{ActionState, Health};
use shared::components::{
    CharacterName, Faction, HitRadius, Hotbar, Motion, PlayerId, VisualKey, Zone,
};
use shared::enemy_ai::ground_distance;
use shared::gamedata::{GameData, Zones};
use shared::movement::{self, MoveInput, MoveState};
use shared::progression::ClassLevels;
use shared::protocol::{Link, ServerEvent};
use shared::statuses::Statuses;
use shared::threat::ThreatTable;

use crate::classes::FlameChange;
use crate::database::CharacterSave;
use crate::encounters::WIPE_PAUSE;
use crate::instances::Instances;
use crate::progression::PendingRewards;
use crate::progression::{
    Build, player_hotbar, player_stats, restore, restore_quests, starting_gear,
};
use crate::travel::{CameFrom, People, Riding, Travel, talk, use_portal};
use shared::components::ExitPortal;
use shared::level::Portal;
use shared::quests::QuestLog;

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
/// The name a character is known (and saved) by.
pub fn clean_name(name: &str) -> &str {
    if name.trim().is_empty() {
        "Adventurer"
    } else {
        name.trim()
    }
}

/// Bring a player's character into the world: from their save if they
/// have one, otherwise as a new character in the starting zone.
pub fn spawn_player(
    commands: &mut Commands,
    index: &mut PlayerIndex,
    link: &mut Link,
    data: &GameData,
    zones: &Zones,
    instances: &mut Instances,
    player: PlayerId,
    name: &str,
    save: Option<&CharacterSave>,
) {
    if let Some(&entity) = index.0.get(&player) {
        link.to_client.push(ServerEvent::Joined { player, entity });
        return;
    }
    // A saved class or zone that no longer exists falls back to the start.
    let class_id = save
        .map(|s| s.class.clone())
        .filter(|c| data.classes.contains_key(c))
        .unwrap_or_else(|| data.player.start_class.clone());
    // Someone who quit in the middle of a ride arrives at its end.
    let ride_end = save.and_then(|s| data.rides.values().find(|r| r.zone == s.zone));
    let saved_zone = save.filter(|s| zones.get(&s.zone).is_some() && ride_end.is_none());
    // Someone who quit inside a dungeon or trial comes back outside it
    // (that copy of it is gone).
    let left_instance = saved_zone
        .and_then(|s| zones.get(&s.zone))
        .filter(|level| level.instanced)
        .and_then(|level| level.exit.as_ref());
    let (zone, motion) = match (ride_end, left_instance, saved_zone) {
        (Some(ride), ..) => (ride.to.clone(), (ride.arrive, ride.arrive_yaw)),
        (None, Some(exit), _) => (exit.to.clone(), (exit.arrive, exit.arrive_yaw)),
        (None, None, Some(s)) => (s.zone.clone(), (s.position, s.yaw)),
        (None, None, None) => {
            let start = &data.player.start_zone;
            let at = zones
                .get(start)
                .map_or((Vec3::ZERO, 0.0), |l| (l.spawn_point, l.spawn_yaw));
            (start.clone(), at)
        }
    };
    // A start zone that is instanced (only in demos) gets its own copy.
    let zone = instances.enter(commands, data, zones, &zone, &[]);
    // Both checked when the data was loaded.
    let Some(class) = data.classes.get(&class_id) else {
        return;
    };
    let motion = MoveState {
        yaw: motion.1,
        ..MoveState::spawn_at(motion.0)
    };
    let (levels, bag, worn, secondaries) = match save {
        Some(save) => restore(save, data),
        None => {
            let (bag, worn) = starting_gear(data);
            (ClassLevels::default(), bag, worn, Secondaries::default())
        }
    };
    let quests = save.map_or_else(QuestLog::default, |s| restore_quests(s, data));
    let build = Build {
        levels: &levels,
        bag: &bag,
        worn: &worn,
        secondaries: &secondaries,
    };
    let Some(stats) = player_stats(data, zones, &class_id, &zone, &build) else {
        return;
    };
    let current = CurrentClass {
        class: class_id,
        spec: class.default_spec.clone(),
    };
    let hotbar = player_hotbar(data, &current, &levels, &secondaries);
    let entity = commands
        .spawn((
            (
                player,
                CharacterName(clean_name(name).to_owned()),
                Motion(motion),
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
                Hotbar(hotbar),
                current,
            ),
            (levels, bag, worn, secondaries, quests),
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
    mut characters: Query<
        (
            Entity,
            &Zone,
            &mut PlayerInput,
            &mut Motion,
            &mut ActionState,
            Has<Defeated>,
            Option<&FlameChange>,
        ),
        Without<Riding>,
    >,
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

/// A player pressed the interact key: use the portal they are standing
/// in, or talk to someone nearby.
pub fn interact(
    travel: &mut Travel,
    player: PlayerId,
    entity: Entity,
    state: (&Zone, &mut Motion, &CombatClock, bool, Option<&CameFrom>),
    quest: (&mut PendingRewards, Option<&mut QuestLog>),
    exits: &Query<(&ExitPortal, &Zone)>,
    npcs: &mut People,
) {
    let (zone, motion, clock, defeated, came_from) = state;
    let reject = |link: &mut Link, reason| {
        link.to_client
            .push(ServerEvent::Rejected { player, reason })
    };
    let here = motion.0.position;
    let fixed = travel
        .zones
        .get(&zone.0)
        .and_then(|level| level.portal_at(here))
        .cloned();
    // A way out that appeared after a boss fell.
    let appeared = exits
        .iter()
        .find(|(exit, z)| *z == zone && ground_distance(exit.position, here) <= exit.radius)
        .map(|(exit, _)| Portal {
            position: exit.position,
            radius: exit.radius,
            to: String::new(),
            arrive: Vec3::ZERO,
            arrive_yaw: 0.0,
            label: exit.label.clone(),
            ride: None,
            closed: None,
            visual: String::new(),
            back: true,
            board: false,
        });
    let Some(portal) = fixed.or(appeared) else {
        if !talk(
            travel.link,
            travel.data,
            quest,
            (player, entity),
            zone,
            here,
            npcs,
        ) {
            reject(travel.link, Reject::NothingHere);
        }
        return;
    };
    if defeated {
        return reject(travel.link, Reject::Dead);
    }
    let in_combat = clock.in_combat(travel.now, travel.data.config.combat.combat_timeout);
    if portal.closed.is_none() && !portal.board && in_combat {
        return reject(travel.link, Reject::InCombat);
    }
    use_portal(travel, player, entity, motion, zone, &portal, came_from);
}

/// In dungeons and trials nobody gets back up alone. If everyone in one
/// falls outside a boss fight (the boss fight handles its own), they all
/// get back up at the entrance after a short pause.
pub fn recover_wipes(
    mut commands: Commands,
    time: Res<Time>,
    zones: Res<Zones>,
    mut link: ResMut<Link>,
    mut players: Query<
        (
            Entity,
            &Zone,
            Option<&Defeated>,
            &mut Health,
            &mut Motion,
            &mut CombatClock,
        ),
        With<PlayerId>,
    >,
    fights: Query<&crate::Encounter>,
) {
    let now = time.elapsed_secs_f64();
    let mut latest: HashMap<String, Option<f64>> = HashMap::new();
    for (_, zone, defeated, ..) in &players {
        let entry = latest.entry(zone.0.clone()).or_insert(Some(f64::MIN));
        *entry = match (*entry, defeated) {
            (Some(t), Some(d)) => Some(t.max(d.at)),
            _ => None,
        };
    }
    for (zone, latest) in latest {
        let Some(fell) = latest else {
            continue;
        };
        let handled_by_fight = fights.iter().any(|f| {
            f.zone == zone
                && matches!(
                    f.state,
                    crate::FightState::Fighting | crate::FightState::Wiping { .. }
                )
        });
        let Some(level) = zones.get(&zone) else {
            continue;
        };
        if level.revive_in_place || handled_by_fight || now - fell < WIPE_PAUSE {
            continue;
        }
        for (entity, player_zone, _, mut health, mut motion, mut clock) in &mut players {
            if player_zone.0 != zone {
                continue;
            }
            *health = Health::full(health.max);
            motion.0 = MoveState {
                yaw: level.spawn_yaw,
                ..MoveState::spawn_at(level.spawn_point)
            };
            *clock = CombatClock::default();
            commands.entity(entity).remove::<Defeated>();
            link.to_client.push(ServerEvent::Revived { entity });
        }
    }
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
