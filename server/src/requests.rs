//! Reading what players asked for this tick.

use std::collections::HashSet;

use bevy::prelude::*;
use shared::classes::CurrentClass;
use shared::combat::ActionState;
use shared::components::{Motion, PlayerId, Zone};
use shared::gamedata::GameData;
use shared::gamedata::Zones;
use shared::protocol::{ClientRequest, Link};

use crate::accounts::{AccountRequests, Sessions};
use crate::actions::{self, Actors, Targets, UseContext};
use crate::characters::{CombatClock, Defeated, PlayerIndex, clean_name, interact, spawn_player};
use crate::classes;
use crate::database::Database;
use crate::effects::PendingEffects;
use crate::instances::Instances;
use crate::progression::PendingRewards;
use crate::progression::{GearRequest, PendingGear};
use crate::travel::{CameFrom, People, Riding, Travel, busy_riding, enter_from_board};
use shared::combat::Reject;
use shared::components::ExitPortal;
use shared::protocol::ServerEvent;
use shared::quests::QuestLog;

pub fn receive_requests(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    zones: Res<Zones>,
    mut interactions: ResMut<PendingInteractions>,
    mut link: ResMut<Link>,
    mut index: ResMut<PlayerIndex>,
    mut effects: ResMut<PendingEffects>,
    mut queues: (ResMut<PendingGear>, ResMut<classes::PendingSpecs>),
    (mut joining, mut account_requests, sessions): (
        ResMut<PendingJoins>,
        ResMut<AccountRequests>,
        Res<Sessions>,
    ),
    mut instances: ResMut<Instances>,
    database: Option<Res<Database>>,
    riders: Query<(), With<Riding>>,
    mut actors: Actors,
    targets: Targets,
    class_state: Query<(&CurrentClass, &CombatClock, Has<Defeated>)>,
) {
    let now = time.elapsed_secs_f64();
    let requests = std::mem::take(&mut link.to_authority);
    let mut ctx = UseContext {
        data: &data,
        now,
        link: &mut link,
        effects: &mut effects,
    };
    for (player, request) in requests {
        if account_requests.take(player, &request) {
            continue;
        }
        if let ClientRequest::Join { name } = &request {
            match &database {
                // Load their save first (the reply arrives in `finish_joins`).
                // With a save file you play your own account's characters.
                Some(database) if !index.0.contains_key(&player) => {
                    let Some((account, _)) = sessions.0.get(&player) else {
                        ctx.link.to_client.push(ServerEvent::AccountError {
                            player,
                            message: "Log in first.".into(),
                        });
                        continue;
                    };
                    if joining.0.insert(player) {
                        database.load(player, clean_name(name), Some(*account));
                    }
                }
                _ => spawn_player(
                    &mut commands,
                    &mut index,
                    ctx.link,
                    &data,
                    &zones,
                    &mut instances,
                    player,
                    name,
                    None,
                ),
            }
            continue;
        }
        let Some(&entity) = index.0.get(&player) else {
            continue;
        };
        // On a ride you can only steer the camera.
        if riders.contains(entity)
            && matches!(
                request,
                ClientRequest::UseAbility { .. }
                    | ClientRequest::ChangeClass { .. }
                    | ClientRequest::Interact
                    | ClientRequest::EnterFromBoard { .. }
            )
        {
            busy_riding(ctx.link, player);
            continue;
        }
        match request {
            ClientRequest::Join { .. }
            | ClientRequest::Register { .. }
            | ClientRequest::Login { .. }
            | ClientRequest::Logout
            | ClientRequest::CreateCharacter { .. }
            | ClientRequest::DeleteCharacter { .. } => {}
            ClientRequest::Interact => interactions.0.push((player, entity, None)),
            ClientRequest::ChangeSpec { spec } => queues.1.0.push((player, entity, spec)),
            ClientRequest::EnterFromBoard { zone } => {
                interactions.0.push((player, entity, Some(zone)));
            }
            ClientRequest::Equip { item } => {
                queues.0.0.push((player, entity, GearRequest::Equip(item)));
            }
            ClientRequest::Unequip { slot } => {
                queues
                    .0
                    .0
                    .push((player, entity, GearRequest::Unequip(slot)));
            }
            ClientRequest::Discard { item } => {
                queues
                    .0
                    .0
                    .push((player, entity, GearRequest::Discard(item)));
            }
            ClientRequest::SetSecondary { choice } => {
                queues
                    .0
                    .0
                    .push((player, entity, GearRequest::SetSecondary(choice)));
            }
            ClientRequest::Move(input) => {
                if let Ok((.., Some(mut player_input), _, _)) = actors.get_mut(entity) {
                    // A jump stays requested until a movement tick uses it.
                    let jump = player_input.input.jump || input.jump;
                    player_input.input = input;
                    player_input.input.jump = jump;
                }
            }
            ClientRequest::UseAbility { slot, target } => {
                actions::request_slot(&mut ctx, entity, slot, target, &mut actors, &targets);
            }
            ClientRequest::ChangeClass { class } => {
                let (Ok((current, clock, defeated)), Ok((.., action_state, _, _, _))) =
                    (class_state.get(entity), actors.get(entity))
                else {
                    continue;
                };
                let action_state: &ActionState = action_state;
                classes::request_change(
                    &mut commands,
                    ctx.link,
                    &data,
                    now,
                    player,
                    entity,
                    &class,
                    (current, action_state, clock, defeated),
                );
            }
        }
    }
}

