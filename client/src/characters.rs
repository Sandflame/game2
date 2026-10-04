//! Showing characters: building placeholder bodies for entities the rules
//! half creates, smoothing their movement between ticks, sending the
//! player's movement keys, and small reactions such as hit wobbles.

use bevy::prelude::*;
use server::{Defeated, EnemyKind, FlameChange};
use shared::classes::CurrentClass;
use shared::combat::ActionState;
use shared::components::{Motion, PlayerId, VisualKey};
use shared::gamedata::GameData;
use shared::movement::{MoveInput, MoveState};
use shared::protocol::{ClientRequest, Link, ServerEvent};

use crate::animation::{BossRig, Hop, NoFlash};
use crate::camera::FollowCamera;
use crate::creatures::{
    self, BURROW_PUP, MATRIARCH, MOTHER_SPORECAP, ROT_SPORE, SPORE_CAP, THORNWOLF,
};
use crate::session::{LocalPlayerId, Received, send};
use crate::toon::{Outline, ToonAssets, ToonMaterial};

pub struct CharactersPlugin;

impl Plugin for CharactersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LanternSettings>()
            .init_resource::<ScriptedMove>()
            .add_systems(
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
            .add_systems(
                Update,
                (
                    animate_lanterns,
                    animate_lantern_holding,
                    show_defeated.after(animate_reactions),
                ),
            );
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
    /// The flame's colour and glow, set by the owner's class.
    color: Color,
    glow: LinearRgba,
    /// Extra brightness after using an ability; fades quickly.
    flare: f32,
}

/// Where the lantern hangs at the side (when "always show" is on).
const LANTERN_AT_SIDE: Vec3 = Vec3::new(0.48, 0.75, -0.12);
/// Where it is held up in both hands while the flame changes: in front of
/// the face, with the bottom of the lantern at about eye level.
/// (Eyes are at ~1.51 m; the held lantern's bottom is ~0.22 m below its centre.)
const LANTERN_HELD_OUT: Vec3 = Vec3::new(0.0, 1.74, -0.55);
/// How far the head tilts up to look at the held lantern (radians).
const LOOK_UP: f32 = 0.35;
/// Where the head sits on the body.
const HEAD_POSITION: Vec3 = Vec3::new(0.0, 1.48, 0.0);
/// The lantern looks a bit bigger while held up.
const LANTERN_HELD_SCALE: f32 = 1.5;
/// How long the lantern stays out after the new flame catches.
const LANTERN_LINGER: f32 = 1.2;
/// How quickly the lantern appears, disappears and moves (per second).
const LANTERN_SPEED: f32 = 4.0;

/// A character's lantern and how visible / held out it is right now.
#[derive(Component)]
struct Lantern {
    owner: Entity,
    hands: Entity,
    /// The owner's head, which tilts up to look at the held lantern.
    head: Entity,
    /// 0 = put away, 1 = fully visible.
    shown: f32,
    /// 0 = at the side, 1 = held out in front.
    held: f32,
    /// Seconds left to keep showing it after a flame change.
    linger: f32,
}

/// A direction the developer demo script is walking in (world X/Z).
/// Normal play never sets this.
#[derive(Resource, Default)]
pub struct ScriptedMove(pub Option<Vec2>);

/// Player's choice: keep the lantern visible all the time (off by default).
#[derive(Resource, Default)]
pub struct LanternSettings {
    pub always_show: bool,
}

/// A short squash-and-wobble after being hit.
#[derive(Component)]
struct HitWobble(f32);

/// The lantern flame's glow before a class is known.
const FLAME_GLOW: LinearRgba = LinearRgba::rgb(4.0, 1.8, 0.4);

