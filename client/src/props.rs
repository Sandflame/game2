//! Placeholder scenery for the hub city, the giant root and the forest:
//! buildings, towers, the fountain, trees, rocks, lamps, campfires, the city
//! wall, the root tunnel and district gates. All built from simple shapes
//! with the toon look; zone data picks them by `visual` name.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use shared::level::Level;
use shared::rides::RideDef;

use crate::toon::{Outline, ToonAssets};

/// A repeatable "random" number from 0 to 1, so scenery looks natural but
/// the same every time.
pub fn scatter(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x % 10_000) as f32 / 10_000.0
}

/// An empty parent placed in the world, for building a prop out of parts.
fn anchor(commands: &mut Commands, root: Entity, transform: Transform) -> Entity {
    commands
        .spawn((transform, Visibility::default(), ChildOf(root)))
        .id()
}

const WARM_GLOW: LinearRgba = LinearRgba::rgb(3.0, 1.9, 0.6);
const CRYSTAL_GLOW: LinearRgba = LinearRgba::rgb(0.6, 2.2, 3.0);

/// Solid scenery from zone obstacles. Returns false for unknown looks.
pub fn obstacle(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    root: Entity,
    visual: &str,
    position: Vec3,
    size: Vec3,
) -> bool {
    // `size` is (width, height, depth) for boxes and (radius, height, radius)
    // for cylinders.
    let base = Transform::from_translation(position);
    match visual {
        "house" => house(commands, toon, root, base, size),
        "tower" => tower(commands, toon, root, base, size.x, size.y),
        "fountain" => fountain(commands, toon, root, base, size.x, size.y),
        "giant_root" => giant_root(commands, toon, root, base, size.x, size.y),
        "ancient_tree" => ancient_tree(commands, toon, root, base, size.x, size.y),
        "pine" => pine(commands, toon, root, base, size.x, size.y),
        "rock" => rock(commands, toon, root, base, size),
        _ => return false,
    }
    true
}

/// Scenery without collision (zone `decorations`). Returns false for
/// unknown looks.
pub fn decoration(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    root: Entity,
    visual: &str,
    transform: Transform,
) -> bool {
    let at = anchor(commands, root, transform);
    match visual {
        "lamp_post" => lamp_post(commands, toon, at),
        "root_arch" => root_arch(commands, toon, at),
        "bush" => bush(commands, toon, at),
        "banner" => banner(commands, toon, at),
        "fern" => fern(commands, toon, at),
        "mushrooms" => mushrooms(commands, toon, at),
        "flowers" => flowers(commands, toon, at),
        "campfire" => campfire(commands, toon, at),
        _ => {
            commands.entity(at).despawn();
            return false;
        }
    }
    true
}

fn house(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    root: Entity,
    base: Transform,
    size: Vec3,
) {
    let walls = toon.material(Color::srgb(0.92, 0.86, 0.74));
    let timber = toon.material(Color::srgb(0.45, 0.32, 0.24));
    let roof_color = if base.translation.x > 0.0 {
        Color::srgb(0.30, 0.40, 0.62)
    } else {
        Color::srgb(0.70, 0.36, 0.26)
    };
    let roof = toon.material(roof_color);
    let window = toon.glowing(Color::srgb(1.0, 0.85, 0.5), WARM_GLOW * 0.6);
    let at = anchor(commands, root, base);
    let (w, h, d) = (size.x, size.y, size.z);
    toon.spawn_part(
        commands,
        at,
        Cuboid::new(w, h, d),
        walls,
        Outline::Box,
        Transform::from_xyz(0.0, h / 2.0, 0.0),
    );
    // Timber frame along the bottom and top.
    for y in [0.15, h - 0.15] {
        toon.spawn_part(
            commands,
            at,
            Cuboid::new(w + 0.1, 0.3, d + 0.1),
            timber.clone(),
            Outline::Box,
            Transform::from_xyz(0.0, y, 0.0),
        );
    }
    // A gable roof (a triangle stretched along the house).
    let peak = (w.min(d) * 0.45).max(1.2);
    let roof_mesh = Extrusion::new(
        Triangle2d::new(
            Vec2::new(-w / 2.0 - 0.4, 0.0),
            Vec2::new(w / 2.0 + 0.4, 0.0),
            Vec2::new(0.0, peak),
        ),
        d + 0.6,
    );
    toon.spawn_part(
        commands,
        at,
        roof_mesh,
        roof,
        Outline::None,
        Transform::from_xyz(0.0, h, 0.0),
    );
    // Glowing windows on the side facing the street (towards x = 0).
    let street = if base.translation.x > 0.0 { -1.0 } else { 1.0 };
    for z in [-d / 4.0, d / 4.0] {
        toon.spawn_part(
            commands,
            at,
            Cuboid::new(0.1, 0.9, 0.7),
            window.clone(),
            Outline::None,
            Transform::from_xyz(street * (w / 2.0 + 0.02), h * 0.6, z),
        );
    }
    toon.spawn_part(
        commands,
        at,
        Cuboid::new(0.12, 1.8, 1.0),
        timber,
        Outline::None,
        Transform::from_xyz(street * (w / 2.0 + 0.03), 0.9, 0.0),
    );
}

