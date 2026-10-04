//! Placeholder bodies for Whisperwood's creatures and Lanternhold's
//! townsfolk, built from simple shapes. Everything faces -Z.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use crate::animation::Hop;
use crate::toon::{Outline, ToonAssets};

/// A wolf with a ridge of thorns down its back.
pub fn thornwolf(commands: &mut Commands, toon: &mut ToonAssets, wolf: Entity) {
    let fur = toon.material(Color::srgb(0.36, 0.38, 0.34));
    let belly = toon.material(Color::srgb(0.55, 0.55, 0.48));
    let thorn = toon.material(Color::srgb(0.48, 0.62, 0.30));
    let eyes = toon.glowing(Color::srgb(1.0, 0.5, 0.3), LinearRgba::rgb(3.0, 0.8, 0.3));
    // Body lying along the Z axis.
    let body = toon.spawn_part(
        commands,
        wolf,
        Capsule3d::new(0.38, 1.0),
        fur.clone(),
        Outline::Smooth,
        Transform::from_xyz(0.0, 0.85, 0.0).with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
    );
    toon.spawn_part(
        commands,
        body,
        Capsule3d::new(0.3, 0.6),
        belly,
        Outline::None,
        Transform::from_xyz(0.0, 0.0, 0.12),
    );
    // Head and snout.
    let head = toon.spawn_part(
        commands,
        wolf,
        Sphere::new(0.32).mesh().uv(16, 10),
        fur.clone(),
        Outline::Smooth,
        Transform::from_xyz(0.0, 1.15, -0.95),
    );
    toon.spawn_part(
        commands,
        head,
        Cone::new(0.18, 0.45),
        fur.clone(),
        Outline::Smooth,
        Transform::from_xyz(0.0, -0.06, -0.3).with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
    );
    for side in [-1.0, 1.0] {
        toon.spawn_part(
            commands,
            head,
            Cone::new(0.1, 0.28),
            fur.clone(),
            Outline::Smooth,
            Transform::from_xyz(0.16 * side, 0.3, 0.02),
        );
        toon.spawn_part(
            commands,
            head,
            Sphere::new(0.05).mesh().uv(8, 6),
            eyes.clone(),
            Outline::None,
            Transform::from_xyz(0.13 * side, 0.06, -0.26),
        );
    }
    // Legs.
    for (x, z) in [(-0.25, -0.5), (0.25, -0.5), (-0.25, 0.5), (0.25, 0.5)] {
        toon.spawn_part(
            commands,
            wolf,
            Capsule3d::new(0.1, 0.5),
            fur.clone(),
            Outline::Smooth,
            Transform::from_xyz(x, 0.35, z),
        );
    }
    // Tail.
    toon.spawn_part(
        commands,
        wolf,
        Capsule3d::new(0.09, 0.6),
        fur,
        Outline::Smooth,
        Transform::from_xyz(0.0, 1.0, 0.95).with_rotation(Quat::from_rotation_x(1.0)),
    );
    // Thorns along the back.
    for i in 0..5 {
        let z = -0.5 + i as f32 * 0.25;
        toon.spawn_part(
            commands,
            wolf,
            Cone::new(0.07, 0.3),
            thorn.clone(),
            Outline::Smooth,
            Transform::from_xyz(0.0, 1.3, z).with_rotation(Quat::from_rotation_x(0.3)),
        );
    }
}

