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
