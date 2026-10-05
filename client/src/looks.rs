//! A player's look (`shared::appearance::Appearance`) on their model: the
//! chosen face and hair (a head from one of the pack's bodies, attached to
//! the head bone), skin and hair recoloured, the race's own pieces (ears,
//! horns, tails, scales) and height. Rebuilt whenever the look or the body
//! changes (the creation screen changes it on every click).

use std::collections::HashMap;

use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::mesh::{Mesh, VertexAttributeValues};
use bevy::prelude::*;
use shared::appearance::Appearance;
use shared::gamedata::GameData;

use crate::animation::BodyMaterials;
use crate::models::{ModelLibrary, Rig};
use crate::shapes::{Part, blade, curve, is_skin, split_by_colour, taper, tint_towards, tube};
use crate::toon::{
    OUTLINE_COLOR, OUTLINE_THICKNESS, OutlineMaterial, OutlineSettings, ToonExtension, ToonMaterial,
};

/// Every piece a race feature can add (`models.ron` `features`).
pub const PARTS: &[&str] = &[
    "long_ears",
    "swept_ears",
    "cat_ears",
    "lynx_ears",
    "swept_horns",
    "crowned_horns",
    "rising_horns",
    "ram_horns",
    "cheek_scales",
    "dragon_tail",
    "demon_tail",
    "cat_tail",
];

pub struct LooksPlugin;

impl Plugin for LooksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HeadCache>()
            .add_systems(Startup, load_head_files)
            .add_systems(Update, apply_looks);
    }
}

/// A head ready to wear: its pieces moved into the head bone's own space,
/// and the average colour of the skin and hair in its texture.
struct Head {
    material: StandardMaterial,
    skin: Option<(Handle<Mesh>, Vec3)>,
    hair: Option<(Handle<Mesh>, Vec3)>,
    other: Option<Handle<Mesh>>,
}

/// A body part split into its skin and the rest.
struct SplitPart {
    skin: Handle<Mesh>,
    other: Handle<Mesh>,
    average: Vec3,
}

#[derive(Resource, Default)]
struct HeadCache {
    files: HashMap<String, Handle<Gltf>>,
    heads: HashMap<String, Head>,
    /// Body meshes already split (`None`: no skin in them).
    parts: HashMap<AssetId<Mesh>, Option<SplitPart>>,
}

fn load_head_files(
    models: Res<ModelLibrary>,
    assets: Res<AssetServer>,
    mut cache: ResMut<HeadCache>,
) {
    for head in models.heads.values() {
        cache
            .files
            .entry(head.file.clone())
            .or_insert_with(|| assets.load(head.file.clone()));
    }
}

/// What a character's look is built from; kept so it can be rebuilt.
#[derive(Component)]
struct Dressed {
    /// The model it was built on (a new body means starting again).
    root: Entity,
    /// Hangs from the head bone, sized like the reshaped head.
    head: Entity,
    /// Hangs from the hips bone.
    hips: Entity,
    /// Skin pieces of the body, with their texture's average colour.
    skin: Vec<(Handle<ToonMaterial>, Vec3)>,
    /// The head and race pieces (rebuilt with the look).
    pieces: Vec<Entity>,
    capes: Vec<Entity>,
    built: Option<Appearance>,
}

#[derive(Clone, Copy)]
enum Paint {
    Skin,
    Hair,
    Feature,
    /// A fixed colour (the inside of cat ears).
    Fixed(Color),
    /// The feature colour, darker (spines, tail tips).
    FeatureDark,
}

#[derive(Clone, Copy, PartialEq)]
enum On {
    Head,
    Hips,
}

/// One piece of a race feature.
struct Piece {
    mesh: Mesh,
    at: Transform,
    paint: Paint,
    on: On,
}

fn piece(mesh: Mesh, at: Transform, paint: Paint, on: On) -> Piece {
    Piece {
        mesh,
        at,
        paint,
        on,
    }
}

