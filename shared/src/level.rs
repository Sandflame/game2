//! Static level geometry: the ground, walls, pillars and other solid
//! objects that characters bump into. Shared by server and client so both
//! simulate movement identically.
//!
//! Kept deliberately simple: flat ground at height 0, plus upright boxes
//! (axis-aligned) and cylinders. See DESIGN.md §2.1 for why we don't use a
//! physics engine yet.

use std::path::Path;

use bevy::math::{Vec2, Vec3};
use bevy::prelude::Resource;
use serde::Deserialize;

use crate::data::{DataError, Problems, Validate, load_ron};

/// The footprint and height of a solid object.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub enum Shape {
    /// An upright box aligned with the world X and Z axes.
    Box {
        half_x: f32,
        half_z: f32,
        height: f32,
    },
    /// An upright cylinder.
    Cylinder { radius: f32, height: f32 },
}

/// One solid object in a level.
#[derive(Debug, Clone, Deserialize)]
pub struct Obstacle {
    pub shape: Shape,
    /// Centre of the object's *bottom* face.
    pub position: Vec3,
    /// Name of the look to use (client only, e.g. "crate", "tree").
    #[serde(default)]
    pub visual: String,
}

impl Obstacle {
    pub fn bottom(&self) -> f32 {
        self.position.y
    }

    pub fn top(&self) -> f32 {
        self.position.y
            + match self.shape {
                Shape::Box { height, .. } | Shape::Cylinder { height, .. } => height,
            }
    }

    fn centre_xz(&self) -> Vec2 {
        Vec2::new(self.position.x, self.position.z)
    }

    /// Closest point of this object's footprint to `point` (all on the ground plane).
    fn closest_footprint_point(&self, point: Vec2) -> Vec2 {
        let centre = self.centre_xz();
        match self.shape {
            Shape::Box { half_x, half_z, .. } => Vec2::new(
                point.x.clamp(centre.x - half_x, centre.x + half_x),
                point.y.clamp(centre.y - half_z, centre.y + half_z),
            ),
            Shape::Cylinder { radius, .. } => {
                let offset = point - centre;
                if offset.length() <= radius {
                    point
                } else {
                    centre + offset.normalize() * radius
                }
            }
        }
    }

    /// Does a circle (a character seen from above) overlap this object's footprint?
    pub fn footprint_overlaps_circle(&self, point: Vec2, radius: f32) -> bool {
        self.closest_footprint_point(point).distance(point) < radius
    }

    /// If a circle overlaps the footprint, return the shortest move that
    /// pushes it back out. Returns `None` when there is no overlap.
    pub fn push_out_circle(&self, point: Vec2, radius: f32) -> Option<Vec2> {
        let closest = self.closest_footprint_point(point);
        let offset = point - closest;
        let distance = offset.length();
        if distance >= radius {
            return None;
        }
        if distance > 1e-5 {
            return Some(offset / distance * (radius - distance));
        }
        // The circle's centre is inside the footprint: leave by the nearest side.
        let centre = self.centre_xz();
        match self.shape {
            Shape::Cylinder { radius: r, .. } => {
                let from_centre = point - centre;
                let dir = from_centre.try_normalize().unwrap_or(Vec2::X);
                Some(dir * (r + radius - from_centre.length()))
            }
            Shape::Box { half_x, half_z, .. } => {
                let local = point - centre;
                let exit_x = half_x - local.x.abs() + radius;
                let exit_z = half_z - local.y.abs() + radius;
                if exit_x < exit_z {
                    Some(Vec2::new(exit_x.copysign(local.x), 0.0))
                } else {
                    Some(Vec2::new(0.0, exit_z.copysign(local.y)))
                }
            }
        }
    }
}

/// An enemy placed in a level.
#[derive(Debug, Clone, Deserialize)]
pub struct EnemySpawn {
    /// Enemy id: the file name in `assets/data/enemies/` without `.ron`.
    pub enemy: String,
    pub position: Vec3,
    /// Facing in radians (0 faces -Z).
    #[serde(default)]
    pub yaw: f32,
}

