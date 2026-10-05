//! Playing over a network, with lightyear (UDP + netcode.io).
//!
//! - [`ProtocolPlugin`]: what travels between the `server` program and the
//!   game, registered the same way on both sides: [`ClientRequest`]s and
//!   [`ServerEvent`]s as messages, and the logic components the game reads
//!   as replicated state.
//! - [`NetServerPlugin`]: the server's side. Each connection gets a
//!   [`PlayerId`]; its requests go into the rules half's [`Link`] like the
//!   game's own did when it ran the rules itself, and each event goes to the
//!   players it concerns. Everything in a zone is only sent to players in
//!   that zone.
//!
//! The game's side is `client/src/net.rs`. See DESIGN.md §3.4.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use bevy::prelude::*;
use lightyear::prelude as ly;
use lightyear::prelude::server as lys;
use lightyear::prelude::{AppChannelExt, AppComponentExt, AppMessageExt, VisibilityExt};
use shared::appearance::Appearance;
use shared::classes::{ChosenSpecs, CurrentClass, Secondaries, Stats};
use shared::combat::{ActionState, Health};
use shared::components::{
    CharacterName, ExitPortal, Faction, HitRadius, Hotbar, Motion, MoveEpoch, NpcId, PlayerId,
    VisualKey, WorldClock, Zone,
};
use shared::gamedata::GameData;
use shared::items::{Bag, Equipment};
use shared::progression::ClassLevels;
use shared::protocol::{ClientRequest, Link, ServerEvent};
use shared::quests::QuestLog;
use shared::statuses::Statuses;
use shared::telegraphs::Telegraph;

use crate::{AuthoritySystems, Defeated, EnemyKind, FlameChange, PlayerIndex, Riding};

/// Identifies Lanternflame's protocol: other programs' packets are refused.
pub const PROTOCOL_ID: u64 = 0x4C41_4E54_4552_4E31;

/// The key connections are encrypted with. It is built into the game, so
/// this keeps out casual snooping, not a determined attacker who has the
/// program ("friends-and-family security", DESIGN.md §3.6).
pub const PRIVATE_KEY: [u8; 32] = [
    0x6c, 0x61, 0x6e, 0x74, 0x65, 0x72, 0x6e, 0x66, 0x6c, 0x61, 0x6d, 0x65, 0x2d, 0x66, 0x72, 0x69,
    0x65, 0x6e, 0x64, 0x73, 0x2d, 0x6f, 0x6e, 0x6c, 0x79, 0x2d, 0x6b, 0x65, 0x79, 0x21, 0x21, 0x21,
];

/// Requests and events that must all arrive, in order.
pub struct Reliable;

/// Movement reports: only the newest one matters.
pub struct Moves;

/// Registers the protocol. Both the `server` program and the game add it
/// (after lightyear's own plugins), in the same order.
pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        app.register_message::<ClientRequest>()
            .add_map_entities()
            .add_direction(ly::NetworkDirection::ClientToServer);
        app.register_message::<ServerEvent>()
            .add_map_entities()
            .add_direction(ly::NetworkDirection::ServerToClient);
        app.add_channel::<Reliable>(ly::ChannelSettings {
            mode: ly::ChannelMode::OrderedReliable(ly::ReliableSettings::default()),
            ..default()
        })
        .add_direction(ly::NetworkDirection::Bidirectional);
        app.add_channel::<Moves>(ly::ChannelSettings {
            mode: ly::ChannelMode::SequencedUnreliable,
            ..default()
        })
        .add_direction(ly::NetworkDirection::ClientToServer);

        // Everything the game reads about the world.
        app.component::<PlayerId>().replicate();
        app.component::<CharacterName>().replicate();
        app.component::<Motion>().replicate();
        app.component::<MoveEpoch>().replicate();
        app.component::<Zone>().replicate();
        app.component::<Faction>().replicate();
        app.component::<VisualKey>().replicate();
        app.component::<NpcId>().replicate();
        app.component::<HitRadius>().replicate();
        app.component::<Hotbar>().replicate();
        app.component::<ExitPortal>().replicate();
        app.component::<Health>().replicate();
        app.component::<ActionState>().replicate();
        app.component::<Statuses>().replicate();
        app.component::<CurrentClass>().replicate();
        app.component::<ChosenSpecs>().replicate();
        app.component::<Secondaries>().replicate();
        app.component::<Stats>().replicate();
        app.component::<ClassLevels>().replicate();
        app.component::<Bag>().replicate();
        app.component::<Equipment>().replicate();
        app.component::<QuestLog>().replicate();
        app.component::<Appearance>().replicate();
        app.component::<Telegraph>().replicate();
        app.component::<Defeated>().replicate();
        app.component::<EnemyKind>().replicate();
        app.component::<FlameChange>().replicate();
        app.component::<Riding>().replicate();
        app.component::<WorldClock>().replicate();
    }
}