fn tower(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    root: Entity,
    base: Transform,
    radius: f32,
    height: f32,
) {
    let stone = toon.material(Color::srgb(0.42, 0.40, 0.52));
    let brass = toon.material(Color::srgb(0.82, 0.64, 0.30));
    let crystal = toon.glowing(Color::srgb(0.5, 0.9, 1.0), CRYSTAL_GLOW);
    let at = anchor(commands, root, base);
    toon.spawn_part(
        commands,
        at,
        Cylinder::new(radius, height),
        stone,
        Outline::Cylinder,
        Transform::from_xyz(0.0, height / 2.0, 0.0),
    );
    for y in [height * 0.3, height * 0.65, height] {
        toon.spawn_part(
            commands,
            at,
            Cylinder::new(radius * 1.15, 0.35),
            brass.clone(),
            Outline::Cylinder,
            Transform::from_xyz(0.0, y, 0.0),
        );
    }
    toon.spawn_part(
        commands,
        at,
        Sphere::new(radius * 0.8).mesh().uv(16, 10),
        crystal.clone(),
        Outline::Smooth,
        Transform::from_xyz(0.0, height + radius * 1.2, 0.0).with_scale(Vec3::new(0.7, 1.4, 0.7)),
    );
    // A floating ring around the crystal.
    toon.spawn_part(
        commands,
        at,
        Torus::new(radius * 1.2, radius * 1.4),
        crystal,
        Outline::None,
        Transform::from_xyz(0.0, height + radius * 1.2, 0.0),
    );
}

fn fountain(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    root: Entity,
    base: Transform,
    radius: f32,
    height: f32,
) {
    let stone = toon.material(Color::srgb(0.78, 0.76, 0.72));
    let water = toon.glowing(
        Color::srgb(0.45, 0.75, 0.95),
        LinearRgba::rgb(0.15, 0.45, 0.7),
    );
    let brass = toon.material(Color::srgb(0.82, 0.64, 0.30));
    let flame = toon.glowing(Color::srgb(1.0, 0.75, 0.35), WARM_GLOW * 1.6);
    let at = anchor(commands, root, base);
    toon.spawn_part(
        commands,
        at,
        Cylinder::new(radius, height),
        stone.clone(),
        Outline::Cylinder,
        Transform::from_xyz(0.0, height / 2.0, 0.0),
    );
    toon.spawn_part(
        commands,
        at,
        Cylinder::new(radius * 0.88, 0.05),
        water,
        Outline::None,
        Transform::from_xyz(0.0, height + 0.01, 0.0),
    );
    // The great lantern on its pillar.
    toon.spawn_part(
        commands,
        at,
        Cylinder::new(0.35, 3.0),
        stone,
        Outline::Cylinder,
        Transform::from_xyz(0.0, height + 1.5, 0.0),
    );
    for y in [height + 3.1, height + 4.7] {
        toon.spawn_part(
            commands,
            at,
            Cylinder::new(0.75, 0.2),
            brass.clone(),
            Outline::Cylinder,
            Transform::from_xyz(0.0, y, 0.0),
        );
    }
    for i in 0..4 {
        let angle = i as f32 * TAU / 4.0;
        toon.spawn_part(
            commands,
            at,
            Cylinder::new(0.06, 1.6),
            brass.clone(),
            Outline::None,
            Transform::from_xyz(angle.cos() * 0.6, height + 3.9, angle.sin() * 0.6),
        );
    }
    toon.spawn_part(
        commands,
        at,
        Sphere::new(0.45).mesh().uv(16, 10),
        flame,
        Outline::None,
        Transform::from_xyz(0.0, height + 3.9, 0.0),
    );
}

