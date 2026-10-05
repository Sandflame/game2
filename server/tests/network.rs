//! The server and a bare client talking over real UDP on this computer:
//! connecting, joining, the world arriving, movement reports being
//! accepted or refused, and entities named in requests and events meaning
//! the same thing on both sides. Uses the real game data.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use lightyear::prelude as ly;
use lightyear::prelude::client as lyc;
use server::net::{
    Moves, NetServerPlugin, PRIVATE_KEY, PROTOCOL_ID, ProtocolPlugin, Reliable, server_plugins,
};
use server::{AuthorityPlugin, EnemyKind};
use shared::combat::Reject;
use shared::components::{CharacterName, Motion, MoveEpoch, PlayerId};
use shared::data::find_assets_dir;
use shared::gamedata::GameData;
use shared::movement::MoveInput;
use shared::protocol::{ClientRequest, ServerEvent};

/// Events the test client has received.
#[derive(Resource, Default)]
struct Inbox(Vec<ServerEvent>);

/// Requests the test client will send.
#[derive(Resource, Default)]
struct Outbox(Vec<ClientRequest>);

fn read_events(
    mut receivers: Query<&mut ly::MessageReceiver<ServerEvent>>,
    mut inbox: ResMut<Inbox>,
) {
    for mut receiver in &mut receivers {
        inbox.0.extend(receiver.receive());
    }
}

fn send_requests(
    mut senders: Query<&mut ly::MessageSender<ClientRequest>>,
    mut outbox: ResMut<Outbox>,
) {
    let Ok(mut sender) = senders.single_mut() else {
        return;
    };
    for request in outbox.0.drain(..) {
        if matches!(request, ClientRequest::Moved { .. }) {
            sender.send::<Moves>(request);
        } else {
            sender.send::<Reliable>(request);
        }
    }
}

fn game_data() -> GameData {
    let mut data = GameData::load(&find_assets_dir().unwrap()).unwrap();
    // Start next to the training dummies.
    data.player.start_zone = "sandbox".into();
    data
}

fn server_app(port: u16) -> App {
    let assets = find_assets_dir().unwrap();
    let data = game_data();
    let zones = data.load_zones(&assets).unwrap();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .insert_resource(Time::<Fixed>::from_hz(data.config.simulation.tick_hz))
        .insert_resource(data.clone())
        .insert_resource(zones)
        .add_plugins(AuthorityPlugin);
    server_plugins(&mut app, &data);
    app.add_plugins(NetServerPlugin { port });
    // What `App::run` would do before the first update.
    app.finish();
    app.cleanup();
    app
}

fn client_app(port: u16) -> App {
    let data = game_data();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .add_plugins(lyc::ClientPlugins {
            tick_duration: Duration::from_secs_f64(1.0 / data.config.simulation.tick_hz),
        })
        .add_plugins(ProtocolPlugin)
        .init_resource::<Inbox>()
        .init_resource::<Outbox>()
        .add_systems(PreUpdate, read_events.after(ly::MessageSystems::Receive))
        .add_systems(PostUpdate, send_requests.before(ly::MessageSystems::Send));
    let server = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let auth = ly::Authentication::Manual {
        server_addr: server,
        client_id: 7,
        private_key: PRIVATE_KEY,
        protocol_id: PROTOCOL_ID,
    };
    let netcode = lyc::NetcodeClient::new(
        auth,
        lyc::NetcodeConfig {
            token_expire_secs: -1,
            ..default()
        },
    )
    .unwrap();
    let client = app
        .world_mut()
        .spawn((
            lyc::Client,
            ly::Link::default(),
            ly::LocalAddr(std::net::SocketAddr::from(([0, 0, 0, 0], 0))),
            ly::PeerAddr(server),
            ly::ReplicationReceiver,
            netcode,
            ly::UdpIo::default(),
        ))
        .id();
    app.finish();
    app.cleanup();
    app.world_mut().trigger(lyc::Connect { entity: client });
    app
}

