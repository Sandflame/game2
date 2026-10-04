//! FFXIV-style follow camera:
//! - left-drag orbits the camera around the character,
//! - right-drag orbits and turns the character to match,
//! - the mouse wheel zooms.

use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

use shared::gamedata::Zones;

use crate::characters::{LocalPlayer, interpolate_transforms, send_movement};
use crate::hud::options::OptionsMenu;
use crate::world::CurrentZone;

/// Radians of turn per pixel of mouse movement.
const MOUSE_SENSITIVITY: f32 = 0.005;
const MIN_PITCH: f32 = -0.35;
const MAX_PITCH: f32 = 1.35;
const MIN_DISTANCE: f32 = 2.5;
const MAX_DISTANCE: f32 = 25.0;
/// Zoom change per wheel notch, in metres.
const ZOOM_STEP: f32 = 1.5;
/// Height above the character's feet that the camera looks at.
const FOCUS_HEIGHT: f32 = 1.4;
/// In zones with a wall around the edge, the camera stays this far inside
/// the edge (moving closer to the character) instead of going into the wall.
const WALL_MARGIN: f32 = 0.6;
/// The closest the camera is pushed in by a wall.
const CLOSEST_TO_WALL: f32 = 0.5;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraShake>()
            .init_resource::<CameraDrag>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                RunFixedMainLoop,
                (
                    (track_drag, orbit_camera, grab_cursor_while_dragging)
                        .chain()
                        .before(send_movement)
                        .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
                    follow_player
                        .after(interpolate_transforms)
                        .in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
                ),
            );
    }
}

/// Shaking from big impacts (0–1); fades on its own.
#[derive(Resource, Default)]
pub struct CameraShake(pub f32);

impl CameraShake {
    pub fn add(&mut self, amount: f32) {
        self.0 = (self.0 + amount).min(1.0);
    }
}

/// How far the strongest shake moves the camera (metres), and how fast it fades.
const SHAKE_SIZE: f32 = 0.18;
const SHAKE_FADE: f32 = 2.5;

#[derive(Component)]
pub struct FollowCamera {
    /// Rotation around the character (radians). 0 looks towards -Z.
    pub yaw: f32,
    /// How far the camera looks down (radians).
    pub pitch: f32,
    pub distance: f32,
    /// Where the zoom is heading; `distance` eases towards it.
    pub target_distance: f32,
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Hdr,
        // Flat anime colours look best without film-style tone mapping.
        Tonemapping::None,
        Bloom::NATURAL,
        DistanceFog {
            color: Color::srgba(0.70, 0.85, 0.97, 1.0),
            falloff: FogFalloff::Linear {
                start: 45.0,
                end: 140.0,
            },
            ..default()
        },
        FollowCamera {
            yaw: 0.0,
            pitch: 0.35,
            distance: 8.0,
            target_distance: 8.0,
        },
        Transform::from_xyz(0.0, 4.0, 16.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Whether the mouse is dragging the camera. Only a press that starts in
/// the game world (not on a menu, slider or button) turns the camera, so
/// the pointer stays free for the interface.
#[derive(Resource, Default)]
pub struct CameraDrag {
    pub active: bool,
}

fn track_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    menu: Res<OptionsMenu>,
    ui: Query<&Interaction>,
    mut drag: ResMut<CameraDrag>,
) {
    let held = mouse.pressed(MouseButton::Left) || mouse.pressed(MouseButton::Right);
    if !held {
        drag.active = false;
    } else if mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right) {
        let over_ui = ui.iter().any(|i| *i != Interaction::None);
        // A second button pressed during a drag keeps it going.
        drag.active = drag.active || (!menu.open && !over_ui);
    }
}

