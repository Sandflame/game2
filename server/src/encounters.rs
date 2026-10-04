//! Boss fights. Each zone with an encounter gets one boss and a director
//! that runs the fight: it starts when the boss is pulled, follows the
//! phases and timeline from the data file, summons adds, enrages, and
//! handles victory and wipes (everyone falls → reset at the entrance).

use std::collections::VecDeque;

use bevy::prelude::*;
use shared::classes::Stats;
use shared::combat::{ActionState, Health};
use shared::components::{ExitPortal, Motion, PlayerId, Zone};
use shared::encounters::{EncounterAction, Progress};
use shared::gamedata::{GameData, Zones};
use shared::movement::MoveState;
use shared::protocol::{Link, ServerEvent};
use shared::statuses::Statuses;
use shared::telegraphs::Telegraph;
use shared::threat::ThreatTable;

use crate::actions::{Actors, Targets, UseContext, can_act_now, use_ability};
use crate::characters::{CombatClock, Defeated};
use crate::effects::PendingEffects;
use crate::enemies::spawn_enemy;
use crate::progression::{PendingRewards, Reward};

/// After a wipe, players are brought back to the entrance after this long.
pub const WIPE_PAUSE: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FightState {
    /// Waiting for someone to attack the boss.
    Waiting,
    Fighting,
    /// The boss is down; resets once everyone has left the zone.
    Won,
    /// Everyone fell; resets after a short pause.
    Wiping {
        at: f64,
    },
}

/// The director of one boss fight.
#[derive(Component, Debug)]
pub struct Encounter {
    /// Encounter id (file name in `assets/data/encounters/`).
    pub id: String,
    pub zone: String,
    pub boss: Entity,
    pub adds: Vec<Entity>,
    /// The way out that appeared when the boss fell.
    pub exit: Option<Entity>,
    pub state: FightState,
    pub progress: Progress,
    /// Actions waiting for the boss to be free.
    pub pending: VecDeque<EncounterAction>,
}

/// Changes the director wants made to characters (done by
/// [`apply_encounter_changes`], which is allowed to change them).
#[derive(Resource, Default, Debug)]
pub struct EncounterChanges(Vec<Change>);

#[derive(Debug)]
enum Change {
    /// Set the boss's health for this many players.
    Scale {
        encounter: String,
        boss: Entity,
        players: usize,
    },
    /// Make an enemy notice these players.
    Aggro { enemy: Entity, players: Vec<Entity> },
    /// Remove these enemies.
    Despawn(Vec<Entity>),
    /// Clear the ground markers in a zone.
    ClearMarkers(String),
    /// Put the boss and the players back to the start of the fight.
    Reset {
        encounter: String,
        zone: String,
        boss: Entity,
    },
}

/// Create the boss and director of one boss fight in a zone.
pub fn spawn_encounter(commands: &mut Commands, data: &GameData, id: &str, zone: &str) {
    let Some(def) = data.encounters.get(id) else {
        return;
    };
    let Some(boss) = spawn_enemy(
        commands,
        data,
        &def.boss,
        zone,
        def.boss_position,
        def.boss_yaw,
    ) else {
        return;
    };
    commands.spawn(Encounter {
        id: id.to_owned(),
        zone: zone.to_owned(),
        boss,
        adds: Vec::new(),
        exit: None,
        state: FightState::Waiting,
        progress: Progress::new(0.0),
        pending: VecDeque::new(),
    });
}