/// Colour and glow of each class's lantern flame (the `flame` key in
/// class data). Looks only, so it lives in the client.
pub fn flame_look(key: &str) -> (Color, LinearRgba) {
    match key {
        "crimson" => (Color::srgb(1.0, 0.35, 0.3), LinearRgba::rgb(5.0, 0.7, 0.5)),
        "amber" => (Color::srgb(1.0, 0.7, 0.3), LinearRgba::rgb(4.5, 2.0, 0.3)),
        "azure" => (Color::srgb(0.4, 0.7, 1.0), LinearRgba::rgb(0.6, 1.8, 5.0)),
        "pearl" => (Color::srgb(1.0, 0.97, 0.88), LinearRgba::rgb(3.5, 3.3, 2.6)),
        _ => (Color::srgb(1.0, 0.7, 0.3), FLAME_GLOW),
    }
}

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
            "training_dummy" => build_training_dummy(
                &mut commands,
                &mut toon,
                entity,
                Color::srgb(0.80, 0.22, 0.20),
            ),
            "sparring_dummy" => build_training_dummy(
                &mut commands,
                &mut toon,
                entity,
                Color::srgb(0.20, 0.40, 0.85),
            ),
            "rootwarden" => build_rootwarden(&mut commands, &mut toon, entity),
            "thornling" => build_thornling(&mut commands, &mut toon, entity),
            "thornwolf" => creatures::thornwolf(&mut commands, &mut toon, entity, &THORNWOLF),
            "spore_cap" => creatures::spore_cap(&mut commands, &mut toon, entity, &SPORE_CAP),
            "burrow_pup" => {
                let body = creatures::scaled(&mut commands, entity, 0.75);
                creatures::thornwolf(&mut commands, &mut toon, body, &BURROW_PUP);
            }
            "burrow_matriarch" => {
                let body = creatures::scaled(&mut commands, entity, 2.2);
                creatures::thornwolf(&mut commands, &mut toon, body, &MATRIARCH);
            }
            "rot_spore" => creatures::spore_cap(&mut commands, &mut toon, entity, &ROT_SPORE),
            "mother_sporecap" => {
                let body = creatures::scaled(&mut commands, entity, 2.6);
                creatures::spore_cap(&mut commands, &mut toon, body, &MOTHER_SPORECAP);
            }
            "rotheart" => creatures::rotheart(&mut commands, &mut toon, entity),
            townsfolk if townsfolk.starts_with("townsfolk") => {
                creatures::townsfolk(&mut commands, &mut toon, entity, townsfolk)
            }
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
    let hand_skin = skin.clone();
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
        Transform::from_translation(HEAD_POSITION),
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

    // The lantern: hidden until the flame is changed (or always shown, if
    // the player chose that in the lantern panel).
    let lantern = commands
        .spawn((
            Transform::from_translation(LANTERN_AT_SIDE).with_scale(Vec3::ZERO),
            Visibility::Hidden,
            ChildOf(player),
        ))
        .id();
    // Placeholder hands, shown while the lantern is held out.
    let hands = commands
        .spawn((Transform::default(), Visibility::Hidden, ChildOf(lantern)))
        .id();
    for side in [-1.0, 1.0] {
        toon.spawn_part(
            commands,
            hands,
            Sphere::new(0.075).mesh().uv(12, 8),
            hand_skin.clone(),
            Outline::Smooth,
            Transform::from_xyz(0.15 * side, -0.02, 0.06),
        );
    }
    commands.entity(lantern).insert(Lantern {
        owner: player,
        hands,
        head,
        shown: 0.0,
        held: 0.0,
        linger: 0.0,
    });
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
    commands.entity(flame_part).insert((
        NoFlash,
        LanternFlame {
            owner: player,
            material: flame,
            color: Color::srgb(1.0, 0.7, 0.3),
            glow: FLAME_GLOW,
            flare: 0.0,
        },
    ));
}

