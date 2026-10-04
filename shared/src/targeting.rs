//! Tab targeting: picking which enemy to target next.

use bevy::math::{Vec2, Vec3};
use bevy::prelude::Entity;

/// A possible target and where it is.
#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    pub entity: Entity,
    pub position: Vec3,
}

/// Choose the next target when Tab is pressed.
///
/// Enemies within `max_distance` that are in front of the camera are
/// considered, nearest first. If the current target is among them, the
/// next one in that order is chosen (wrapping around); otherwise the
/// nearest. If nothing is in front, enemies behind are considered too.
pub fn next_tab_target(
    candidates: &[Candidate],
    player: Vec3,
    camera_forward: Vec2,
    current: Option<Entity>,
    max_distance: f32,
) -> Option<Entity> {
    let forward = camera_forward.normalize_or_zero();
    let mut in_range: Vec<(f32, bool, Entity)> = candidates
        .iter()
        .filter_map(|c| {
            let offset = Vec2::new(c.position.x - player.x, c.position.z - player.z);
            let distance = offset.length();
            (distance <= max_distance).then(|| {
                let in_front = distance < 1e-3 || offset.dot(forward) / distance > -0.2;
                (distance, in_front, c.entity)
            })
        })
        .collect();
    in_range.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));

    let front: Vec<Entity> = in_range.iter().filter(|c| c.1).map(|c| c.2).collect();
    let order = if front.is_empty() {
        in_range.iter().map(|c| c.2).collect()
    } else {
        front
    };
    if order.is_empty() {
        return None;
    }
    match current.and_then(|cur| order.iter().position(|e| *e == cur)) {
        Some(i) => Some(order[(i + 1) % order.len()]),
        None => Some(order[0]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(n: u32) -> Entity {
        Entity::from_raw_u32(n).unwrap()
    }

    fn candidates() -> Vec<Candidate> {
        vec![
            Candidate {
                entity: entity(1),
                position: Vec3::new(0.0, 0.0, -10.0),
            },
            Candidate {
                entity: entity(2),
                position: Vec3::new(0.0, 0.0, -5.0),
            },
            Candidate {
                entity: entity(3),
                position: Vec3::new(0.0, 0.0, 8.0),
            }, // behind
            Candidate {
                entity: entity(4),
                position: Vec3::new(0.0, 0.0, -100.0),
            }, // too far
        ]
    }

    const FORWARD: Vec2 = Vec2::new(0.0, -1.0);

    #[test]
    fn picks_nearest_in_front_first() {
        let pick = next_tab_target(&candidates(), Vec3::ZERO, FORWARD, None, 40.0);
        assert_eq!(pick, Some(entity(2)));
    }

    #[test]
    fn cycles_and_wraps() {
        let c = candidates();
        let second = next_tab_target(&c, Vec3::ZERO, FORWARD, Some(entity(2)), 40.0);
        assert_eq!(second, Some(entity(1)));
        let wrapped = next_tab_target(&c, Vec3::ZERO, FORWARD, Some(entity(1)), 40.0);
        assert_eq!(wrapped, Some(entity(2)));
    }

    #[test]
    fn falls_back_to_targets_behind() {
        let c = candidates();
        let pick = next_tab_target(&c, Vec3::ZERO, Vec2::new(0.0, 1.0), None, 9.0);
        assert_eq!(pick, Some(entity(3)));
        let behind_only = [c[2]];
        assert_eq!(
            next_tab_target(&behind_only, Vec3::ZERO, FORWARD, None, 40.0),
            Some(entity(3))
        );
    }

    #[test]
    fn nothing_in_range() {
        let c = candidates();
        assert_eq!(next_tab_target(&c, Vec3::ZERO, FORWARD, None, 1.0), None);
        assert_eq!(next_tab_target(&[], Vec3::ZERO, FORWARD, None, 40.0), None);
    }
}