/// A thick, twisting root rising out of sight, built from leaning capsules.
fn giant_root(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    root: Entity,
    base: Transform,
    radius: f32,
    height: f32,
) {
    let bark = toon.material(Color::srgb(0.44, 0.32, 0.23));
    let dark = toon.material(Color::srgb(0.32, 0.23, 0.17));
    let moss = toon.material(Color::srgb(0.32, 0.55, 0.30));
    let at = anchor(commands, root, base);
    // Strands twisting around each other.
    for i in 0..5 {
        let angle = i as f32 * TAU / 5.0;
        let lean = 0.12 * if i % 2 == 0 { 1.0 } else { -1.0 };
        let out = Vec3::new(angle.cos(), 0.0, angle.sin()) * radius * 0.45;
        toon.spawn_part(
            commands,
            at,
            Capsule3d::new(radius * 0.55, height),
            if i % 2 == 0 {
                bark.clone()
            } else {
                dark.clone()
            },
            Outline::Smooth,
            Transform::from_translation(out + Vec3::Y * height / 2.0)
                .with_rotation(Quat::from_rotation_z(lean) * Quat::from_rotation_x(-lean)),
        );
    }
    // Roots spreading over the ground.
    for i in 0..8 {
        let angle = i as f32 * TAU / 8.0 + 0.2;
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        toon.spawn_part(
            commands,
            at,
            Capsule3d::new(radius * 0.18, radius * 1.1),
            dark.clone(),
            Outline::Smooth,
            Transform::from_translation(out * radius * 1.05 + Vec3::Y * 0.6).with_rotation(
                Quat::from_rotation_arc(Vec3::Y, (out + Vec3::Y * 0.3).normalize()),
            ),
        );
    }
    // Moss patches.
    for i in 0..6 {
        let angle = i as f32 * 1.3;
        toon.spawn_part(
            commands,
            at,
            Sphere::new(radius * 0.25).mesh().uv(12, 8),
            moss.clone(),
            Outline::Smooth,
            Transform::from_xyz(
                angle.cos() * radius * 0.9,
                3.0 + i as f32 * 4.5,
                angle.sin() * radius * 0.9,
            )
            .with_scale(Vec3::new(1.0, 0.6, 1.0)),
        );
    }
}

fn ancient_tree(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    root: Entity,
    base: Transform,
    radius: f32,
    height: f32,
) {
    giant_root(commands, toon, root, base, radius, height * 0.7);
    let leaves = toon.material(Color::srgb(0.24, 0.50, 0.28));
    let at = anchor(commands, root, base);
    for (offset, size) in [
        (Vec3::new(0.0, height * 0.75, 0.0), radius * 2.6),
        (
            Vec3::new(radius * 1.6, height * 0.65, radius * 0.5),
            radius * 1.8,
        ),
        (
            Vec3::new(-radius * 1.5, height * 0.68, -radius * 0.6),
            radius * 1.9,
        ),
        (
            Vec3::new(radius * 0.4, height * 0.62, -radius * 1.6),
            radius * 1.6,
        ),
    ] {
        let part = toon.spawn_part(
            commands,
            at,
            Sphere::new(size).mesh().uv(24, 14),
            leaves.clone(),
            Outline::Smooth,
            Transform::from_translation(offset).with_scale(Vec3::new(1.0, 0.65, 1.0)),
        );
        commands.entity(part).insert(NotShadowCaster);
    }
}

