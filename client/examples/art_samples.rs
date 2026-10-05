//! Art direction sample scene (not part of the game, 2026-10-05): KayKit
//! characters with longer, slimmer proportions, the game's toon look and
//! skinned outlines, race parts (ears, horns, tails) and four gear tiers.
//! The approved look that M11 stage 1 builds into the real game; see
//! `docs/art-direction.md`.
//!
//! Run: `cargo run -p client --example art_samples` (flies the camera past
//! each group, then quits). `SHOTS=<prefix>` also saves `<prefix>-N.png`.

#[path = "../src/toon.rs"]
#[allow(dead_code)]
mod toon;

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::app::AnimationSystems;
use bevy::asset::RenderAssetUsages;
use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::shader::ShaderRef;
use bevy::world_serialization::WorldInstanceReady;
use toon::{ToonExtension, ToonMaterial, ToonPlugin};

const DIR: &str = "models/kaykit";
/// Stretch level for the race and gear groups.
const LEVEL: usize = 3;

// ---------- skinned outline ----------

#[derive(ShaderType, Debug, Clone, Copy)]
struct SkinOutlineSettings {
    color: LinearRgba,
    params: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
struct SkinOutline {
    #[uniform(0)]
    settings: SkinOutlineSettings,
}

impl Material for SkinOutline {
    fn vertex_shader() -> ShaderRef {
        "shaders/outline_skinned.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/outline_skinned.wgsl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(
        _p: &MaterialPipeline,
        d: &mut RenderPipelineDescriptor,
        _l: &MeshVertexBufferLayoutRef,
        _k: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        d.primitive.cull_mode = Some(Face::Front);
        Ok(())
    }
}

// ---------- characters ----------

#[derive(Clone, Copy, PartialEq)]
enum Race {
    Human,
    Elf,
    Drake,
    Demon,
}

#[derive(Clone, Copy, PartialEq)]
enum Tier {
    /// The pack's own weapon(s).
    Plain,
    Rare,
    Epic,
    Legendary,
}

#[derive(Component, Clone)]
struct Hero {
    file: &'static str,
    anim: &'static str,
    /// 0 = as the pack made it, 1 = a bit longer, 2 = much longer.
    stretch: usize,
    race: Race,
    tier: Tier,
    /// Pack items to keep showing (everything else in the hands is hidden).
    show: &'static [&'static str],
    /// Pack parts to hide (hats).
    hide: &'static [&'static str],
    armour_tint: Option<Color>,
    skin_tint: Option<Color>,
}

/// Fixed bone scale applied after animation (the longer proportions).
#[derive(Component)]
struct Stretch(Vec3);

#[derive(Component)]
struct Spin(Vec3, f32);

#[derive(Component)]
struct Bob {
    base: Vec3,
    amp: f32,
    speed: f32,
    phase: f32,
}

/// Per stretch level: model scale, head, leg, arm, spine, chest, slim
/// (how much narrower the hips, waist and chest are).
const LEVELS: [[f32; 7]; 4] = [
    [0.75, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
    [0.8, 0.6, 1.75, 1.3, 1.2, 1.12, 1.0],
    [0.72, 0.5, 2.6, 1.6, 1.4, 1.15, 1.0],
    [0.7, 0.5, 3.1, 1.75, 1.35, 1.12, 0.84],
];

/// Extra height the longer legs need (model units).
fn lift(level: usize) -> f32 {
    0.38 * (LEVELS[level][2] - 1.0) * 0.82
}

fn stretch_for(name: &str, level: usize) -> Option<Vec3> {
    let [_, head, leg, arm, spine, chest, slim] = LEVELS[level];
    // Hips, spine and chest stack up (their rest rotations are all upright),
    // so the head undoes their squash and stretch to stay round.
    let below_head = Vec3::new(
        slim * slim * (slim + 0.06),
        spine * chest,
        slim * slim * (slim + 0.06),
    );
    Some(match name {
        "head" => head / below_head,
        "hips" => Vec3::new(slim, 1.0, slim),
        "upperleg.l" | "upperleg.r" => Vec3::new(1.0, leg, 1.0),
        "foot.l" | "foot.r" => Vec3::new(1.0, 1.0 / leg, 1.0),
        "upperarm.l" | "upperarm.r" => Vec3::new(1.0, arm, 1.0),
        "hand.l" | "hand.r" => Vec3::new(1.0, 1.0 / arm, 1.0),
        "spine" => Vec3::new(slim, spine, slim),
        "chest" => Vec3::new(slim + 0.06, chest, slim + 0.06),
        _ => return None,
    })
}

#[derive(Resource)]
struct Script(Vec<(f32, Vec3, Vec3, bool)>, usize);

fn main() {
    let assets = shared::data::find_assets_dir()
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "assets".into());
    let s = |t, e: [f32; 3], a: [f32; 3], shot| (t, Vec3::from(e), Vec3::from(a), shot);
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.55, 0.78, 0.95)))
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: assets,
            ..default()
        }))
        .add_plugins((ToonPlugin, MaterialPlugin::<SkinOutline>::default()))
        .insert_resource(Script(
            vec![
                s(0.0, [0.0, 1.45, 5.6], [0.0, 1.0, 0.0], false),
                s(7.0, [0.0, 1.45, 5.6], [0.0, 1.0, 0.0], true),
                s(8.5, [12.0, 1.45, 5.6], [12.0, 1.0, 0.0], false),
                s(10.5, [12.0, 1.45, 5.6], [12.0, 1.0, 0.0], true),
                s(12.0, [14.6, 2.0, 2.2], [12.9, 1.5, 0.0], false),
                s(14.0, [14.6, 2.0, 2.2], [12.9, 1.5, 0.0], true),
                s(14.8, [15.2, 1.7, -3.2], [12.9, 0.9, 0.0], false),
                s(16.8, [15.2, 1.7, -3.2], [12.9, 0.9, 0.0], true),
                s(18.0, [24.2, 1.6, 7.0], [24.2, 1.15, 0.0], false),
                s(20.0, [24.2, 1.6, 7.0], [24.2, 1.15, 0.0], true),
                s(21.5, [27.6, 1.7, 3.0], [26.7, 1.25, 0.0], false),
                s(23.5, [27.6, 1.7, 3.0], [26.7, 1.25, 0.0], true),
                s(25.0, [40.0, 1.3, 4.6], [40.0, 1.05, 0.0], false),
                s(27.0, [40.0, 1.3, 4.6], [40.0, 1.05, 0.0], true),
                s(28.5, [0.0; 3], [0.0; 3], false),
            ],
            0,
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, (run, spin, bob))
        .add_systems(
            PostUpdate,
            stretch
                .after(AnimationSystems)
                .before(TransformSystems::Propagate),
        )
        .run();
}

