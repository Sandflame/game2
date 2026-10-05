//! Gear looks by rarity: rarer gear makes a character look grander. The
//! weapon's rarity lights up the weapon (a tint, a glow, a gem at the
//! grip, a ring of light, sparks); the Body piece's rarity adds armour
//! (shoulder plates, spikes, a belt gem, floating lights, wings of blades,
//! a halo). Which pieces and colours each tier uses is data (`tiers` in
//! `models.ron`); the shapes are built here. Common gear is the model's
//! own clothes and weapons.

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::prelude::*;
use serde::Deserialize;
use shared::classes::CurrentClass;
use shared::gamedata::GameData;
use shared::items::{Bag, Equipment, Rarity, Slot};

use crate::models::{ModelLibrary, ModelPending, Rig};
use crate::shapes::blade;
use crate::toon::{Outline, ToonAssets, ToonMaterial};

pub struct GearPlugin;

impl Plugin for GearPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (show_gear, light_weapons, spin, bob).chain());
    }
}

/// Armour pieces a tier adds (on the chest, head or around the feet).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum ArmourPart {
    /// Small round shoulder plates.
    RoundPauldrons,
    /// Big shoulder plates with a trim ring.
    Pauldrons,
    /// Three spikes on each shoulder plate; `LongSpikes` are longer and
    /// in the accent colour.
    Spikes,
    LongSpikes,
    /// A gem on the belt.
    BeltGem,
    /// This many little lights circling the feet.
    Motes(usize),
    /// Floating blades spread like wings behind the back.
    Wings,
    /// A turning ring of light above the head.
    Halo,
}

/// Weapon pieces a tier adds (they hang from the hand holding it).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum WeaponPart {
    /// A gem at the grip.
    GripGem,
    /// A ring of light turning around the hand.
    GripRing,
    /// This many sparks drifting around the hand.
    Sparks(usize),
}

fn white() -> (f32, f32, f32) {
    (1.0, 1.0, 1.0)
}

/// How one rarity looks (`tiers` in `models.ron`). Colours are red,
/// green, blue (0–1); glows multiply the colour's light.
#[derive(Debug, Clone, Deserialize)]
pub struct TierLook {
    /// Item names of this rarity in the panels.
    pub name_color: (f32, f32, f32),
    #[serde(default = "white")]
    pub plate: (f32, f32, f32),
    #[serde(default = "white")]
    pub trim: (f32, f32, f32),
    #[serde(default)]
    pub trim_glow: f32,
    /// Gold bits: spikes, wing blades, the halo's teeth.
    #[serde(default = "white")]
    pub accent: (f32, f32, f32),
    #[serde(default)]
    pub accent_glow: f32,
    /// Multiplies the weapon's own colours.
    #[serde(default = "white")]
    pub weapon_tint: (f32, f32, f32),
    /// Lights the weapon in the trim colour.
    #[serde(default)]
    pub weapon_glow: f32,
    #[serde(default)]
    pub armour: Vec<ArmourPart>,
    #[serde(default)]
    pub weapon: Vec<WeaponPart>,
}

fn color((r, g, b): (f32, f32, f32)) -> Color {
    Color::srgb(r, g, b)
}

fn glow(c: (f32, f32, f32), strength: f32) -> LinearRgba {
    let l = color(c).to_linear();
    LinearRgba::rgb(l.red * strength, l.green * strength, l.blue * strength)
}

/// The colour for an item name of this rarity.
pub fn rarity_color(models: &ModelLibrary, rarity: Rarity) -> Option<Color> {
    models.tiers.get(&rarity).map(|t| color(t.name_color))
}

/// Demos only: show this rarity whatever the character wears.
#[derive(Component, Clone, Copy)]
pub struct ShowRarity(pub Rarity);

/// The gear pieces built on a character, and what they were built for.
#[derive(Component)]
struct GearShown {
    key: GearKey,
    pieces: Vec<Entity>,
}

#[derive(Clone, PartialEq)]
struct GearKey {
    weapon: Rarity,
    armour: Rarity,
    root: Entity,
    weapons: Vec<Entity>,
}

/// A weapon mesh's own colours, kept so a new tier starts from them.
#[derive(Component)]
struct OwnPaint {
    base: Color,
    emissive: LinearRgba,
}

/// The rarity a character shows for its weapon and its armour.
fn worn_rarity(
    data: &GameData,
    equipment: Option<&Equipment>,
    bag: Option<&Bag>,
    class: Option<&CurrentClass>,
) -> (Rarity, Rarity) {
    let (Some(equipment), Some(bag)) = (equipment, bag) else {
        return (Rarity::Common, Rarity::Common);
    };
    let rarity = |slot: Slot| {
        let class = class.map_or("", |c| c.class.as_str());
        equipment
            .in_slot(slot, class)
            .and_then(|id| bag.get(id))
            .and_then(|owned| data.items.get(&owned.item))
            .map_or(Rarity::Common, |def| def.rarity)
    };
    (rarity(Slot::Weapon), rarity(Slot::Body))
}