fn orbit_camera(
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    menu: Res<OptionsMenu>,
    drag: Res<CameraDrag>,
    mut camera: Single<&mut FollowCamera>,
) {
    // The mouse works the options menu while it is open.
    if menu.open {
        return;
    }
    if drag.active {
        camera.yaw -= motion.delta.x * MOUSE_SENSITIVITY;
        camera.pitch =
            (camera.pitch + motion.delta.y * MOUSE_SENSITIVITY).clamp(MIN_PITCH, MAX_PITCH);
    }
    let notches = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 100.0,
    };
    if notches != 0.0 {
        camera.target_distance =
            (camera.target_distance - notches * ZOOM_STEP).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }
}

/// Hide and hold the mouse pointer while dragging the camera.
fn grab_cursor_while_dragging(drag: Res<CameraDrag>, mut cursor: Single<&mut CursorOptions>) {
    let dragging = drag.active;
    let grab = if dragging {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    if cursor.grab_mode != grab {
        cursor.grab_mode = grab;
        cursor.visible = !dragging;
    }
}

/// How far the camera can go from `focus` along `direction` before
/// leaving a square of half-size `limit`.
fn room_before_wall(focus: Vec3, direction: Vec3, distance: f32, limit: f32) -> f32 {
    let mut room = distance;
    for (from, towards) in [(focus.x, direction.x), (focus.z, direction.z)] {
        if towards.abs() > 1e-4 {
            let edge = limit.copysign(towards);
            let reach = (edge - from) / towards;
            if reach >= 0.0 {
                room = room.min(reach);
            }
        }
    }
    room.max(CLOSEST_TO_WALL.min(distance))
}

fn follow_player(
    time: Res<Time>,
    current: Res<CurrentZone>,
    zones: Res<Zones>,
    mut shake: ResMut<CameraShake>,
    player: Single<&Transform, (With<LocalPlayer>, Without<FollowCamera>)>,
    camera: Single<(&mut FollowCamera, &mut Transform)>,
) {
    let (mut follow, mut transform) = camera.into_inner();
    // Ease the zoom so wheel steps feel smooth.
    let ease = 1.0 - (-12.0 * time.delta_secs()).exp();
    follow.distance += (follow.target_distance - follow.distance) * ease;

    let focus = player.translation + Vec3::Y * FOCUS_HEIGHT;
    let rotation = Quat::from_euler(EulerRot::YXZ, follow.yaw, -follow.pitch, 0.0);
    let direction = rotation * Vec3::Z;
    let mut distance = follow.distance;
    if let Some(level) = current.0.as_deref().and_then(|zone| zones.get(zone))
        && !level.border.is_empty()
    {
        distance = room_before_wall(focus, direction, distance, level.half_size - WALL_MARGIN);
    }
    let mut position = focus + direction * distance;
    // Never dip below the ground.
    position.y = position.y.max(0.3);
    *transform = Transform::from_translation(position).looking_at(focus, Vec3::Y);
    if shake.0 > 0.0 {
        let t = time.elapsed_secs();
        let strength = shake.0 * shake.0 * SHAKE_SIZE;
        let offset = Vec3::new((t * 47.0).sin(), (t * 61.0).sin(), 0.0) * strength;
        let rotation = transform.rotation;
        transform.translation += rotation * offset;
        shake.0 = (shake.0 - time.delta_secs() * SHAKE_FADE).max(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_stops_at_the_wall() {
        let focus = Vec3::new(0.0, 1.4, 15.0);
        // Looking back towards +Z, the wall at 20 is 5 m away.
        let room = room_before_wall(focus, Vec3::Z, 13.0, 20.0);
        assert!((room - 5.0).abs() < 1e-4);
        // Plenty of room the other way.
        assert_eq!(room_before_wall(focus, Vec3::NEG_Z, 13.0, 20.0), 13.0);
        // Right against the wall, it still stays a little behind the player.
        let close = Vec3::new(0.0, 1.4, 20.0);
        assert_eq!(
            room_before_wall(close, Vec3::Z, 13.0, 20.0),
            CLOSEST_TO_WALL
        );
    }
}