/// Placeholder training dummy: a straw figure on a wooden post.
fn build_training_dummy(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    dummy: Entity,
    sash: Color,
) {
    let wood = toon.material(Color::srgb(0.48, 0.33, 0.22));
    let straw = toon.material(Color::srgb(0.90, 0.78, 0.45));
    let cloth = toon.material(sash);

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

/// Placeholder Rootwarden: a towering tree spirit with glowing eyes.
fn build_rootwarden(commands: &mut Commands, toon: &mut ToonAssets, boss: Entity) {
    let bark = toon.material(Color::srgb(0.55, 0.40, 0.28));
    let dark_bark = toon.material(Color::srgb(0.40, 0.29, 0.21));
    let leaves = toon.material(Color::srgb(0.30, 0.62, 0.32));
    let blossom = toon.material(Color::srgb(0.95, 0.70, 0.80));
    let eyes = toon.glowing(Color::srgb(1.0, 0.85, 0.4), LinearRgba::rgb(4.0, 2.6, 0.6));

    // Roots spreading over the ground (they stay put while the body moves).
    let mut roots = Vec::new();
    for i in 0..6 {
        let angle = i as f32 * std::f32::consts::TAU / 6.0 + 0.3;
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        roots.push(toon.spawn_part(
            commands,
            boss,
            Capsule3d::new(0.3, 1.6),
            dark_bark.clone(),
            Outline::Smooth,
            Transform::from_translation(out * 1.6 + Vec3::Y * 0.3).with_rotation(
                Quat::from_rotation_arc(Vec3::Y, (out + Vec3::Y * 0.25).normalize()),
            ),
        ));
    }
    // Everything above the roots bends at the base, so it can sway, lean
    // back while casting, and slam forwards.
    let torso = commands
        .spawn((Transform::default(), Visibility::default(), ChildOf(boss)))
        .id();
    // Trunk body.
    toon.spawn_part(
        commands,
        torso,
        Capsule3d::new(1.3, 2.6),
        bark.clone(),
        Outline::Smooth,
        Transform::from_xyz(0.0, 2.6, 0.0),
    );
    // Arms (great branches).
    let mut arms = [Entity::PLACEHOLDER; 2];
    for (arm_slot, side) in arms.iter_mut().zip([-1.0, 1.0]) {
        // A shoulder to swing the arm from.
        let shoulder = commands
            .spawn((
                Transform::from_xyz(1.0 * side, 3.6, -0.3),
                Visibility::default(),
                ChildOf(torso),
            ))
            .id();
        let arm = toon.spawn_part(
            commands,
            shoulder,
            Capsule3d::new(0.35, 2.2),
            bark.clone(),
            Outline::Smooth,
            Transform::from_xyz(0.6 * side, -0.2, 0.0)
                .with_rotation(Quat::from_rotation_z(-0.9 * side) * Quat::from_rotation_x(0.3)),
        );
        toon.spawn_part(
            commands,
            arm,
            Sphere::new(0.7).mesh().uv(20, 12),
            leaves.clone(),
            Outline::Smooth,
            Transform::from_xyz(0.0, 1.5, 0.0),
        );
        *arm_slot = shoulder;
    }
    // Leafy crown with a few blossoms.
    for (offset, size) in [
        (Vec3::new(0.0, 5.6, 0.0), 1.9),
        (Vec3::new(1.2, 5.0, 0.6), 1.3),
        (Vec3::new(-1.2, 5.1, 0.4), 1.4),
        (Vec3::new(0.2, 5.0, 1.2), 1.2),
    ] {
        toon.spawn_part(
            commands,
            torso,
            Sphere::new(size).mesh().uv(28, 16),
            leaves.clone(),
            Outline::Smooth,
            Transform::from_translation(offset),
        );
    }
    for offset in [
        Vec3::new(0.9, 6.4, -1.1),
        Vec3::new(-1.0, 5.9, -1.2),
        Vec3::new(0.0, 7.2, -0.4),
    ] {
        toon.spawn_part(
            commands,
            torso,
            Sphere::new(0.25).mesh().uv(12, 8),
            blossom.clone(),
            Outline::Smooth,
            Transform::from_translation(offset),
        );
    }
    // Glowing eyes, facing forward (-Z).
    for side in [-1.0, 1.0] {
        toon.spawn_part(
            commands,
            torso,
            Sphere::new(0.2).mesh().uv(12, 8),
            eyes.clone(),
            Outline::None,
            Transform::from_xyz(0.45 * side, 3.9, -1.2),
        );
    }
    commands
        .entity(boss)
        .insert(BossRig::new(torso, arms).with_roots(roots));
}

/// Placeholder Thornling: a small spiky sapling.
fn build_thornling(commands: &mut Commands, toon: &mut ToonAssets, add: Entity) {
    let green = toon.material(Color::srgb(0.35, 0.65, 0.30));
    let thorn = toon.material(Color::srgb(0.55, 0.40, 0.25));
    let eyes = toon.glowing(Color::srgb(1.0, 0.4, 0.3), LinearRgba::rgb(3.0, 0.5, 0.3));
    let body = toon.spawn_part(
        commands,
        add,
        Cone::new(0.55, 1.3),
        green,
        Outline::Smooth,
        Transform::from_xyz(0.0, 0.65, 0.0),
    );
    commands.entity(body).insert(Hop { rest: 0.65 });
    for i in 0..5 {
        let angle = i as f32 * std::f32::consts::TAU / 5.0;
        let out = Vec3::new(angle.cos(), 0.3, angle.sin()).normalize();
        toon.spawn_part(
            commands,
            body,
            Cone::new(0.08, 0.4),
            thorn.clone(),
            Outline::Smooth,
            Transform::from_translation(out * 0.35)
                .with_rotation(Quat::from_rotation_arc(Vec3::Y, out)),
        );
    }
    for side in [-1.0, 1.0] {
        toon.spawn_part(
            commands,
            body,
            Sphere::new(0.07).mesh().uv(10, 6),
            eyes.clone(),
            Outline::None,
            Transform::from_xyz(0.13 * side, 0.1, -0.33),
        );
    }
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
    scripted: Res<ScriptedMove>,
    mut link: ResMut<Link>,
) {
    // The interact key uses portals (and later, other things).
    if keys.just_pressed(KeyCode::KeyE) {
        send(&mut link, *me, ClientRequest::Interact);
    }
    if let Some(direction) = scripted.0 {
        let input = MoveInput {
            direction,
            ..default()
        };
        send(&mut link, *me, ClientRequest::Move(input));
        return;
    }
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
    mut lanterns: Query<&mut Lantern>,
    enemies: Query<(), With<EnemyKind>>,
) {
    for Received(event) in received.read() {
        match event {
            ServerEvent::AbilityUsed { user, .. } => {
                for mut flame in &mut flames {
                    if flame.owner == *user {
                        flame.flare = 1.0;
                    }
                }
                // Enemies give a little lurch when they attack.
                if enemies.contains(*user)
                    && let Ok(mut entity) = commands.get_entity(*user)
                {
                    entity.insert(HitWobble(0.8));
                }
            }
            ServerEvent::ClassChanged { user, .. } => {
                for mut flame in &mut flames {
                    if flame.owner == *user {
                        flame.flare = 2.0;
                    }
                }
                for mut lantern in &mut lanterns {
                    if lantern.owner == *user {
                        lantern.linger = LANTERN_LINGER;
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

/// Lantern flames take their class's colour, flicker, glow brighter while
/// casting, flare on use, and sputter while the flame is being changed.
fn animate_lanterns(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    data: Res<GameData>,
    owners: Query<(&ActionState, Option<&CurrentClass>, Has<FlameChange>)>,
    mut flames: Query<&mut LanternFlame>,
    mut materials: ResMut<Assets<ToonMaterial>>,
) {
    let t = time.elapsed_secs();
    let now = fixed.elapsed_secs_f64() + fixed.overstep().as_secs_f64();
    let flicker = 1.0 + 0.12 * (t * 9.0).sin() + 0.08 * (t * 23.0 + 1.3).sin();
    for mut flame in &mut flames {
        flame.flare = (flame.flare - time.delta_secs() * 3.0).max(0.0);
        let Ok((actions, class, changing)) = owners.get(flame.owner) else {
            continue;
        };
        if let Some(class) = class.and_then(|c| data.classes.get(&c.class)) {
            (flame.color, flame.glow) = flame_look(&class.flame);
        }
        let casting = actions.cast_progress(now).unwrap_or(0.0);
        let sputter = if changing {
            0.6 + 0.6 * (t * 30.0).sin()
        } else {
            1.0
        };
        let boost = (1.0 + casting * 1.5 + flame.flare * 3.0) * sputter;
        if let Some(mut material) = materials.get_mut(&flame.material) {
            material.base.base_color = flame.color;
            material.base.emissive = flame.glow * flicker * boost;
        }
    }
}

/// The lantern appears in the character's hands while the flame changes,
/// stays a moment after the new flame catches, then is put away.
fn animate_lantern_holding(
    time: Res<Time>,
    settings: Res<LanternSettings>,
    owners: Query<Has<FlameChange>>,
    mut lanterns: Query<(&mut Lantern, &mut Transform, &mut Visibility)>,
    mut hands: Query<&mut Visibility, Without<Lantern>>,
    mut heads: Query<&mut Transform, Without<Lantern>>,
) {
    let dt = time.delta_secs();
    let step = dt * LANTERN_SPEED;
    for (mut lantern, mut transform, mut visibility) in &mut lanterns {
        let changing = owners.get(lantern.owner).unwrap_or(false);
        lantern.linger = (lantern.linger - dt).max(0.0);
        let holding = changing || lantern.linger > 0.0;
        let want_shown = holding || settings.always_show;
        lantern.shown = approach(lantern.shown, if want_shown { 1.0 } else { 0.0 }, step);
        lantern.held = approach(lantern.held, if holding { 1.0 } else { 0.0 }, step);

        let held = smooth(lantern.held);
        let bob = Vec3::Y * 0.03 * held * (time.elapsed_secs() * 3.0).sin();
        transform.translation = LANTERN_AT_SIDE.lerp(LANTERN_HELD_OUT, held) + bob;
        transform.scale =
            Vec3::splat(smooth(lantern.shown) * (1.0 + (LANTERN_HELD_SCALE - 1.0) * held));
        *visibility = if lantern.shown > 0.01 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if let Ok(mut head) = heads.get_mut(lantern.head) {
            head.rotation = Quat::from_rotation_x(LOOK_UP * held);
        }
        if let Ok(mut hands_visibility) = hands.get_mut(lantern.hands) {
            *hands_visibility = if lantern.held > 0.5 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
}

fn approach(value: f32, target: f32, step: f32) -> f32 {
    if value < target {
        (value + step).min(target)
    } else {
        (value - step).max(target)
    }
}

/// Ease in and out (0 → 1).
fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Defeated characters lie down (bosses sink instead; see `animation.rs`).
fn show_defeated(mut fallen: Query<&mut Transform, (With<Defeated>, Without<BossRig>)>) {
    for mut transform in &mut fallen {
        transform.rotation *= Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        transform.translation.y += 0.35;
    }
}
