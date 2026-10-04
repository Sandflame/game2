//! Zones and their contents. Ordinary zones are filled once at startup.
//! Instanced zones (dungeons, trials) get a fresh copy, `zone#n`, when a
//! group enters, and the copy is removed when the last player leaves.

use std::collections::HashSet;

use bevy::prelude::*;
use shared::components::{PlayerId, Zone};
use shared::gamedata::{GameData, Zones};
use shared::level::{INSTANCE_MARK, Level, base_zone};

use crate::encounters::spawn_encounter;
use crate::enemies::{ResetWhenIdle, spawn_enemy};
use crate::travel::spawn_npc;

/// The open copies of instanced zones.
#[derive(Resource, Default, Debug)]
pub struct Instances {
    next: u32,
    pub open: HashSet<String>,
}

impl Instances {
    /// The zone a character entering `zone` should go to: the zone itself,
    /// or for an instanced zone, the copy their group is already in (anyone
    /// in `occupied` until parties arrive in Milestone 11), else a new one.
    pub fn enter(
        &mut self,
        commands: &mut Commands,
        data: &GameData,
        zones: &Zones,
        zone: &str,
        occupied: &[String],
    ) -> String {
        let base = base_zone(zone);
        let Some(level) = zones.get(base) else {
            return base.to_owned();
        };
        if !level.instanced {
            return base.to_owned();
        }
        if let Some(existing) = occupied
            .iter()
            .find(|z| base_zone(z) == base && self.open.contains(*z))
        {
            return existing.clone();
        }
        self.next += 1;
        let copy = format!("{base}{INSTANCE_MARK}{}", self.next);
        fill_zone(commands, data, level, &copy);
        self.open.insert(copy.clone());
        copy
    }
}

/// Put a zone's enemies, boss fights and people in place.
pub fn fill_zone(commands: &mut Commands, data: &GameData, level: &Level, zone: &str) {
    for spawn in &level.spawns {
        let enemy = spawn_enemy(
            commands,
            data,
            &spawn.enemy,
            zone,
            spawn.position,
            spawn.yaw,
        );
        // In dungeons and trials, defeated enemies stay down.
        if level.instanced
            && let Some(enemy) = enemy
        {
            commands.entity(enemy).remove::<ResetWhenIdle>();
        }
    }
    for encounter in &level.encounters {
        spawn_encounter(commands, data, encounter, zone);
    }
    for npc in &level.npcs {
        spawn_npc(commands, data, npc, zone);
    }
}

/// Fill every ordinary zone at startup.
pub fn fill_zones(mut commands: Commands, data: Res<GameData>, zones: Res<Zones>) {
    for (zone, level) in &zones.0 {
        if !level.instanced {
            fill_zone(&mut commands, &data, level, zone);
        }
    }
}

/// Remove copies of instanced zones that nobody is in any more.
pub fn close_empty_instances(
    mut commands: Commands,
    mut instances: ResMut<Instances>,
    players: Query<&Zone, With<PlayerId>>,
    things: Query<(Entity, &Zone), Without<PlayerId>>,
    encounters: Query<(Entity, &crate::Encounter)>,
) {
    let occupied: HashSet<&str> = players.iter().map(|z| z.0.as_str()).collect();
    let empty: Vec<String> = instances
        .open
        .iter()
        .filter(|zone| !occupied.contains(zone.as_str()))
        .cloned()
        .collect();
    for zone in empty {
        for (entity, thing_zone) in &things {
            if thing_zone.0 == zone {
                commands.entity(entity).despawn();
            }
        }
        for (entity, encounter) in &encounters {
            if encounter.zone == zone {
                commands.entity(entity).despawn();
            }
        }
        instances.open.remove(&zone);
    }
}