/// Players whose save is being loaded.
#[derive(Resource, Default)]
pub struct PendingJoins(HashSet<PlayerId>);

/// Characters whose save finished loading enter the world.
pub fn finish_joins(
    mut commands: Commands,
    data: Res<GameData>,
    zones: Res<Zones>,
    database: Option<Res<Database>>,
    mut joining: ResMut<PendingJoins>,
    mut index: ResMut<PlayerIndex>,
    mut instances: ResMut<Instances>,
    mut link: ResMut<Link>,
) {
    let Some(database) = database else {
        return;
    };
    for loaded in database.take_loaded() {
        joining.0.remove(&loaded.player);
        if let Some(message) = loaded.refused {
            link.to_client.push(ServerEvent::AccountError {
                player: loaded.player,
                message,
            });
            continue;
        }
        spawn_player(
            &mut commands,
            &mut index,
            &mut link,
            &data,
            &zones,
            &mut instances,
            loaded.player,
            &loaded.name,
            loaded.save.as_ref(),
        );
    }
}

/// Interact requests (and dungeon board choices), handled by
/// [`handle_interactions`] (which may move characters, something
/// `receive_requests` can't do while it reads them).
#[derive(Resource, Default)]
pub struct PendingInteractions(Vec<(PlayerId, Entity, Option<String>)>);

pub fn handle_interactions(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    zones: Res<Zones>,
    mut instances: ResMut<Instances>,
    mut link: ResMut<Link>,
    mut pending: ResMut<PendingInteractions>,
    mut rewards: ResMut<PendingRewards>,
    mut players: Query<
        (
            &Zone,
            &mut Motion,
            &CombatClock,
            Has<Defeated>,
            Option<&CameFrom>,
            Option<&mut QuestLog>,
        ),
        With<PlayerId>,
    >,
    exits: Query<(&ExitPortal, &Zone)>,
    mut npcs: People,
) {
    let now = time.elapsed_secs_f64();
    for (player, entity, board_choice) in pending.0.drain(..) {
        let occupied: Vec<String> = players.iter().map(|(z, ..)| z.0.clone()).collect();
        let mut travel = Travel {
            commands: &mut commands,
            link: &mut link,
            data: &data,
            zones: &zones,
            instances: &mut instances,
            occupied: &occupied,
            now,
        };
        let Ok((zone, mut motion, clock, defeated, came_from, log)) = players.get_mut(entity)
        else {
            continue;
        };
        match board_choice {
            None => interact(
                &mut travel,
                player,
                entity,
                (zone, &mut motion, clock, defeated, came_from),
                (&mut rewards, log.map(|l| l.into_inner())),
                &exits,
                &mut npcs,
            ),
            Some(to) => {
                let in_combat = clock.in_combat(now, data.config.combat.combat_timeout);
                let result = if defeated {
                    Err(Reject::Dead)
                } else if in_combat {
                    Err(Reject::InCombat)
                } else {
                    enter_from_board(&mut travel, entity, &mut motion, zone, &to)
                };
                if let Err(reason) = result {
                    travel
                        .link
                        .to_client
                        .push(ServerEvent::Rejected { player, reason });
                }
            }
        }
    }
}