fn pine(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    root: Entity,
    base: Transform,
    radius: f32,
    height: f32,
) {
    let bark = toon.material(Color::srgb(0.40, 0.28, 0.20));
    let needles = toon.material(Color::srgb(0.18, 0.42, 0.30));
    let at = anchor(commands, root, base);
    toon.spawn_part(
        commands,
        at,
        Cylinder::new(radius, height),
        bark,
        Outline::Cylinder,
        Transform::from_xyz(0.0, height / 2.0, 0.0),
    );
    for (i, scale) in [1.0, 0.78, 0.55].into_iter().enumerate() {
        let tiers = radius * 5.0 * scale;
        toon.spawn_part(
            commands,
            at,
            Cone::new(tiers, tiers * 1.4),
            needles.clone(),
            Outline::Smooth,
            Transform::from_xyz(0.0, height * 0.45 + i as f32 * tiers * 0.9, 0.0),
        );
    }
}

fn rock(commands: &mut Commands, toon: &mut ToonAssets, root: Entity, base: Transform, size: Vec3) {
    let stone = toon.material(Color::srgb(0.55, 0.56, 0.54));
    let moss = toon.material(Color::srgb(0.34, 0.52, 0.30));
    let at = anchor(commands, root, base);
    toon.spawn_part(
        commands,
        at,
        Sphere::new(0.5).mesh().uv(14, 9),
        stone,
        Outline::Smooth,
        Transform::from_xyz(0.0, size.y * 0.45, 0.0).with_scale(size * Vec3::new(1.05, 1.0, 1.05)),
    );
    toon.spawn_part(
        commands,
        at,
        Sphere::new(0.5).mesh().uv(12, 8),
        moss,
        Outline::None,
        Transform::from_xyz(0.0, size.y * 0.8, 0.0).with_scale(size * Vec3::new(0.7, 0.35, 0.7)),
    );
}

fn lamp_post(commands: &mut Commands, toon: &mut ToonAssets, at: Entity) {
    let metal = toon.material(Color::srgb(0.20, 0.20, 0.26));
    let brass = toon.material(Color::srgb(0.82, 0.64, 0.30));
    let glow = toon.glowing(Color::srgb(1.0, 0.8, 0.45), WARM_GLOW);
    toon.spawn_part(
        commands,
        at,
        Cylinder::new(0.08, 3.2),
        metal,
        Outline::Cylinder,
        Transform::from_xyz(0.0, 1.6, 0.0),
    );
    toon.spawn_part(
        commands,
        at,
        Sphere::new(0.25).mesh().uv(12, 8),
        glow,
        Outline::None,
        Transform::from_xyz(0.0, 3.35, 0.0),
    );
    toon.spawn_part(
        commands,
        at,
        Cone::new(0.32, 0.25),
        brass,
        Outline::Smooth,
        Transform::from_xyz(0.0, 3.68, 0.0),
    );
}

fn root_arch(commands: &mut Commands, toon: &mut ToonAssets, at: Entity) {
    let bark = toon.material(Color::srgb(0.40, 0.29, 0.21));
    toon.spawn_part(
        commands,
        at,
        Torus::new(2.6, 3.2),
        bark,
        Outline::Smooth,
        Transform::from_rotation(Quat::from_rotation_x(FRAC_PI_2))
            .with_scale(Vec3::new(1.0, 1.0, 1.4)),
    );
}

fn bush(commands: &mut Commands, toon: &mut ToonAssets, at: Entity) {
    let leaves = toon.material(Color::srgb(0.28, 0.55, 0.30));
    for (offset, size) in [
        (Vec3::new(0.0, 0.45, 0.0), 0.6),
        (Vec3::new(0.5, 0.35, 0.2), 0.45),
        (Vec3::new(-0.45, 0.3, -0.15), 0.42),
    ] {
        toon.spawn_part(
            commands,
            at,
            Sphere::new(size).mesh().uv(12, 8),
            leaves.clone(),
            Outline::Smooth,
            Transform::from_translation(offset),
        );
    }
}

