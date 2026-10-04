//! Reading what players asked for this tick.

use bevy::prelude::*;
use shared::gamedata::GameData;
use shared::level::Level;
use shared::protocol::{ClientRequest, Link};

use crate::actions::{self, Actors, PendingHits, Targets, UseContext};
use crate::characters::{PlayerIndex, spawn_player};

pub fn receive_requests(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    level: Res<Level>,
    mut link: ResMut<Link>,
    mut index: ResMut<PlayerIndex>,
    mut hits: ResMut<PendingHits>,
    mut actors: Actors,
    targets: Targets,
) {
    let requests = std::mem::take(&mut link.to_authority);
    let mut ctx = UseContext {
        data: &data,
        now: time.elapsed_secs_f64(),
        link: &mut link,
        hits: &mut hits,
    };
    for (player, request) in requests {
        match request {
            ClientRequest::Join { name } => {
                spawn_player(
                    &mut commands,
                    &mut index,
                    ctx.link,
                    &data,
                    &level,
                    player,
                    &name,
                );
            }
            ClientRequest::Move(input) => {
                let Some(&entity) = index.0.get(&player) else {
                    continue;
                };
                if let Ok((.., mut player_input)) = actors.get_mut(entity) {
                    // A jump stays requested until a movement tick uses it.
                    let jump = player_input.input.jump || input.jump;
                    player_input.input = input;
                    player_input.input.jump = jump;
                }
            }
            ClientRequest::UseAbility { slot, target } => {
                let Some(&entity) = index.0.get(&player) else {
                    continue;
                };
                actions::request_ability(&mut ctx, entity, slot, target, &mut actors, &targets);
            }
        }
    }
}