fn hero(commands: &mut Commands, assets: &AssetServer, x: f32, hero: Hero) {
    let path = format!("{DIR}/{}", hero.file);
    let scale = LEVELS[hero.stretch][0];
    let lift = lift(hero.stretch) * scale;
    commands
        .spawn((
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
            Transform::from_xyz(x, lift, 0.0).with_scale(Vec3::splat(scale)),
            hero,
        ))
        .observe(dress);
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut toon: toon::ToonAssets,
    mut keep: Local<Vec<Handle<Gltf>>>,
) {
    for f in ["Knight", "Mage", "Rogue", "Barbarian"] {
        keep.push(assets.load(format!("{DIR}/{f}.glb")));
    }
    commands.spawn((
        Camera3d::default(),
        Hdr,
        Tonemapping::None,
        Bloom::NATURAL,
        Transform::default(),
    ));
    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.5, -1.0, -0.35), Vec3::Y),
        CascadeShadowConfigBuilder {
            maximum_distance: 40.0,
            first_cascade_far_bound: 10.0,
            ..default()
        }
        .build(),
    ));
    let ground = toon.ground(Color::srgb(0.45, 0.62, 0.36));
    commands.spawn((
        Mesh3d(toon.meshes.add(Plane3d::default().mesh().size(140.0, 60.0))),
        MeshMaterial3d(ground),
        Transform::from_xyz(20.0, 0.0, 0.0),
    ));

    let base = |file, anim| Hero {
        file,
        anim,
        stretch: LEVEL,
        race: Race::Human,
        tier: Tier::Plain,
        show: &[],
        hide: &[],
        armour_tint: None,
        skin_tint: None,
    };
    // 1. Proportions: the pack as it is, then longer.
    hero(
        &mut commands,
        &assets,
        -1.7,
        Hero {
            stretch: 0,
            show: &["1H_Sword", "Round_Shield"],
            ..base("Knight.glb", "Idle")
        },
    );
    hero(
        &mut commands,
        &assets,
        -0.5,
        Hero {
            stretch: 2,
            show: &["1H_Sword", "Round_Shield"],
            ..base("Knight.glb", "Idle")
        },
    );
    hero(
        &mut commands,
        &assets,
        0.7,
        Hero {
            stretch: 3,
            show: &["1H_Sword", "Round_Shield"],
            ..base("Knight.glb", "Idle")
        },
    );
    // Today's placeholder body.
    let body = toon.material(Color::srgb(0.85, 0.75, 0.6));
    let capsule = commands.spawn(Transform::from_xyz(1.9, 0.0, 0.0)).id();
    toon.spawn_part(
        &mut commands,
        capsule,
        Capsule3d::new(0.35, 0.9),
        body.clone(),
        toon::Outline::Smooth,
        Transform::from_xyz(0.0, 0.8, 0.0),
    );
    toon.spawn_part(
        &mut commands,
        capsule,
        Sphere::new(0.27),
        body,
        toon::Outline::Smooth,
        Transform::from_xyz(0.0, 1.55, 0.0),
    );

    // 2. Races.
    hero(
        &mut commands,
        &assets,
        10.2,
        Hero {
            show: &["Knife"],
            ..base("Rogue.glb", "Idle")
        },
    );
    hero(
        &mut commands,
        &assets,
        11.4,
        Hero {
            race: Race::Elf,
            hide: &["Mage_Hat"],
            show: &["2H_Staff"],
            ..base("Mage.glb", "Idle")
        },
    );
    hero(
        &mut commands,
        &assets,
        12.6,
        Hero {
            race: Race::Drake,
            hide: &["Barbarian_Hat"],
            show: &["1H_Axe"],
            skin_tint: Some(Color::srgb(0.85, 0.9, 1.0)),
            ..base("Barbarian.glb", "Idle")
        },
    );
    hero(
        &mut commands,
        &assets,
        13.8,
        Hero {
            race: Race::Demon,
            // The cape would cover the tail.
            hide: &["Rogue_Cape"],
            show: &["Knife", "Knife_Offhand"],
            skin_tint: Some(Color::srgb(1.0, 0.78, 0.82)),
            ..base("Rogue.glb", "Idle")
        },
    );

    // 3. Gear tiers on the same knight.
    hero(
        &mut commands,
        &assets,
        21.5,
        Hero {
            show: &["1H_Sword", "Round_Shield"],
            ..base("Knight.glb", "Idle")
        },
    );
    hero(
        &mut commands,
        &assets,
        23.2,
        Hero {
            tier: Tier::Rare,
            armour_tint: Some(Color::srgb(0.7, 0.85, 1.15)),
            show: &["Badge_Shield"],
            ..base("Knight.glb", "Idle")
        },
    );
    hero(
        &mut commands,
        &assets,
        24.9,
        Hero {
            tier: Tier::Epic,
            armour_tint: Some(Color::srgb(0.55, 0.42, 0.85)),
            ..base("Knight.glb", "2H_Melee_Idle")
        },
    );
    hero(
        &mut commands,
        &assets,
        26.7,
        Hero {
            tier: Tier::Legendary,
            armour_tint: Some(Color::srgb(1.7, 1.35, 0.65)),
            ..base("Knight.glb", "2H_Melee_Idle")
        },
    );

    // 4. Weapons on display.
    let kay = |file: &str, x: f32, commands: &mut Commands| {
        commands.spawn((
            WorldAssetRoot(
                assets.load(GltfAssetLabel::Scene(0).from_asset(format!("{DIR}/{file}"))),
            ),
            Transform::from_xyz(x, 0.35, 0.0)
                .with_rotation(Quat::from_rotation_y(0.5))
                .with_scale(Vec3::splat(0.8)),
        ));
    };
    kay("sword_1handed.gltf", 37.4, &mut commands);
    kay("axe_2handed.gltf", 38.3, &mut commands);
    for (x, tier) in [
        (39.3, Tier::Rare),
        (40.4, Tier::Epic),
        (41.6, Tier::Legendary),
    ] {
        let holder = commands
            .spawn(
                Transform::from_xyz(x, 0.25, 0.0)
                    .with_rotation(Quat::from_rotation_y(0.6))
                    .with_scale(Vec3::splat(0.8)),
            )
            .id();
        weapon(&mut commands, &mut toon, holder, tier);
    }
    let holder = commands
        .spawn(
            Transform::from_xyz(42.8, 0.2, 0.0)
                .with_rotation(Quat::from_rotation_y(0.4))
                .with_scale(Vec3::splat(0.8)),
        )
        .id();
    staff(&mut commands, &mut toon, holder);
}

