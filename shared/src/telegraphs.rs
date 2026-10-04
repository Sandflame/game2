//! Telegraphs: glowing ground markers that warn of a big attack. When the
//! cast finishes, everyone standing inside takes the hit. The shapes and
//! "is this point inside?" tests live here.

use bevy::math::{Vec2, Vec3};
use bevy::prelude::{Component, Entity};
use serde::{Deserialize, Serialize};

use crate::data::Problems;

/// The shape on the ground.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub enum MarkerShape {
    /// A filled circle.
    Circle { radius: f32 },
    /// A ring: safe inside `inner`, dangerous out to `outer`.
    Donut { inner: f32, outer: f32 },
    /// A wedge pointing the way the marker faces. `angle` in degrees.
    Cone { radius: f32, angle: f32 },
    /// A rectangle starting at the marker and running `length` metres
    /// the way it faces.
    Line { length: f32, width: f32 },
}

/// Where a telegraph is placed when the cast starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Placement {
    /// Under the caster.
    Caster,
    /// Where the target is standing when the cast starts (it stays there).
    Target,
    /// Under the caster, pointing at the target (cones and lines).
    CasterTowardsTarget,
    /// On the target, following them until it goes off. Everyone inside
    /// shares the damage: stand together!
    StackOnTarget,
    /// On every player, following them. Spread out so they don't overlap!
    SpreadOnEveryone,
}

impl Placement {
    /// Markers that move with a player until they go off.
    pub fn follows(self) -> bool {
        matches!(self, Placement::StackOnTarget | Placement::SpreadOnEveryone)
    }
}

/// A telegraph as written in an ability's data.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
pub struct TelegraphDef {
    pub shape: MarkerShape,
    pub placement: Placement,
}

impl TelegraphDef {
    pub fn problems(&self) -> Vec<String> {
        let mut p = Problems::default();
        match self.shape {
            MarkerShape::Circle { radius } => p.positive("telegraph radius", radius),
            MarkerShape::Donut { inner, outer } => {
                p.positive("telegraph inner", inner);
                if outer <= inner {
                    p.push("telegraph `outer` must be bigger than `inner`");
                }
            }
            MarkerShape::Cone { radius, angle } => {
                p.positive("telegraph radius", radius);
                if !(angle > 0.0 && angle <= 360.0) {
                    p.push("telegraph `angle` must be between 0 and 360 degrees");
                }
            }
            MarkerShape::Line { length, width } => {
                p.positive("telegraph length", length);
                p.positive("telegraph width", width);
            }
        }
        p.0
    }
}

/// Does a marker of this shape, centred at `origin` and facing `yaw`,
/// cover `point`? `grace` shrinks the danger area slightly so players
/// right on the edge are forgiven (useful once there is network lag).
pub fn covers(shape: MarkerShape, origin: Vec3, yaw: f32, point: Vec3, grace: f32) -> bool {
    let offset = Vec2::new(point.x - origin.x, point.z - origin.z);
    let distance = offset.length();
    // The way the marker faces, on the ground (yaw 0 faces -Z).
    let forward = Vec2::new(-yaw.sin(), -yaw.cos());
    match shape {
        MarkerShape::Circle { radius } => distance <= radius - grace,
        MarkerShape::Donut { inner, outer } => {
            distance >= inner + grace && distance <= outer - grace
        }
        MarkerShape::Cone { radius, angle } => {
            if distance > radius - grace {
                return false;
            }
            if distance < 1e-4 {
                return true;
            }
            let half = (angle.to_radians() / 2.0).min(std::f32::consts::PI);
            let cos_between = (offset / distance).dot(forward).clamp(-1.0, 1.0);
            cos_between.acos() <= half
        }
        MarkerShape::Line { length, width } => {
            let along = offset.dot(forward);
            let side = offset.dot(Vec2::new(-forward.y, forward.x)).abs();
            along >= 0.0 && along <= length - grace && side <= width / 2.0 - grace
        }
    }
}

/// A telegraph currently on the ground (a logic entity; the client draws it).
#[derive(Component, Debug, Clone)]
pub struct Telegraph {
    pub shape: MarkerShape,
    pub placement: Placement,
    pub origin: Vec3,
    pub yaw: f32,
    /// The player it follows (stack and spread markers).
    pub follow: Option<Entity>,
    /// Who is casting it, and which ability.
    pub caster: Entity,
    pub ability: String,
    pub starts: f64,
    pub resolves: f64,
}