/// The lightyear plugins for a server, then the protocol.
pub fn server_plugins(app: &mut App, data: &GameData) {
    app.add_plugins(lys::ServerPlugins {
        tick_duration: Duration::from_secs_f64(1.0 / data.config.simulation.tick_hz),
    });
    app.add_plugins(ProtocolPlugin);
}

/// The server's side: listens on `port` (UDP, all network addresses).
pub struct NetServerPlugin {
    pub port: u16,
}

impl Plugin for NetServerPlugin {
    fn build(&self, app: &mut App) {
        let (send_every, timeout) = {
            let data = app.world().resource::<GameData>();
            (data.config.network.send_every, data.config.network.timeout)
        };
        app.insert_resource(ly::ReplicationMetadata::new(Duration::from_secs_f32(
            send_every,
        )))
        .insert_resource(ListenOn {
            port: self.port,
            timeout,
        })
        .init_resource::<Peers>()
        .add_systems(Startup, start_server)
        .add_observer(new_link)
        .add_observer(connected)
        .add_observer(disconnected)
        .add_observer(replicate_zoned)
        .add_observer(replicate_clock)
        // Requests are read as they arrive (they wait in the rules half's
        // inbox for its next tick); events go out after each tick.
        .add_systems(
            PreUpdate,
            receive_requests.after(ly::MessageSystems::Receive),
        )
        .add_systems(
            FixedUpdate,
            (zone_visibility, send_events)
                .chain()
                .after(AuthoritySystems::Maintain),
        );
    }
}

#[derive(Resource)]
struct ListenOn {
    port: u16,
    timeout: f32,
}

/// Who is connected: player → their connection (link entity).
#[derive(Resource, Default)]
pub struct Peers {
    pub links: HashMap<PlayerId, Entity>,
    next: u64,
}

/// On a connection: the player it belongs to.
#[derive(Component, Debug, Clone, Copy)]
pub struct Peer(pub PlayerId);

fn start_server(mut commands: Commands, listen: Res<ListenOn>) {
    let address = SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), listen.port);
    let server = commands
        .spawn((
            lys::NetcodeServer::new(lys::NetcodeConfig {
                protocol_id: PROTOCOL_ID,
                private_key: PRIVATE_KEY,
                client_timeout_secs: listen.timeout.ceil() as i32,
                // Friends connect to the host's public address, which the
                // server can't know; don't insist on it.
                server_addr_check: false,
                ..default()
            }),
            ly::LocalAddr(address),
            lys::ServerUdpIo::default(),
        ))
        .id();
    commands.trigger(lys::Start { entity: server });
    info!("Listening for players on UDP port {}", listen.port);
}

/// A new connection may send and receive world state.
fn new_link(added: On<Add, ly::LinkOf>, mut commands: Commands) {
    commands.entity(added.entity).insert(ly::ReplicationSender);
}

/// A connection was accepted: give it a player id and say hello.
fn connected(
    added: On<Add, ly::Connected>,
    mut commands: Commands,
    links: Query<(), With<ly::LinkOf>>,
    mut peers: ResMut<Peers>,
    mut senders: Query<&mut ly::MessageSender<ServerEvent>>,
) {
    if !links.contains(added.entity) {
        return;
    }
    peers.next += 1;
    let player = PlayerId(peers.next);
    peers.links.insert(player, added.entity);
    commands.entity(added.entity).insert(Peer(player));
    if let Ok(mut sender) = senders.get_mut(added.entity) {
        sender.send::<Reliable>(ServerEvent::Welcome { player });
    }
    info!("A player connected ({player:?})");
}

