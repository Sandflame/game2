//! Playing on a server: connecting, passing requests and events through
//! the connection, the server's clock, moving your own character at once
//! (the server checks the moves) and smoothing everyone else's movement.
//!
//! Playing on this computer the rules half runs inside the game (as before)
//! and none of this is used. The server's side is `server/src/net.rs`.

use std::collections::VecDeque;
use std::net::{SocketAddr, ToSocketAddrs};

use bevy::prelude::*;
use lightyear::prelude as ly;
use lightyear::prelude::client as lyc;
use server::net::{Moves, PRIVATE_KEY, PROTOCOL_ID, Reliable};
use server::{AuthorityActive, Defeated, Riding};
use shared::components::{Motion, MoveEpoch, WorldClock, Zone};
use shared::gamedata::{GameData, Zones};
use shared::movement::{self, MoveInput, MoveState};
use shared::protocol::{ClientRequest, Link, ServerEvent};
use shared::statuses::Statuses;

use crate::characters::{DisplayMotion, LocalPlayer, interpolate_transforms};
use crate::session::{LocalPlayerId, receive_events};

pub struct NetPlugin;

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Connection>()
            .init_resource::<GameClock>()
            .init_resource::<HeldInput>()
            .add_observer(connecting)
            .add_observer(connected)
            .add_observer(disconnected)
            .add_systems(
                PreUpdate,
                (receive_from_server, tick_clock)
                    .chain()
                    .after(ly::MessageSystems::Receive)
                    .before(receive_events),
            )
            .add_systems(FixedUpdate, move_myself.run_if(online))
            .add_systems(PostUpdate, send_to_server.before(ly::MessageSystems::Send))
            .add_systems(
                RunFixedMainLoop,
                (remember_motion, smooth_others)
                    .chain()
                    .run_if(online)
                    .after(interpolate_transforms)
                    .in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
            );
    }
}

/// Where the game's rules run.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub enum Connection {
    /// On this computer (or nothing chosen yet).
    #[default]
    Local,
    /// Trying to reach a server.
    Connecting(String),
    /// Connected to a server.
    Online(String),
    /// The connection failed or closed (shown on the login screen).
    Lost(String),
}

pub fn online(connection: Res<Connection>) -> bool {
    matches!(*connection, Connection::Online(_))
}

/// Turn "host", "host:port" or "1.2.3.4:port" into an address; the port is
/// `default_port` if not given.
pub fn parse_address(text: &str, default_port: u16) -> Result<SocketAddr, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Type the server's address.".into());
    }
    let with_port = if text
        .rsplit_once(':')
        .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
    {
        text.to_owned()
    } else {
        format!("{text}:{default_port}")
    };
    with_port
        .to_socket_addrs()
        .ok()
        .and_then(|mut found| found.find(SocketAddr::is_ipv4).or(found.next()))
        .ok_or_else(|| format!("Couldn't find a server called \"{text}\"."))
}

/// Start connecting to a server.
pub fn connect(
    commands: &mut Commands,
    address: SocketAddr,
    data: &GameData,
) -> Result<(), String> {
    let auth = ly::Authentication::Manual {
        server_addr: address,
        // Players are told apart by the server, not by this number; it
        // only has to differ between connections.
        client_id: rand_id(),
        private_key: PRIVATE_KEY,
        protocol_id: PROTOCOL_ID,
    };
    let config = lyc::NetcodeConfig {
        client_timeout_secs: data.config.network.timeout.ceil() as i32,
        token_expire_secs: -1,
        ..default()
    };
    let netcode = lyc::NetcodeClient::new(auth, config).map_err(|e| e.to_string())?;
    let local = SocketAddr::new(std::net::Ipv4Addr::UNSPECIFIED.into(), 0);
    let client = commands
        .spawn((
            lyc::Client,
            ly::Link::default(),
            ly::LocalAddr(local),
            ly::PeerAddr(address),
            ly::ReplicationReceiver,
            netcode,
            ly::UdpIo::default(),
            Attempt(false),
        ))
        .id();
    commands.trigger(lyc::Connect { entity: client });
    Ok(())
}

/// A number that is different every time the game starts.
fn rand_id() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    shared::formulas::Rng::new(nanos ^ u64::from(std::process::id())).next_u64()
}

/// On our connection: whether it has started trying (a new connection
/// starts out "disconnected", which isn't a failure).
#[derive(Component)]
struct Attempt(bool);

fn connecting(added: On<Add, lyc::Connecting>, mut attempts: Query<&mut Attempt>) {
    if let Ok(mut attempt) = attempts.get_mut(added.entity) {
        attempt.0 = true;
    }
}

fn connected(
    _added: On<Add, lyc::Connected>,
    mut connection: ResMut<Connection>,
    mut authority: ResMut<AuthorityActive>,
) {
    if let Connection::Connecting(address) = &*connection {
        *connection = Connection::Online(address.clone());
        // The server runs the rules now.
        authority.0 = false;
    }
}