fn horn(start: Vec3, dir: Vec3, bend: f32, step: f32, n: usize, radius: f32) -> Vec<Piece> {
    let pts = curve(start, dir, Vec3::X, bend, step, n);
    vec![
        piece(
            tube(&pts, &taper(n, radius), 7),
            Transform::IDENTITY,
            Paint::Feature,
            On::Head,
        ),
        // Rooted in the head, not floating above it.
        piece(
            Sphere::new(radius)
                .mesh()
                .ico(1)
                .unwrap_or_else(|_| Sphere::new(radius).mesh().uv(8, 6)),
            Transform::from_translation(start),
            Paint::Feature,
            On::Head,
        ),
    ]
}

fn ball(radius: f32) -> Mesh {
    Sphere::new(radius).mesh().uv(10, 7)
}

/// Build one piece (in the head bone's or hips bone's own space; the model
/// faces +Z, so the back of the head is -Z).
fn build_part(part: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let sides = [-1.0f32, 1.0];
    match part {
        "long_ears" | "swept_ears" => {
            let (profile, tilt): (&[(f32, f32)], f32) = if part == "long_ears" {
                (&[(0.0, 0.11), (0.18, 0.1), (0.45, 0.04), (0.6, 0.0)], -0.45)
            } else {
                (&[(0.0, 0.1), (0.2, 0.09), (0.55, 0.03), (0.78, 0.0)], -0.95)
            };
            for side in sides {
                out.push(piece(
                    blade(profile, 0.3),
                    Transform::from_xyz(side * 0.5, 0.62, -0.02).with_rotation(
                        Quat::from_rotation_z(-side * 1.15) * Quat::from_rotation_x(tilt),
                    ),
                    Paint::Skin,
                    On::Head,
                ));
            }
        }
        "cat_ears" | "lynx_ears" => {
            let lynx = part == "lynx_ears";
            let outer: &[(f32, f32)] = if lynx {
                &[(0.0, 0.18), (0.2, 0.13), (0.48, 0.0)]
            } else {
                &[(0.0, 0.19), (0.14, 0.15), (0.38, 0.0)]
            };
            for side in sides {
                let at = Transform::from_xyz(side * 0.3, 0.78, 0.0).with_rotation(
                    Quat::from_rotation_z(-side * 0.3) * Quat::from_rotation_x(-0.12),
                );
                out.push(piece(blade(outer, 0.4), at, Paint::Hair, On::Head));
                out.push(piece(
                    blade(&[(0.05, 0.11), (0.15, 0.08), (0.3, 0.0)], 0.2),
                    at * Transform::from_xyz(0.0, 0.0, 0.04),
                    Paint::Fixed(Color::srgb(0.98, 0.7, 0.7)),
                    On::Head,
                ));
                if lynx {
                    // A dark tuft at the tip.
                    out.push(piece(
                        blade(&[(0.0, 0.03), (0.08, 0.02), (0.2, 0.0)], 0.6),
                        at * Transform::from_xyz(0.0, 0.46, 0.0),
                        Paint::Fixed(Color::srgb(0.1, 0.08, 0.08)),
                        On::Head,
                    ));
                }
            }
        }
        "swept_horns" => {
            for side in sides {
                out.extend(horn(
                    Vec3::new(side * 0.4, 0.74, 0.08),
                    Vec3::new(side * 0.3, 0.0, -1.0),
                    0.09,
                    0.15,
                    8,
                    0.15,
                ));
            }
        }
        "crowned_horns" => {
            for side in sides {
                out.extend(horn(
                    Vec3::new(side * 0.32, 0.84, 0.12),
                    Vec3::new(side * 0.55, 1.0, -0.25),
                    0.22,
                    0.12,
                    7,
                    0.12,
                ));
            }
        }
        "rising_horns" => {
            for side in sides {
                out.extend(horn(
                    Vec3::new(side * 0.3, 0.8, 0.12),
                    Vec3::new(side * 0.5, 0.8, -0.6),
                    0.3,
                    0.1,
                    8,
                    0.12,
                ));
            }
        }
        "ram_horns" => {
            for side in sides {
                out.extend(horn(
                    Vec3::new(side * 0.42, 0.78, 0.0),
                    Vec3::new(side * 0.35, 0.6, -0.7),
                    0.5,
                    0.1,
                    12,
                    0.14,
                ));
            }
        }
        "cheek_scales" => {
            for side in sides {
                for k in 0..3 {
                    let k = k as f32;
                    out.push(piece(
                        blade(&[(0.0, 0.05), (0.06, 0.05), (0.1, 0.0)], 0.4),
                        Transform::from_xyz(side * 0.53, 0.38 + k * 0.08, 0.1 - k * 0.05)
                            .with_rotation(Quat::from_rotation_z(side * 0.4)),
                        Paint::Feature,
                        On::Head,
                    ));
                }
            }
        }
        "dragon_tail" => {
            // Thick and scaly, with spines along the top, curling up.
            let n = 10;
            let pts = curve(
                Vec3::new(0.0, 0.0, -0.25),
                Vec3::new(0.12, -0.55, -1.0),
                Vec3::X,
                0.13,
                0.17,
                n,
            );
            let radii = taper(n, 0.24);
            out.push(piece(
                tube(&pts, &radii, 8),
                Transform::IDENTITY,
                Paint::Feature,
                On::Hips,
            ));
            for i in 0..n - 1 {
                let (a, b) = (pts[i], pts[i + 1]);
                let along = (b - a).normalize_or(Vec3::NEG_Z);
                let r = radii[i];
                out.push(piece(
                    ball(r * 1.08),
                    Transform::from_translation(a.lerp(b, 0.5))
                        .with_rotation(Quat::from_rotation_arc(Vec3::Y, along))
                        .with_scale(Vec3::new(1.0, 0.75, 1.0)),
                    if i % 2 == 0 {
                        Paint::Feature
                    } else {
                        Paint::FeatureDark
                    },
                    On::Hips,
                ));
                let up = (Vec3::Y - along * along.y).normalize_or(Vec3::Y);
                let lean = Quat::from_rotation_arc(Vec3::Y, (up + along * 0.8).normalize());
                out.push(piece(
                    blade(&[(0.0, r * 0.6), (r * 0.5, r * 0.45), (r * 1.4, 0.0)], 0.35),
                    Transform::from_translation(a + up * r * 0.8)
                        .with_rotation(lean * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
                    Paint::FeatureDark,
                    On::Hips,
                ));
            }
        }
        "demon_tail" => {
            // Long and thin, in an S-curve, ending in a heart-shaped spade.
            let mut pts = curve(
                Vec3::new(0.05, 0.3, -0.5),
                Vec3::new(0.35, -0.3, -1.0),
                Vec3::X,
                0.0,
                0.15,
                4,
            );
            let end = pts[pts.len() - 1];
            pts.extend(
                curve(end, Vec3::new(0.45, -0.25, -1.0), Vec3::X, 0.16, 0.14, 8)
                    .into_iter()
                    .skip(1),
            );
            let n = pts.len() - 1;
            let radii: Vec<f32> = (0..=n)
                .map(|i| 0.07 - 0.035 * i as f32 / n as f32)
                .collect();
            out.push(piece(
                tube(&pts, &radii, 6),
                Transform::IDENTITY,
                Paint::Feature,
                On::Hips,
            ));
            let along = (pts[n] - pts[n - 1]).normalize_or(Vec3::NEG_Z);
            out.push(piece(
                blade(&[(0.0, 0.04), (0.1, 0.21), (0.24, 0.18), (0.4, 0.0)], 0.25),
                Transform::from_translation(pts[n])
                    .with_rotation(Quat::from_rotation_arc(Vec3::Y, along)),
                Paint::FeatureDark,
                On::Hips,
            ));
        }
        "cat_tail" => {
            let n = 12;
            let pts = curve(
                Vec3::new(0.0, 0.3, -0.5),
                Vec3::new(-0.12, -0.55, -1.0),
                Vec3::X,
                0.24,
                0.12,
                n,
            );
            let radii: Vec<f32> = (0..=n)
                .map(|i| 0.075 - 0.02 * i as f32 / n as f32)
                .collect();
            out.push(piece(
                tube(&pts, &radii, 7),
                Transform::IDENTITY,
                Paint::Hair,
                On::Hips,
            ));
            out.push(piece(
                ball(0.07),
                Transform::from_translation(pts[n]),
                Paint::Fixed(Color::srgb(0.95, 0.9, 0.85)),
                On::Hips,
            ));
        }
        _ => {}
    }
    out
}

fn has_tail(parts: &[String]) -> bool {
    parts.iter().any(|p| p.ends_with("_tail"))
}

/// Turn a head mesh (in its file's model space, skinned) into a plain mesh
/// in the head bone's own space, split into skin, hair and the rest.
fn prepare_head(
    mesh: &Mesh,
    image: &Image,
    head_rest: Mat4,
    meshes: &mut Assets<Mesh>,
    material: StandardMaterial,
) -> Head {
    let mut plain = mesh.clone();
    plain.remove_attribute(Mesh::ATTRIBUTE_JOINT_INDEX);
    plain.remove_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT);
    let to_local = head_rest.inverse();
    if let Some(VertexAttributeValues::Float32x3(positions)) =
        plain.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for p in positions.iter_mut() {
            *p = to_local.transform_point3(Vec3::from(*p)).to_array();
        }
    }
    if let Some(VertexAttributeValues::Float32x3(normals)) =
        plain.attribute_mut(Mesh::ATTRIBUTE_NORMAL)
    {
        for n in normals.iter_mut() {
            *n = to_local
                .transform_vector3(Vec3::from(*n))
                .normalize_or_zero()
                .to_array();
        }
    }
    let pieces = split_by_colour(&plain, image, |middle, colour| {
        let dark = u32::from(colour[0]) + u32::from(colour[1]) + u32::from(colour[2]) < 180;
        if is_skin(colour) {
            Part::Skin
        } else if dark && middle.z > 0.38 && (0.3..0.6).contains(&middle.y) {
            // Eyes, brows and mouth on the front of the face.
            Part::Other
        } else {
            Part::Hair
        }
    });
    let mut add = |part: Part| {
        pieces
            .get(&part)
            .map(|p| (meshes.add(p.mesh.clone()), p.average))
    };
    Head {
        material,
        skin: add(Part::Skin),
        hair: add(Part::Hair),
        other: add(Part::Other).map(|(m, _)| m),
    }
}

