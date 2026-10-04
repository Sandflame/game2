//! How regular enemies behave, as plain functions: they notice players who
//! come close, call nearby friends, chase until they are close enough to
//! attack, and give up and walk home ("leash") if pulled too far from home.

use bevy::math::{Vec2, Vec3};

/// Ground distance between two points (height ignored).
pub fn ground_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

/// Does an idle enemy notice a player at this distance?
pub fn notices(enemy: Vec3, player: Vec3, aggro_radius: f32) -> bool {
    aggro_radius > 0.0 && ground_distance(enemy, player) <= aggro_radius
}

/// Has the enemy been pulled too far from home?
pub fn too_far_from_home(home: Vec3, position: Vec3, leash_radius: f32) -> bool {
    leash_radius > 0.0 && ground_distance(home, position) > leash_radius
}

/// Which way (and how hard, 0–1) to push the movement stick to approach
/// `target` until within `stop_at` metres. `speed_fraction` is the
/// enemy's speed compared with a player's walking speed.
/// `None` when already close enough.
pub fn approach(from: Vec3, target: Vec3, stop_at: f32, speed_fraction: f32) -> Option<Vec2> {
    let offset = Vec2::new(target.x - from.x, target.z - from.z);
    let distance = offset.length();
    if distance <= stop_at || distance < 1e-4 {
        return None;
    }
    Some(offset / distance * speed_fraction.clamp(0.0, 1.0))
}

/// How close an enemy wants to get to its target: its shortest attack
/// range, measured (like ability ranges) to the edge of the target, minus
/// a little so it is safely inside.
pub fn stop_distance(attack_range: f32, target_radius: f32) -> f32 {
    const INSIDE: f32 = 0.5;
    (attack_range + target_radius - INSIDE).max(0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noticing_needs_the_player_close() {
        assert!(notices(Vec3::ZERO, Vec3::new(5.0, 3.0, 0.0), 6.0));
        assert!(!notices(Vec3::ZERO, Vec3::new(7.0, 0.0, 0.0), 6.0));
        assert!(
            !notices(Vec3::ZERO, Vec3::ZERO, 0.0),
            "radius 0 never notices"
        );
    }

    #[test]
    fn leashing_after_the_radius() {
        assert!(!too_far_from_home(
            Vec3::ZERO,
            Vec3::new(10.0, 0.0, 0.0),
            20.0
        ));
        assert!(too_far_from_home(
            Vec3::ZERO,
            Vec3::new(25.0, 0.0, 0.0),
            20.0
        ));
        assert!(!too_far_from_home(
            Vec3::ZERO,
            Vec3::new(500.0, 0.0, 0.0),
            0.0
        ));
    }

    #[test]
    fn approaching_stops_in_range() {
        let push = approach(Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), 3.0, 0.8).unwrap();
        assert!((push - Vec2::new(0.8, 0.0)).length() < 1e-5);
        assert_eq!(
            approach(Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0), 3.0, 0.8),
            None
        );
        assert!((stop_distance(3.5, 0.5) - 3.5).abs() < 1e-5);
    }
}