fn disconnected(
    added: On<Add, lyc::Disconnected>,
    mut commands: Commands,
    reasons: Query<(&lyc::Disconnected, &Attempt)>,
    mut connection: ResMut<Connection>,
) {
    if !reasons
        .get(added.entity)
        .is_ok_and(|(_, attempt)| attempt.0)
    {
        return;
    }
    let why = reasons
        .get(added.entity)
        .map(|(d, _)| format!("{:?}", d.reason))
        .unwrap_or_default();
    *connection = match &*connection {
        Connection::Connecting(address) => {
            Connection::Lost(format!("Couldn't reach the server at {address}."))
        }
        Connection::Online(_) => Connection::Lost("Lost the connection to the server.".into()),
        other => other.clone(),
    };
    info!("Disconnected: {why}");
    commands.entity(added.entity).try_despawn();
}

/// Events from the server go where the rules half's events would.
fn receive_from_server(
    mut receivers: Query<&mut ly::MessageReceiver<ServerEvent>, With<lyc::Client>>,
    mut link: ResMut<Link>,
    mut me: ResMut<LocalPlayerId>,
) {
    for mut receiver in &mut receivers {
        for event in receiver.receive() {
            if let ServerEvent::Welcome { player } = event {
                me.0 = player;
            }
            link.to_client.push(event);
        }
    }
}

/// Requests go to the server: movement reports on their own channel
/// (only the newest matters), everything else in order.
fn send_to_server(
    connection: Res<Connection>,
    mut senders: Query<&mut ly::MessageSender<ClientRequest>, With<lyc::Client>>,
    mut link: ResMut<Link>,
) {
    if !matches!(*connection, Connection::Online(_)) {
        return;
    }
    let Ok(mut sender) = senders.single_mut() else {
        return;
    };
    for (_, request) in std::mem::take(&mut link.to_authority) {
        match request {
            // The server moves us only from our reports.
            ClientRequest::Move(_) => {}
            ClientRequest::Moved { .. } => sender.send::<Moves>(request),
            _ => sender.send::<Reliable>(request),
        }
    }
}

/// The time on the rules half's clock (cooldowns, casts and markers use
/// it). Playing on this computer it is the game's own fixed clock; on a
/// server it is the server's, worked out from its clock updates.
#[derive(Resource, Default)]
pub struct GameClock {
    pub now: f64,
    /// Server time minus this computer's time.
    offset: Option<f64>,
}

/// How quickly the clock follows new news from the server (0–1 a frame).
const CLOCK_FOLLOW: f64 = 0.05;
/// Clock jumps bigger than this (seconds) are taken at once.
const CLOCK_SNAP: f64 = 0.5;

fn tick_clock(
    fixed: Res<Time<Fixed>>,
    real: Res<Time<Real>>,
    connection: Res<Connection>,
    clocks: Query<Ref<WorldClock>>,
    mut clock: ResMut<GameClock>,
) {
    if !matches!(*connection, Connection::Online(_)) {
        clock.now = fixed.elapsed_secs_f64() + fixed.overstep().as_secs_f64();
        return;
    }
    let local = real.elapsed_secs_f64();
    if let Ok(world) = clocks.single()
        && world.is_changed()
    {
        let sample = world.0 - local;
        clock.offset = Some(match clock.offset {
            Some(offset) if (sample - offset).abs() < CLOCK_SNAP => {
                offset + (sample - offset) * CLOCK_FOLLOW
            }
            _ => sample,
        });
    }
    clock.now = local + clock.offset.unwrap_or(0.0);
}

/// The movement keys of this frame, kept until a tick uses them (a jump
/// pressed between two ticks still happens).
#[derive(Resource, Default)]
pub struct HeldInput(pub MoveInput);

impl HeldInput {
    pub fn set(&mut self, input: MoveInput) {
        let jump = self.0.jump || input.jump;
        self.0 = input;
        self.0.jump = jump;
    }
}

/// Where our own game has our character (on a server).
#[derive(Component)]
pub struct Predicted {
    pub state: MoveState,
    pub epoch: u32,
}

/// Move our own character at once with the same rules the server uses,
/// and tell the server where we got to. When the server moved us itself
/// (a portal, a lunge, a revive…) its move counter changes and we jump to
/// where it has us.
fn move_myself(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    zones: Res<Zones>,
    mut held: ResMut<HeldInput>,
    me: Res<LocalPlayerId>,
    mut link: ResMut<Link>,
    mine: Option<
        Single<
            (
                Entity,
                &Motion,
                &MoveEpoch,
                &Zone,
                &Statuses,
                Has<Riding>,
                Has<Defeated>,
                Option<&mut Predicted>,
                &mut DisplayMotion,
            ),
            With<LocalPlayer>,
        >,
    >,
) {
    let Some(mine) = mine else {
        return;
    };
    let (entity, motion, epoch, zone, statuses, riding, defeated, predicted, mut display) =
        mine.into_inner();
    let keys = held.0;
    held.0.jump = false;
    let Some(mut predicted) = predicted else {
        commands.entity(entity).insert(Predicted {
            state: motion.0,
            epoch: epoch.0,
        });
        return;
    };
    if predicted.epoch != epoch.0 || riding || defeated {
        predicted.state = motion.0;
        predicted.epoch = epoch.0;
    } else if let Some(level) = zones.get(&zone.0) {
        let speed = statuses.modifiers(|id| data.statuses.get(id)).move_speed;
        let mut input = keys;
        input.direction = input.direction.clamp_length_max(1.0) * speed;
        predicted.state = movement::step(
            predicted.state,
            input,
            &data.config.movement,
            level,
            time.delta_secs(),
        );
        // The keys as pressed: the server uses them to tell whether we are
        // moving (moving interrupts casts).
        link.to_authority.push((
            me.0,
            ClientRequest::Moved {
                input: keys,
                state: predicted.state,
                epoch: predicted.epoch,
            },
        ));
    }
    // Drawn between the last two ticks, like everything was before.
    display.previous = display.current;
    display.current = predicted.state;
}

