//! Getting around and talking: portals (including closed gates and
//! scripted rides such as the root slide) and the people you can talk to.

use bevy::prelude::*;
use shared::combat::Reject;
use shared::components::{
    CharacterName, Faction, HitRadius, Motion, NpcId, PlayerId, VisualKey, Zone,
};
use shared::quests::QuestLog;

use crate::progression::PendingRewards;
use crate::quests;
use shared::enemy_ai::ground_distance;
use shared::gamedata::{GameData, Zones};
use shared::level::{NpcDef, Portal};
use shared::movement::{MoveState, yaw_from_direction};

use crate::instances::Instances;
use shared::protocol::{Link, ServerEvent};

/// Someone to talk to. Each time a player talks to them they say their
/// next line.
#[derive(Component, Debug, Clone)]
pub struct Npc {
    pub lines: Vec<String>,
    pub next: usize,
}

/// A character on a scripted ride (e.g. sliding down the giant root).
/// They can't move or act until it ends.
#[derive(Component, Debug, Clone)]
pub struct Riding {
    pub ride: String,
    pub started: f64,
}

/// Where a character came into a dungeon or trial from: "back" portals
/// (and the way out after the boss) take them there.
#[derive(Component, Debug, Clone)]
pub struct CameFrom {
    pub zone: String,
    pub position: Vec3,
    pub yaw: f32,
}

/// Put one person in place.
pub fn spawn_npc(commands: &mut Commands, data: &GameData, npc: &NpcDef, zone: &str) {
    commands.spawn((
        Npc {
            lines: npc.lines.clone(),
            next: 0,
        },
        NpcId(npc.id.clone()),
        CharacterName(npc.name.clone()),
        Motion(MoveState {
            yaw: npc.yaw,
            ..MoveState::spawn_at(npc.position)
        }),
        Zone(zone.to_owned()),
        Faction::Neutral,
        HitRadius(data.player.hit_radius),
        VisualKey(npc.visual.clone()),
    ));
}

/// What using a portal did.
pub enum PortalUse {
    /// Moved to another zone (or onto a ride).
    Travelled,
    /// A closed gate: says why.
    Closed,
    /// A dungeon board: the player was shown the list.
    Board,
}

/// Everything needed to move characters between zones.
pub struct Travel<'a, 'w, 's> {
    pub commands: &'a mut Commands<'w, 's>,
    pub link: &'a mut Link,
    pub data: &'a GameData,
    pub zones: &'a Zones,
    pub instances: &'a mut Instances,
    /// The zones players are in right now (to join a group's dungeon copy).
    pub occupied: &'a [String],
    pub now: f64,
}

impl Travel<'_, '_, '_> {
    /// Move a character to `zone` (for an instanced zone: their group's
    /// copy of it) at `position`, facing `yaw`.
    pub fn go_to(
        &mut self,
        entity: Entity,
        motion: &mut Motion,
        zone: &str,
        position: Vec3,
        yaw: f32,
    ) {
        let zone = self
            .instances
            .enter(self.commands, self.data, self.zones, zone, self.occupied);
        motion.0 = MoveState {
            yaw,
            ..MoveState::spawn_at(position)
        };
        self.commands.entity(entity).insert(Zone(zone.clone()));
        self.link
            .to_client
            .push(ServerEvent::ZoneChanged { entity, zone });
    }

    /// Go to a dungeon or trial, remembering where to come back to.
    fn enter_from(
        &mut self,
        entity: Entity,
        motion: &mut Motion,
        here: &Zone,
        doorway: (Vec3, f32),
        to: &str,
        arrive: Vec3,
        arrive_yaw: f32,
    ) {
        if self.zones.get(to).is_some_and(|l| l.instanced) {
            let (position, yaw) = step_away(doorway.0, doorway.1, motion.0.position);
            self.commands.entity(entity).insert(CameFrom {
                zone: here.0.clone(),
                position,
                yaw,
            });
        }
        self.go_to(entity, motion, to, arrive, arrive_yaw);
    }

    /// Leave by a "back" portal: to where the character came from, or the
    /// zone's `exit` if that isn't known.
    fn go_back(
        &mut self,
        entity: Entity,
        motion: &mut Motion,
        here: &Zone,
        came_from: Option<&CameFrom>,
    ) {
        let known = came_from.filter(|c| {
            let instanced = self.zones.get(&c.zone).is_some_and(|l| l.instanced);
            self.zones.get(&c.zone).is_some()
                && (!instanced || self.instances.open.contains(&c.zone))
        });
        let (zone, position, yaw) =
            match (known, self.zones.get(&here.0).and_then(|l| l.exit.as_ref())) {
                (Some(c), _) => (c.zone.clone(), c.position, c.yaw),
                (None, Some(exit)) => (exit.to.clone(), exit.arrive, exit.arrive_yaw),
                (None, None) => {
                    let start = &self.data.player.start_zone;
                    let level = self.zones.get(start);
                    (
                        start.clone(),
                        level.map_or(Vec3::ZERO, |l| l.spawn_point),
                        level.map_or(0.0, |l| l.spawn_yaw),
                    )
                }
            };
        self.commands.entity(entity).remove::<CameFrom>();
        self.go_to(entity, motion, &zone, position, yaw);
    }
}

/// Just outside a doorway of this radius, on the side `from` is on, facing
/// away from it (so you don't arrive standing in it again).
pub fn step_away(centre: Vec3, radius: f32, from: Vec3) -> (Vec3, f32) {
    const OUTSIDE: f32 = 1.5;
    let offset = Vec2::new(from.x - centre.x, from.z - centre.z);
    let direction = offset.try_normalize().unwrap_or(Vec2::Y);
    let position = centre + Vec3::new(direction.x, 0.0, direction.y) * (radius + OUTSIDE);
    (position, yaw_from_direction(direction))
}

