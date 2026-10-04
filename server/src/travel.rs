//! Getting around and talking: portals (including closed gates and
//! scripted rides such as the root slide) and the people you can talk to.

use bevy::prelude::*;
use shared::combat::Reject;
use shared::components::{CharacterName, Faction, HitRadius, Motion, PlayerId, VisualKey, Zone};
use shared::enemy_ai::ground_distance;
use shared::gamedata::{GameData, Zones};
use shared::level::Portal;
use shared::movement::MoveState;
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

/// Put every zone's people in place.
pub fn spawn_npcs(mut commands: Commands, data: Res<GameData>, zones: Res<Zones>) {
    for (zone, level) in &zones.0 {
        for npc in &level.npcs {
            commands.spawn((
                Npc {
                    lines: npc.lines.clone(),
                    next: 0,
                },
                CharacterName(npc.name.clone()),
                Motion(MoveState {
                    yaw: npc.yaw,
                    ..MoveState::spawn_at(npc.position)
                }),
                Zone(zone.clone()),
                Faction::Neutral,
                HitRadius(data.player.hit_radius),
                VisualKey(npc.visual.clone()),
            ));
        }
    }
}

/// What using a portal did.
pub enum PortalUse {
    /// Moved to another zone (or onto a ride).
    Travelled,
    /// A closed gate: says why.
    Closed,
}

/// Use a portal the player is standing in.
pub fn use_portal(
    commands: &mut Commands,
    link: &mut Link,
    data: &GameData,
    now: f64,
    player: PlayerId,
    entity: Entity,
    motion: &mut Motion,
    portal: &Portal,
) -> PortalUse {
    if let Some(message) = &portal.closed {
        link.to_client.push(ServerEvent::Speech {
            player,
            speaker: portal.label.clone(),
            text: message.clone(),
        });
        return PortalUse::Closed;
    }
    if let Some(ride_id) = &portal.ride
        && let Some(ride) = data.rides.get(ride_id)
    {
        let (start, yaw) = ride.at(0.0);
        motion.0 = MoveState {
            yaw,
            ..MoveState::spawn_at(start)
        };
        commands.entity(entity).insert((
            Zone(ride.zone.clone()),
            Riding {
                ride: ride_id.clone(),
                started: now,
            },
        ));
        link.to_client.push(ServerEvent::ZoneChanged {
            entity,
            zone: ride.zone.clone(),
        });
        return PortalUse::Travelled;
    }
    motion.0 = MoveState {
        yaw: portal.arrive_yaw,
        ..MoveState::spawn_at(portal.arrive)
    };
    commands.entity(entity).insert(Zone(portal.to.clone()));
    link.to_client.push(ServerEvent::ZoneChanged {
        entity,
        zone: portal.to.clone(),
    });
    PortalUse::Travelled
}

/// Talk to the nearest person in reach, if any. Returns whether someone
/// answered.
pub fn talk(
    link: &mut Link,
    data: &GameData,
    player: PlayerId,
    zone: &Zone,
    position: Vec3,
    npcs: &mut Query<(&mut Npc, &CharacterName, &Zone, &Motion), Without<PlayerId>>,
) -> bool {
    let nearest = npcs
        .iter_mut()
        .filter(|(_, _, z, m)| {
            *z == zone && ground_distance(m.0.position, position) <= data.player.talk_distance
        })
        .min_by(|a, b| {
            ground_distance(a.3.0.position, position)
                .total_cmp(&ground_distance(b.3.0.position, position))
        });
    let Some((mut npc, name, ..)) = nearest else {
        return false;
    };
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
    mut link: ResMut<Link>,
    mut riders: Query<(Entity, &Riding, &mut Motion)>,
) {
    let now = time.elapsed_secs_f64();
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
        motion.0 = MoveState {
            yaw: ride.arrive_yaw,
            ..MoveState::spawn_at(ride.arrive)
        };
        commands
            .entity(entity)
            .remove::<Riding>()
            .insert(Zone(ride.to.clone()));
        link.to_client.push(ServerEvent::ZoneChanged {
            entity,
            zone: ride.to.clone(),
        });
    }
}

/// Riders can't do anything else until the ride ends.
pub fn busy_riding(link: &mut Link, player: PlayerId) {
    link.to_client.push(ServerEvent::Rejected {
        player,
        reason: Reject::Busy,
    });
}