/// A walking mushroom with a big spotted cap.
pub fn spore_cap(commands: &mut Commands, toon: &mut ToonAssets, cap_entity: Entity) {
    let stem = toon.material(Color::srgb(0.92, 0.88, 0.76));
    let cap = toon.material(Color::srgb(0.62, 0.32, 0.70));
    let spots = toon.glowing(Color::srgb(0.9, 0.95, 0.6), LinearRgba::rgb(1.5, 1.6, 0.6));
    let eyes = toon.material(Color::srgb(0.08, 0.06, 0.12));
    let body = toon.spawn_part(
        commands,
        cap_entity,
        Cylinder::new(0.32, 1.0),
        stem,
        Outline::Cylinder,
        Transform::from_xyz(0.0, 0.5, 0.0),
    );
    commands.entity(body).insert(Hop { rest: 0.5 });
    let top = toon.spawn_part(
        commands,
        body,
        Sphere::new(0.75).mesh().uv(20, 12),
        cap,
        Outline::Smooth,
        Transform::from_xyz(0.0, 0.65, 0.0).with_scale(Vec3::new(1.0, 0.55, 1.0)),
    );
    for i in 0..6 {
        let angle = i as f32 * TAU / 6.0;
        toon.spawn_part(
            commands,
            top,
            Sphere::new(0.1).mesh().uv(8, 6),
            spots.clone(),
            Outline::None,
            Transform::from_xyz(angle.cos() * 0.5, 0.55, angle.sin() * 0.5),
        );
    }
    for side in [-1.0, 1.0] {
        toon.spawn_part(
            commands,
            body,
            Sphere::new(0.06).mesh().uv(8, 6),
            eyes.clone(),
            Outline::None,
            Transform::from_xyz(0.12 * side, 0.15, -0.3),
        );
    }
}

/// Townsfolk: a body like the player's, coloured by their job, with a hat
/// or hood. `key` is their `visual`, e.g. "townsfolk_guard".
pub fn townsfolk(commands: &mut Commands, toon: &mut ToonAssets, person: Entity, key: &str) {
    let (robe, hat) = match key {
        "townsfolk_lamplighter" => (Color::srgb(0.75, 0.50, 0.25), Color::srgb(0.30, 0.22, 0.18)),
        "townsfolk_keeper" => (Color::srgb(0.30, 0.52, 0.32), Color::srgb(0.45, 0.32, 0.22)),
        "townsfolk_guard" => (Color::srgb(0.55, 0.58, 0.66), Color::srgb(0.70, 0.72, 0.78)),
        "townsfolk_ranger" => (Color::srgb(0.42, 0.34, 0.22), Color::srgb(0.30, 0.45, 0.28)),
        _ => (Color::srgb(0.6, 0.4, 0.6), Color::srgb(0.4, 0.3, 0.4)),
    };
    let robe = toon.material(robe);
    let hat = toon.material(hat);
    let skin = toon.material(Color::srgb(0.95, 0.80, 0.68));
    let eye = toon.material(Color::srgb(0.08, 0.06, 0.12));
    toon.spawn_part(
        commands,
        person,
        Capsule3d::new(0.36, 0.7),
        robe.clone(),
        Outline::Smooth,
        Transform::from_xyz(0.0, 0.7, 0.0),
    );
    // A robe that widens at the bottom.
    toon.spawn_part(
        commands,
        person,
        Cone::new(0.5, 0.9),
        robe,
        Outline::Smooth,
        Transform::from_xyz(0.0, 0.45, 0.0),
    );
    let head = toon.spawn_part(
        commands,
        person,
        Sphere::new(0.28).mesh().uv(24, 14),
        skin,
        Outline::Smooth,
        Transform::from_xyz(0.0, 1.52, 0.0),
    );
    for side in [-1.0, 1.0] {
        toon.spawn_part(
            commands,
            head,
            Sphere::new(0.04).mesh().uv(8, 6),
            eye.clone(),
            Outline::None,
            Transform::from_xyz(0.09 * side, 0.03, -0.25),
        );
    }
    match key {
        // A helmet.
        "townsfolk_guard" => {
            toon.spawn_part(
                commands,
                head,
                Sphere::new(0.3).mesh().uv(16, 10),
                hat,
                Outline::Smooth,
                Transform::from_xyz(0.0, 0.08, 0.02).with_scale(Vec3::new(1.05, 0.8, 1.05)),
            );
        }
        // A pointed hat.
        _ => {
            toon.spawn_part(
                commands,
                head,
                Cylinder::new(0.42, 0.05),
                hat.clone(),
                Outline::Cylinder,
                Transform::from_xyz(0.0, 0.2, 0.0),
            );
            toon.spawn_part(
                commands,
                head,
                Cone::new(0.24, 0.5),
                hat,
                Outline::Smooth,
                Transform::from_xyz(0.0, 0.45, 0.0),
            );
        }
    }
}
