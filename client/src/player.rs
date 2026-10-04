//! The local player's character: reading the keyboard, running the shared
//! movement rules at a fixed rate, and smoothing the result for display.

use bevy::input::mouse::MouseButton;
use bevy::prelude::*;
use shared::config::GameConfig;
use shared::level::Level;
use shared::movement::{self, MoveInput, MoveState};

use crate::camera::FollowCamera;
use crate::toon::{Outline, ToonAssets};

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingInput>()
            .add_systems(Startup, spawn_player)
            .add_systems(FixedUpdate, simulate_player)
            .add_systems(
                RunFixedMainLoop,
                (
                    gather_input.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
                    interpolate_transforms.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
                ),
            )
            .add_systems(Update, flicker_lanterns);
    }
}

/// Marks the character this client controls.
#[derive(Component)]
pub struct LocalPlayer;

/// Movement state at the latest and the previous simulation tick. The
/// screen shows a blend of the two, so motion looks smooth at any frame
/// rate even though the rules run at a fixed rate.
#[derive(Component)]
pub struct Movement {
    pub current: MoveState,
    pub previous: MoveState,
}

/// Input collected every frame, consumed by the next simulation tick.
#[derive(Resource, Default)]
pub struct PendingInput {
    input: MoveInput,
    /// Jump is a tap, so remember it until a tick has used it.
    jump_queued: bool,
}

#[derive(Component)]
struct LanternFlame(Handle<crate::toon::ToonMaterial>);

fn spawn_player(mut commands: Commands, level: Res<Level>, mut toon: ToonAssets) {
    let state = MoveState::spawn_at(level.spawn_point);
    let player = commands
        .spawn((
            Name::new("Player"),
            LocalPlayer,
            Movement {
                current: state,
                previous: state,
            },
            Transform::from_translation(state.position),
            Visibility::default(),
        ))
        .id();

    // Placeholder body: capsule, head, eyes and a glowing lantern.
    let cloth = toon.material(Color::srgb(0.30, 0.38, 0.70));
    let skin = toon.material(Color::srgb(1.0, 0.86, 0.74));
    let eye = toon.material(Color::srgb(0.08, 0.06, 0.12));
    let brass = toon.material(Color::srgb(0.80, 0.62, 0.25));
    let flame = toon.glowing(Color::srgb(1.0, 0.7, 0.3), LinearRgba::rgb(4.0, 1.8, 0.4));

    toon.spawn_part(
        &mut commands,
        player,
        Capsule3d::new(0.35, 0.6),
        cloth,
        Outline::Smooth,
        Transform::from_xyz(0.0, 0.65, 0.0),
    );
    let head = toon.spawn_part(
        &mut commands,
        player,
        Sphere::new(0.3).mesh().uv(32, 18),
        skin,
        Outline::Smooth,
        Transform::from_xyz(0.0, 1.48, 0.0),
    );
    for side in [-1.0, 1.0] {
        toon.spawn_part(
            &mut commands,
            head,
            Sphere::new(0.045).mesh().uv(12, 8),
            eye.clone(),
            Outline::None,
            Transform::from_xyz(0.1 * side, 0.03, -0.27),
        );
    }

    // The lantern, held at the character's right side.
    let lantern = commands
        .spawn((
            Transform::from_xyz(0.48, 0.75, -0.12),
            Visibility::default(),
            ChildOf(player),
        ))
        .id();
    for y in [-0.13, 0.13] {
        toon.spawn_part(
            &mut commands,
            lantern,
            Cylinder::new(0.1, 0.04),
            brass.clone(),
            Outline::Cylinder,
            Transform::from_xyz(0.0, y, 0.0),
        );
    }
    let flame_part = toon.spawn_part(
        &mut commands,
        lantern,
        Sphere::new(0.09).mesh().uv(16, 10),
        flame.clone(),
        Outline::None,
        Transform::default(),
    );
    commands.entity(flame_part).insert(LanternFlame(flame));
}

/// Turn keys and mouse buttons into a [`MoveInput`], relative to where the
/// camera is looking.
pub fn gather_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    camera: Single<&FollowCamera>,
    mut pending: ResMut<PendingInput>,
) {
    let mut wish = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        wish.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        wish.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        wish.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        wish.x += 1.0;
    }
    // Holding both mouse buttons runs forward (as in FFXIV).
    let both_buttons = mouse.pressed(MouseButton::Left) && mouse.pressed(MouseButton::Right);
    if both_buttons {
        wish.y = 1.0;
    }

    // Convert "forward/right relative to the camera" into world directions.
    let yaw = camera.yaw;
    let forward = Vec2::new(-yaw.sin(), -yaw.cos());
    let right = Vec2::new(yaw.cos(), -yaw.sin());
    let direction = (forward * wish.y + right * wish.x).normalize_or_zero();

    if keys.just_pressed(KeyCode::Space) {
        pending.jump_queued = true;
    }
    pending.input = MoveInput {
        direction,
        jump: pending.jump_queued,
        // Steering with the right mouse button turns the character to match the camera.
        face_yaw: mouse.pressed(MouseButton::Right).then_some(yaw),
    };
}

fn simulate_player(
    time: Res<Time>,
    config: Res<GameConfig>,
    level: Res<Level>,
    mut pending: ResMut<PendingInput>,
    mut players: Query<&mut Movement, With<LocalPlayer>>,
) {
    let dt = time.delta_secs();
    for mut movement in &mut players {
        movement.previous = movement.current;
        movement.current = movement::step(
            movement.current,
            pending.input,
            &config.movement,
            &level,
            dt,
        );
    }
    pending.jump_queued = false;
    pending.input.jump = false;
}

/// Place each character between its last two simulated positions,
/// according to how far we are towards the next tick.
pub fn interpolate_transforms(
    fixed_time: Res<Time<Fixed>>,
    mut characters: Query<(&Movement, &mut Transform)>,
) {
    let alpha = fixed_time.overstep_fraction();
    for (movement, mut transform) in &mut characters {
        let (previous, current) = (movement.previous, movement.current);
        transform.translation = previous.position.lerp(current.position, alpha);
        transform.rotation =
            Quat::from_rotation_y(previous.yaw).slerp(Quat::from_rotation_y(current.yaw), alpha);
    }
}

fn flicker_lanterns(
    time: Res<Time>,
    flames: Query<&LanternFlame>,
    mut materials: ResMut<Assets<crate::toon::ToonMaterial>>,
) {
    let t = time.elapsed_secs();
    let flicker = 1.0 + 0.12 * (t * 9.0).sin() + 0.08 * (t * 23.0 + 1.3).sin();
    for flame in &flames {
        if let Some(mut material) = materials.get_mut(&flame.0) {
            material.base.emissive = LinearRgba::rgb(4.0, 1.8, 0.4) * flicker;
        }
    }
}