/// A doorway to another zone: stand in it and press the interact key.
#[derive(Debug, Clone, Deserialize)]
pub struct Portal {
    /// Centre of the portal on the ground.
    pub position: Vec3,
    /// How close (metres) you must be to use it.
    pub radius: f32,
    /// The zone it leads to (file name in `assets/data/zones/`).
    #[serde(default)]
    pub to: String,
    /// Where you appear in that zone, and which way you face (radians).
    #[serde(default)]
    pub arrive: Vec3,
    #[serde(default)]
    pub arrive_yaw: f32,
    /// Shown to the player, e.g. "Enter the Rootwarden's Hollow".
    pub label: String,
    /// Instead of jumping straight there, take this ride
    /// (`assets/data/rides/`), e.g. the slide down the giant root.
    #[serde(default)]
    pub ride: Option<String>,
    /// A gate that isn't open yet: using it shows this message instead.
    #[serde(default)]
    pub closed: Option<String>,
    /// Look of the doorway (client only): "" for the glowing ring.
    #[serde(default)]
    pub visual: String,
    /// Leads back to wherever you came in from (the way out of dungeons
    /// and trials, which can be entered from several places).
    #[serde(default)]
    pub back: bool,
    /// A board listing dungeons and trials: using it opens the list, and
    /// choosing one takes you there.
    #[serde(default)]
    pub board: bool,
}

/// Where you end up when leaving an instanced zone (dungeon, trial) and
/// the game doesn't know where you came from, e.g. after logging back in.
#[derive(Debug, Clone, Deserialize)]
pub struct Exit {
    pub to: String,
    pub arrive: Vec3,
    #[serde(default)]
    pub arrive_yaw: f32,
}

/// How a dungeon or trial appears on the board.
#[derive(Debug, Clone, Deserialize)]
pub struct Listing {
    /// "Dungeon" or "Trial".
    pub kind: String,
    pub description: String,
    /// Shown as "for 1–4 players".
    pub players: (u32, u32),
}

/// Zone ids of instanced zones look like `tangled_burrow#3`: the zone's
/// file name, `#`, then which copy. This returns the file name part.
pub fn base_zone(zone: &str) -> &str {
    zone.split_once(INSTANCE_MARK)
        .map_or(zone, |(base, _)| base)
}

/// Separates a zone's file name from the copy number.
pub const INSTANCE_MARK: char = '#';

/// Someone to talk to (press E nearby).
#[derive(Debug, Clone, Deserialize)]
pub struct NpcDef {
    /// Unique in the whole game; quests name people by it.
    pub id: String,
    pub name: String,
    /// Look (client only).
    pub visual: String,
    pub position: Vec3,
    #[serde(default)]
    pub yaw: f32,
    /// What they say; each time you talk they say the next line.
    pub lines: Vec<String>,
}

/// Scenery that is only for looks (no collision), e.g. bushes, lamps.
#[derive(Debug, Clone, Deserialize)]
pub struct Decoration {
    pub visual: String,
    pub position: Vec3,
    #[serde(default)]
    pub yaw: f32,
    #[serde(default = "one")]
    pub scale: f32,
}

fn one() -> f32 {
    1.0
}

/// One zone (`assets/data/zones/<id>.ron`): its solid geometry, enemies,
/// portals and rules.
#[derive(Debug, Clone, Deserialize, Resource)]
pub struct Level {
    pub name: String,
    /// Characters are kept inside a square of this half-size around the origin.
    pub half_size: f32,
    /// Where characters appear when they first enter, and which way they face.
    pub spawn_point: Vec3,
    #[serde(default)]
    pub spawn_yaw: f32,
    /// Look of the ground (client only), e.g. "grass" or "stone".
    #[serde(default = "default_ground")]
    pub ground: String,
    /// Look of the zone's edge (client only), e.g. "roots"; empty for none.
    #[serde(default)]
    pub border: String,
    /// A look (`vfx.ron`) that plays all the time over the zone, such as
    /// drifting pollen (client only); empty for none.
    #[serde(default)]
    pub ambience: String,
    #[serde(default)]
    pub obstacles: Vec<Obstacle>,
    #[serde(default)]
    pub spawns: Vec<EnemySpawn>,
    #[serde(default)]
    pub portals: Vec<Portal>,
    /// Defeated players get back up on their own here (open world).
    /// In trials and dungeons this is false: you need a raise, or the
    /// fight resets when everyone falls.
    #[serde(default = "yes")]
    pub revive_in_place: bool,
    /// People to talk to.
    #[serde(default)]
    pub npcs: Vec<NpcDef>,
    /// Scenery without collision (client only).
    #[serde(default)]
    pub decorations: Vec<Decoration>,
    /// Level sync: characters above this level fight at it here (so friends
    /// of any level can play together).
    #[serde(default)]
    pub level_sync: Option<u32>,
    /// Boss fights that take place here (`assets/data/encounters/`).
    #[serde(default)]
    pub encounters: Vec<String>,
    /// Each group gets its own copy (dungeons, trials). A copy is made when
    /// the first player enters and removed when the last one leaves, so it
    /// is always fresh. Enemies in it don't come back once defeated.
    #[serde(default)]
    pub instanced: bool,
    /// Where you leave to if the game doesn't know where you came from
    /// (needed for instanced zones).
    #[serde(default)]
    pub exit: Option<Exit>,
    /// Shown on the dungeon board.
    #[serde(default)]
    pub listing: Option<Listing>,
}