/// Recent positions of another character, with the server time of each.
#[derive(Component, Default)]
pub struct MotionHistory(VecDeque<(f64, MoveState)>);

/// How many positions to keep per character.
const HISTORY: usize = 32;

/// Note each new position the server sends (with the server's time).
fn remember_motion(
    mut commands: Commands,
    clock: Res<GameClock>,
    world: Query<&WorldClock>,
    mut moved: Query<(Entity, Ref<Motion>, Option<&mut MotionHistory>)>,
) {
    let stamp = world.single().map_or(clock.now, |c| c.0);
    for (entity, motion, history) in &mut moved {
        match history {
            Some(mut history) => {
                if motion.is_changed() {
                    let last = history.0.back().map(|(t, _)| *t);
                    // Several changes within one server update share a time.
                    let at = last.map_or(stamp, |t| stamp.max(t + 1e-3));
                    history.0.push_back((at, motion.0));
                    while history.0.len() > HISTORY {
                        history.0.pop_front();
                    }
                }
            }
            None => {
                commands
                    .entity(entity)
                    .insert(MotionHistory(VecDeque::from([(stamp, motion.0)])));
            }
        }
    }
}

/// Show other characters a little in the past, sliding smoothly between
/// the positions the server sent (and ourselves too while on a ride, which
/// the server moves).
fn smooth_others(
    clock: Res<GameClock>,
    data: Res<GameData>,
    mut characters: Query<(
        &MotionHistory,
        &mut DisplayMotion,
        &mut Transform,
        Has<LocalPlayer>,
        Has<Riding>,
    )>,
) {
    let at = clock.now - f64::from(data.config.network.smoothing);
    for (history, mut display, mut transform, mine, riding) in &mut characters {
        if mine && !riding {
            continue;
        }
        let Some(state) = sample(&history.0, at) else {
            continue;
        };
        display.previous = state;
        display.current = state;
        transform.translation = state.position;
        transform.rotation = Quat::from_rotation_y(state.yaw);
    }
}

/// The state at time `at`, blending the two samples around it.
fn sample(history: &VecDeque<(f64, MoveState)>, at: f64) -> Option<MoveState> {
    let (first, last) = (history.front()?, history.back()?);
    if at <= first.0 {
        return Some(first.1);
    }
    if at >= last.0 {
        return Some(last.1);
    }
    let after = history.iter().position(|(t, _)| *t >= at)?;
    let (t0, a) = history[after - 1];
    let (t1, b) = history[after];
    let f = ((at - t0) / (t1 - t0).max(1e-6)) as f32;
    let mut state = b;
    state.position = a.position.lerp(b.position, f);
    state.yaw = Quat::from_rotation_y(a.yaw)
        .slerp(Quat::from_rotation_y(b.yaw), f)
        .to_euler(EulerRot::YXZ)
        .0;
    Some(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32) -> MoveState {
        MoveState::spawn_at(Vec3::new(x, 0.0, 0.0))
    }

    #[test]
    fn others_slide_between_the_positions_the_server_sent() {
        let history = VecDeque::from([(1.0, at(0.0)), (1.1, at(1.0)), (1.2, at(3.0))]);
        assert_eq!(sample(&history, 0.5).unwrap().position.x, 0.0);
        assert!((sample(&history, 1.05).unwrap().position.x - 0.5).abs() < 1e-4);
        assert!((sample(&history, 1.15).unwrap().position.x - 2.0).abs() < 1e-4);
        assert_eq!(sample(&history, 9.0).unwrap().position.x, 3.0);
        assert!(sample(&VecDeque::new(), 1.0).is_none());
    }

    #[test]
    fn addresses_get_the_default_port() {
        assert_eq!(
            parse_address("127.0.0.1", 5888).unwrap(),
            "127.0.0.1:5888".parse().unwrap()
        );
        assert_eq!(
            parse_address(" 10.0.0.2:7000 ", 5888).unwrap(),
            "10.0.0.2:7000".parse().unwrap()
        );
        assert!(parse_address("", 5888).is_err());
    }
}
