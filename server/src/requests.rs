//! Reading what players asked for this tick.

use bevy::prelude::*;
use shared::classes::CurrentClass;
use shared::combat::ActionState;
use shared::components::{Motion, PlayerId, Zone};
use shared::gamedata::GameData;
use shared::gamedata::Zones;
use shared::protocol::{ClientRequest, Link};

use crate::actions::{self, Actors, Targets, UseContext};
use crate::characters::{CombatClock, Defeated, PlayerIndex, interact, spawn_player};
use crate::classes;
use crate::effects::PendingEffects;

pub fn receive_requests(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    zones: Res<Zones>,
    mut interactions: ResMut<PendingInteractions>,
    mut link: ResMut<Link>,
    mut index: ResMut<PlayerIndex>,
    mut effects: ResMut<PendingEffects>,
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
            spawn_player(
                &mut commands,
                &mut index,
                ctx.link,
                &data,
                &zones,
                player,
                name,
            );
            continue;
        }
        let Some(&entity) = index.0.get(&player) else {
            continue;
        };
        match request {
            ClientRequest::Join { .. } => {}
            ClientRequest::Interact => interactions.0.push((player, entity)),
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
