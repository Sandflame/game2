//! Builds the visible zone from the shared zone data: ground, sky,
//! sunlight, placeholder props for every obstacle, and portals. When the
//! player moves to another zone the scenery is rebuilt, and characters
//! and markers in other zones are hidden.

use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::prelude::*;
use shared::components::Zone;
use shared::gamedata::Zones;
use shared::level::{Level, Obstacle, Portal, Shape};

use crate::characters::LocalPlayer;
use crate::toon::{Outline, ToonAssets};
use crate::vfx::Looks;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.55, 0.78, 0.95)))
            .init_resource::<CurrentZone>()
            .add_systems(Startup, spawn_lighting)
            .add_systems(
                Update,
                (
                    track_current_zone,
                    rebuild_scenery,
                    hide_other_zones,
                    animate_portals,
                    add_lasting_particles,
                )
                    .chain(),
            );
    }
}

/// The zone the local player is in (and so the one being shown).
#[derive(Resource, Default, Debug)]
pub struct CurrentZone(pub Option<String>);

/// Something in a different zone from the local player: hidden, and not
/// targetable.
#[derive(Component)]
pub struct ElsewhereZone;

/// The root of the current zone's scenery.
#[derive(Component)]
struct ZoneScenery(String);

/// A spinning part of a portal.
#[derive(Component)]
struct PortalSpin;

/// Something that gets never-ending particles of this look (`vfx.ron`),
/// raised this far off the ground.
#[derive(Component)]
struct LastingParticles(String, f32);

fn spawn_lighting(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.5, -1.0, -0.35), Vec3::Y),
        CascadeShadowConfigBuilder {
            maximum_distance: 80.0,
            first_cascade_far_bound: 15.0,
            ..default()
        }
        .build(),
    ));
}

fn track_current_zone(
    player: Option<Single<&Zone, With<LocalPlayer>>>,
    mut current: ResMut<CurrentZone>,
) {
    let zone = player.map(|z| z.0.clone());
    if current.0 != zone {
        current.0 = zone;
    }
}

/// Swap the scenery when the current zone changes.
fn rebuild_scenery(
    mut commands: Commands,
    current: Res<CurrentZone>,
    zones: Res<Zones>,
    shown: Query<(Entity, &ZoneScenery)>,
    mut toon: ToonAssets,
    mut standard: ResMut<Assets<StandardMaterial>>,
) {
    let Some(zone) = &current.0 else {
        return;
    };
    if shown.iter().any(|(_, s)| &s.0 == zone) {
        return;
    }
    for (entity, _) in &shown {
        commands.entity(entity).despawn();
    }
    if let Some(level) = zones.get(zone) {
        spawn_level(&mut commands, zone, level, &mut toon, &mut standard);
    }
}