/// Build (or rebuild) the gear pieces when the rarity or the model changes.
fn show_gear(
    mut commands: Commands,
    data: Res<GameData>,
    models: Res<ModelLibrary>,
    mut toon: ToonAssets,
    characters: Query<
        (
            Entity,
            &Rig,
            Option<&Equipment>,
            Option<&Bag>,
            Option<&CurrentClass>,
            Option<&ShowRarity>,
            Option<&GearShown>,
        ),
        Without<ModelPending>,
    >,
) {
    for (character, rig, equipment, bag, class, shown_rarity, shown) in &characters {
        let (weapon, armour) = match shown_rarity {
            Some(ShowRarity(r)) => (*r, *r),
            None => worn_rarity(&data, equipment, bag, class),
        };
        let key = GearKey {
            weapon,
            armour,
            root: rig.root,
            weapons: rig.weapons().to_vec(),
        };
        if shown.is_some_and(|s| s.key == key) {
            continue;
        }
        if let Some(shown) = shown {
            for &piece in &shown.pieces {
                if let Ok(mut e) = commands.get_entity(piece) {
                    e.despawn();
                }
            }
        }
        let mut pieces = Vec::new();
        if let Some(look) = models.tiers.get(&armour) {
            build_armour(&mut commands, &mut toon, &models, rig, look, &mut pieces);
        }
        if let Some(look) = models.tiers.get(&weapon) {
            build_weapon_parts(&mut commands, &mut toon, rig, look, &mut pieces);
        }
        commands.entity(character).insert(GearShown { key, pieces });
    }
}

/// Tint and light the held weapons for their tier. Weapons load a moment
/// after they are put in the hand, so this keeps checking.
fn light_weapons(
    mut commands: Commands,
    models: Res<ModelLibrary>,
    characters: Query<(&Rig, &GearShown)>,
    children: Query<&Children>,
    meshes: Query<(&MeshMaterial3d<ToonMaterial>, Option<&OwnPaint>)>,
    mut materials: ResMut<Assets<ToonMaterial>>,
) {
    for (rig, shown) in &characters {
        let look = models.tiers.get(&shown.key.weapon);
        let (tint, light) = look.map_or((white(), LinearRgba::BLACK), |t| {
            (t.weapon_tint, glow(t.trim, t.weapon_glow))
        });
        for &weapon in rig.weapons() {
            for e in children.iter_descendants(weapon) {
                let Ok((handle, own)) = meshes.get(e) else {
                    continue;
                };
                let Some(material) = materials.get(&handle.0) else {
                    continue;
                };
                let own = match own {
                    Some(own) => (own.base, own.emissive),
                    None => {
                        let own = (material.base.base_color, material.base.emissive);
                        commands.entity(e).insert(OwnPaint {
                            base: own.0,
                            emissive: own.1,
                        });
                        own
                    }
                };
                let (base, by) = (own.0.to_linear(), color(tint).to_linear());
                let wanted_base = Color::LinearRgba(LinearRgba::new(
                    base.red * by.red,
                    base.green * by.green,
                    base.blue * by.blue,
                    base.alpha,
                ));
                let wanted_glow = own.1 + light;
                let changed = material.base.base_color != wanted_base
                    || material.base.emissive != wanted_glow;
                if changed && let Some(mut material) = materials.get_mut(&handle.0) {
                    material.base.base_color = wanted_base;
                    material.base.emissive = wanted_glow;
                }
            }
        }
    }
}

/// Turns around an axis (radians per second).
#[derive(Component)]
struct Spin(Vec3, f32);

/// Floats up and down around a resting place.
#[derive(Component)]
struct Bob {
    base: Vec3,
    amp: f32,
    speed: f32,
    phase: f32,
}

fn spin(time: Res<Time>, mut spinners: Query<(&Spin, &mut Transform)>) {
    for (spin, mut transform) in &mut spinners {
        transform.rotate_local_axis(
            Dir3::new(spin.0).unwrap_or(Dir3::Y),
            spin.1 * time.delta_secs(),
        );
    }
}

