//! Scripted rides, such as sliding down the giant root to the forest:
//! the character follows a fixed smooth path through a ride zone for a
//! few seconds, then arrives in the destination zone. No physics — just a
//! curve and a clock (`assets/data/rides/*.ron`).

use bevy::math::{Vec2, Vec3};
use serde::Deserialize;

use crate::data::{Problems, Validate};
use crate::movement::yaw_from_direction;

#[derive(Debug, Clone, Deserialize)]
pub struct RideDef {
    /// The zone the ride takes place in (its scenery is the tunnel).
    pub zone: String,
    /// Points the path passes through, in order.
    pub path: Vec<Vec3>,
    /// Seconds from start to finish.
    pub duration: f32,
    /// Where you come out: zone, position and facing (radians).
    pub to: String,
    pub arrive: Vec3,
    #[serde(default)]
    pub arrive_yaw: f32,
}

impl Validate for RideDef {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.path.len() < 2 {
            p.push("`path` needs at least 2 points");
        }
        p.positive("duration", self.duration);
        p.0
    }
}

impl RideDef {
    /// Where the rider is `t` of the way through (0–1), and which way
    /// they face.
    pub fn at(&self, t: f32) -> (Vec3, f32) {
        let position = point_on_path(&self.path, t);
        let ahead = point_on_path(&self.path, (t + 0.01).min(1.0));
        let behind = point_on_path(&self.path, (t - 0.01).max(0.0));
        let direction = Vec2::new(ahead.x - behind.x, ahead.z - behind.z);
        let yaw = if direction.length_squared() > 1e-8 {
            yaw_from_direction(direction)
        } else {
            0.0
        };
        (position, yaw)
    }
}

/// A smooth curve (Catmull-Rom) through every point, `t` from 0 to 1.
pub fn point_on_path(points: &[Vec3], t: f32) -> Vec3 {
    match points.len() {
        0 => Vec3::ZERO,
        1 => points[0],
        n => {
            let segments = (n - 1) as f32;
            let scaled = t.clamp(0.0, 1.0) * segments;
            let i = (scaled.floor() as usize).min(n - 2);
            let local = scaled - i as f32;
            let p0 = points[i.saturating_sub(1)];
            let p1 = points[i];
            let p2 = points[i + 1];
            let p3 = points[(i + 2).min(n - 1)];
            catmull_rom(p0, p1, p2, p3, local)
        }
    }
}

fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_passes_through_every_point() {
        let points = vec![
            Vec3::ZERO,
            Vec3::new(10.0, -5.0, 0.0),
            Vec3::new(20.0, -10.0, 10.0),
        ];
        assert!((point_on_path(&points, 0.0) - points[0]).length() < 1e-4);
        assert!((point_on_path(&points, 0.5) - points[1]).length() < 1e-4);
        assert!((point_on_path(&points, 1.0) - points[2]).length() < 1e-4);
        assert!(
            (point_on_path(&points, 2.0) - points[2]).length() < 1e-4,
            "clamped"
        );
    }

    #[test]
    fn riders_face_along_the_path() {
        let ride = RideDef {
            zone: "tunnel".into(),
            path: vec![Vec3::ZERO, Vec3::new(0.0, 0.0, -10.0)],
            duration: 5.0,
            to: "forest".into(),
            arrive: Vec3::ZERO,
            arrive_yaw: 0.0,
        };
        let (_, yaw) = ride.at(0.5);
        // Heading towards -Z is yaw 0.
        assert!(yaw.abs() < 1e-3, "{yaw}");
    }
}