/// Hide characters and markers that are in another zone.
fn hide_other_zones(
    mut commands: Commands,
    current: Res<CurrentZone>,
    mut things: Query<(Entity, &Zone, &mut Visibility, Has<ElsewhereZone>), With<Transform>>,
) {
    for (entity, zone, mut visibility, marked) in &mut things {
        let elsewhere = current.0.as_ref() != Some(&zone.0);
        if elsewhere != marked {
            if elsewhere {
                commands.entity(entity).insert(ElsewhereZone);
            } else {
                commands.entity(entity).remove::<ElsewhereZone>();
            }
        }
        let wanted = if elsewhere {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

fn spawn_level(
    commands: &mut Commands,
    id: &str,
    level: &Level,
    toon: &mut ToonAssets,
    standard: &mut Assets<StandardMaterial>,
) {
    let root = commands
        .spawn((
            Name::new(level.name.clone()),
            ZoneScenery(id.to_owned()),
            Transform::default(),
            Visibility::default(),
        ))
        .id();

    // Ground.
    let ground = toon.ground(ground_color(&level.ground));
    let size = level.half_size * 2.0;
    toon.spawn_part(
        commands,
        root,
        Plane3d::default().mesh().size(size, size),
        ground,
        Outline::None,
        Transform::default(),
    );
    if !level.border.is_empty() {
        spawn_border(commands, toon, root, level);
    }
    if !level.ambience.is_empty() {
        commands.spawn((
            LastingParticles(level.ambience.clone(), 0.3),
            Transform::default(),
            Visibility::default(),
            ChildOf(root),
        ));
    }

    for obstacle in &level.obstacles {
        spawn_obstacle(commands, toon, root, obstacle);
    }
    for portal in &level.portals {
        spawn_portal(commands, toon, standard, root, portal);
    }
}

/// Colour of each `ground` look used in zone data.
fn ground_color(key: &str) -> Color {
    match key {
        "moss" => Color::srgb(0.30, 0.50, 0.32),
        "stone" => Color::srgb(0.62, 0.60, 0.58),
        _ => Color::srgb(0.45, 0.72, 0.36),
    }
}

/// A portal: a glowing ring on the ground with a soft column of light.
fn spawn_portal(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    standard: &mut Assets<StandardMaterial>,
    root: Entity,
    portal: &Portal,
) {
    let glow = toon.glowing(Color::srgb(0.5, 0.9, 1.0), LinearRgba::rgb(0.8, 2.6, 3.4));
    let base = commands
        .spawn((
            Transform::from_translation(portal.position),
            Visibility::default(),
            LastingParticles("portal".to_owned(), 0.1),
            ChildOf(root),
        ))
        .id();
    toon.spawn_part(
        commands,
        base,
        Torus::new(portal.radius - 0.12, portal.radius)
            .mesh()
            .minor_resolution(8)
            .major_resolution(48),
        glow.clone(),
        Outline::None,
        Transform::from_xyz(0.0, 0.05, 0.0),
    );
    let column = standard.add(StandardMaterial {
        base_color: Color::srgba(0.55, 0.9, 1.0, 0.18),
        emissive: LinearRgba::rgb(0.3, 0.9, 1.2),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let column_mesh = toon.meshes.add(Cylinder::new(portal.radius * 0.9, 3.5));
    commands.spawn((
        Mesh3d(column_mesh),
        MeshMaterial3d(column),
        Transform::from_xyz(0.0, 1.75, 0.0),
        NotShadowCaster,
        ChildOf(base),
    ));
    // Floating runes that circle slowly.
    let spinner = commands
        .spawn((
            PortalSpin,
            Transform::from_xyz(0.0, 1.6, 0.0),
            Visibility::default(),
            ChildOf(base),
        ))
        .id();
    for i in 0..3 {
        let angle = i as f32 * std::f32::consts::TAU / 3.0;
        toon.spawn_part(
            commands,
            spinner,
            Sphere::new(0.14).mesh().uv(12, 8),
            glow.clone(),
            Outline::None,
            Transform::from_xyz(
                angle.cos() * portal.radius * 0.7,
                0.0,
                angle.sin() * portal.radius * 0.7,
            ),
        );
    }
}

/// Start never-ending particles (portals, zone ambience). They are
/// children, so they go away with the scenery.
fn add_lasting_particles(
    new: Query<(Entity, &LastingParticles), Added<LastingParticles>>,
    mut looks: Looks,
) {
    for (entity, lasting) in &new {
        looks.attach_lasting_particles(&lasting.0, Vec3::Y * lasting.1, entity);
    }
}

/// A repeatable "random" number from 0 to 1 for the `i`th piece of scenery,
/// so borders look natural but the same every time.
fn scatter(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x % 10_000) as f32 / 10_000.0
}

/// Dressing around the edge of a zone so it doesn't end in sky.
fn spawn_border(commands: &mut Commands, toon: &mut ToonAssets, root: Entity, level: &Level) {
    match level.border.as_str() {
        "roots" => spawn_root_border(commands, toon, root, level.half_size),
        other => warn!("no look for border `{other}`"),
    }
}

/// Points along the four edges of a square of half-size `h`, `spacing`
/// apart, with the direction pointing out of the square.
fn around_square(h: f32, spacing: f32) -> Vec<(Vec3, Vec3)> {
    let steps = ((h * 2.0) / spacing).ceil() as i32;
    let mut points = Vec::new();
    for (outward, along) in [
        (Vec3::X, Vec3::Z),
        (Vec3::NEG_X, Vec3::Z),
        (Vec3::Z, Vec3::X),
        (Vec3::NEG_Z, Vec3::X),
    ] {
        for step in 0..=steps {
            let t = -h + step as f32 * (h * 2.0 / steps as f32);
            points.push((outward * h + along * t, outward));
        }
    }
    points
}

/// The Rootwarden's Hollow: a wall of tangled roots, giant tree trunks
/// behind it, a canopy overhead, and glowing mushrooms.
fn spawn_root_border(commands: &mut Commands, toon: &mut ToonAssets, root: Entity, h: f32) {
    let bark = toon.material(Color::srgb(0.42, 0.30, 0.21));
    let dark_bark = toon.material(Color::srgb(0.30, 0.22, 0.16));
    let canopy = toon.material(Color::srgb(0.20, 0.42, 0.24));
    let far_ground = toon.ground(Color::srgb(0.22, 0.36, 0.22));
    let stem = toon.material(Color::srgb(0.90, 0.88, 0.78));
    let caps = [
        toon.glowing(Color::srgb(0.4, 0.9, 1.0), LinearRgba::rgb(0.3, 1.6, 2.2)),
        toon.glowing(Color::srgb(0.8, 0.5, 1.0), LinearRgba::rgb(1.4, 0.6, 2.2)),
    ];

    // Ground beyond the edge, a little lower so it doesn't flicker.
    toon.spawn_part(
        commands,
        root,
        Plane3d::default().mesh().size(h * 8.0, h * 8.0),
        far_ground,
        Outline::None,
        Transform::from_xyz(0.0, -0.02, 0.0),
    );

    // A wall of thick roots just outside the edge.
    for (i, (point, outward)) in around_square(h + 1.2, 3.5).into_iter().enumerate() {
        let i = i as u32;
        let along = Vec3::new(outward.z, 0.0, -outward.x);
        let radius = 0.6 + 0.6 * scatter(i, 1);
        let length = 3.0 + 2.0 * scatter(i, 2);
        let tilt = (scatter(i, 3) - 0.5) * 0.5;
        let lean = Quat::from_axis_angle(outward, tilt);
        toon.spawn_part(
            commands,
            root,
            Capsule3d::new(radius, length),
            if i.is_multiple_of(3) {
                dark_bark.clone()
            } else {
                bark.clone()
            },
            Outline::Smooth,
            Transform::from_translation(point + outward * scatter(i, 4) + Vec3::Y * radius * 0.7)
                .with_rotation(lean * Quat::from_rotation_arc(Vec3::Y, along)),
        );
        // Some roots arch up out of the ground.
        if i % 4 == 1 {
            toon.spawn_part(
                commands,
                root,
                Capsule3d::new(radius * 0.6, 3.5),
                bark.clone(),
                Outline::Smooth,
                Transform::from_translation(point + outward * 1.5 + Vec3::Y * 1.6)
                    .with_rotation(Quat::from_axis_angle(along, -0.5)),
            );
        }
        // Glowing mushrooms at the foot of the wall.
        if i % 3 == 2 {
            let at = point - outward * 1.4 + along * (scatter(i, 5) - 0.5) * 2.0;
            for (k, (offset, size)) in [(Vec3::ZERO, 0.35), (Vec3::new(0.45, 0.0, 0.25), 0.22)]
                .into_iter()
                .enumerate()
            {
                let height = size * 1.4;
                let mushroom = toon.spawn_part(
                    commands,
                    root,
                    Cylinder::new(size * 0.25, height),
                    stem.clone(),
                    Outline::Cylinder,
                    Transform::from_translation(at + offset + Vec3::Y * height / 2.0),
                );
                toon.spawn_part(
                    commands,
                    mushroom,
                    Sphere::new(size).mesh().uv(16, 8),
                    caps[(i as usize + k) % 2].clone(),
                    Outline::Smooth,
                    Transform::from_xyz(0.0, height / 2.0, 0.0)
                        .with_scale(Vec3::new(1.0, 0.5, 1.0)),
                );
            }
        }
    }

    // Giant trunks behind the wall, with a canopy that closes overhead.
    // Two rings of them, so there are no gaps.
    let near = around_square(h + 7.0, 9.0);
    let far = around_square(h + 15.0, 11.0);
    for (i, (point, outward)) in near.into_iter().chain(far).enumerate() {
        let i = i as u32;
        let radius = 2.2 + 1.6 * scatter(i, 6);
        let height = 26.0 + 8.0 * scatter(i, 7);
        let at = point + outward * 3.0 * scatter(i, 8);
        let trunk = toon.spawn_part(
            commands,
            root,
            Cylinder::new(radius, height),
            if i.is_multiple_of(2) {
                bark.clone()
            } else {
                dark_bark.clone()
            },
            Outline::Cylinder,
            Transform::from_translation(at + Vec3::Y * height / 2.0),
        );
        // Far-off trees don't shade the arena.
        commands.entity(trunk).insert(NotShadowCaster);
        let leaves = toon.spawn_part(
            commands,
            trunk,
            Sphere::new(radius * 3.2).mesh().uv(24, 14),
            canopy.clone(),
            Outline::Smooth,
            Transform::from_translation(Vec3::Y * height * 0.5 - outward * radius * 1.5)
                .with_scale(Vec3::new(1.4, 0.6, 1.4)),
        );
        commands.entity(leaves).insert(NotShadowCaster);
    }
}

fn animate_portals(time: Res<Time>, mut spinners: Query<&mut Transform, With<PortalSpin>>) {
    for mut transform in &mut spinners {
        transform.rotation = Quat::from_rotation_y(time.elapsed_secs() * 0.8);
        transform.translation.y = 1.6 + 0.25 * (time.elapsed_secs() * 1.3).sin();
    }
}

/// Placeholder looks for each `visual` name used in zone data.
fn spawn_obstacle(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    root: Entity,
    obstacle: &Obstacle,
) {
    let base = Transform::from_translation(obstacle.position);
    match (obstacle.visual.as_str(), obstacle.shape) {
        ("tree", Shape::Cylinder { radius, height }) => {
            let bark = toon.material(Color::srgb(0.45, 0.30, 0.20));
            let leaves = toon.material(Color::srgb(0.25, 0.58, 0.30));
            let trunk = toon.spawn_part(
                commands,
                root,
                Cylinder::new(radius, height),
                bark,
                Outline::Cylinder,
                base.with_translation(obstacle.position + Vec3::Y * height / 2.0),
            );
            // Canopy: a few overlapping balls (decoration only, not solid).
            for (offset, size) in [
                (Vec3::new(0.0, height * 0.75, 0.0), 2.0),
                (Vec3::new(1.0, height * 0.45, 0.4), 1.4),
                (Vec3::new(-0.8, height * 0.5, -0.6), 1.5),
            ] {
                toon.spawn_part(
                    commands,
                    trunk,
                    Sphere::new(size).mesh().uv(32, 18),
                    leaves.clone(),
                    Outline::Smooth,
                    Transform::from_translation(offset),
                );
            }
        }
        ("pillar", Shape::Cylinder { radius, height }) => {
            let metal = toon.material(Color::srgb(0.32, 0.30, 0.40));
            let crystal = toon.glowing(Color::srgb(0.5, 0.9, 1.0), LinearRgba::rgb(0.6, 2.4, 3.2));
            let pillar = toon.spawn_part(
                commands,
                root,
                Cylinder::new(radius, height),
                metal,
                Outline::Cylinder,
                base.with_translation(obstacle.position + Vec3::Y * height / 2.0),
            );
            toon.spawn_part(
                commands,
                pillar,
                Sphere::new(radius * 0.7).mesh().uv(24, 16),
                crystal,
                Outline::Smooth,
                Transform::from_xyz(0.0, height / 2.0 + radius * 0.9, 0.0),
            );
        }
        (
            visual,
            Shape::Box {
                half_x,
                half_z,
                height,
            },
        ) => {
            let color = match visual {
                "crate" => Color::srgb(0.70, 0.50, 0.30),
                "stone" => Color::srgb(0.78, 0.74, 0.66),
                "wall" => Color::srgb(0.60, 0.58, 0.68),
                _ => Color::srgb(1.0, 0.0, 1.0),
            };
            let material = toon.material(color);
            toon.spawn_part(
                commands,
                root,
                Cuboid::new(half_x * 2.0, height, half_z * 2.0),
                material,
                Outline::Box,
                base.with_translation(obstacle.position + Vec3::Y * height / 2.0),
            );
        }
        ("standing_stone", Shape::Cylinder { radius, height }) => {
            let stone = toon.material(Color::srgb(0.55, 0.56, 0.52));
            let moss = toon.material(Color::srgb(0.32, 0.55, 0.30));
            let rune = toon.glowing(Color::srgb(0.6, 1.0, 0.6), LinearRgba::rgb(0.6, 2.2, 0.8));
            let pillar = toon.spawn_part(
                commands,
                root,
                Cylinder::new(radius, height),
                stone,
                Outline::Cylinder,
                base.with_translation(obstacle.position + Vec3::Y * height / 2.0),
            );
            toon.spawn_part(
                commands,
                pillar,
                Cylinder::new(radius * 1.05, height * 0.2),
                moss,
                Outline::Cylinder,
                Transform::from_xyz(0.0, -height * 0.4, 0.0),
            );
            toon.spawn_part(
                commands,
                pillar,
                Sphere::new(radius * 0.35).mesh().uv(16, 10),
                rune,
                Outline::None,
                Transform::from_xyz(0.0, height * 0.15, -radius * 0.9),
            );
        }
        (_, Shape::Cylinder { radius, height }) => {
            let material = toon.material(Color::srgb(1.0, 0.0, 1.0));
            toon.spawn_part(
                commands,
                root,
                Cylinder::new(radius, height),
                material,
                Outline::Cylinder,
                base.with_translation(obstacle.position + Vec3::Y * height / 2.0),
            );
        }
    }
}
