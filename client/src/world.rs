//! Builds the visible level from the shared level data: ground, sky,
//! sunlight and placeholder props for every obstacle.

use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use shared::level::{Level, Obstacle, Shape};

use crate::toon::{Outline, ToonAssets};

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.55, 0.78, 0.95)))
            .add_systems(Startup, (spawn_lighting, spawn_level));
    }
}

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

fn spawn_level(mut commands: Commands, level: Res<Level>, mut toon: ToonAssets) {
    let root = commands
        .spawn((
            Name::new(level.name.clone()),
            Transform::default(),
            Visibility::default(),
        ))
        .id();

    // Ground.
    let grass = toon.ground(Color::srgb(0.45, 0.72, 0.36));
    let size = level.half_size * 2.0;
    toon.spawn_part(
        &mut commands,
        root,
        Plane3d::default().mesh().size(size, size),
        grass,
        Outline::None,
        Transform::default(),
    );

    for obstacle in &level.obstacles {
        spawn_obstacle(&mut commands, &mut toon, root, obstacle);
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
