//! Placeholder bodies for Whisperwood's and the Tangled Burrow's creatures
//! and Lanternhold's townsfolk, built from simple shapes. Everything faces -Z.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use crate::animation::{BossRig, Hop};
use crate::toon::{Outline, ToonAssets};

/// Colours of a wolf's coat.
pub struct WolfLook {
    pub fur: Color,
    pub belly: Color,
    pub thorns: Color,
}

/// Whisperwood's grey thornwolves.
pub const THORNWOLF: WolfLook = WolfLook {
    fur: Color::srgb(0.36, 0.38, 0.34),
    belly: Color::srgb(0.55, 0.55, 0.48),
    thorns: Color::srgb(0.48, 0.62, 0.30),
};

/// The brown pups of the Tangled Burrow.
pub const BURROW_PUP: WolfLook = WolfLook {
    fur: Color::srgb(0.45, 0.34, 0.24),
    belly: Color::srgb(0.62, 0.52, 0.40),
    thorns: Color::srgb(0.55, 0.62, 0.32),
};

/// Their mother: dark, with pale thorns.
pub const MATRIARCH: WolfLook = WolfLook {
    fur: Color::srgb(0.22, 0.20, 0.22),
    belly: Color::srgb(0.40, 0.36, 0.36),
    thorns: Color::srgb(0.85, 0.80, 0.62),
};

/// An empty child scaled by `scale`, to build a bigger or smaller body in.
pub fn scaled(commands: &mut Commands, parent: Entity, scale: f32) -> Entity {
    commands
        .spawn((
            Transform::from_scale(Vec3::splat(scale)),
            Visibility::default(),
            ChildOf(parent),
        ))
        .id()
}