fn default_ground() -> String {
    "grass".to_owned()
}

fn yes() -> bool {
    true
}

impl Level {
    /// A flat, empty zone (handy for tests).
    pub fn empty() -> Self {
        Self {
            name: String::new(),
            half_size: 100.0,
            spawn_point: Vec3::ZERO,
            spawn_yaw: 0.0,
            ground: default_ground(),
            border: String::new(),
            ambience: String::new(),
            obstacles: Vec::new(),
            spawns: Vec::new(),
            portals: Vec::new(),
            revive_in_place: true,
            npcs: Vec::new(),
            decorations: Vec::new(),
            level_sync: None,
            encounters: Vec::new(),
            instanced: false,
            exit: None,
            listing: None,
        }
    }

    pub fn load(assets_dir: &Path, zone: &str) -> Result<Self, DataError> {
        load_ron(&Self::path(assets_dir, zone))
    }

    /// Where a zone's data file lives.
    pub fn path(assets_dir: &Path, zone: &str) -> std::path::PathBuf {
        assets_dir
            .join("data")
            .join("zones")
            .join(format!("{zone}.ron"))
    }

    /// The portal (if any) a character standing at `position` can use.
    pub fn portal_at(&self, position: Vec3) -> Option<&Portal> {
        self.portals.iter().find(|p| {
            Vec2::new(p.position.x - position.x, p.position.z - position.z).length() <= p.radius
        })
    }

    /// Height of the highest walkable surface under a character whose feet
    /// are at `feet_y`. Surfaces up to `step_height` above the feet count,
    /// so characters walk up small steps.
    pub fn ground_height(&self, point: Vec2, radius: f32, feet_y: f32, step_height: f32) -> f32 {
        self.obstacles
            .iter()
            .filter(|o| {
                o.top() <= feet_y + step_height && o.footprint_overlaps_circle(point, radius)
            })
            .map(Obstacle::top)
            .fold(0.0, f32::max)
    }