/// Run every boss fight for one tick.
pub fn run_encounters(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut link: ResMut<Link>,
    mut effects: ResMut<PendingEffects>,
    mut changes: ResMut<EncounterChanges>,
    mut rewards: ResMut<PendingRewards>,
    mut encounters: Query<&mut Encounter>,
    players: Query<(Entity, &Zone, Has<Defeated>), With<PlayerId>>,
    bosses: Query<(&Health, &ThreatTable)>,
    mut actors: Actors,
    targets: Targets,
) {
    let now = time.elapsed_secs_f64();
    for mut encounter in &mut encounters {
        let Some(def) = data.encounters.get(&encounter.id) else {
            continue;
        };
        let here: Vec<(Entity, bool)> = players
            .iter()
            .filter(|(_, zone, _)| zone.0 == encounter.zone)
            .map(|(e, _, defeated)| (e, defeated))
            .collect();
        let Ok((boss_health, boss_threat)) = bosses.get(encounter.boss) else {
            continue;
        };
        let zone = encounter.zone.clone();
        let announce = |link: &mut Link, text: &str| {
            link.to_client.push(ServerEvent::Announce {
                zone: zone.clone(),
                text: text.to_owned(),
            });
        };

        match encounter.state {
            FightState::Waiting => {
                if boss_threat.is_empty() || here.is_empty() {
                    continue;
                }
                // Pulled! Scale for the party and start the clock.
                encounter.state = FightState::Fighting;
                encounter.progress = Progress::new(now);
                encounter.pending.clear();
                let present: Vec<Entity> = here.iter().map(|(e, _)| *e).collect();
                changes.0.push(Change::Scale {
                    encounter: encounter.id.clone(),
                    boss: encounter.boss,
                    players: present.len(),
                });
                changes.0.push(Change::Aggro {
                    enemy: encounter.boss,
                    players: present,
                });
                link.to_client.push(ServerEvent::EncounterStarted {
                    zone: zone.clone(),
                    name: def.name.clone(),
                });
                if let Some(text) = &def.pull_message {
                    announce(&mut link, text);
                }
            }
            FightState::Fighting => {
                if here.is_empty() {
                    // Everyone left: quietly put the fight back.
                    reset(&mut encounter, &mut changes);
                    continue;
                }
                if here.iter().all(|(_, defeated)| *defeated) {
                    encounter.state = FightState::Wiping { at: now };
                    changes.0.push(Change::ClearMarkers(zone.clone()));
                    link.to_client.push(ServerEvent::EncounterWiped {
                        zone: zone.clone(),
                        name: def.name.clone(),
                    });
                    continue;
                }
                if boss_health.is_dead() {
                    encounter.state = FightState::Won;
                    rewards.0.push(Reward {
                        players: here.iter().map(|(e, _)| *e).collect(),
                        xp: def.xp,
                        loot: def.loot.clone(),
                    });
                    changes
                        .0
                        .push(Change::Despawn(std::mem::take(&mut encounter.adds)));
                    changes.0.push(Change::ClearMarkers(zone.clone()));
                    if let Some(exit) = &def.exit_portal {
                        encounter.exit = Some(
                            commands
                                .spawn((
                                    ExitPortal {
                                        position: exit.position,
                                        radius: exit.radius,
                                        label: exit.label.clone(),
                                    },
                                    Zone(zone.clone()),
                                ))
                                .id(),
                        );
                    }
                    link.to_client.push(ServerEvent::EncounterWon {
                        zone: zone.clone(),
                        encounter: encounter.id.clone(),
                        name: def.name.clone(),
                        seconds: (now - encounter.progress.started) as f32,
                    });
                    if let Some(text) = &def.victory_message {
                        announce(&mut link, text);
                    }
                    continue;
                }

                // Phase changes at health thresholds.
                let percent = boss_health.fraction() * 100.0;
                let phase = def.phase_for_health(percent);
                if phase > encounter.progress.phase {
                    encounter.progress.enter_phase(phase, now);
                    let on_start = def.phases[phase].on_start.clone();
                    encounter.pending.extend(on_start);
                }
                // Enrage jumps the queue.
                if encounter.progress.enrage_due(def, now)
                    && let Some(enrage) = &def.enrage
                {
                    encounter.progress.enraged = true;
                    encounter
                        .pending
                        .push_front(EncounterAction::Use(enrage.ability.clone()));
                    if let Some(text) = &enrage.message {
                        encounter
                            .pending
                            .push_front(EncounterAction::Say(text.clone()));
                    }
                }
                // Timeline entries that are due join the queue.
                loop {
                    let mut progress = encounter.progress.clone();
                    let Some(action) = progress.due(def, now).cloned() else {
                        encounter.progress = progress;
                        break;
                    };
                    progress.advance();
                    encounter.progress = progress;
                    encounter.pending.push_back(action);
                }

                // Work through the queue; abilities wait until the boss is free.
                let mut ctx = UseContext {
                    data: &data,
                    now,
                    link: &mut link,
                    effects: &mut effects,
                };
                while let Some(action) = encounter.pending.front().cloned() {
                    match action {
                        EncounterAction::Say(text) => {
                            ctx.link.to_client.push(ServerEvent::Announce {
                                zone: zone.clone(),
                                text,
                            });
                        }
                        EncounterAction::Spawn { enemy, at } => {
                            if let Some(add) =
                                spawn_enemy(&mut commands, &data, &enemy, &zone, at, 0.0)
                            {
                                encounter.adds.push(add);
                                changes.0.push(Change::Aggro {
                                    enemy: add,
                                    players: here.iter().map(|(e, _)| *e).collect(),
                                });
                            }
                        }
                        EncounterAction::Use(ability) => {
                            if !can_act_now(&ctx, encounter.boss, &ability, &actors) {
                                break;
                            }
                            let target = boss_threat.top();
                            use_ability(
                                &mut ctx,
                                encounter.boss,
                                &ability,
                                target,
                                &mut actors,
                                &targets,
                            );
                        }
                    }
                    encounter.pending.pop_front();
                }
            }
            FightState::Wiping { at } => {
                if now - at >= WIPE_PAUSE {
                    reset(&mut encounter, &mut changes);
                }
            }
            FightState::Won => {
                if here.is_empty() {
                    reset(&mut encounter, &mut changes);
                }
            }
        }
    }
}

