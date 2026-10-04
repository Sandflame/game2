//! Showing characters: building placeholder bodies for entities the rules
//! half creates, smoothing their movement between ticks, sending the
//! player's movement keys, and small reactions such as hit wobbles.

use bevy::prelude::*;
use shared::combat::ActionState;
use shared::components::{Motion, PlayerId, VisualKey};
use shared::movement::{MoveInput, MoveState};
use shared::protocol::{ClientRequest, Link, ServerEvent};

use crate::camera::FollowCamera;
use crate::session::{LocalPlayerId, Received, send};
use crate::toon::{Outline, ToonAssets, ToonMaterial};

pub struct CharactersPlugin;

impl Plugin for CharactersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (spawn_visuals, react_to_events, animate_reactions).chain(),
        )
        .add_systems(FixedPostUpdate, record_motion)
        .add_systems(
            RunFixedMainLoop,
            (
                send_movement.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
                interpolate_transforms.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
            ),
        )
        .add_systems(Update, animate_lanterns);
    }
}

/// Marks the character this client controls.
#[derive(Component)]
pub struct LocalPlayer;

/// The character's position at the last two ticks; the screen shows a
/// blend of the two so movement is smooth at any frame rate.
#[derive(Component)]
pub struct DisplayMotion {
    pub previous: MoveState,
    pub current: MoveState,
}

/// The glowing part of a character's lantern.
#[derive(Component)]
struct LanternFlame {
    owner: Entity,
    material: Handle<ToonMaterial>,
    /// Extra brightness after using an ability; fades quickly.
    flare: f32,
}

/// A short squash-and-wobble after being hit.
#[derive(Component)]
struct HitWobble(f32);

/// The lantern flame's normal glow.
const FLAME_GLOW: LinearRgba = LinearRgba::rgb(4.0, 1.8, 0.4);

fn spawn_visuals(
    mut commands: Commands,
    new: Query<(Entity, &VisualKey, &Motion, Option<&PlayerId>), Added<VisualKey>>,
    me: Res<LocalPlayerId>,
    mut toon: ToonAssets,
) {
    for (entity, key, motion, player) in &new {
        commands.entity(entity).insert((
            Transform::from_translation(motion.0.position)
                .with_rotation(Quat::from_rotation_y(motion.0.yaw)),
            Visibility::default(),
            DisplayMotion {
                previous: motion.0,
                current: motion.0,
            },
        ));
        if player == Some(&me.0) {
            commands.entity(entity).insert(LocalPlayer);
        }
        match key.0.as_str() {
            "player" => build_player(&mut commands, &mut toon, entity),
            "training_dummy" => build_training_dummy(&mut commands, &mut toon, entity),
            other => {
                warn!("no placeholder look for visual `{other}`");
                let material = toon.material(Color::srgb(1.0, 0.0, 1.0));
                toon.spawn_part(
                    &mut commands,
                    entity,
                    Capsule3d::new(0.4, 1.0),
                    material,
                    Outline::Smooth,
                    Transform::from_xyz(0.0, 0.9, 0.0),
                );
            }
        }
    }
}

/// Placeholder adventurer: capsule body, head, eyes and a glowing lantern.
fn build_player(commands: &mut Commands, toon: &mut ToonAssets, player: Entity) {
    let cloth = toon.material(Color::srgb(0.30, 0.38, 0.70));
    let skin = toon.material(Color::srgb(1.0, 0.86, 0.74));
    let eye = toon.material(Color::srgb(0.08, 0.06, 0.12));
    let brass = toon.material(Color::srgb(0.80, 0.62, 0.25));
    let flame = toon.glowing(Color::srgb(1.0, 0.7, 0.3), FLAME_GLOW);

    toon.spawn_part(
        commands,
        player,
        Capsule3d::new(0.35, 0.6),
        cloth,
        Outline::Smooth,
        Transform::from_xyz(0.0, 0.65, 0.0),
    );
    let head = toon.spawn_part(
        commands,
        player,
        Sphere::new(0.3).mesh().uv(32, 18),
        skin,
        Outline::Smooth,
        Transform::from_xyz(0.0, 1.48, 0.0),
    );
    for side in [-1.0, 1.0] {
        toon.spawn_part(
            commands,
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
            commands,
            lantern,
            Cylinder::new(0.1, 0.04),
            brass.clone(),
            Outline::Cylinder,
            Transform::from_xyz(0.0, y, 0.0),
        );
    }
    let flame_part = toon.spawn_part(
        commands,
        lantern,
        Sphere::new(0.09).mesh().uv(16, 10),
        flame.clone(),
        Outline::None,
        Transform::default(),
    );
    commands.entity(flame_part).insert(LanternFlame {
        owner: player,
        material: flame,
        flare: 0.0,
    });
}