/// Use a portal the player is standing in.
pub fn use_portal(
    travel: &mut Travel,
    player: PlayerId,
    entity: Entity,
    motion: &mut Motion,
    here: &Zone,
    portal: &Portal,
    came_from: Option<&CameFrom>,
) -> PortalUse {
    if let Some(message) = &portal.closed {
        travel.link.to_client.push(ServerEvent::Speech {
            player,
            speaker: portal.label.clone(),
            text: message.clone(),
        });
        return PortalUse::Closed;
    }
    if portal.board {
        travel
            .link
            .to_client
            .push(ServerEvent::OpenBoard { player });
        return PortalUse::Board;
    }
    if portal.back {
        travel.go_back(entity, motion, here, came_from);
        return PortalUse::Travelled;
    }
    if let Some(ride_id) = &portal.ride
        && let Some(ride) = travel.data.rides.get(ride_id)
    {
        let (start, yaw) = ride.at(0.0);
        travel.go_to(entity, motion, &ride.zone, start, yaw);
        travel.commands.entity(entity).insert(Riding {
            ride: ride_id.clone(),
            started: travel.now,
        });
        return PortalUse::Travelled;
    }
    travel.enter_from(
        entity,
        motion,
        here,
        (portal.position, portal.radius),
        &portal.to,
        portal.arrive,
        portal.arrive_yaw,
    );
    PortalUse::Travelled
}

/// At a dungeon board, the player picked a dungeon or trial.
pub fn enter_from_board(
    travel: &mut Travel,
    entity: Entity,
    motion: &mut Motion,
    here: &Zone,
    to: &str,
) -> Result<(), Reject> {
    let board = travel
        .zones
        .get(&here.0)
        .and_then(|level| level.portal_at(motion.0.position))
        .filter(|portal| portal.board)
        .ok_or(Reject::NotAtBoard)?;
    let level = travel
        .zones
        .0
        .get(to)
        .filter(|level| level.listing.is_some())
        .ok_or(Reject::NoSuchPlace)?;
    let (position, yaw) = (level.spawn_point, level.spawn_yaw);
    let doorway = (board.position, board.radius);
    travel.enter_from(entity, motion, here, doorway, to, position, yaw);
    Ok(())
}

/// The people a player can talk to.
pub type People<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Npc,
        &'static CharacterName,
        &'static NpcId,
        &'static Zone,
        &'static Motion,
    ),
    Without<PlayerId>,
>;

/// Talk to the nearest person in reach, if any: their part in a quest if
/// they have one, otherwise their next line. Returns whether someone
/// answered.
pub fn talk(
    link: &mut Link,
    data: &GameData,
    quest: (&mut PendingRewards, Option<&mut QuestLog>),
    who: (PlayerId, Entity),
    zone: &Zone,
    position: Vec3,
    npcs: &mut People,
) -> bool {
    let (player, entity) = who;
    let nearest = npcs
        .iter_mut()
        .filter(|(.., z, m)| {
            *z == zone && ground_distance(m.0.position, position) <= data.player.talk_distance
        })
        .min_by(|a, b| {
            ground_distance(a.4.0.position, position)
                .total_cmp(&ground_distance(b.4.0.position, position))
        });
    let Some((mut npc, name, id, ..)) = nearest else {
        return false;
    };
    let (rewards, log) = quest;
    if let Some(log) = log
        && quests::talked(data, link, rewards, player, entity, &zone.0, log, &id.0)
    {
        return true;
    }
    if npc.lines.is_empty() {
        return false;
    }
    let line = npc.lines[npc.next % npc.lines.len()].clone();
    npc.next = (npc.next + 1) % npc.lines.len();
    link.to_client.push(ServerEvent::Speech {
        player,
        speaker: name.0.clone(),
        text: line,
    });
    true
}

/// Move riders along their ride; at the end they arrive at its destination.
pub fn advance_rides(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    zones: Res<Zones>,
    mut instances: ResMut<Instances>,
    mut link: ResMut<Link>,
    mut riders: Query<(Entity, &Riding, &mut Motion)>,
    players: Query<&Zone, With<PlayerId>>,
) {
    let now = time.elapsed_secs_f64();
    let occupied: Vec<String> = players.iter().map(|z| z.0.clone()).collect();
    for (entity, riding, mut motion) in &mut riders {
        let Some(ride) = data.rides.get(&riding.ride) else {
            commands.entity(entity).remove::<Riding>();
            continue;
        };
        let t = ((now - riding.started) / f64::from(ride.duration)) as f32;
        if t < 1.0 {
            let (position, yaw) = ride.at(t);
            motion.0 = MoveState {
                position,
                yaw,
                ..MoveState::spawn_at(position)
            };
            continue;
        }
        commands.entity(entity).remove::<Riding>();
        let mut travel = Travel {
            commands: &mut commands,
            link: &mut link,
            data: &data,
            zones: &zones,
            instances: &mut instances,
            occupied: &occupied,
            now,
        };
        travel.go_to(entity, &mut motion, &ride.to, ride.arrive, ride.arrive_yaw);
    }
}

/// Riders can't do anything else until the ride ends.
pub fn busy_riding(link: &mut Link, player: PlayerId) {
    link.to_client.push(ServerEvent::Rejected {
        player,
        reason: Reject::Busy,
    });
}