fn banner(commands: &mut Commands, toon: &mut ToonAssets, at: Entity) {
    let wood = toon.material(Color::srgb(0.40, 0.29, 0.21));
    let cloth = toon.material(Color::srgb(0.30, 0.55, 0.32));
    let sigil = toon.glowing(Color::srgb(1.0, 0.85, 0.5), WARM_GLOW * 0.8);
    for x in [-1.6, 1.6] {
        toon.spawn_part(
            commands,
            at,
            Cylinder::new(0.1, 5.0),
            wood.clone(),
            Outline::Cylinder,
            Transform::from_xyz(x, 2.5, 0.0),
        );
    }
    toon.spawn_part(
        commands,
        at,
        Cuboid::new(3.0, 1.6, 0.08),
        cloth,
        Outline::Box,
        Transform::from_xyz(0.0, 4.0, 0.0),
    );
    toon.spawn_part(
        commands,
        at,
        Sphere::new(0.35).mesh().uv(12, 8),
        sigil,
        Outline::None,
        Transform::from_xyz(0.0, 4.0, 0.06).with_scale(Vec3::new(1.0, 1.0, 0.3)),
    );
}

fn fern(commands: &mut Commands, toon: &mut ToonAssets, at: Entity) {
    let green = toon.material(Color::srgb(0.30, 0.60, 0.32));
    for i in 0..5 {
        let angle = i as f32 * TAU / 5.0;
        let out = Vec3::new(angle.cos(), 0.6, angle.sin()).normalize();
        toon.spawn_part(
            commands,
            at,
            Cone::new(0.12, 0.9),
            green.clone(),
            Outline::None,
            Transform::from_translation(out * 0.35)
                .with_rotation(Quat::from_rotation_arc(Vec3::Y, out)),
        );
    }
}

fn mushrooms(commands: &mut Commands, toon: &mut ToonAssets, at: Entity) {
    let stem = toon.material(Color::srgb(0.90, 0.88, 0.78));
    let cap = toon.glowing(Color::srgb(0.8, 0.5, 1.0), LinearRgba::rgb(1.2, 0.5, 2.0));
    for (offset, size) in [(Vec3::ZERO, 0.3), (Vec3::new(0.35, 0.0, 0.2), 0.2)] {
        let height = size * 1.4;
        let part = toon.spawn_part(
            commands,
            at,
            Cylinder::new(size * 0.25, height),
            stem.clone(),
            Outline::Cylinder,
            Transform::from_translation(offset + Vec3::Y * height / 2.0),
        );
        toon.spawn_part(
            commands,
            part,
            Sphere::new(size).mesh().uv(14, 8),
            cap.clone(),
            Outline::Smooth,
            Transform::from_xyz(0.0, height / 2.0, 0.0).with_scale(Vec3::new(1.0, 0.5, 1.0)),
        );
    }
}

fn flowers(commands: &mut Commands, toon: &mut ToonAssets, at: Entity) {
    let colors = [
        Color::srgb(1.0, 0.85, 0.3),
        Color::srgb(1.0, 0.55, 0.7),
        Color::srgb(0.95, 0.95, 1.0),
    ];
    for i in 0..6u32 {
        let material = toon.material(colors[i as usize % colors.len()]);
        let x = (scatter(i, 31) - 0.5) * 1.2;
        let z = (scatter(i, 32) - 0.5) * 1.2;
        toon.spawn_part(
            commands,
            at,
            Sphere::new(0.09).mesh().uv(8, 6),
            material,
            Outline::None,
            Transform::from_xyz(x, 0.25, z),
        );
    }
}

fn campfire(commands: &mut Commands, toon: &mut ToonAssets, at: Entity) {
    let stone = toon.material(Color::srgb(0.50, 0.50, 0.50));
    let wood = toon.material(Color::srgb(0.35, 0.24, 0.16));
    let fire = toon.glowing(Color::srgb(1.0, 0.6, 0.2), LinearRgba::rgb(4.0, 1.6, 0.3));
    for i in 0..8 {
        let angle = i as f32 * TAU / 8.0;
        toon.spawn_part(
            commands,
            at,
            Sphere::new(0.2).mesh().uv(10, 6),
            stone.clone(),
            Outline::Smooth,
            Transform::from_xyz(angle.cos() * 0.8, 0.12, angle.sin() * 0.8),
        );
    }
    for i in 0..3 {
        let angle = i as f32 * TAU / 3.0;
        toon.spawn_part(
            commands,
            at,
            Capsule3d::new(0.09, 1.0),
            wood.clone(),
            Outline::Smooth,
            Transform::from_xyz(0.0, 0.25, 0.0)
                .with_rotation(Quat::from_rotation_y(angle) * Quat::from_rotation_z(1.2)),
        );
    }
    toon.spawn_part(
        commands,
        at,
        Cone::new(0.35, 0.9),
        fire,
        Outline::None,
        Transform::from_xyz(0.0, 0.55, 0.0),
    );
}