/// A wolf with a ridge of thorns down its back.
pub fn thornwolf(commands: &mut Commands, toon: &mut ToonAssets, wolf: Entity, look: &WolfLook) {
    let fur = toon.material(look.fur);
    let belly = toon.material(look.belly);
    let thorn = toon.material(look.thorns);
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

/// Colours of a walking mushroom.
pub struct CapLook {
    pub stem: Color,
    pub cap: Color,
    pub spots: (Color, LinearRgba),
}

/// Whisperwood's purple spore caps.
pub const SPORE_CAP: CapLook = CapLook {
    stem: Color::srgb(0.92, 0.88, 0.76),
    cap: Color::srgb(0.62, 0.32, 0.70),
    spots: (Color::srgb(0.9, 0.95, 0.6), LinearRgba::rgb(1.5, 1.6, 0.6)),
};

/// The Burrow's rotting ones: grey-green with sickly glowing spots.
pub const ROT_SPORE: CapLook = CapLook {
    stem: Color::srgb(0.70, 0.70, 0.60),
    cap: Color::srgb(0.38, 0.42, 0.30),
    spots: (Color::srgb(0.8, 0.5, 1.0), LinearRgba::rgb(1.4, 0.6, 2.2)),
};

/// Mother Sporecap: a huge red cap with glowing spots.
pub const MOTHER_SPORECAP: CapLook = CapLook {
    stem: Color::srgb(0.95, 0.90, 0.80),
    cap: Color::srgb(0.78, 0.28, 0.24),
    spots: (Color::srgb(1.0, 0.95, 0.7), LinearRgba::rgb(2.0, 1.8, 0.8)),
};

/// A walking mushroom with a big spotted cap.
pub fn spore_cap(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    cap_entity: Entity,
    look: &CapLook,
) {
    let stem = toon.material(look.stem);
    let cap = toon.material(look.cap);
    let spots = toon.glowing(look.spots.0, look.spots.1);
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

/// The Rotheart: a great rotting heart wrapped in roots, with two lashing
/// root-arms. Posed like the Rootwarden (see `BossRig`).
pub fn rotheart(commands: &mut Commands, toon: &mut ToonAssets, boss: Entity) {
    let flesh = toon.material(Color::srgb(0.42, 0.22, 0.34));
    let veins = toon.glowing(Color::srgb(0.8, 0.45, 1.0), LinearRgba::rgb(1.8, 0.6, 2.6));
    let bark = toon.material(Color::srgb(0.32, 0.24, 0.18));
    let rot = toon.material(Color::srgb(0.30, 0.36, 0.22));
    let eye = toon.glowing(Color::srgb(1.0, 0.8, 0.4), LinearRgba::rgb(3.0, 2.0, 0.5));
    // Roots gripping the floor (they draw into the ground when it falls).
    let mut roots = Vec::new();
    for i in 0..7 {
        let angle = i as f32 * TAU / 7.0 + 0.4;
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        roots.push(toon.spawn_part(
            commands,
            boss,
            Capsule3d::new(0.4, 2.4),
            bark.clone(),
            Outline::Smooth,
            Transform::from_translation(out * 2.4 + Vec3::Y * 0.4).with_rotation(
                Quat::from_rotation_arc(Vec3::Y, (out + Vec3::Y * 0.35).normalize()),
            ),
        ));
    }
    let torso = commands
        .spawn((Transform::default(), Visibility::default(), ChildOf(boss)))
        .id();
    // The heart.
    let heart = toon.spawn_part(
        commands,
        torso,
        Sphere::new(2.2).mesh().uv(28, 16),
        flesh,
        Outline::Smooth,
        Transform::from_xyz(0.0, 3.4, 0.0).with_scale(Vec3::new(1.0, 1.15, 0.9)),
    );
    // Glowing veins and a single eye.
    for i in 0..6 {
        let angle = i as f32 * TAU / 6.0;
        toon.spawn_part(
            commands,
            heart,
            Capsule3d::new(0.08, 1.6),
            veins.clone(),
            Outline::None,
            Transform::from_xyz(angle.cos() * 1.9, 0.2, angle.sin() * 1.9)
                .with_rotation(Quat::from_rotation_y(-angle) * Quat::from_rotation_x(0.4)),
        );
    }
    toon.spawn_part(
        commands,
        heart,
        Sphere::new(0.45).mesh().uv(14, 8),
        eye,
        Outline::None,
        Transform::from_xyz(0.0, 0.3, -2.0),
    );
    // Roots wrapped around it and rot growing on top.
    for (tilt, turn) in [(0.5, 0.0), (-0.6, 1.2), (0.3, 2.4)] {
        toon.spawn_part(
            commands,
            torso,
            Torus::new(2.1, 2.5),
            bark.clone(),
            Outline::Smooth,
            Transform::from_xyz(0.0, 3.4, 0.0)
                .with_rotation(Quat::from_rotation_y(turn) * Quat::from_rotation_x(tilt)),
        );
    }
    for (offset, size) in [
        (Vec3::new(0.6, 5.7, 0.3), 0.8),
        (Vec3::new(-0.7, 5.5, 0.5), 0.6),
    ] {
        toon.spawn_part(
            commands,
            torso,
            Sphere::new(size).mesh().uv(14, 8),
            rot.clone(),
            Outline::Smooth,
            Transform::from_translation(offset),
        );
    }
    // Two root-arms.
    let mut arms = [Entity::PLACEHOLDER; 2];
    for (slot, side) in arms.iter_mut().zip([-1.0, 1.0]) {
        let shoulder = commands
            .spawn((
                Transform::from_xyz(1.8 * side, 3.8, -0.4),
                Visibility::default(),
                ChildOf(torso),
            ))
            .id();
        toon.spawn_part(
            commands,
            shoulder,
            Capsule3d::new(0.35, 3.0),
            bark.clone(),
            Outline::Smooth,
            Transform::from_xyz(0.9 * side, -0.4, 0.0)
                .with_rotation(Quat::from_rotation_z(-side) * Quat::from_rotation_x(0.4)),
        );
        *slot = shoulder;
    }
    commands
        .entity(boss)
        .insert(BossRig::new(torso, arms).with_roots(roots));
}