fn reset(encounter: &mut Encounter, changes: &mut EncounterChanges) {
    encounter.state = FightState::Waiting;
    encounter.pending.clear();
    let mut gone = std::mem::take(&mut encounter.adds);
    gone.extend(encounter.exit.take());
    changes.0.push(Change::Despawn(gone));
    changes.0.push(Change::ClearMarkers(encounter.zone.clone()));
    changes.0.push(Change::Reset {
        encounter: encounter.id.clone(),
        zone: encounter.zone.clone(),
        boss: encounter.boss,
    });
}

/// Carry out the director's changes to characters.
pub fn apply_encounter_changes(
    mut commands: Commands,
    data: Res<GameData>,
    zones: Res<Zones>,
    mut changes: ResMut<EncounterChanges>,
    mut characters: Query<(
        &mut Health,
        &mut Statuses,
        &mut ActionState,
        &mut Motion,
        Option<&mut Stats>,
        Option<&mut ThreatTable>,
        Option<&mut CombatClock>,
    )>,
    players: Query<(Entity, &Zone), With<PlayerId>>,
    markers: Query<(Entity, &Zone), With<Telegraph>>,
) {
    for change in changes.0.drain(..) {
        match change {
            Change::Scale {
                encounter,
                boss,
                players,
            } => {
                let (Some(def), Ok((mut health, _, _, _, stats, ..))) =
                    (data.encounters.get(&encounter), characters.get_mut(boss))
                else {
                    continue;
                };
                let base = data
                    .enemies
                    .get(&def.boss)
                    .map_or(health.max, |e| e.max_health);
                let max = def.scaled_health(base, players);
                *health = Health::full(max);
                if let Some(mut stats) = stats {
                    stats.max_health = max;
                }
            }
            Change::Aggro { enemy, players } => {
                if let Ok((.., Some(mut threat), _)) = characters.get_mut(enemy) {
                    for player in players {
                        threat.add(player, 1.0);
                    }
                }
            }
            Change::Despawn(entities) => {
                for entity in entities {
                    if let Ok(mut e) = commands.get_entity(entity) {
                        e.despawn();
                    }
                }
            }
            Change::ClearMarkers(zone) => {
                for (marker, marker_zone) in &markers {
                    if marker_zone.0 == zone {
                        commands.entity(marker).despawn();
                    }
                }
            }
            Change::Reset {
                encounter,
                zone,
                boss,
            } => {
                let Some(def) = data.encounters.get(&encounter) else {
                    continue;
                };
                // The boss: full (one-player) health, calm, back in place.
                if let Ok((mut health, mut statuses, mut actions, mut motion, stats, threat, _)) =
                    characters.get_mut(boss)
                {
                    let base = data
                        .enemies
                        .get(&def.boss)
                        .map_or(health.max, |e| e.max_health);
                    *health = Health::full(base);
                    if let Some(mut stats) = stats {
                        stats.max_health = base;
                    }
                    statuses.0.clear();
                    actions.reset();
                    motion.0 = MoveState {
                        yaw: def.boss_yaw,
                        ..MoveState::spawn_at(def.boss_position)
                    };
                    if let Some(mut threat) = threat {
                        threat.clear();
                    }
                    commands.entity(boss).remove::<Defeated>();
                }
                // The players: back on their feet at the entrance.
                let (entrance, facing) = zones
                    .get(&zone)
                    .map_or((Vec3::ZERO, 0.0), |z| (z.spawn_point, z.spawn_yaw));
                for (player, player_zone) in &players {
                    if player_zone.0 != zone {
                        continue;
                    }
                    if let Ok((mut health, mut statuses, mut actions, mut motion, .., clock)) =
                        characters.get_mut(player)
                    {
                        *health = Health::full(health.max);
                        statuses.0.clear();
                        actions.reset();
                        motion.0 = MoveState {
                            yaw: facing,
                            ..MoveState::spawn_at(entrance)
                        };
                        if let Some(mut clock) = clock {
                            *clock = CombatClock::default();
                        }
                    }
                    commands.entity(player).remove::<Defeated>();
                }
            }
        }
    }
}