/// The city wall around Lanternhold, with towers, and far-off spires beyond.
pub fn city_wall(commands: &mut Commands, toon: &mut ToonAssets, root: Entity, h: f32) {
    let stone = toon.material(Color::srgb(0.66, 0.63, 0.60));
    let dark = toon.material(Color::srgb(0.48, 0.46, 0.50));
    let far_ground = toon.ground(Color::srgb(0.50, 0.62, 0.44));
    toon.spawn_part(
        commands,
        root,
        Plane3d::default().mesh().size(h * 8.0, h * 8.0),
        far_ground,
        Outline::None,
        Transform::from_xyz(0.0, -0.02, 0.0),
    );
    let wall = h + 2.0;
    let height = 5.0;
    for (centre, along) in [
        (Vec3::new(0.0, 0.0, -wall), Vec3::X),
        (Vec3::new(0.0, 0.0, wall), Vec3::X),
        (Vec3::new(-wall, 0.0, 0.0), Vec3::Z),
        (Vec3::new(wall, 0.0, 0.0), Vec3::Z),
    ] {
        let size = along * (wall * 2.0) + (Vec3::ONE - along) * 1.6;
        toon.spawn_part(
            commands,
            root,
            Cuboid::new(size.x, height, size.z),
            stone.clone(),
            Outline::Box,
            Transform::from_translation(centre + Vec3::Y * height / 2.0),
        );
        // Battlements.
        let mut t = -wall;
        let mut i = 0;
        while t <= wall {
            if i % 2 == 0 {
                toon.spawn_part(
                    commands,
                    root,
                    Cuboid::new(1.2, 0.8, 1.2),
                    dark.clone(),
                    Outline::Box,
                    Transform::from_translation(centre + along * t + Vec3::Y * (height + 0.4)),
                );
            }
            t += 2.0;
            i += 1;
        }
    }
    for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let base = Transform::from_xyz(x * wall, 0.0, z * wall);
        tower(commands, toon, root, base, 2.5, 10.0);
    }
    // Spires of the rest of the city, far beyond the wall.
    for i in 0..14u32 {
        let angle = i as f32 * TAU / 14.0 + 0.2;
        let distance = h * 1.6 + scatter(i, 41) * h * 0.6;
        let base = Transform::from_xyz(angle.cos() * distance, 0.0, angle.sin() * distance);
        tower(
            commands,
            toon,
            root,
            base,
            2.0 + scatter(i, 42) * 2.0,
            16.0 + scatter(i, 43) * 20.0,
        );
    }
}

/// The inside of the giant root: rings of bark along the ride's path,
/// with glowing moss between them.
pub fn root_tunnel(commands: &mut Commands, toon: &mut ToonAssets, root: Entity, ride: &RideDef) {
    let bark = toon.material(Color::srgb(0.36, 0.25, 0.18));
    let dark = toon.material(Color::srgb(0.26, 0.18, 0.13));
    let glow = toon.glowing(Color::srgb(0.4, 0.8, 0.5), LinearRgba::rgb(0.3, 1.2, 0.5));
    const RINGS: u32 = 70;
    for i in 0..=RINGS {
        let t = i as f32 / RINGS as f32;
        let (centre, yaw) = ride.at(t);
        let ahead = ride.at((t + 0.01).min(1.0)).0;
        let behind = ride.at((t - 0.01).max(0.0)).0;
        let forward = (ahead - behind).normalize_or(Vec3::NEG_Z);
        let facing = Quat::from_rotation_arc(Vec3::Y, forward);
        // Centre the tunnel around the rider's body.
        let middle = centre + Vec3::Y * 1.2;
        toon.spawn_part(
            commands,
            root,
            Torus::new(3.2, 4.4),
            if i % 2 == 0 {
                bark.clone()
            } else {
                dark.clone()
            },
            Outline::None,
            Transform::from_translation(middle).with_rotation(facing),
        );
        if i % 3 == 0 {
            let around = scatter(i, 51) * TAU;
            let side = Quat::from_rotation_y(yaw) * Vec3::new(around.cos(), around.sin(), 0.0);
            toon.spawn_part(
                commands,
                root,
                Sphere::new(0.18).mesh().uv(10, 6),
                glow.clone(),
                Outline::None,
                Transform::from_translation(middle + side * 3.25),
            );
        }
    }
}