/// Once a character has loaded: play its animation, toon materials and
/// outlines, hide unused pack items, add race parts and gear.
fn dress(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    heroes: Query<&Hero>,
    children: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    std_mats: Query<(
        &MeshMaterial3d<StandardMaterial>,
        &Mesh3d,
        Option<&SkinnedMesh>,
    )>,
    mut players: Query<&mut AnimationPlayer>,
    gltfs: Res<Assets<Gltf>>,
    assets: Res<AssetServer>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    standard: Res<Assets<StandardMaterial>>,
    mut outlines: ResMut<Assets<SkinOutline>>,
    mut toon: toon::ToonAssets,
) {
    let Ok(hero) = heroes.get(ready.entity) else {
        return;
    };
    let hero = hero.clone();
    let mut bones: HashMap<String, Entity> = HashMap::new();
    for e in children.iter_descendants(ready.entity) {
        if let Ok(name) = names.get(e) {
            bones.insert(name.as_str().to_owned(), e);
        }
    }
    // Animation.
    let path = format!("{DIR}/{}", hero.file);
    if let Some(gltf) = assets.get_handle::<Gltf>(path).and_then(|h| gltfs.get(&h))
        && let Some(clip) = gltf.named_animations.get(hero.anim)
    {
        let (graph, index) = AnimationGraph::from_clip(clip.clone());
        let graph = graphs.add(graph);
        for e in children.iter_descendants(ready.entity) {
            if let Ok(mut player) = players.get_mut(e) {
                player.play(index).repeat();
                commands
                    .entity(e)
                    .insert(AnimationGraphHandle(graph.clone()));
            }
        }
    }
    // Hide hand items not wanted, and hats.
    for side in ["handslot.l", "handslot.r"] {
        if let Some(&slot) = bones.get(side) {
            for item in children.get(slot).into_iter().flatten().copied() {
                let shown = names
                    .get(item)
                    .is_ok_and(|n| hero.show.contains(&n.as_str()));
                if !shown && names.get(item).is_ok() {
                    commands.entity(item).insert(Visibility::Hidden);
                }
            }
        }
    }
    for hide in hero.hide {
        if let Some(&e) = bones.get(*hide) {
            commands.entity(e).insert(Visibility::Hidden);
        }
    }
    // Toon materials + outlines.
    let outline = outlines.add(SkinOutline {
        settings: SkinOutlineSettings {
            color: LinearRgba::new(0.02, 0.015, 0.03, 1.0),
            params: Vec4::new(0.0016, 0.0, 0.0, 0.0),
        },
    });
    for e in children.iter_descendants(ready.entity) {
        let Ok((material, mesh, skin)) = std_mats.get(e) else {
            continue;
        };
        // Glb primitives are children of the named node.
        let name = parents
            .get(e)
            .ok()
            .and_then(|p| names.get(p.parent()).ok())
            .map(|n| n.as_str().to_owned())
            .unwrap_or_default();
        let mut base = standard.get(&material.0).cloned().unwrap_or_default();
        base.perceptual_roughness = 1.0;
        base.metallic = 0.0;
        let armour = ["Body", "Arm", "Leg", "Cape", "Helmet"]
            .iter()
            .any(|p| name.contains(p));
        let tint = if armour {
            hero.armour_tint
        } else if name.contains("Head") {
            hero.skin_tint
        } else {
            None
        };
        if let Some(tint) = tint {
            let t = tint.to_linear();
            let b = base.base_color.to_linear();
            base.base_color = Color::LinearRgba(LinearRgba::rgb(
                b.red * t.red,
                b.green * t.green,
                b.blue * t.blue,
            ));
        }
        let toon_mat = toon.materials.add(ToonMaterial {
            base,
            extension: ToonExtension::default(),
        });
        commands
            .entity(e)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(toon_mat));
        let mut hull = commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(outline.clone()),
            NotShadowCaster,
            ChildOf(e),
        ));
        if let Some(skin) = skin {
            hull.insert(skin.clone());
        }
    }
    // Longer proportions.
    if hero.stretch > 0 {
        for (name, &e) in &bones {
            if let Some(s) = stretch_for(name, hero.stretch) {
                commands.entity(e).insert(Stretch(s));
            }
        }
    }
    // Race parts.
    if let Some(&head) = bones.get("head") {
        race_parts(
            &mut commands,
            &mut toon,
            head,
            bones.get("hips").copied(),
            hero.race,
        );
    }
    // Gear.
    if hero.tier != Tier::Plain {
        if let Some(&hand) = bones.get("handslot.r") {
            let holder = commands
                .spawn((Transform::from_xyz(0.0, 0.03, 0.0), ChildOf(hand)))
                .id();
            weapon(&mut commands, &mut toon, holder, hero.tier);
        }
        if let Some(&chest) = bones.get("chest") {
            armour(
                &mut commands,
                &mut toon,
                chest,
                ready.entity,
                bones.get("head").copied(),
                hero.tier,
            );
        }
    }
}

