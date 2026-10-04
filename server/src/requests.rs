//! Reading what players asked for this tick.

use std::collections::HashSet;

use bevy::prelude::*;
use shared::classes::CurrentClass;
use shared::combat::ActionState;
use shared::components::{Motion, PlayerId, Zone};
use shared::gamedata::GameData;
use shared::gamedata::Zones;
use shared::protocol::{ClientRequest, Link};

use crate::actions::{self, Actors, Targets, UseContext};
use crate::characters::{CombatClock, Defeated, PlayerIndex, clean_name, interact, spawn_player};
use crate::classes;
use crate::database::Database;
use crate::effects::PendingEffects;
use crate::progression::{GearRequest, PendingGear};

pub fn receive_requests(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    zones: Res<Zones>,
    mut interactions: ResMut<PendingInteractions>,
    mut link: ResMut<Link>,
    mut index: ResMut<PlayerIndex>,
    mut effects: ResMut<PendingEffects>,
    mut gear: ResMut<PendingGear>,
    mut joining: ResMut<PendingJoins>,
    database: Option<Res<Database>>,
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
        if let ClientRequest::Join { name } = &request {
            match &database {
                // Load their save first (the reply arrives in `finish_joins`).
                Some(database) if !index.0.contains_key(&player) => {
                    if joining.0.insert(player) {
                        database.load(player, clean_name(name));
                    }
                }
                _ => spawn_player(
                    &mut commands,
                    &mut index,
                    ctx.link,
                    &data,
                    &zones,
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
        match request {
            ClientRequest::Join { .. } => {}
            ClientRequest::Interact => interactions.0.push((player, entity)),
            ClientRequest::Equip { item } => {
                gear.0.push((player, entity, GearRequest::Equip(item)));
            }
            ClientRequest::Unequip { slot } => {
                gear.0.push((player, entity, GearRequest::Unequip(slot)));
            }
            ClientRequest::Discard { item } => {
                gear.0.push((player, entity, GearRequest::Discard(item)));
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
    mut link: ResMut<Link>,
) {
    let Some(database) = database else {
        return;
    };
    for loaded in database.take_loaded() {
        joining.0.remove(&loaded.player);
        spawn_player(
            &mut commands,
            &mut index,
            &mut link,
            &data,
            &zones,
            loaded.player,
            &loaded.name,
            loaded.save.as_ref(),
        );
    }
}

/// Interact requests, handled by [`handle_interactions`] (which may move
/// characters, something `receive_requests` can't do while it reads them).
#[derive(Resource, Default)]
pub struct PendingInteractions(Vec<(PlayerId, Entity)>);

pub fn handle_interactions(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    zones: Res<Zones>,
    mut link: ResMut<Link>,
    mut pending: ResMut<PendingInteractions>,
    mut players: Query<(&Zone, &mut Motion, &CombatClock, Has<Defeated>)>,
) {
    let now = time.elapsed_secs_f64();
    for (player, entity) in pending.0.drain(..) {
        if let Ok((zone, mut motion, clock, defeated)) = players.get_mut(entity) {
            interact(
                &mut commands,
                &mut link,
                &data,
                &zones,
                now,
                player,
                entity,
                (zone, &mut motion, clock, defeated),
            );
        }
    }
}