/// Placeholder training dummy: a straw figure on a wooden post.
fn build_training_dummy(commands: &mut Commands, toon: &mut ToonAssets, dummy: Entity) {
    let wood = toon.material(Color::srgb(0.48, 0.33, 0.22));
    let straw = toon.material(Color::srgb(0.90, 0.78, 0.45));
    let cloth = toon.material(Color::srgb(0.80, 0.22, 0.20));

    toon.spawn_part(
        commands,
        dummy,
        Cylinder::new(0.08, 1.0),
        wood.clone(),
        Outline::Cylinder,
        Transform::from_xyz(0.0, 0.5, 0.0),
    );
    toon.spawn_part(
        commands,
        dummy,
        Cylinder::new(0.4, 0.05),
        wood.clone(),
        Outline::Cylinder,
        Transform::from_xyz(0.0, 0.025, 0.0),
    );
    toon.spawn_part(
        commands,
        dummy,
        Capsule3d::new(0.36, 0.5),
        straw.clone(),
        Outline::Smooth,
        Transform::from_xyz(0.0, 1.25, 0.0),
    );
    toon.spawn_part(
        commands,
        dummy,
        Cylinder::new(0.38, 0.14),
        cloth,
        Outline::Cylinder,
        Transform::from_xyz(0.0, 1.15, 0.0),
    );
    toon.spawn_part(
        commands,
        dummy,
        Cuboid::new(1.5, 0.14, 0.14),
        wood,
        Outline::Box,
        Transform::from_xyz(0.0, 1.45, 0.0),
    );
    toon.spawn_part(
        commands,
        dummy,
        Sphere::new(0.26).mesh().uv(24, 14),
        straw,
        Outline::Smooth,
        Transform::from_xyz(0.0, 1.98, 0.0),
    );
}

/// After each tick, remember where every character was and now is.
fn record_motion(mut characters: Query<(&Motion, &mut DisplayMotion)>) {
    for (motion, mut display) in &mut characters {
        display.previous = display.current;
        display.current = motion.0;
    }
}

/// Place each character between its last two ticks.
pub fn interpolate_transforms(
    fixed_time: Res<Time<Fixed>>,
    mut characters: Query<(&DisplayMotion, &mut Transform)>,
) {
    let alpha = fixed_time.overstep_fraction();
    for (display, mut transform) in &mut characters {
        let (previous, current) = (display.previous, display.current);
        transform.translation = previous.position.lerp(current.position, alpha);
        transform.rotation =
            Quat::from_rotation_y(previous.yaw).slerp(Quat::from_rotation_y(current.yaw), alpha);
    }
}

/// Send the movement keys to the rules half, relative to the camera.
pub fn send_movement(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    camera: Single<&FollowCamera>,
    me: Res<LocalPlayerId>,
    mut link: ResMut<Link>,
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
    if mouse.pressed(MouseButton::Left) && mouse.pressed(MouseButton::Right) {
        wish.y = 1.0;
    }

    let yaw = camera.yaw;
    let forward = Vec2::new(-yaw.sin(), -yaw.cos());
    let right = Vec2::new(yaw.cos(), -yaw.sin());
    let input = MoveInput {
        direction: (forward * wish.y + right * wish.x).normalize_or_zero(),
        jump: keys.just_pressed(KeyCode::Space),
        // Steering with the right mouse button turns the character to match the camera.
        face_yaw: mouse.pressed(MouseButton::Right).then_some(yaw),
    };
    send(&mut link, *me, ClientRequest::Move(input));
}

fn react_to_events(
    mut commands: Commands,
    mut received: MessageReader<Received>,
    mut flames: Query<&mut LanternFlame>,
) {
    for Received(event) in received.read() {
        match event {
            ServerEvent::AbilityUsed { user, .. } => {
                for mut flame in &mut flames {
                    if flame.owner == *user {
                        flame.flare = 1.0;
                    }
                }
            }
            ServerEvent::Damage { target, .. } => {
                if let Ok(mut entity) = commands.get_entity(*target) {
                    entity.insert(HitWobble(1.0));
                }
            }
            _ => {}
        }
    }
}

/// Squash and tilt characters that were just hit.
fn animate_reactions(
    time: Res<Time>,
    mut commands: Commands,
    mut wobbling: Query<(Entity, &mut HitWobble, &mut Transform)>,
) {
    for (entity, mut wobble, mut transform) in &mut wobbling {
        wobble.0 -= time.delta_secs() * 4.0;
        if wobble.0 <= 0.0 {
            transform.scale = Vec3::ONE;
            commands.entity(entity).remove::<HitWobble>();
            continue;
        }
        let w = wobble.0 * wobble.0;
        let squash = 1.0 - 0.12 * w * (wobble.0 * 18.0).cos();
        transform.scale = Vec3::new(2.0 - squash, squash, 2.0 - squash);
        transform.rotation *= Quat::from_rotation_x(0.12 * w * (wobble.0 * 14.0).sin());
    }
}

/// Lantern flames flicker, glow brighter while casting, and flare on use.
fn animate_lanterns(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    casters: Query<&ActionState>,
    mut flames: Query<&mut LanternFlame>,
    mut materials: ResMut<Assets<ToonMaterial>>,
) {
    let t = time.elapsed_secs();
    let now = fixed.elapsed_secs_f64() + fixed.overstep().as_secs_f64();
    let flicker = 1.0 + 0.12 * (t * 9.0).sin() + 0.08 * (t * 23.0 + 1.3).sin();
    for mut flame in &mut flames {
        flame.flare = (flame.flare - time.delta_secs() * 3.0).max(0.0);
        let casting = casters
            .get(flame.owner)
            .ok()
            .and_then(|a| a.cast_progress(now))
            .unwrap_or(0.0);
        let boost = 1.0 + casting * 1.5 + flame.flare * 3.0;
        if let Some(mut material) = materials.get_mut(&flame.material) {
            material.base.emissive = FLAME_GLOW * flicker * boost;
        }
    }
}