fn stretch(mut bones: Query<(&Stretch, &mut Transform)>) {
    for (s, mut t) in &mut bones {
        t.scale = s.0;
    }
}

fn spin(time: Res<Time>, mut q: Query<(&Spin, &mut Transform)>) {
    for (s, mut t) in &mut q {
        t.rotate_local(Quat::from_axis_angle(s.0, s.1 * time.delta_secs()));
    }
}

fn bob(time: Res<Time>, mut q: Query<(&Bob, &mut Transform)>) {
    for (b, mut t) in &mut q {
        t.translation = b.base + Vec3::Y * b.amp * (time.elapsed_secs() * b.speed + b.phase).sin();
    }
}

// ---------- meshes ----------

fn finish(positions: Vec<[f32; 3]>, indices: Vec<u32>) -> Mesh {
    let n = positions.len();
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32, 0.0]; n]);
    m.insert_indices(Indices::U32(indices));
    m.duplicate_vertices();
    m.compute_flat_normals();
    m
}

/// A blade along +Y with a diamond cross-section: (height, half width).
/// `thick` is the half thickness as a fraction of the width.
fn blade(profile: &[(f32, f32)], thick: f32) -> Mesh {
    let mut p = Vec::new();
    for &(y, w) in profile {
        let t = (w * thick).max(0.0);
        p.extend([[w, y, 0.0], [0.0, y, t], [-w, y, 0.0], [0.0, y, -t]]);
    }
    let mut idx = vec![0, 2, 1, 0, 3, 2];
    for i in 0..profile.len() as u32 - 1 {
        for k in 0..4 {
            let a = i * 4 + k;
            let b = i * 4 + (k + 1) % 4;
            let c = a + 4;
            let d = b + 4;
            idx.extend([a, b, d, a, d, c]);
        }
    }
    finish(p, idx)
}

/// A tube along a path, `sides` around, with a radius at each point.
fn tube(path: &[Vec3], radii: &[f32], sides: u32) -> Mesh {
    let mut p = Vec::new();
    let mut normal = Vec3::X;
    for (i, (&c, &r)) in path.iter().zip(radii).enumerate() {
        let next = path
            .get(i + 1)
            .copied()
            .unwrap_or_else(|| c + (c - path[i - 1]));
        let prev = if i == 0 { c - (next - c) } else { path[i - 1] };
        let tangent = (next - prev).normalize();
        normal = (normal - tangent * normal.dot(tangent)).normalize_or(Vec3::Z);
        let bi = tangent.cross(normal);
        for k in 0..sides {
            let a = k as f32 / sides as f32 * TAU;
            p.push((c + (normal * a.cos() + bi * a.sin()) * r).to_array());
        }
    }
    let mut idx = Vec::new();
    for i in 0..path.len() as u32 - 1 {
        for k in 0..sides {
            let a = i * sides + k;
            let b = i * sides + (k + 1) % sides;
            idx.extend([a, b, b + sides, a, b + sides, a + sides]);
        }
    }
    finish(p, idx)
}

/// Points along a curve: starts going `dir`, turning by `bend` each step.
fn curve(start: Vec3, dir: Vec3, axis: Vec3, bend: f32, step: f32, n: usize) -> Vec<Vec3> {
    let mut pts = vec![start];
    let mut d = dir.normalize();
    let mut c = start;
    for _ in 0..n {
        c += d * step;
        pts.push(c);
        d = Quat::from_axis_angle(axis.normalize(), bend) * d;
    }
    pts
}

fn taper(n: usize, r0: f32) -> Vec<f32> {
    (0..=n)
        .map(|i| r0 * (1.0 - i as f32 / n as f32).max(0.02))
        .collect()
}

// ---------- materials ----------

fn mat(toon: &mut toon::ToonAssets, color: Color, glow: f32) -> Handle<ToonMaterial> {
    let l = color.to_linear();
    let mut m = ToonMaterial {
        base: StandardMaterial {
            base_color: color,
            emissive: LinearRgba::rgb(l.red * glow, l.green * glow, l.blue * glow),
            perceptual_roughness: 1.0,
            cull_mode: None,
            double_sided: true,
            ..default()
        },
        extension: ToonExtension::default(),
    };
    m.extension.settings.rim_color.alpha = 0.35;
    toon.materials.add(m)
}