    /// Move a character's horizontal position so it no longer overlaps any
    /// object that is too tall to step onto, and keep it inside the level.
    pub fn resolve_horizontal(
        &self,
        mut point: Vec2,
        radius: f32,
        feet_y: f32,
        body_height: f32,
        step_height: f32,
    ) -> Vec2 {
        // A few passes handle corners where two objects push at once.
        for _ in 0..4 {
            let mut moved = false;
            for obstacle in &self.obstacles {
                let blocks = obstacle.top() > feet_y + step_height
                    && obstacle.bottom() < feet_y + body_height;
                if !blocks {
                    continue;
                }
                if let Some(push) = obstacle.push_out_circle(point, radius) {
                    point += push;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
        let limit = (self.half_size - radius).max(0.0);
        point.clamp(Vec2::splat(-limit), Vec2::splat(limit))
    }
}

impl Validate for Level {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        p.positive("half_size", self.half_size);
        let outside = |v: Vec3| v.x.abs() > self.half_size || v.z.abs() > self.half_size;
        if outside(self.spawn_point) {
            p.push("`spawn_point` is outside the level");
        }
        for (i, spawn) in self.spawns.iter().enumerate() {
            if outside(spawn.position) {
                p.push(format!("spawns[{i}] is outside the level"));
            }
        }
        for (i, portal) in self.portals.iter().enumerate() {
            if outside(portal.position) {
                p.push(format!("portals[{i}] is outside the level"));
            }
            p.positive(&format!("portals[{i}].radius"), portal.radius);
            let ways = [
                !portal.to.is_empty(),
                portal.ride.is_some(),
                portal.closed.is_some(),
                portal.back,
                portal.board,
            ];
            if ways.iter().filter(|w| **w).count() != 1 {
                p.push(format!(
                    "portals[{i}] needs exactly one of: a `to` zone, a `ride`, a `closed` message, `back: true` or `board: true`"
                ));
            }
        }
        if self.instanced && self.exit.is_none() {
            p.push("an `instanced` zone needs an `exit`");
        }
        if let Some(listing) = &self.listing {
            let (min, max) = listing.players;
            if min == 0 || max < min {
                p.push("`listing.players` must be (smallest, largest) with 1 or more");
            }
        }
        for (i, npc) in self.npcs.iter().enumerate() {
            if outside(npc.position) {
                p.push(format!("npcs[{i}] is outside the level"));
            }
            if npc.lines.is_empty() {
                p.push(format!("npcs[{i}] (`{}`) has nothing to say", npc.name));
            }
        }
        for (i, o) in self.obstacles.iter().enumerate() {
            match o.shape {
                Shape::Box {
                    half_x,
                    half_z,
                    height,
                } => {
                    p.positive(&format!("obstacles[{i}].half_x"), half_x);
                    p.positive(&format!("obstacles[{i}].half_z"), half_z);
                    p.positive(&format!("obstacles[{i}].height"), height);
                }
                Shape::Cylinder { radius, height } => {
                    p.positive(&format!("obstacles[{i}].radius"), radius);
                    p.positive(&format!("obstacles[{i}].height"), height);
                }
            }
        }
        p.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall() -> Obstacle {
        Obstacle {
            shape: Shape::Box {
                half_x: 1.0,
                half_z: 0.5,
                height: 3.0,
            },
            position: Vec3::ZERO,
            visual: String::new(),
        }
    }

    fn pillar() -> Obstacle {
        Obstacle {
            shape: Shape::Cylinder {
                radius: 1.0,
                height: 3.0,
            },
            position: Vec3::ZERO,
            visual: String::new(),
        }
    }

    #[test]
    fn no_push_when_apart() {
        assert_eq!(wall().push_out_circle(Vec2::new(0.0, 2.0), 0.4), None);
        assert_eq!(pillar().push_out_circle(Vec2::new(2.0, 0.0), 0.4), None);
    }

    #[test]
    fn box_pushes_out_of_nearest_face() {
        let push = wall().push_out_circle(Vec2::new(0.0, 0.7), 0.4).unwrap();
        assert!((push - Vec2::new(0.0, 0.2)).length() < 1e-5, "{push}");
    }

    #[test]
    fn box_pushes_out_when_centre_inside() {
        let push = wall().push_out_circle(Vec2::new(0.9, 0.0), 0.4).unwrap();
        assert!((push - Vec2::new(0.5, 0.0)).length() < 1e-5, "{push}");
    }

    #[test]
    fn cylinder_pushes_radially() {
        let push = pillar().push_out_circle(Vec2::new(0.0, -1.2), 0.4).unwrap();
        assert!((push - Vec2::new(0.0, -0.2)).length() < 1e-5, "{push}");
    }

    #[test]
    fn ground_includes_low_objects_only() {
        let level = Level {
            name: "test".into(),
            spawns: vec![],
            half_size: 50.0,
            spawn_point: Vec3::ZERO,
            obstacles: vec![
                Obstacle {
                    shape: Shape::Box {
                        half_x: 1.0,
                        half_z: 1.0,
                        height: 0.3,
                    },
                    position: Vec3::ZERO,
                    visual: String::new(),
                },
                Obstacle {
                    shape: Shape::Box {
                        half_x: 1.0,
                        half_z: 1.0,
                        height: 2.0,
                    },
                    position: Vec3::new(5.0, 0.0, 0.0),
                    visual: String::new(),
                },
            ],
            ..Level::empty()
        };
        assert_eq!(level.ground_height(Vec2::ZERO, 0.4, 0.0, 0.4), 0.3);
        // Too tall to step onto from the floor…
        assert_eq!(level.ground_height(Vec2::new(5.0, 0.0), 0.4, 0.0, 0.4), 0.0);
        // …but it is ground once you are on top of it.
        assert_eq!(level.ground_height(Vec2::new(5.0, 0.0), 0.4, 2.0, 0.4), 2.0);
    }

    #[test]
    fn instance_ids_name_their_zone() {
        assert_eq!(base_zone("burrow#12"), "burrow");
        assert_eq!(base_zone("hub"), "hub");
    }

    #[test]
    fn stays_inside_level_bounds() {
        let level = Level {
            name: "test".into(),
            spawns: vec![],
            half_size: 10.0,
            spawn_point: Vec3::ZERO,
            obstacles: vec![],
            ..Level::empty()
        };
        let p = level.resolve_horizontal(Vec2::new(20.0, -20.0), 0.5, 0.0, 1.8, 0.4);
        assert_eq!(p, Vec2::new(9.5, -9.5));
    }

    #[test]
    fn validation_catches_bad_sizes() {
        let level = Level {
            name: "bad".into(),
            spawns: vec![],
            half_size: 10.0,
            spawn_point: Vec3::new(20.0, 0.0, 0.0),
            obstacles: vec![Obstacle {
                shape: Shape::Cylinder {
                    radius: -1.0,
                    height: 2.0,
                },
                position: Vec3::ZERO,
                visual: String::new(),
            }],
            ..Level::empty()
        };
        let problems = level.validate();
        assert_eq!(problems.len(), 2, "{problems:?}");
    }
}

#[cfg(test)]
mod shipped_data_tests {
    use super::*;
    use crate::data::find_assets_dir;

    #[test]
    fn sandbox_zone_is_valid() {
        let level = Level::load(&find_assets_dir().unwrap(), "sandbox").unwrap();
        assert!(!level.obstacles.is_empty());
    }
}