/// Doorways: the root's entrance and closed district gates. Returns false
/// for the default glowing ring.
pub fn portal(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    root: Entity,
    visual: &str,
    position: Vec3,
) -> bool {
    match visual {
        "root_entrance" => {
            let hole = toon.material(Color::srgb(0.05, 0.03, 0.02));
            let rim = toon.glowing(Color::srgb(0.5, 1.0, 0.6), LinearRgba::rgb(0.6, 2.4, 1.0));
            let at = anchor(commands, root, Transform::from_translation(position));
            toon.spawn_part(
                commands,
                at,
                Cylinder::new(2.2, 0.1),
                hole,
                Outline::None,
                Transform::from_xyz(0.0, 2.2, -0.4)
                    .with_rotation(Quat::from_rotation_x(FRAC_PI_2))
                    .with_scale(Vec3::new(1.0, 1.0, 1.25)),
            );
            toon.spawn_part(
                commands,
                at,
                Torus::new(2.1, 2.4),
                rim,
                Outline::None,
                Transform::from_xyz(0.0, 2.2, -0.3)
                    .with_rotation(Quat::from_rotation_x(FRAC_PI_2))
                    .with_scale(Vec3::new(1.0, 1.0, 1.25)),
            );
            true
        }
        "district_gate" => {
            let stone = toon.material(Color::srgb(0.70, 0.67, 0.62));
            let door = toon.material(Color::srgb(0.38, 0.27, 0.20));
            let crystal = toon.glowing(Color::srgb(0.9, 0.5, 0.4), LinearRgba::rgb(2.4, 0.8, 0.5));
            // Face the plaza (the gates sit on the city's edge).
            let facing =
                Quat::from_rotation_arc(Vec3::NEG_Z, (-position).with_y(0.0).normalize_or(Vec3::Z));
            let at = anchor(
                commands,
                root,
                Transform::from_translation(position).with_rotation(facing),
            );
            for x in [-2.6, 2.6] {
                toon.spawn_part(
                    commands,
                    at,
                    Cuboid::new(1.0, 6.0, 1.0),
                    stone.clone(),
                    Outline::Box,
                    Transform::from_xyz(x, 3.0, 1.2),
                );
            }
            toon.spawn_part(
                commands,
                at,
                Cuboid::new(6.4, 1.0, 1.2),
                stone,
                Outline::Box,
                Transform::from_xyz(0.0, 6.4, 1.2),
            );
            toon.spawn_part(
                commands,
                at,
                Cuboid::new(4.2, 5.6, 0.3),
                door,
                Outline::Box,
                Transform::from_xyz(0.0, 2.8, 1.4),
            );
            toon.spawn_part(
                commands,
                at,
                Sphere::new(0.4).mesh().uv(12, 8),
                crystal,
                Outline::None,
                Transform::from_xyz(0.0, 7.3, 1.2),
            );
            true
        }
        _ => false,
    }
}

/// The edge dressing named by a zone's `border`, when it is one of these.
pub fn border(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    root: Entity,
    level: &Level,
    zone: &str,
    rides: &std::collections::HashMap<String, RideDef>,
) -> bool {
    match level.border.as_str() {
        "city_wall" => city_wall(commands, toon, root, level.half_size),
        "root_tunnel" => {
            for ride in rides.values().filter(|r| r.zone == zone) {
                root_tunnel(commands, toon, root, ride);
            }
        }
        _ => return false,
    }
    true
}