fn part(
    commands: &mut Commands,
    toon: &mut toon::ToonAssets,
    parent: Entity,
    mesh: Mesh,
    material: &Handle<ToonMaterial>,
    transform: Transform,
) -> Entity {
    let mesh = toon.meshes.add(mesh);
    commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            transform,
            ChildOf(parent),
        ))
        .id()
}

// ---------- race parts ----------

fn race_parts(
    commands: &mut Commands,
    toon: &mut toon::ToonAssets,
    head: Entity,
    hips: Option<Entity>,
    race: Race,
) {
    let skin = mat(toon, Color::srgb(0.96, 0.76, 0.62), 0.0);
    match race {
        Race::Human => {}
        Race::Elf => {
            for side in [-1.0f32, 1.0] {
                let ear = blade(&[(0.0, 0.11), (0.18, 0.1), (0.45, 0.04), (0.6, 0.0)], 0.3);
                part(
                    commands,
                    toon,
                    head,
                    ear,
                    &skin,
                    Transform::from_xyz(side * 0.5, 0.62, -0.02).with_rotation(
                        Quat::from_rotation_z(-side * 1.15) * Quat::from_rotation_x(-0.45),
                    ),
                );
            }
        }
        Race::Drake => {
            let horn = mat(toon, Color::srgb(0.16, 0.18, 0.3), 0.0);
            let scale = mat(toon, Color::srgb(0.35, 0.55, 0.85), 0.25);
            for side in [-1.0f32, 1.0] {
                // Swept back along the head, curling up at the end (like Au Ra).
                let pts = curve(
                    Vec3::new(side * 0.4, 0.74, 0.08),
                    Vec3::new(side * 0.3, 0.0, -1.0),
                    Vec3::X,
                    0.09,
                    0.15,
                    8,
                );
                part(
                    commands,
                    toon,
                    head,
                    tube(&pts, &taper(8, 0.15), 7),
                    &horn,
                    Transform::IDENTITY,
                );
                // Rooted in the head, not floating.
                part(
                    commands,
                    toon,
                    head,
                    Sphere::new(0.15).mesh().ico(1).unwrap(),
                    &horn,
                    Transform::from_translation(pts[0]),
                );
                // Scales on the cheek.
                for k in 0..3 {
                    let s = blade(&[(0.0, 0.05), (0.06, 0.05), (0.1, 0.0)], 0.4);
                    part(
                        commands,
                        toon,
                        head,
                        s,
                        &scale,
                        Transform::from_xyz(
                            side * 0.53,
                            0.38 + k as f32 * 0.08,
                            0.1 - k as f32 * 0.05,
                        )
                        .with_rotation(Quat::from_rotation_z(side * 0.4)),
                    );
                }
            }
            if let Some(hips) = hips {
                // A thick dragon tail: overlapping scaled segments with a
                // row of spines along the top, curling up at the end.
                let pts = curve(
                    Vec3::new(0.0, 0.0, -0.25),
                    Vec3::new(0.12, -0.55, -1.0),
                    Vec3::X,
                    0.13,
                    0.17,
                    10,
                );
                let radii = taper(10, 0.24);
                part(
                    commands,
                    toon,
                    hips,
                    tube(&pts, &radii, 8),
                    &scale,
                    Transform::IDENTITY,
                );
                let belly = mat(toon, Color::srgb(0.75, 0.82, 0.92), 0.0);
                let spine = mat(toon, Color::srgb(0.16, 0.18, 0.3), 0.0);
                for i in 0..9 {
                    let (a, b) = (pts[i], pts[i + 1]);
                    let along = (b - a).normalize();
                    let r = radii[i];
                    let turn = Quat::from_rotation_arc(Vec3::Y, along);
                    // A band of scales around each segment.
                    part(
                        commands,
                        toon,
                        hips,
                        Sphere::new(r * 1.08).mesh().ico(0).unwrap(),
                        if i % 2 == 0 { &scale } else { &belly },
                        Transform::from_translation(a.lerp(b, 0.5))
                            .with_rotation(turn)
                            .with_scale(Vec3::new(1.0, 0.75, 1.0)),
                    );
                    // A spine on top, leaning back along the tail.
                    let up = (Vec3::Y - along * along.y).normalize_or(Vec3::Y);
                    let fin = blade(&[(0.0, r * 0.6), (r * 0.5, r * 0.45), (r * 1.4, 0.0)], 0.35);
                    let lean =
                        Quat::from_rotation_arc(Vec3::Y, (up * 1.0 + along * 0.8).normalize());
                    part(
                        commands,
                        toon,
                        hips,
                        fin,
                        &spine,
                        Transform::from_translation(a + up * r * 0.8)
                            .with_rotation(lean * Quat::from_rotation_y(FRAC_PI_2)),
                    );
                }
            }
        }
        Race::Demon => {
            let horn = mat(toon, Color::srgb(0.12, 0.05, 0.08), 0.0);
            let red = mat(toon, Color::srgb(0.75, 0.1, 0.18), 0.6);
            for side in [-1.0f32, 1.0] {
                // Up and back, then curving forward (like Castanic).
                let pts = curve(
                    Vec3::new(side * 0.3, 0.8, 0.12),
                    Vec3::new(side * 0.5, 0.8, -0.6),
                    Vec3::X,
                    0.3,
                    0.1,
                    8,
                );
                part(
                    commands,
                    toon,
                    head,
                    tube(&pts, &taper(8, 0.12), 7),
                    &horn,
                    Transform::IDENTITY,
                );
                part(
                    commands,
                    toon,
                    head,
                    Sphere::new(0.12).mesh().ico(1).unwrap(),
                    &horn,
                    Transform::from_translation(pts[0]),
                );
            }
            if let Some(hips) = hips {
                // A long, thin tail in an S-curve, ending in a heart-shaped spade.
                let mut pts = curve(
                    Vec3::new(0.05, 0.3, -0.5),
                    Vec3::new(0.35, -0.3, -1.0),
                    Vec3::X,
                    0.0,
                    0.15,
                    4,
                );
                let end = *pts.last().unwrap();
                pts.extend(
                    curve(end, Vec3::new(0.5, -0.2, -1.0), Vec3::X, 0.1, 0.14, 8)
                        .into_iter()
                        .skip(1),
                );
                let n = pts.len() - 1;
                let radii: Vec<f32> = (0..=n)
                    .map(|i| 0.07 - 0.035 * i as f32 / n as f32)
                    .collect();
                part(
                    commands,
                    toon,
                    hips,
                    tube(&pts, &radii, 6),
                    &horn,
                    Transform::IDENTITY,
                );
                let tip = pts[n];
                let along = (pts[n] - pts[n - 1]).normalize();
                let spade = blade(&[(0.0, 0.04), (0.1, 0.21), (0.24, 0.18), (0.4, 0.0)], 0.25);
                part(
                    commands,
                    toon,
                    hips,
                    spade,
                    &red,
                    Transform::from_translation(tip)
                        .with_rotation(Quat::from_rotation_arc(Vec3::Y, along)),
                );
            }
        }
    }
}