fn bob(time: Res<Time>, mut bobbers: Query<(&Bob, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (bob, mut transform) in &mut bobbers {
        transform.translation = bob.base + Vec3::Y * (bob.amp * (t * bob.speed + bob.phase).sin());
    }
}

/// Materials of one tier.
struct Paints {
    plate: Handle<ToonMaterial>,
    trim: Handle<ToonMaterial>,
    accent: Handle<ToonMaterial>,
}

fn paints(toon: &mut ToonAssets, look: &TierLook) -> Paints {
    Paints {
        plate: toon.material(color(look.plate)),
        trim: toon.glowing(color(look.trim), glow(look.trim, look.trim_glow)),
        accent: toon.glowing(color(look.accent), glow(look.accent, look.accent_glow)),
    }
}

fn holder(commands: &mut Commands, parent: Entity, transform: Transform) -> Entity {
    commands
        .spawn((transform, Visibility::default(), ChildOf(parent)))
        .id()
}

fn gem() -> Mesh {
    Sphere::new(1.0)
        .mesh()
        .ico(1)
        .unwrap_or_else(|_| Sphere::new(1.0).into())
}

fn build_armour(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    models: &ModelLibrary,
    rig: &Rig,
    look: &TierLook,
    pieces: &mut Vec<Entity>,
) {
    if look.armour.is_empty() {
        return;
    }
    let paint = paints(toon, look);
    let has = |part: ArmourPart| look.armour.contains(&part);
    if let Some(&chest) = rig.bones.get("chest") {
        // Pieces hang from a holder sized like the reshaped chest, so they
        // sit on its surface.
        let shape = models.bone_shape("chest").unwrap_or(Vec3::ONE);
        let on_chest = holder(commands, chest, Transform::from_scale(shape));
        pieces.push(on_chest);
        for side in [-1.0f32, 1.0] {
            if has(ArmourPart::RoundPauldrons) {
                toon.spawn_part(
                    commands,
                    on_chest,
                    gem(),
                    paint.plate.clone(),
                    Outline::Smooth,
                    Transform::from_xyz(side * 0.42, 0.2, 0.0)
                        .with_scale(Vec3::new(0.2, 0.11, 0.19)),
                );
            }
            if !has(ArmourPart::Pauldrons) {
                continue;
            }
            let shoulder = holder(
                commands,
                on_chest,
                Transform::from_xyz(side * 0.42, 0.22, 0.0)
                    .with_rotation(Quat::from_rotation_z(side * -0.35)),
            );
            toon.spawn_part(
                commands,
                shoulder,
                gem(),
                paint.plate.clone(),
                Outline::Smooth,
                Transform::from_scale(Vec3::new(0.26, 0.15, 0.25)),
            );
            toon.spawn_part(
                commands,
                shoulder,
                Torus::new(0.22, 0.26),
                paint.trim.clone(),
                Outline::None,
                Transform::from_xyz(0.0, -0.04, 0.0).with_scale(Vec3::new(1.05, 1.0, 1.0)),
            );
            let long = has(ArmourPart::LongSpikes);
            if has(ArmourPart::Spikes) || long {
                for k in 0..3 {
                    let a = (k as f32 - 1.0) * 0.45;
                    let length = if long { 0.44 } else { 0.32 };
                    toon.spawn_part(
                        commands,
                        shoulder,
                        blade(&[(0.0, 0.06), (0.1, 0.05), (length, 0.0)], 0.6),
                        if long {
                            paint.accent.clone()
                        } else {
                            paint.plate.clone()
                        },
                        Outline::None,
                        Transform::from_xyz(0.0, 0.1, a * 0.3).with_rotation(
                            Quat::from_rotation_z(side * -0.5) * Quat::from_rotation_x(a),
                        ),
                    );
                }
            }
        }
        if has(ArmourPart::BeltGem) {
            toon.spawn_part(
                commands,
                on_chest,
                gem(),
                paint.trim.clone(),
                Outline::None,
                Transform::from_xyz(0.0, -0.05, 0.36).with_scale(Vec3::splat(0.07)),
            );
        }
        if has(ArmourPart::Wings) {
            for side in [-1.0f32, 1.0] {
                for k in 0..4 {
                    let length = 0.9 - k as f32 * 0.15;
                    let shard = commands
                        .spawn((
                            Transform::default(),
                            Visibility::default(),
                            Bob {
                                base: Vec3::new(
                                    side * (0.25 + k as f32 * 0.05),
                                    0.35 - k as f32 * 0.05,
                                    -0.45 - k as f32 * 0.03,
                                ),
                                amp: 0.025,
                                speed: 1.5,
                                phase: k as f32 * 0.7,
                            },
                            ChildOf(on_chest),
                        ))
                        .id();
                    let tilt = Quat::from_rotation_z(-side * (0.75 + k as f32 * 0.38))
                        * Quat::from_rotation_y(side * 0.35);
                    toon.spawn_part(
                        commands,
                        shard,
                        blade(&[(0.0, 0.03), (0.15, 0.09), (length, 0.0)], 0.2),
                        if k % 2 == 0 {
                            paint.trim.clone()
                        } else {
                            paint.accent.clone()
                        },
                        Outline::None,
                        Transform::from_rotation(tilt),
                    );
                }
            }
        }
    }
    if has(ArmourPart::Halo)
        && let Some(&head) = rig.bones.get("head")
    {
        let shape = models.bone_shape("head").unwrap_or(Vec3::ONE);
        let halo = holder(
            commands,
            head,
            Transform::from_scale(shape).with_translation(Vec3::new(0.0, 1.25, -0.25) * shape),
        );
        pieces.push(halo);
        let tilted = holder(
            commands,
            halo,
            Transform::from_rotation(Quat::from_rotation_x(-0.35)),
        );
        let spinner = commands
            .spawn((
                Transform::default(),
                Visibility::default(),
                Spin(Vec3::Y, 0.8),
                ChildOf(tilted),
            ))
            .id();
        toon.spawn_part(
            commands,
            spinner,
            Torus::new(0.36, 0.4),
            paint.trim.clone(),
            Outline::None,
            Transform::IDENTITY,
        );
        for k in 0..6 {
            let a = k as f32 / 6.0 * TAU;
            toon.spawn_part(
                commands,
                spinner,
                blade(&[(0.0, 0.04), (0.06, 0.04), (0.14, 0.0)], 0.5),
                paint.accent.clone(),
                Outline::None,
                Transform::from_xyz(a.cos() * 0.38, 0.0, a.sin() * 0.38),
            );
        }
    }
    for part in &look.armour {
        if let ArmourPart::Motes(count) = *part {
            pieces.push(motes(
                commands,
                toon,
                rig.root,
                &paint.trim,
                count,
                0.9,
                0.1,
            ));
        }
    }
}

fn build_weapon_parts(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    rig: &Rig,
    look: &TierLook,
    pieces: &mut Vec<Entity>,
) {
    if look.weapon.is_empty() {
        return;
    }
    let paint = paints(toon, look);
    // The weapon hand: the right hand holds the main weapon.
    let Some(&hand) = rig.bones.get("handslot.r") else {
        return;
    };
    let grip = holder(commands, hand, Transform::IDENTITY);
    pieces.push(grip);
    for part in &look.weapon {
        match *part {
            WeaponPart::GripGem => {
                toon.spawn_part(
                    commands,
                    grip,
                    gem(),
                    paint.trim.clone(),
                    Outline::None,
                    Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::splat(0.07)),
                );
            }
            WeaponPart::GripRing => {
                let ring = commands
                    .spawn((
                        Transform::from_xyz(0.0, 0.1, 0.0),
                        Visibility::default(),
                        Spin(Vec3::Y, 1.5),
                        ChildOf(grip),
                    ))
                    .id();
                toon.spawn_part(
                    commands,
                    ring,
                    Torus::new(0.2, 0.225),
                    paint.trim.clone(),
                    Outline::None,
                    Transform::from_rotation(Quat::from_rotation_x(0.35)),
                );
            }
            WeaponPart::Sparks(count) => {
                motes(commands, toon, grip, &paint.trim, count, 0.3, -0.2);
            }
        }
    }
}

