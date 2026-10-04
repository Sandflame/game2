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

use crate::player::{LocalPlayer, gather_input, interpolate_transforms};

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

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera).add_systems(
            RunFixedMainLoop,
            (
                (orbit_camera, grab_cursor_while_dragging)
                    .before(gather_input)
                    .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
                follow_player
                    .after(interpolate_transforms)
                    .in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
            ),
        );
    }
}

#[derive(Component)]
pub struct FollowCamera {
    /// Rotation around the character (radians). 0 looks towards -Z.
    pub yaw: f32,
    /// How far the camera looks down (radians).
    pub pitch: f32,
    pub distance: f32,
    /// Where the zoom is heading; `distance` eases towards it.
    target_distance: f32,
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

fn orbit_camera(
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut camera: Single<&mut FollowCamera>,
) {
    if mouse.pressed(MouseButton::Left) || mouse.pressed(MouseButton::Right) {
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
fn grab_cursor_while_dragging(
    mouse: Res<ButtonInput<MouseButton>>,
    mut cursor: Single<&mut CursorOptions>,
) {
    let dragging = mouse.pressed(MouseButton::Left) || mouse.pressed(MouseButton::Right);
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

fn follow_player(
    time: Res<Time>,
    player: Single<&Transform, (With<LocalPlayer>, Without<FollowCamera>)>,
    camera: Single<(&mut FollowCamera, &mut Transform)>,
) {
    let (mut follow, mut transform) = camera.into_inner();
    // Ease the zoom so wheel steps feel smooth.
    let ease = 1.0 - (-12.0 * time.delta_secs()).exp();
    follow.distance += (follow.target_distance - follow.distance) * ease;

    let focus = player.translation + Vec3::Y * FOCUS_HEIGHT;
    let rotation = Quat::from_euler(EulerRot::YXZ, follow.yaw, -follow.pitch, 0.0);
    let mut position = focus + rotation * Vec3::Z * follow.distance;
    // Never dip below the ground.
    position.y = position.y.max(0.3);
    *transform = Transform::from_translation(position).looking_at(focus, Vec3::Y);
}