// ---------- gear ----------

fn weapon(commands: &mut Commands, toon: &mut toon::ToonAssets, holder: Entity, tier: Tier) {
    let grip = mat(toon, Color::srgb(0.25, 0.15, 0.1), 0.0);
    match tier {
        Tier::Plain => {}
        Tier::Rare => {
            let steel = mat(toon, Color::srgb(0.82, 0.88, 0.95), 0.0);
            let gold = mat(toon, Color::srgb(0.95, 0.75, 0.3), 0.0);
            let gem = mat(toon, Color::srgb(0.3, 0.6, 1.0), 2.0);
            part(
                commands,
                toon,
                holder,
                Cylinder::new(0.035, 0.28).into(),
                &grip,
                Transform::from_xyz(0.0, 0.0, 0.0),
            );
            part(
                commands,
                toon,
                holder,
                blade(&[(0.0, 0.15), (0.1, 0.13), (0.75, 0.13), (1.0, 0.0)], 0.3),
                &steel,
                Transform::from_xyz(0.0, 0.17, 0.0),
            );
            part(
                commands,
                toon,
                holder,
                blade(&[(0.0, 0.1), (0.15, 0.08), (0.34, 0.0)], 0.6),
                &gold,
                Transform::from_xyz(0.0, 0.17, 0.0)
                    .with_rotation(Quat::from_rotation_z(-FRAC_PI_2 - 0.25)),
            );
            part(
                commands,
                toon,
                holder,
                blade(&[(0.0, 0.1), (0.15, 0.08), (0.34, 0.0)], 0.6),
                &gold,
                Transform::from_xyz(0.0, 0.17, 0.0)
                    .with_rotation(Quat::from_rotation_z(FRAC_PI_2 + 0.25)),
            );
            part(
                commands,
                toon,
                holder,
                Sphere::new(0.07).mesh().ico(1).unwrap(),
                &gem,
                Transform::from_xyz(0.0, 0.17, 0.05),
            );
            part(
                commands,
                toon,
                holder,
                Sphere::new(0.05).mesh().ico(1).unwrap(),
                &gem,
                Transform::from_xyz(0.0, -0.17, 0.0),
            );
        }
        Tier::Epic => {
            let dark = mat(toon, Color::srgb(0.22, 0.16, 0.32), 0.0);
            let rune = mat(toon, Color::srgb(0.85, 0.3, 1.0), 4.0);
            let silver = mat(toon, Color::srgb(0.75, 0.75, 0.85), 0.0);
            part(
                commands,
                toon,
                holder,
                Cylinder::new(0.04, 0.5).into(),
                &grip,
                Transform::from_xyz(0.0, -0.05, 0.0),
            );
            let profile = [
                (0.0, 0.22),
                (0.25, 0.27),
                (0.32, 0.18),
                (1.15, 0.2),
                (1.45, 0.1),
                (1.62, 0.0),
            ];
            part(
                commands,
                toon,
                holder,
                blade(&profile, 0.24),
                &dark,
                Transform::from_xyz(0.0, 0.2, 0.0),
            );
            // Glowing channel down the middle.
            let channel = [(0.0, 0.05), (1.2, 0.05), (1.38, 0.0)];
            part(
                commands,
                toon,
                holder,
                blade(&channel, 1.5),
                &rune,
                Transform::from_xyz(0.0, 0.25, 0.0),
            );
            // Swept guard.
            for side in [-1.0f32, 1.0] {
                part(
                    commands,
                    toon,
                    holder,
                    blade(&[(0.0, 0.07), (0.25, 0.06), (0.42, 0.0)], 0.5),
                    &silver,
                    Transform::from_xyz(0.0, 0.2, 0.0)
                        .with_rotation(Quat::from_rotation_z(-side * 1.15)),
                );
            }
            part(
                commands,
                toon,
                holder,
                Sphere::new(0.07).mesh().ico(1).unwrap(),
                &rune,
                Transform::from_xyz(0.0, 0.22, 0.05),
            );
            part(
                commands,
                toon,
                holder,
                Sphere::new(0.06).mesh().ico(1).unwrap(),
                &rune,
                Transform::from_xyz(0.0, -0.33, 0.0),
            );
        }
        Tier::Legendary => {
            let white = mat(toon, Color::srgb(0.97, 0.95, 0.9), 0.0);
            let gold = mat(toon, Color::srgb(1.0, 0.8, 0.35), 0.4);
            let core = mat(toon, Color::srgb(0.35, 0.95, 1.0), 6.0);
            part(
                commands,
                toon,
                holder,
                Cylinder::new(0.04, 0.5).into(),
                &gold,
                Transform::from_xyz(0.0, -0.05, 0.0),
            );
            // Glowing core blade.
            part(
                commands,
                toon,
                holder,
                blade(&[(0.0, 0.09), (1.5, 0.11), (1.95, 0.0)], 0.5),
                &core,
                Transform::from_xyz(0.0, 0.25, 0.0),
            );
            // Outer edges float apart from the core.
            for side in [-1.0f32, 1.0] {
                let edge = commands
                    .spawn((
                        Transform::default(),
                        Bob {
                            base: Vec3::new(side * 0.22, 0.32, 0.0),
                            amp: 0.03,
                            speed: 2.0,
                            phase: side,
                        },
                        ChildOf(holder),
                    ))
                    .id();
                let shape = [(0.0, 0.07), (0.2, 0.15), (1.2, 0.11), (1.55, 0.0)];
                part(
                    commands,
                    toon,
                    edge,
                    blade(&shape, 0.3),
                    &white,
                    Transform::from_xyz(side * 0.02, 0.0, 0.0),
                );
                // Wing-shaped guard.
                for k in 0..3 {
                    part(
                        commands,
                        toon,
                        holder,
                        blade(
                            &[(0.0, 0.05), (0.2, 0.05), (0.38 - k as f32 * 0.08, 0.0)],
                            0.4,
                        ),
                        if k == 0 { &gold } else { &white },
                        Transform::from_xyz(side * 0.08, 0.22, 0.0)
                            .with_rotation(Quat::from_rotation_z(-side * (1.0 + k as f32 * 0.35))),
                    );
                }
            }
            // A ring of light around the blade.
            let ring = commands
                .spawn((
                    Transform::from_xyz(0.0, 0.6, 0.0),
                    Spin(Vec3::Y, 1.5),
                    ChildOf(holder),
                ))
                .id();
            part(
                commands,
                toon,
                ring,
                Torus::new(0.2, 0.225).into(),
                &core,
                Transform::from_rotation(Quat::from_rotation_x(0.35)),
            );
            part(
                commands,
                toon,
                holder,
                Sphere::new(0.07).mesh().ico(1).unwrap(),
                &core,
                Transform::from_xyz(0.0, -0.33, 0.0),
            );
        }
    }
}