/// Little lights circling a point.
fn motes(
    commands: &mut Commands,
    toon: &mut ToonAssets,
    parent: Entity,
    material: &Handle<ToonMaterial>,
    count: usize,
    radius: f32,
    height: f32,
) -> Entity {
    let ring = commands
        .spawn((
            Transform::from_xyz(0.0, height, 0.0),
            Visibility::default(),
            Spin(Vec3::Y, 0.9),
            ChildOf(parent),
        ))
        .id();
    let mote = toon
        .meshes
        .add(blade(&[(0.0, 0.0), (0.05, 0.04), (0.1, 0.0)], 1.0));
    for k in 0..count {
        let a = k as f32 / count.max(1) as f32 * TAU;
        commands.spawn((
            Mesh3d(mote.clone()),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            Bob {
                base: Vec3::new(
                    a.cos() * radius,
                    0.25 + (k % 3) as f32 * 0.25,
                    a.sin() * radius,
                ),
                amp: 0.12,
                speed: 1.3,
                phase: a * 2.0,
            },
            ChildOf(ring),
        ));
    }
    ring
}

/// Checks for `models.ron` tiers.
pub fn check_tiers(tiers: &HashMap<Rarity, TierLook>) -> Vec<String> {
    Rarity::ALL
        .iter()
        .filter(|r| !tiers.contains_key(r))
        .map(|r| format!("models.ron: `tiers` has no look for {}", r.label()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::data::find_assets_dir;

    #[test]
    fn every_rarity_has_a_look_and_rarer_looks_add_more() {
        let assets = find_assets_dir().unwrap();
        let models = ModelLibrary::load(&assets).unwrap();
        assert!(check_tiers(&models.tiers).is_empty());
        let parts = |r: Rarity| {
            let t = &models.tiers[&r];
            t.armour.len() + t.weapon.len()
        };
        for pair in Rarity::ALL.windows(2) {
            assert!(parts(pair[0]) <= parts(pair[1]), "{:?}", pair[1]);
        }
    }
}