impl Telegraph {
    /// Progress from 0 (just appeared) to 1 (about to go off).
    pub fn progress(&self, now: f64) -> f32 {
        let length = (self.resolves - self.starts).max(1e-6);
        (((now - self.starts) / length) as f32).clamp(0.0, 1.0)
    }

    pub fn covers(&self, point: Vec3, grace: f32) -> bool {
        covers(self.shape, self.origin, self.yaw, point, grace)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AHEAD: f32 = 0.0; // yaw 0 faces -Z

    fn at(x: f32, z: f32) -> Vec3 {
        Vec3::new(x, 0.0, z)
    }

    #[test]
    fn circles() {
        let shape = MarkerShape::Circle { radius: 5.0 };
        assert!(covers(shape, Vec3::ZERO, AHEAD, at(3.0, 3.0), 0.0));
        assert!(!covers(shape, Vec3::ZERO, AHEAD, at(4.0, 4.0), 0.0));
    }

    #[test]
    fn grace_forgives_the_edge() {
        let shape = MarkerShape::Circle { radius: 5.0 };
        assert!(covers(shape, Vec3::ZERO, AHEAD, at(4.9, 0.0), 0.0));
        assert!(!covers(shape, Vec3::ZERO, AHEAD, at(4.9, 0.0), 0.2));
    }

    #[test]
    fn donuts_are_safe_in_the_middle() {
        let shape = MarkerShape::Donut {
            inner: 4.0,
            outer: 15.0,
        };
        assert!(!covers(shape, Vec3::ZERO, AHEAD, at(2.0, 0.0), 0.0));
        assert!(covers(shape, Vec3::ZERO, AHEAD, at(8.0, 0.0), 0.0));
        assert!(!covers(shape, Vec3::ZERO, AHEAD, at(20.0, 0.0), 0.0));
    }

    #[test]
    fn cones_cover_the_front_only() {
        let shape = MarkerShape::Cone {
            radius: 10.0,
            angle: 90.0,
        };
        assert!(
            covers(shape, Vec3::ZERO, AHEAD, at(0.0, -5.0), 0.0),
            "straight ahead"
        );
        assert!(
            covers(shape, Vec3::ZERO, AHEAD, at(3.0, -5.0), 0.0),
            "inside the wedge"
        );
        assert!(
            !covers(shape, Vec3::ZERO, AHEAD, at(6.0, -2.0), 0.0),
            "outside the wedge"
        );
        assert!(
            !covers(shape, Vec3::ZERO, AHEAD, at(0.0, 5.0), 0.0),
            "behind"
        );
    }

    #[test]
    fn cones_turn_with_their_facing() {
        let shape = MarkerShape::Cone {
            radius: 10.0,
            angle: 60.0,
        };
        // Facing +X (yaw -90°).
        let yaw = -std::f32::consts::FRAC_PI_2;
        assert!(covers(shape, Vec3::ZERO, yaw, at(5.0, 0.0), 0.0));
        assert!(!covers(shape, Vec3::ZERO, yaw, at(0.0, -5.0), 0.0));
    }

    #[test]
    fn lines_run_forward_from_the_origin() {
        let shape = MarkerShape::Line {
            length: 20.0,
            width: 4.0,
        };
        assert!(covers(shape, Vec3::ZERO, AHEAD, at(1.5, -15.0), 0.0));
        assert!(
            !covers(shape, Vec3::ZERO, AHEAD, at(2.5, -15.0), 0.0),
            "too far to the side"
        );
        assert!(
            !covers(shape, Vec3::ZERO, AHEAD, at(0.0, 5.0), 0.0),
            "behind"
        );
        assert!(
            !covers(shape, Vec3::ZERO, AHEAD, at(0.0, -25.0), 0.0),
            "past the end"
        );
    }

    #[test]
    fn markers_report_progress() {
        let t = Telegraph {
            shape: MarkerShape::Circle { radius: 1.0 },
            placement: Placement::Target,
            origin: Vec3::ZERO,
            yaw: 0.0,
            follow: None,
            caster: Entity::PLACEHOLDER,
            ability: String::new(),
            starts: 10.0,
            resolves: 14.0,
        };
        assert_eq!(t.progress(10.0), 0.0);
        assert_eq!(t.progress(12.0), 0.5);
        assert_eq!(t.progress(20.0), 1.0);
    }

    #[test]
    fn bad_shapes_are_reported() {
        let ring = TelegraphDef {
            shape: MarkerShape::Donut {
                inner: 5.0,
                outer: 3.0,
            },
            placement: Placement::Caster,
        };
        assert_eq!(ring.problems().len(), 1);
    }
}