fn staff(commands: &mut Commands, toon: &mut toon::ToonAssets, holder: Entity) {
    let wood = mat(toon, Color::srgb(0.45, 0.3, 0.55), 0.0);
    let gold = mat(toon, Color::srgb(1.0, 0.8, 0.35), 0.4);
    let orb = mat(toon, Color::srgb(1.0, 0.5, 0.2), 6.0);
    part(
        commands,
        toon,
        holder,
        Cylinder::new(0.06, 1.6).into(),
        &wood,
        Transform::from_xyz(0.0, 0.8, 0.0),
    );
    // A crescent of blades holding a floating flame.
    for side in [-1.0f32, 1.0] {
        let pts = curve(
            Vec3::new(0.0, 1.6, 0.0),
            Vec3::new(side, 0.9, 0.0),
            Vec3::Z,
            side * -0.3,
            0.14,
            8,
        );
        part(
            commands,
            toon,
            holder,
            tube(&pts, &taper(8, 0.08), 6),
            &gold,
            Transform::IDENTITY,
        );
    }
    let orb_holder = commands
        .spawn((
            Transform::default(),
            Bob {
                base: Vec3::new(0.0, 2.05, 0.0),
                amp: 0.04,
                speed: 1.6,
                phase: 0.0,
            },
            ChildOf(holder),
        ))
        .id();
    part(
        commands,
        toon,
        orb_holder,
        Sphere::new(0.16).mesh().ico(2).unwrap(),
        &orb,
        Transform::IDENTITY,
    );
    for (axis, speed) in [(Vec3::Y, 2.0), (Vec3::X, -1.4)] {
        let ring = commands
            .spawn((Transform::default(), Spin(axis, speed), ChildOf(orb_holder)))
            .id();
        part(
            commands,
            toon,
            ring,
            Torus::new(0.25, 0.27).into(),
            &gold,
            Transform::from_rotation(Quat::from_rotation_x(0.6)),
        );
    }
}