/// A free UDP port on this computer.
fn free_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Run both until `done` says so (or fail after a while).
fn run_until(
    server: &mut App,
    client: &mut App,
    what: &str,
    mut done: impl FnMut(&mut App) -> bool,
) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        server.update();
        client.update();
        if done(client) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for: {what}");
}

fn my_character(client: &mut App, me: PlayerId) -> Option<(Entity, Motion, MoveEpoch)> {
    let world = client.world_mut();
    let mut query = world.query::<(Entity, &PlayerId, &Motion, &MoveEpoch, &CharacterName)>();
    query
        .iter(world)
        .find(|(_, id, ..)| **id == me)
        .map(|(e, _, m, epoch, _)| (e, *m, *epoch))
}

#[test]
fn a_player_connects_joins_moves_and_targets_over_the_network() {
    let port = free_port();
    let mut server = server_app(port);
    let mut client = client_app(port);

    // Connecting: the server says hello with our player id.
    let mut me = None;
    run_until(&mut server, &mut client, "a welcome", |client| {
        me = client
            .world()
            .resource::<Inbox>()
            .0
            .iter()
            .find_map(|e| match e {
                ServerEvent::Welcome { player } => Some(*player),
                _ => None,
            });
        me.is_some()
    });
    let me = me.unwrap();

    // Joining (no save file on this server: any name will do).
    client
        .world_mut()
        .resource_mut::<Outbox>()
        .0
        .push(ClientRequest::Join {
            name: "Tester".into(),
        });
    run_until(&mut server, &mut client, "our character", |client| {
        my_character(client, me).is_some()
    });
    let (_, start, epoch) = my_character(&mut client, me).unwrap();

    // A small step forward is accepted.
    let mut step = start.0;
    step.position.z -= 0.1;
    client
        .world_mut()
        .resource_mut::<Outbox>()
        .0
        .push(ClientRequest::Moved {
            input: MoveInput {
                direction: Vec2::new(0.0, -1.0),
                ..default()
            },
            state: step,
            epoch: epoch.0,
        });
    run_until(&mut server, &mut client, "the step", |client| {
        my_character(client, me)
            .is_some_and(|(_, m, _)| m.0.position.distance(step.position) < 1e-3)
    });

    // A leap across the zone is refused: the move counter goes up and we
    // stay where we were.
    let mut leap = step;
    leap.position.z -= 40.0;
    client
        .world_mut()
        .resource_mut::<Outbox>()
        .0
        .push(ClientRequest::Moved {
            input: MoveInput::default(),
            state: leap,
            epoch: epoch.0,
        });
    run_until(&mut server, &mut client, "the leap refused", |client| {
        my_character(client, me)
            .is_some_and(|(_, m, e)| e.0 != epoch.0 && m.0.position.distance(step.position) < 1e-3)
    });

    // Aiming at a training dummy we see: the server knows which one we
    // mean (it is too far away, rather than not a target at all).
    let dummy = {
        let world = client.world_mut();
        let mut dummies = world.query::<(Entity, &EnemyKind)>();
        dummies
            .iter(world)
            .find(|(_, kind)| kind.0 == "training_dummy")
            .map(|(e, _)| e)
            .expect("the dummies came over the network")
    };
    client.world_mut().resource_mut::<Inbox>().0.clear();
    client
        .world_mut()
        .resource_mut::<Outbox>()
        .0
        .push(ClientRequest::UseAbility {
            slot: 0,
            target: Some(dummy),
        });
    run_until(
        &mut server,
        &mut client,
        "an answer about the dummy",
        |client| {
            client.world().resource::<Inbox>().0.iter().any(|e| {
                matches!(
                    e,
                    ServerEvent::Rejected { .. } | ServerEvent::AbilityUsed { .. }
                )
            })
        },
    );
    let answer = client.world().resource::<Inbox>().0.clone();
    for event in answer {
        match event {
            ServerEvent::Rejected { reason, .. } => assert_eq!(reason, Reject::OutOfRange),
            ServerEvent::AbilityUsed { target, .. } => assert_eq!(target, Some(dummy)),
            _ => {}
        }
    }
}
