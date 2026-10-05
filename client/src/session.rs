//! The client's side of the connection to the rules half: joining the
//! game, sending requests, and receiving events.

use bevy::prelude::*;
use shared::components::PlayerId;
use shared::protocol::{ClientRequest, Link, ServerEvent};

use crate::menus::Screen;

/// This client's player id. (Single-player: always the same.)
#[derive(Resource, Debug, Clone, Copy)]
pub struct LocalPlayerId(pub PlayerId);

/// Character name used when joining without a save file (demos). With a
/// save file the character list sends its own `Join`.
const LOCAL_NAME: &str = "Adventurer";

/// An event from the rules half, re-sent inside the client as a Bevy
/// message so any client system can react to it.
#[derive(Message, Debug, Clone)]
pub struct Received(pub ServerEvent);

pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LocalPlayerId(PlayerId(1)))
            .add_message::<Received>()
            .add_systems(Startup, join)
            .add_systems(PreUpdate, receive_events);
    }
}

/// Send a request to the rules half on behalf of this client's player.
pub fn send(link: &mut Link, me: LocalPlayerId, request: ClientRequest) {
    link.to_authority.push((me.0, request));
}

fn join(mut link: ResMut<Link>, me: Res<LocalPlayerId>, screen: Res<State<Screen>>) {
    if *screen.get() != Screen::Playing {
        return;
    }
    send(
        &mut link,
        *me,
        ClientRequest::Join {
            name: LOCAL_NAME.to_owned(),
        },
    );
}

pub fn receive_events(mut link: ResMut<Link>, mut received: MessageWriter<Received>) {
    received.write_batch(link.to_client.drain(..).map(Received));
}