/// A connection closed: the player leaves the world (and is saved).
fn disconnected(
    added: On<Add, ly::Disconnected>,
    peers_on: Query<&Peer>,
    mut peers: ResMut<Peers>,
    mut link: ResMut<Link>,
) {
    let Ok(&Peer(player)) = peers_on.get(added.entity) else {
        return;
    };
    peers.links.remove(&player);
    link.to_authority.push((player, ClientRequest::Leave));
    info!("A player left ({player:?})");
}

/// Everything in a zone is sent to players (only to those in the same
/// zone; see `zone_visibility`).
fn replicate_zoned(added: On<Add, Zone>, mut commands: Commands) {
    commands
        .entity(added.entity)
        .insert(ly::Replicate::to_clients(ly::NetworkTarget::All));
}

/// The clock is sent to everyone.
fn replicate_clock(added: On<Add, WorldClock>, mut commands: Commands) {
    commands
        .entity(added.entity)
        .insert(ly::Replicate::to_clients(ly::NetworkTarget::All));
}

/// Requests from every connection go to the rules half, as from its player.
fn receive_requests(
    mut connections: Query<(&Peer, &mut ly::MessageReceiver<ClientRequest>)>,
    mut link: ResMut<Link>,
) {
    for (peer, mut receiver) in &mut connections {
        for request in receiver.receive() {
            // Players can't leave on someone else's behalf, or pretend a
            // connection closed.
            if request != ClientRequest::Leave {
                link.to_authority.push((peer.0, request));
            }
        }
    }
}

/// Each player only receives what is in their character's zone.
fn zone_visibility(
    mut commands: Commands,
    peers: Res<Peers>,
    index: Res<PlayerIndex>,
    things: Query<(Entity, &Zone), With<ly::Replicate>>,
    mut shown: Local<HashMap<(Entity, Entity), bool>>,
) {
    shown.retain(|(thing, link), _| {
        things.contains(*thing) && peers.links.values().any(|l| l == link)
    });
    for (player, &link) in &peers.links {
        let here = index
            .0
            .get(player)
            .and_then(|&me| things.get(me).ok())
            .map(|(_, zone)| zone.0.as_str());
        for (thing, zone) in &things {
            let visible = here == Some(zone.0.as_str());
            if shown.get(&(thing, link)) != Some(&visible) {
                if visible {
                    commands.gain_visibility(thing, link);
                } else {
                    commands.lose_visibility(thing, link);
                }
                shown.insert((thing, link), visible);
            }
        }
    }
}

/// Events go to the players they concern: private ones to their player,
/// the rest to everyone in the zone where they happened.
fn send_events(
    mut link: ResMut<Link>,
    peers: Res<Peers>,
    index: Res<PlayerIndex>,
    zones: Query<&Zone>,
    mut senders: Query<&mut ly::MessageSender<ServerEvent>>,
) {
    let zone_of = |player: &PlayerId| {
        index
            .0
            .get(player)
            .and_then(|&e| zones.get(e).ok())
            .map(|z| z.0.clone())
    };
    for mut event in std::mem::take(&mut link.to_client) {
        let to: Vec<Entity> = if let Some(player) = event.for_player() {
            peers.links.get(&player).copied().into_iter().collect()
        } else {
            let zone = match event.zone() {
                Some(zone) => Some(zone.to_owned()),
                None => event
                    .entities_mut()
                    .first()
                    .and_then(|e| zones.get(**e).ok())
                    .map(|z| z.0.clone()),
            };
            peers
                .links
                .iter()
                .filter(|(player, _)| zone.is_none() || zone_of(player) == zone)
                .map(|(_, &l)| l)
                .collect()
        };
        for l in to {
            if let Ok(mut sender) = senders.get_mut(l) {
                sender.send::<Reliable>(event.clone());
            }
        }
    }
}