fn swatch(list: &[shared::appearance::Swatch], index: usize) -> Option<Vec3> {
    list.get(index)
        .map(|s| Vec3::new(s.color.0, s.color.1, s.color.2))
}

fn outline_hull(
    commands: &mut Commands,
    outlines: &mut Assets<OutlineMaterial>,
    mesh: Handle<Mesh>,
    parent: Entity,
) {
    let material = outlines.add(OutlineMaterial {
        settings: OutlineSettings {
            color: OUTLINE_COLOR,
            params: Vec4::new(OUTLINE_THICKNESS, 0.0, 0.0, 0.0),
        },
    });
    commands.spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        NotShadowCaster,
        ChildOf(parent),
    ));
}

fn toon_from(base: StandardMaterial) -> ToonMaterial {
    ToonMaterial {
        base: StandardMaterial {
            perceptual_roughness: 1.0,
            metallic: 0.0,
            ..base
        },
        extension: ToonExtension::default(),
    }
}

/// Build (or rebuild) the look of every character whose look or body
/// changed.
fn apply_looks(
    mut commands: Commands,
    data: Res<GameData>,
    models: Res<ModelLibrary>,
    mut cache: ResMut<HeadCache>,
    mut characters: Query<(Entity, &Rig, &Appearance, Option<&mut Dressed>)>,
    nodes: Query<(&Name, Option<&Children>)>,
    children: Query<&Children>,
    parts: Query<(
        &Mesh3d,
        &MeshMaterial3d<ToonMaterial>,
        &SkinnedMesh,
        &ChildOf,
    )>,
    mut transforms: Query<&mut Transform>,
    (gltfs, gltf_nodes, gltf_meshes, gltf_materials, images): (
        Res<Assets<Gltf>>,
        Res<Assets<bevy::gltf::GltfNode>>,
        Res<Assets<bevy::gltf::GltfMesh>>,
        Res<Assets<bevy::gltf::GltfMaterial>>,
        Res<Assets<Image>>,
    ),
    (mut meshes, mut toon, mut outlines): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<ToonMaterial>>,
        ResMut<Assets<OutlineMaterial>>,
    ),
) {
    for (character, rig, look, dressed) in &mut characters {
        let fresh = dressed.as_ref().is_none_or(|d| d.root != rig.root);
        if !fresh
            && dressed
                .as_ref()
                .is_some_and(|d| d.built.as_ref() == Some(look))
        {
            continue;
        }
        let Some(race) = data.races.get(&look.race) else {
            continue;
        };
        // The head must be loaded before anything is built.
        let Some(face) = race.faces.get(look.face) else {
            continue;
        };
        let Some(head_def) = models.heads.get(&face.id) else {
            continue;
        };
        if !cache.heads.contains_key(&face.id) {
            let Some(gltf) = cache.files.get(&head_def.file).and_then(|h| gltfs.get(h)) else {
                continue;
            };
            let Some(primitive) = gltf
                .named_nodes
                .get(head_def.mesh.as_str())
                .and_then(|n| gltf_nodes.get(n))
                .and_then(|n| n.mesh.as_ref())
                .and_then(|m| gltf_meshes.get(m))
                .and_then(|m| m.primitives.first())
            else {
                continue;
            };
            let Some(source) = primitive
                .material
                .as_ref()
                .and_then(|m| gltf_materials.get(m))
            else {
                continue;
            };
            let material = StandardMaterial {
                base_color: source.base_color,
                base_color_texture: source.base_color_texture.clone(),
                ..default()
            };
            let Some(mesh) = meshes.get(&primitive.mesh).cloned() else {
                continue;
            };
            let Some(image) = source
                .base_color_texture
                .as_ref()
                .and_then(|t| images.get(t))
            else {
                continue;
            };
            let head = prepare_head(&mesh, image, rig.head_rest, &mut meshes, material);
            cache.heads.insert(face.id.clone(), head);
        }

        let mut state = if fresh {
            let Some(new) = start_dressing(
                &mut commands,
                &models,
                &mut cache,
                rig,
                &nodes,
                &children,
                &parts,
                &images,
                &mut meshes,
                &mut toon,
                &mut outlines,
            ) else {
                continue;
            };
            new
        } else {
            match dressed {
                Some(mut d) => std::mem::replace(
                    &mut *d,
                    Dressed {
                        root: rig.root,
                        head: Entity::PLACEHOLDER,
                        hips: Entity::PLACEHOLDER,
                        skin: Vec::new(),
                        pieces: Vec::new(),
                        capes: Vec::new(),
                        built: None,
                    },
                ),
                None => continue,
            }
        };

        // Throw away the old head and pieces.
        for old in state.pieces.drain(..) {
            commands.entity(old).despawn();
        }
        let skin = swatch(&race.skins, look.skin).unwrap_or(Vec3::ONE);
        let hair = swatch(&race.hair, look.hair).unwrap_or(Vec3::ONE);
        let feature = swatch(&race.feature_colors, look.feature_color).unwrap_or(hair);

        // Recolour the body's skin.
        for (material, average) in &state.skin {
            if let Some(mut m) = toon.get_mut(material) {
                m.base.base_color = tint_towards(*average, skin);
            }
        }

        // The face and hair.
        if let Some(head) = cache.heads.get(&face.id) {
            let base = head.material.clone();
            let mut wear = |mesh: &Handle<Mesh>, colour: Option<Color>| {
                let mut material = toon_from(base.clone());
                if let Some(colour) = colour {
                    material.base.base_color = colour;
                }
                let e = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(toon.add(material)),
                        Transform::IDENTITY,
                        ChildOf(state.head),
                    ))
                    .id();
                outline_hull(&mut commands, &mut outlines, mesh.clone(), e);
                state.pieces.push(e);
            };
            if let Some((mesh, average)) = &head.skin {
                wear(mesh, Some(tint_towards(*average, skin)));
            }
            if let Some((mesh, average)) = &head.hair {
                wear(mesh, Some(tint_towards(*average, hair)));
            }
            if let Some(mesh) = &head.other {
                wear(mesh, None);
            }
        }

        // The race's own pieces.
        let feature_parts = race
            .features
            .get(look.feature)
            .and_then(|f| models.features.get(&f.id))
            .cloned()
            .unwrap_or_default();
        for part in &feature_parts {
            for p in build_part(part) {
                let colour = match p.paint {
                    Paint::Skin => Color::srgb(skin.x, skin.y, skin.z),
                    Paint::Hair => Color::srgb(hair.x, hair.y, hair.z),
                    Paint::Feature => Color::srgb(feature.x, feature.y, feature.z),
                    Paint::FeatureDark => {
                        let d = feature * 0.55;
                        Color::srgb(d.x, d.y, d.z)
                    }
                    Paint::Fixed(c) => c,
                };
                let mesh = meshes.add(p.mesh);
                let e = commands
                    .spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(toon.add(toon_from(StandardMaterial {
                            base_color: colour,
                            cull_mode: None,
                            double_sided: true,
                            ..default()
                        }))),
                        p.at,
                        ChildOf(if p.on == On::Head {
                            state.head
                        } else {
                            state.hips
                        }),
                    ))
                    .id();
                outline_hull(&mut commands, &mut outlines, mesh, e);
                state.pieces.push(e);
            }
        }
        // Capes would cover a tail.
        let cape = if has_tail(&feature_parts) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        for e in &state.capes {
            commands.entity(*e).insert(cape);
        }

        // Height.
        let height = look.height_scale(&data.races);
        if let Ok(mut t) = transforms.get_mut(rig.root) {
            let scale = models.proportions.scale * height;
            t.scale = Vec3::splat(scale);
            t.translation.y = rig.lift * scale;
        }

        state.built = Some(look.clone());
        // The hit flash collects the new materials again.
        commands
            .entity(character)
            .remove::<BodyMaterials>()
            .insert(state);
    }
}