fn armour(
    commands: &mut Commands,
    toon: &mut toon::ToonAssets,
    chest: Entity,
    root: Entity,
    head: Option<Entity>,
    tier: Tier,
) {
    let (plate, trim, glow) = match tier {
        Tier::Rare => (
            Color::srgb(0.75, 0.82, 0.95),
            Color::srgb(0.95, 0.75, 0.3),
            0.0,
        ),
        Tier::Epic => (
            Color::srgb(0.25, 0.18, 0.36),
            Color::srgb(0.85, 0.3, 1.0),
            3.0,
        ),
        _ => (
            Color::srgb(0.97, 0.95, 0.9),
            Color::srgb(0.35, 0.95, 1.0),
            5.0,
        ),
    };
    let plate = mat(toon, plate, 0.0);
    let trim = mat(toon, trim, glow);
    let gold = mat(toon, Color::srgb(1.0, 0.8, 0.35), 0.3);
    // Shoulder plates.
    for side in [-1.0f32, 1.0] {
        let shoulder = commands
            .spawn((
                Transform::from_xyz(side * 0.42, 0.22, 0.0)
                    .with_rotation(Quat::from_rotation_z(side * -0.35)),
                ChildOf(chest),
            ))
            .id();
        part(
            commands,
            toon,
            shoulder,
            Sphere::new(0.25).mesh().ico(1).unwrap(),
            &plate,
            Transform::from_scale(Vec3::new(1.05, 0.6, 1.0)),
        );
        part(
            commands,
            toon,
            shoulder,
            Torus::new(0.22, 0.26).into(),
            &trim,
            Transform::from_xyz(0.0, -0.04, 0.0).with_scale(Vec3::new(1.05, 1.0, 1.0)),
        );
        if tier != Tier::Rare {
            for k in 0..3 {
                let a = (k as f32 - 1.0) * 0.45;
                let spike = blade(
                    &[
                        (0.0, 0.06),
                        (0.1, 0.05),
                        (0.32 + if tier == Tier::Legendary { 0.12 } else { 0.0 }, 0.0),
                    ],
                    0.6,
                );
                part(
                    commands,
                    toon,
                    shoulder,
                    spike,
                    if tier == Tier::Legendary {
                        &gold
                    } else {
                        &plate
                    },
                    Transform::from_xyz(0.0, 0.1, a * 0.3).with_rotation(
                        Quat::from_rotation_z(side * -0.5) * Quat::from_rotation_x(a),
                    ),
                );
            }
        }
    }
    // Belt gem.
    part(
        commands,
        toon,
        chest,
        Sphere::new(0.07).mesh().ico(1).unwrap(),
        &trim,
        Transform::from_xyz(0.0, -0.05, 0.38),
    );
    if tier != Tier::Legendary {
        if tier == Tier::Epic {
            motes(commands, toon, root, &trim, 6, 0.75);
        }
        return;
    }
    // Wings of floating blades.
    let core = mat(toon, Color::srgb(0.35, 0.95, 1.0), 5.0);
    for side in [-1.0f32, 1.0] {
        for k in 0..4 {
            let len = 0.9 - k as f32 * 0.15;
            let shard = commands
                .spawn((
                    Transform::default(),
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
                    ChildOf(chest),
                ))
                .id();
            let tilt = Quat::from_rotation_z(-side * (0.75 + k as f32 * 0.38))
                * Quat::from_rotation_y(side * 0.35);
            part(
                commands,
                toon,
                shard,
                blade(&[(0.0, 0.03), (0.15, 0.09), (len, 0.0)], 0.2),
                if k % 2 == 0 { &core } else { &gold },
                Transform::from_rotation(tilt),
            );
        }
    }
    // Halo.
    if let Some(head) = head {
        let halo = commands
            .spawn((
                Transform::from_xyz(0.0, 1.25, -0.25).with_rotation(Quat::from_rotation_x(-0.35)),
                ChildOf(head),
            ))
            .id();
        let spinner = commands
            .spawn((Transform::default(), Spin(Vec3::Y, 0.8), ChildOf(halo)))
            .id();
        part(
            commands,
            toon,
            spinner,
            Torus::new(0.36, 0.4).into(),
            &core,
            Transform::IDENTITY,
        );
        for k in 0..6 {
            let a = k as f32 / 6.0 * TAU;
            part(
                commands,
                toon,
                spinner,
                blade(&[(0.0, 0.04), (0.06, 0.04), (0.14, 0.0)], 0.5),
                &gold,
                Transform::from_xyz(a.cos() * 0.38, 0.0, a.sin() * 0.38),
            );
        }
    }
    motes(commands, toon, root, &core, 10, 0.9);
}

/// Little lights circling the feet.
fn motes(
    commands: &mut Commands,
    toon: &mut toon::ToonAssets,
    root: Entity,
    m: &Handle<ToonMaterial>,
    n: usize,
    r: f32,
) {
    let ring = commands
        .spawn((
            Transform::from_xyz(0.0, 0.1, 0.0),
            Spin(Vec3::Y, 0.9),
            ChildOf(root),
        ))
        .id();
    for k in 0..n {
        let a = k as f32 / n as f32 * TAU;
        let mote = commands
            .spawn((
                Transform::default(),
                Bob {
                    base: Vec3::new(a.cos() * r, 0.25 + (k % 3) as f32 * 0.25, a.sin() * r),
                    amp: 0.12,
                    speed: 1.3,
                    phase: a * 2.0,
                },
                ChildOf(ring),
            ))
            .id();
        part(
            commands,
            toon,
            mote,
            blade(&[(0.0, 0.0), (0.05, 0.04), (0.1, 0.0)], 1.0),
            m,
            Transform::IDENTITY,
        );
    }
}

fn run(
    time: Res<Time>,
    mut script: ResMut<Script>,
    mut camera: Single<&mut Transform, With<Camera3d>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let t = time.elapsed_secs();
    let i = script.1;
    if i >= script.0.len() || t < script.0[i].0 {
        return;
    }
    let (_, eye, at, shot) = script.0[i];
    script.1 += 1;
    if i == script.0.len() - 1 {
        exit.write(AppExit::Success);
        return;
    }
    **camera = Transform::from_translation(eye).looking_at(at, Vec3::Y);
    if shot {
        let prefix = std::env::var("SHOTS").unwrap_or("gear".into());
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(format!("{prefix}-{i}.png")));
    }
}
