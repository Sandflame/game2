//! Choosing a target: Tab cycles enemies, clicking picks a character,
//! Esc clears. A glowing ring marks the current target.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use shared::combat::Health;
use shared::components::{Faction, HitRadius, Motion};
use shared::gamedata::GameData;
use shared::targeting::{Candidate, next_tab_target};

use crate::camera::FollowCamera;
use crate::characters::{DisplayMotion, LocalPlayer};
use crate::toon::{Outline, ToonAssets};
use crate::world::ElsewhereZone;

/// How tall characters are for click-picking, in metres.
const PICK_HEIGHT: f32 = 2.2;
/// A left click that moves the mouse more than this (pixels) is a camera drag.
const CLICK_SLOP: f32 = 6.0;

pub struct TargetingPlugin;

impl Plugin for TargetingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentTarget>()
            .init_resource::<ClickTracker>()
            .add_systems(Startup, spawn_target_ring)
            .add_systems(
                Update,
                (
                    forget_missing_target,
                    tab_target,
                    clear_target,
                    click_target,
                    place_target_ring,
                )
                    .chain(),
            );
    }
}

/// The character this player has selected, if any.
#[derive(Resource, Default, Debug)]
pub struct CurrentTarget(pub Option<Entity>);

#[derive(Resource, Default)]
struct ClickTracker {
    pressed: bool,
    travelled: f32,
}

#[derive(Component)]
struct TargetRing;

/// Forget a target that vanished or is in another zone.
fn forget_missing_target(
    mut target: ResMut<CurrentTarget>,
    characters: Query<(), (With<Motion>, Without<ElsewhereZone>)>,
) {
    if target.0.is_some_and(|t| characters.get(t).is_err()) {
        target.0 = None;
    }
}

fn tab_target(
    keys: Res<ButtonInput<KeyCode>>,
    data: Res<GameData>,
    camera: Single<&FollowCamera>,
    player: Single<&Motion, With<LocalPlayer>>,
    enemies: Query<(Entity, &Motion, &Faction, &Health), Without<ElsewhereZone>>,
    mut target: ResMut<CurrentTarget>,
) {
    if !keys.just_pressed(KeyCode::Tab) {
        return;
    }
    let candidates: Vec<Candidate> = enemies
        .iter()
        .filter(|(_, _, faction, health)| **faction == Faction::Enemy && !health.is_dead())
        .map(|(entity, motion, ..)| Candidate {
            entity,
            position: motion.0.position,
        })
        .collect();
    let forward = Vec2::new(-camera.yaw.sin(), -camera.yaw.cos());
    target.0 = next_tab_target(
        &candidates,
        player.0.position,
        forward,
        target.0,
        data.config.combat.tab_target_range,
    )
    .or(target.0);
}

/// Esc clears the target; F1 targets yourself (for heals and buffs).
fn clear_target(
    keys: Res<ButtonInput<KeyCode>>,
    player: Option<Single<Entity, With<LocalPlayer>>>,
    mut target: ResMut<CurrentTarget>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        target.0 = None;
    }
    if keys.just_pressed(KeyCode::F1)
        && let Some(player) = player
    {
        target.0 = Some(*player);
    }
}