/// First time on a body: hide the pack's own face, hair and hats, split the
/// body's skin out so it can be recoloured, and add the places the look's
/// pieces hang from.
fn start_dressing(
    commands: &mut Commands,
    models: &ModelLibrary,
    cache: &mut HeadCache,
    rig: &Rig,
    nodes: &Query<(&Name, Option<&Children>)>,
    children: &Query<&Children>,
    parts: &Query<(
        &Mesh3d,
        &MeshMaterial3d<ToonMaterial>,
        &SkinnedMesh,
        &ChildOf,
    )>,
    images: &Assets<Image>,
    meshes: &mut Assets<Mesh>,
    toon: &mut Assets<ToonMaterial>,
    outlines: &mut Assets<OutlineMaterial>,
) -> Option<Dressed> {
    let head_bone = *rig.bones.get("head")?;
    let hips_bone = *rig.bones.get("hips")?;
    let mut capes = Vec::new();
    for e in children.iter_descendants(rig.root) {
        let Ok((name, _)) = nodes.get(e) else {
            continue;
        };
        if models
            .player_hides
            .iter()
            .any(|h| name.as_str().contains(h.as_str()))
        {
            commands.entity(e).insert(Visibility::Hidden);
        }
        if name.as_str().contains("_Cape") {
            capes.push(e);
        }
    }
    let mut skin = Vec::new();
    for e in children.iter_descendants(rig.root) {
        let Ok((mesh, material, skinned, parent)) = parts.get(e) else {
            continue;
        };
        let part_name = nodes
            .get(parent.parent())
            .map(|(n, _)| n.as_str())
            .unwrap_or("");
        if !["Body", "Arm", "Leg"].iter().any(|p| part_name.contains(p)) {
            continue;
        }
        let Some(base) = toon.get(&material.0).map(|m| m.base.clone()) else {
            continue;
        };
        let id = mesh.0.id();
        if let std::collections::hash_map::Entry::Vacant(entry) = cache.parts.entry(id) {
            let split = match (
                meshes.get(&mesh.0),
                base.base_color_texture.as_ref().and_then(|t| images.get(t)),
            ) {
                (Some(m), Some(image)) => {
                    let pieces = split_by_colour(m, image, |_, c| {
                        if is_skin(c) { Part::Skin } else { Part::Other }
                    });
                    match (pieces.get(&Part::Skin), pieces.get(&Part::Other)) {
                        (Some(s), Some(o)) => Some(SplitPart {
                            skin: meshes.add(s.mesh.clone()),
                            other: meshes.add(o.mesh.clone()),
                            average: s.average,
                        }),
                        _ => None,
                    }
                }
                _ => None,
            };
            entry.insert(split);
        }
        let Some(Some(split)) = cache.parts.get(&id) else {
            continue;
        };
        // The clothes stay on the original; the skin becomes its own piece
        // with its own material.
        commands.entity(e).insert(Mesh3d(split.other.clone()));
        let material = toon.add(toon_from(base));
        let piece = commands
            .spawn((
                Mesh3d(split.skin.clone()),
                MeshMaterial3d(material.clone()),
                skinned.clone(),
                NoFrustumCulling,
                Transform::IDENTITY,
                ChildOf(parent.parent()),
            ))
            .id();
        let hull_material = outlines.add(OutlineMaterial {
            settings: OutlineSettings {
                color: OUTLINE_COLOR,
                params: Vec4::new(OUTLINE_THICKNESS, 0.0, 0.0, 0.0),
            },
        });
        commands.spawn((
            Mesh3d(split.skin.clone()),
            MeshMaterial3d(hull_material),
            skinned.clone(),
            NoFrustumCulling,
            NotShadowCaster,
            ChildOf(piece),
        ));
        skin.push((material, split.average));
    }
    let head_shape = models.bone_shape("head").unwrap_or(Vec3::ONE);
    let head = commands
        .spawn((
            Transform::from_scale(head_shape),
            Visibility::default(),
            ChildOf(head_bone),
        ))
        .id();
    let hips = commands
        .spawn((
            Transform::IDENTITY,
            Visibility::default(),
            ChildOf(hips_bone),
        ))
        .id();
    Some(Dressed {
        root: rig.root,
        head,
        hips,
        skin,
        pieces: Vec::new(),
        capes,
        built: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_builds_something() {
        for part in PARTS {
            assert!(!build_part(part).is_empty(), "{part}");
        }
        assert!(build_part("no_such_piece").is_empty());
    }

    #[test]
    fn tails_are_noticed() {
        assert!(has_tail(&["swept_horns".into(), "dragon_tail".into()]));
        assert!(!has_tail(&["long_ears".into()]));
    }
}