/// A left click (not a drag, not on the HUD) selects the character under the mouse.
fn click_target(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut tracker: ResMut<ClickTracker>,
    ui: Query<&Interaction>,
    window: Single<&Window>,
    camera: Single<(&Camera, &GlobalTransform)>,
    characters: Query<
        (Entity, &Transform, &HitRadius),
        (With<DisplayMotion>, Without<ElsewhereZone>),
    >,
    mut target: ResMut<CurrentTarget>,
) {
    let over_ui = ui.iter().any(|i| *i != Interaction::None);
    if mouse.just_pressed(MouseButton::Left) {
        tracker.pressed = !over_ui;
        tracker.travelled = 0.0;
    }
    if tracker.pressed {
        tracker.travelled += motion.delta.length();
    }
    if !mouse.just_released(MouseButton::Left) || !tracker.pressed {
        return;
    }
    tracker.pressed = false;
    if tracker.travelled > CLICK_SLOP {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let (camera, camera_transform) = *camera;
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let picked = characters
        .iter()
        .filter_map(|(entity, transform, radius)| {
            ray_hits_upright_cylinder(
                ray.origin,
                *ray.direction,
                transform.translation,
                radius.0,
                PICK_HEIGHT,
            )
            .map(|distance| (distance, entity))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, entity)) = picked {
        target.0 = Some(entity);
    }
}

/// Distance along a ray to where it first hits an upright cylinder
/// standing on `base`, or `None` if it misses.
fn ray_hits_upright_cylinder(
    origin: Vec3,
    direction: Vec3,
    base: Vec3,
    radius: f32,
    height: f32,
) -> Option<f32> {
    let within_height = |t: f32| {
        let y = origin.y + direction.y * t;
        t >= 0.0 && y >= base.y && y <= base.y + height
    };
    let offset = Vec2::new(origin.x - base.x, origin.z - base.z);
    let dir = Vec2::new(direction.x, direction.z);
    let a = dir.length_squared();
    let mut best: Option<f32> = None;
    if a > 1e-8 {
        // Side wall: solve |offset + t·dir|² = radius².
        let b = 2.0 * offset.dot(dir);
        let c = offset.length_squared() - radius * radius;
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            let root = disc.sqrt();
            for t in [(-b - root) / (2.0 * a), (-b + root) / (2.0 * a)] {
                if within_height(t) {
                    best = Some(best.map_or(t, |b: f32| b.min(t)));
                }
            }
        }
    }
    // Top and bottom caps.
    if direction.y.abs() > 1e-8 {
        for cap_y in [base.y, base.y + height] {
            let t = (cap_y - origin.y) / direction.y;
            if t >= 0.0 && (offset + dir * t).length() <= radius {
                best = Some(best.map_or(t, |b: f32| b.min(t)));
            }
        }
    }
    best
}

fn spawn_target_ring(mut commands: Commands, mut toon: ToonAssets) {
    let ring = commands
        .spawn((TargetRing, Transform::default(), Visibility::Hidden))
        .id();
    let material = toon.glowing(Color::srgb(1.0, 0.35, 0.25), LinearRgba::rgb(2.5, 0.5, 0.3));
    toon.spawn_part(
        &mut commands,
        ring,
        Torus::new(0.96, 1.0)
            .mesh()
            .minor_resolution(8)
            .major_resolution(48),
        material,
        Outline::None,
        Transform::from_xyz(0.0, 0.03, 0.0),
    );
}

fn place_target_ring(
    time: Res<Time>,
    target: Res<CurrentTarget>,
    characters: Query<(&Transform, &HitRadius), Without<TargetRing>>,
    mut ring: Single<(&mut Transform, &mut Visibility), With<TargetRing>>,
) {
    let (ring_transform, visibility) = &mut *ring;
    match target.0.and_then(|t| characters.get(t).ok()) {
        Some((transform, radius)) => {
            **visibility = Visibility::Visible;
            ring_transform.translation = transform.translation;
            let pulse = 1.0 + 0.04 * (time.elapsed_secs() * 4.0).sin();
            ring_transform.scale = Vec3::new(radius.0 * pulse, 1.0, radius.0 * pulse);
        }
        None => **visibility = Visibility::Hidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_cylinder_side() {
        let t =
            ray_hits_upright_cylinder(Vec3::new(0.0, 1.0, 10.0), Vec3::NEG_Z, Vec3::ZERO, 1.0, 2.0);
        assert!((t.unwrap() - 9.0).abs() < 1e-5);
    }

    #[test]
    fn ray_misses_beside_or_above() {
        let beside =
            ray_hits_upright_cylinder(Vec3::new(2.0, 1.0, 10.0), Vec3::NEG_Z, Vec3::ZERO, 1.0, 2.0);
        assert_eq!(beside, None);
        let above =
            ray_hits_upright_cylinder(Vec3::new(0.0, 3.0, 10.0), Vec3::NEG_Z, Vec3::ZERO, 1.0, 2.0);
        assert_eq!(above, None);
    }

    #[test]
    fn ray_from_above_hits_the_top() {
        let t =
            ray_hits_upright_cylinder(Vec3::new(0.2, 10.0, 0.0), Vec3::NEG_Y, Vec3::ZERO, 1.0, 2.0);
        assert!((t.unwrap() - 8.0).abs() < 1e-5);
    }

    #[test]
    fn ray_behind_the_camera_does_not_count() {
        let t = ray_hits_upright_cylinder(
            Vec3::new(0.0, 1.0, -10.0),
            Vec3::NEG_Z,
            Vec3::ZERO,
            1.0,
            2.0,
        );
        assert_eq!(t, None);
    }
}
