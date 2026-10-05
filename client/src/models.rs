//! Real character models (KayKit, `assets/data/client/models.ron`): bodies
//! made longer and slimmer by stretching bones, the toon look with outlines
//! that follow the skeleton, weapons in the hands by class and
//! specialization, and animations chosen from what the character is doing
//! (idle, run, jump, cast, attack, ride, defeated). Looks only: everything
//! here reads logic components and never changes them.

use std::collections::HashMap;
use std::time::Duration;

use bevy::animation::RepeatAnimation;
use bevy::app::AnimationSystems;
use bevy::light::NotShadowCaster;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use serde::Deserialize;
use server::{Defeated, FlameChange, Riding};
use shared::classes::CurrentClass;
use shared::combat::ActionState;
use shared::data::{DataError, Problems, Validate, load_ron};
use shared::gamedata::{GameData, Zones};
use shared::protocol::ServerEvent;

use crate::animation::BodyMaterials;
use crate::characters::DisplayMotion;
use crate::hud::game_now;
use crate::session::Received;
use crate::toon::{
    OUTLINE_COLOR, OUTLINE_THICKNESS, OutlineMaterial, OutlineSettings, ToonExtension, ToonMaterial,
};

pub struct ModelsPlugin;

impl Plugin for ModelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AnimationLibrary>()
            .add_systems(
                Update,
                (
                    dress_new_characters,
                    follow_class_changes,
                    start_actions,
                    choose_animations,
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                (
                    stretch_bones
                        .after(AnimationSystems)
                        .before(TransformSystems::Propagate),
                    remove_old_models,
                ),
            );
    }
}

// ---------- data ----------

/// How an armed character stands and swings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Style {
    OneHanded,
    TwoHanded,
    DualWield,
    Caster,
}

/// The bone stretch that makes bodies longer and slimmer.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Proportions {
    pub scale: f32,
    pub head: f32,
    pub legs: f32,
    pub arms: f32,
    pub spine: f32,
    pub chest: f32,
    pub slim: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BodyDef {
    pub file: String,
    #[serde(default)]
    pub hide: Vec<String>,
    #[serde(default)]
    pub tint: Option<(f32, f32, f32)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WeaponDef {
    pub file: String,
    #[serde(default)]
    pub offset: (f32, f32, f32),
    /// How the item is turned in the hand, as a quaternion (x, y, z, w)
    /// copied from the pack's own models. Without one, items in the right
    /// hand are turned half way round (the hand bones mirror each other).
    #[serde(default)]
    pub rotation: Option<(f32, f32, f32, f32)>,
}

/// What is held in each hand, and how it is used.
#[derive(Debug, Clone, Deserialize)]
pub struct Loadout {
    #[serde(default)]
    pub right: Option<String>,
    #[serde(default)]
    pub left: Option<String>,
    pub style: Style,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClassLook {
    pub body: String,
    pub specs: HashMap<String, Loadout>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PersonLook {
    pub body: String,
    #[serde(default)]
    pub right: Option<String>,
    #[serde(default)]
    pub left: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnimationNames {
    pub idle: HashMap<Style, String>,
    pub attacks: HashMap<Style, Vec<String>>,
    pub run: String,
    pub jump: String,
    pub cast: String,
    pub release: String,
    pub lantern: String,
    pub ride: String,
    pub defeated: String,
    pub run_above: f32,
    pub blend: f32,
    pub attack_speed: f32,
}

/// Every character look (`assets/data/client/models.ron`).
#[derive(Resource, Debug, Clone, Deserialize)]
pub struct ModelLibrary {
    pub proportions: Proportions,
    pub bodies: HashMap<String, BodyDef>,
    pub weapons: HashMap<String, WeaponDef>,
    pub classes: HashMap<String, ClassLook>,
    pub people: HashMap<String, PersonLook>,
    pub animations: AnimationNames,
}

const STYLES: [Style; 4] = [
    Style::OneHanded,
    Style::TwoHanded,
    Style::DualWield,
    Style::Caster,
];

impl Validate for ModelLibrary {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        let pr = &self.proportions;
        for (name, value) in [
            ("scale", pr.scale),
            ("head", pr.head),
            ("legs", pr.legs),
            ("arms", pr.arms),
            ("spine", pr.spine),
            ("chest", pr.chest),
            ("slim", pr.slim),
        ] {
            p.positive(&format!("proportions.{name}"), value);
        }
        p.non_negative("animations.run_above", self.animations.run_above);
        p.non_negative("animations.blend", self.animations.blend);
        p.positive("animations.attack_speed", self.animations.attack_speed);
        for style in STYLES {
            if !self.animations.idle.contains_key(&style) {
                p.push(format!("animations.idle: missing {style:?}"));
            }
            if self
                .animations
                .attacks
                .get(&style)
                .is_none_or(Vec::is_empty)
            {
                p.push(format!("animations.attacks: missing {style:?}"));
            }
        }
        let mut body = |user: String, name: &str| {
            if !self.bodies.contains_key(name) {
                p.push(format!("{user}: no body called `{name}`"));
            }
        };
        for (id, class) in &self.classes {
            body(format!("classes.{id}"), &class.body);
        }
        for (key, person) in &self.people {
            body(format!("people.{key}"), &person.body);
        }
        let mut weapon = |user: String, name: &Option<String>| {
            if let Some(name) = name
                && !self.weapons.contains_key(name)
            {
                p.push(format!("{user}: no weapon called `{name}`"));
            }
        };
        for (id, class) in &self.classes {
            for (spec, loadout) in &class.specs {
                weapon(format!("classes.{id}.{spec}"), &loadout.right);
                weapon(format!("classes.{id}.{spec}"), &loadout.left);
            }
        }
        for (key, person) in &self.people {
            weapon(format!("people.{key}"), &person.right);
            weapon(format!("people.{key}"), &person.left);
        }
        p.0
    }
}

impl ModelLibrary {
    pub fn load(assets_dir: &std::path::Path) -> Result<Self, DataError> {
        load_ron(&assets_dir.join("data").join("client").join("models.ron"))
    }

    /// Check that every class and specialization has a look, that the
    /// model files exist, and that townsfolk looks match zone data.
    pub fn check_references(
        &self,
        assets_dir: &std::path::Path,
        data: &GameData,
        zones: &Zones,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        for (id, class) in &data.classes {
            let Some(look) = self.classes.get(id) else {
                problems.push(format!("models.ron: class `{id}` has no look in `classes`"));
                continue;
            };
            for spec in &class.specializations {
                if !look.specs.contains_key(&spec.id) {
                    problems.push(format!(
                        "models.ron: classes.{id} has no loadout for specialization `{}`",
                        spec.id
                    ));
                }
            }
        }
        for id in self.classes.keys() {
            if !data.classes.contains_key(id) {
                problems.push(format!("models.ron: classes.{id}: no such class"));
            }
        }
        let files = self
            .bodies
            .iter()
            .map(|(k, b)| (format!("bodies.{k}"), &b.file))
            .chain(
                self.weapons
                    .iter()
                    .map(|(k, w)| (format!("weapons.{k}"), &w.file)),
            );
        for (user, file) in files {
            let path: std::path::PathBuf = file.split('/').collect();
            if !assets_dir.join(path).is_file() {
                problems.push(format!("models.ron: {user}: file `{file}` not found"));
            }
        }
        for zone in zones.0.values() {
            for npc in &zone.npcs {
                if npc.visual.starts_with("townsfolk") && !self.people.contains_key(&npc.visual) {
                    problems.push(format!(
                        "models.ron: no look in `people` for `{}` (used by {})",
                        npc.visual, npc.id
                    ));
                }
            }
        }
        problems.sort();
        problems
    }

    /// The body and loadout a class's specialization uses.
    fn class_look(&self, class: &CurrentClass) -> Option<(&str, Loadout)> {
        let look = self.classes.get(&class.class)?;
        let loadout = look.specs.get(&class.spec)?.clone();
        Some((look.body.as_str(), loadout))
    }

    /// How much each named bone is stretched (`None`: not at all).
    fn stretch(&self, bone: &str) -> Option<Vec3> {
        let p = &self.proportions;
        // Hips, spine and chest stack up (their rest poses are all
        // upright), so the head undoes their squash and stretch to stay
        // round.
        let chest_slim = p.slim + (1.0 - p.slim) * 0.4;
        let below_head = Vec3::new(
            p.slim * p.slim * chest_slim,
            p.spine * p.chest,
            p.slim * p.slim * chest_slim,
        );
        Some(match bone {
            "head" => p.head / below_head,
            "hips" => Vec3::new(p.slim, 1.0, p.slim),
            "spine" => Vec3::new(p.slim, p.spine, p.slim),
            "chest" => Vec3::new(chest_slim, p.chest, chest_slim),
            "upperleg.l" | "upperleg.r" => Vec3::new(1.0, p.legs, 1.0),
            "foot.l" | "foot.r" => Vec3::new(1.0, 1.0 / p.legs, 1.0),
            "upperarm.l" | "upperarm.r" => Vec3::new(1.0, p.arms, 1.0),
            "hand.l" | "hand.r" => Vec3::new(1.0, 1.0 / p.arms, 1.0),
            _ => return None,
        })
    }

    /// How far the longer legs lift the body (model units, before scale).
    fn lift(&self) -> f32 {
        // KayKit legs are this long from hip to ankle.
        const LEG_LENGTH: f32 = 0.31;
        LEG_LENGTH * (self.proportions.legs - 1.0)
    }
}

// ---------- components ----------

/// What a character should look like (set when its visual is built).
#[derive(Component, Clone)]
pub enum ModelLook {
    /// A player: the look follows the current class and specialization.
    Player,
    /// A townsperson (a key in `people`).
    Person(String),
}

/// A character whose model is still loading. The hit flash waits until it
/// is ready (it needs the final materials).
#[derive(Component)]
pub struct ModelPending;

/// The body file has been asked for.
#[derive(Component)]
struct BodyLoading;

/// A character's loaded model.
#[derive(Component)]
pub struct Rig {
    /// The model's root (a child of the character).
    root: Entity,
    body: String,
    file: String,
    /// The entity with the `AnimationPlayer`.
    player: Option<Entity>,
    bones: HashMap<String, Entity>,
    weapons: Vec<Entity>,
    /// The class and specialization the look was built for.
    built_for: Option<CurrentClass>,
    style: Style,
    playing: Option<AnimationNodeIndex>,
    /// A one-off animation (an attack) and the game time it ends.
    one_shot: Option<(AnimationNodeIndex, f64)>,
    /// Which attack swing comes next.
    next_attack: usize,
}

/// A bone with a fixed stretch, applied after animation every frame.
#[derive(Component)]
struct Stretch(Vec3);

/// The model root waiting to be dressed; points back at its character.
#[derive(Component)]
struct ModelOf {
    character: Entity,
    body: String,
    file: String,
    tint: Option<(f32, f32, f32)>,
    hide: Vec<String>,
}

/// A replaced body, removed after a couple of frames (frames left).
#[derive(Component)]
struct OldModel(u8);

/// A weapon model waiting to be dressed.
#[derive(Component)]
struct WeaponModel;

/// Animation graphs, one per model file, with each clip's node and length.
#[derive(Resource, Default)]
struct AnimationLibrary {
    sets: HashMap<String, ClipSet>,
    /// Model files kept loaded (their animations are read from them).
    files: HashMap<String, Handle<Gltf>>,
}

struct ClipSet {
    graph: Handle<AnimationGraph>,
    clips: HashMap<String, (AnimationNodeIndex, f32)>,
}

impl Rig {
    /// The hand that holds up the lantern while the flame changes.
    pub fn lantern_hand(&self) -> Option<Entity> {
        self.bones.get("handslot.r").copied()
    }

    /// The weapons in the hands.
    pub fn weapons(&self) -> &[Entity] {
        &self.weapons
    }
}

// ---------- building ----------

/// Start loading a body under `character`.
fn spawn_body(
    commands: &mut Commands,
    assets: &AssetServer,
    library: &mut AnimationLibrary,
    models: &ModelLibrary,
    character: Entity,
    body_key: &str,
) -> Option<(Entity, String)> {
    let body = models.bodies.get(body_key)?;
    library
        .files
        .entry(body.file.clone())
        .or_insert_with(|| assets.load(body.file.clone()));
    let scale = models.proportions.scale;
    let root = commands
        .spawn((
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(body.file.clone()))),
            // The pack's models face +Z; the game's characters face -Z.
            Transform::from_xyz(0.0, models.lift() * scale, 0.0)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
                .with_scale(Vec3::splat(scale)),
            ModelOf {
                character,
                body: body_key.to_owned(),
                file: body.file.clone(),
                tint: body.tint,
                hide: body.hide.clone(),
            },
            ChildOf(character),
        ))
        .observe(dress_body)
        .id();
    Some((root, body.file.clone()))
}

/// Put a weapon in a hand bone.
fn spawn_weapon(
    commands: &mut Commands,
    assets: &AssetServer,
    models: &ModelLibrary,
    hand: Entity,
    name: &str,
    right: bool,
) -> Option<Entity> {
    let weapon = models.weapons.get(name)?;
    let (x, y, z) = weapon.offset;
    let turn = match weapon.rotation {
        Some((qx, qy, qz, qw)) => Quat::from_xyzw(qx, qy, qz, qw).normalize(),
        None if right => Quat::from_rotation_y(std::f32::consts::PI),
        None => Quat::IDENTITY,
    };
    Some(
        commands
            .spawn((
                WorldAssetRoot(
                    assets.load(GltfAssetLabel::Scene(0).from_asset(weapon.file.clone())),
                ),
                Transform::from_xyz(x, y, z).with_rotation(turn),
                WeaponModel,
                ChildOf(hand),
            ))
            .observe(dress_weapon)
            .id(),
    )
}

/// New characters with a model look get their body.
fn dress_new_characters(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut library: ResMut<AnimationLibrary>,
    models: Res<ModelLibrary>,
    new: Query<
        (Entity, &ModelLook, Option<&CurrentClass>),
        (With<ModelPending>, Without<BodyLoading>),
    >,
) {
    for (character, look, class) in &new {
        let body = match look {
            ModelLook::Player => {
                let Some(class) = class else { continue };
                let Some((body, _)) = models.class_look(class) else {
                    continue;
                };
                body.to_owned()
            }
            ModelLook::Person(key) => match models.people.get(key) {
                Some(person) => person.body.clone(),
                None => continue,
            },
        };
        if spawn_body(
            &mut commands,
            &assets,
            &mut library,
            &models,
            character,
            &body,
        )
        .is_some()
        {
            commands.entity(character).insert(BodyLoading);
        }
    }
}

/// The scene is in the world: toon materials, outlines, hidden parts,
/// stretched bones, animations and weapons.
fn dress_body(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    roots: Query<&ModelOf>,
    looks: Query<(&ModelLook, Option<&CurrentClass>)>,
    children: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    meshes: Query<(
        &MeshMaterial3d<StandardMaterial>,
        &Mesh3d,
        Option<&SkinnedMesh>,
    )>,
    mut players: Query<&mut AnimationPlayer>,
    old_rigs: Query<&Rig>,
    (models, assets, gltfs, clips): (
        Res<ModelLibrary>,
        Res<AssetServer>,
        Res<Assets<Gltf>>,
        Res<Assets<AnimationClip>>,
    ),
    (mut graphs, mut library): (ResMut<Assets<AnimationGraph>>, ResMut<AnimationLibrary>),
    (standard, mut toon, mut outlines): (
        Res<Assets<StandardMaterial>>,
        ResMut<Assets<ToonMaterial>>,
        ResMut<Assets<OutlineMaterial>>,
    ),
) {
    let root = ready.entity;
    let Ok(model) = roots.get(root) else {
        return;
    };
    let character = model.character;
    let Ok((look, class)) = looks.get(character) else {
        return;
    };
    let mut bones = HashMap::new();
    for e in children.iter_descendants(root) {
        if let Ok(name) = names.get(e) {
            bones.insert(name.as_str().to_owned(), e);
        }
    }
    // Hide everything the pack put in the hands, and any extra parts.
    for slot in ["handslot.l", "handslot.r"] {
        if let Some(&slot) = bones.get(slot) {
            for item in children.get(slot).into_iter().flatten() {
                commands.entity(*item).insert(Visibility::Hidden);
            }
        }
    }
    for part in &model.hide {
        if let Some(&e) = bones.get(part) {
            commands.entity(e).insert(Visibility::Hidden);
        }
    }
    // Toon materials (one set per character, so hit flashes stay on the
    // character that was hit) and outlines that follow the skeleton.
    let outline = outline_material(&mut outlines);
    let mut made: HashMap<AssetId<StandardMaterial>, Handle<ToonMaterial>> = HashMap::new();
    for e in children.iter_descendants(root) {
        let Ok((material, mesh, skin)) = meshes.get(e) else {
            continue;
        };
        let part = parents
            .get(e)
            .ok()
            .and_then(|p| names.get(p.parent()).ok())
            .map(|n| n.as_str().to_owned())
            .unwrap_or_default();
        let clothes = ["Body", "Arm", "Leg", "Cape", "Helmet", "Hat"]
            .iter()
            .any(|p| part.contains(p));
        let key = material.0.id();
        let tinted = clothes && model.tint.is_some();
        let handle = if !tinted && let Some(h) = made.get(&key) {
            h.clone()
        } else {
            let mut base = standard.get(&material.0).cloned().unwrap_or_default();
            base.perceptual_roughness = 1.0;
            base.metallic = 0.0;
            if tinted && let Some((r, g, b)) = model.tint {
                let c = base.base_color.to_linear();
                base.base_color =
                    Color::LinearRgba(LinearRgba::new(c.red * r, c.green * g, c.blue * b, c.alpha));
            }
            let h = toon.add(ToonMaterial {
                base,
                extension: ToonExtension::default(),
            });
            if !tinted {
                made.insert(key, h.clone());
            }
            h
        };
        commands
            .entity(e)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(handle));
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
    // Longer, slimmer proportions.
    for (name, &e) in &bones {
        if let Some(s) = models.stretch(name) {
            commands.entity(e).insert(Stretch(s));
        }
    }
    // Animations: the graph for this file is built once and shared.
    let file = model.file.clone();
    if !library.sets.contains_key(&file)
        && let Some(gltf) = library.files.get(&file).and_then(|h| gltfs.get(h))
    {
        let named: Vec<(String, Handle<AnimationClip>)> = gltf
            .named_animations
            .iter()
            .map(|(name, clip)| (name.to_string(), clip.clone()))
            .collect();
        let (graph, nodes) = AnimationGraph::from_clips(named.iter().map(|(_, clip)| clip.clone()));
        let clip_set = ClipSet {
            graph: graphs.add(graph),
            clips: named
                .iter()
                .zip(nodes)
                .map(|((name, clip), node)| {
                    let length = clips.get(clip).map_or(1.0, AnimationClip::duration);
                    (name.clone(), (node, length))
                })
                .collect(),
        };
        library.sets.insert(file.clone(), clip_set);
    }
    let mut animator = None;
    if let Some(set) = library.sets.get(&file) {
        for e in children.iter_descendants(root) {
            if players.get_mut(e).is_ok() {
                commands.entity(e).insert((
                    AnimationGraphHandle(set.graph.clone()),
                    AnimationTransitions::new(),
                ));
                animator = Some(e);
            }
        }
    }
    // Weapons.
    let loadout = match look {
        ModelLook::Player => match class.and_then(|c| models.class_look(c)) {
            Some((_, loadout)) => loadout,
            None => return,
        },
        ModelLook::Person(key) => match models.people.get(key) {
            Some(person) => Loadout {
                right: person.right.clone(),
                left: person.left.clone(),
                style: Style::OneHanded,
            },
            None => return,
        },
    };
    let weapons = spawn_loadout(&mut commands, &assets, &models, &bones, &loadout);
    let rig = Rig {
        root,
        body: model.body.clone(),
        file,
        player: animator,
        bones,
        weapons,
        built_for: class.cloned(),
        style: loadout.style,
        playing: None,
        one_shot: None,
        next_attack: 0,
    };
    // A body swap: the old model goes.
    // (It goes a frame later, once anything held in its hands - the
    // lantern - has moved to the new one.)
    if let Ok(old) = old_rigs.get(character)
        && old.root != root
    {
        commands
            .entity(old.root)
            .insert((OldModel(2), Visibility::Hidden));
    }
    // The hit flash collects the new materials again.
    commands
        .entity(character)
        .remove::<(ModelPending, BodyLoading, BodyMaterials)>()
        .insert(rig);
}

fn spawn_loadout(
    commands: &mut Commands,
    assets: &AssetServer,
    models: &ModelLibrary,
    bones: &HashMap<String, Entity>,
    loadout: &Loadout,
) -> Vec<Entity> {
    let mut weapons = Vec::new();
    for (slot, item, right) in [
        ("handslot.r", &loadout.right, true),
        ("handslot.l", &loadout.left, false),
    ] {
        if let (Some(item), Some(&hand)) = (item, bones.get(slot))
            && let Some(weapon) = spawn_weapon(commands, assets, models, hand, item, right)
        {
            weapons.push(weapon);
        }
    }
    weapons
}

fn outline_material(outlines: &mut Assets<OutlineMaterial>) -> Handle<OutlineMaterial> {
    outlines.add(OutlineMaterial {
        settings: OutlineSettings {
            color: OUTLINE_COLOR,
            // Smooth: pushed out along the normals.
            params: Vec4::new(OUTLINE_THICKNESS, 0.0, 0.0, 0.0),
        },
    })
}

/// A weapon's scene is in the world: toon materials and outlines.
fn dress_weapon(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    meshes: Query<(&MeshMaterial3d<StandardMaterial>, &Mesh3d)>,
    standard: Res<Assets<StandardMaterial>>,
    mut toon: ResMut<Assets<ToonMaterial>>,
    mut outlines: ResMut<Assets<OutlineMaterial>>,
) {
    let outline = outline_material(&mut outlines);
    for e in children.iter_descendants(ready.entity) {
        let Ok((material, mesh)) = meshes.get(e) else {
            continue;
        };
        let mut base = standard.get(&material.0).cloned().unwrap_or_default();
        base.perceptual_roughness = 1.0;
        base.metallic = 0.0;
        let handle = toon.add(ToonMaterial {
            base,
            extension: ToonExtension::default(),
        });
        commands
            .entity(e)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(handle));
        commands.spawn((
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(outline.clone()),
            NotShadowCaster,
            ChildOf(e),
        ));
    }
}

/// Changing class can change the body; changing specialization changes
/// what is in the hands.
fn follow_class_changes(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut library: ResMut<AnimationLibrary>,
    models: Res<ModelLibrary>,
    mut rigs: Query<
        (Entity, &CurrentClass, &mut Rig),
        (Changed<CurrentClass>, Without<ModelPending>),
    >,
) {
    for (character, class, mut rig) in &mut rigs {
        if rig.built_for.as_ref() == Some(class) {
            continue;
        }
        let Some((body, loadout)) = models.class_look(class) else {
            continue;
        };
        if body != rig.body {
            // A new body; the old one stays until the new one is ready.
            if spawn_body(
                &mut commands,
                &assets,
                &mut library,
                &models,
                character,
                body,
            )
            .is_some()
            {
                commands
                    .entity(character)
                    .insert((ModelPending, BodyLoading));
            }
            continue;
        }
        for weapon in rig.weapons.drain(..) {
            commands.entity(weapon).despawn();
        }
        rig.weapons = spawn_loadout(&mut commands, &assets, &models, &rig.bones, &loadout);
        rig.style = loadout.style;
        rig.built_for = Some(class.clone());
    }
}

// ---------- animation ----------

fn remove_old_models(mut commands: Commands, mut old: Query<(Entity, &mut OldModel)>) {
    for (model, mut frames) in &mut old {
        if frames.0 == 0 {
            commands.entity(model).despawn();
        } else {
            frames.0 -= 1;
        }
    }
}

fn stretch_bones(mut bones: Query<(&Stretch, &mut Transform)>) {
    for (stretch, mut transform) in &mut bones {
        transform.scale = stretch.0;
    }
}

/// Attacks and spell releases play once over whatever else is going on.
fn start_actions(
    mut received: MessageReader<Received>,
    data: Res<GameData>,
    models: Res<ModelLibrary>,
    library: Res<AnimationLibrary>,
    fixed: Res<Time<Fixed>>,
    mut rigs: Query<&mut Rig>,
) {
    let now = game_now(&fixed);
    let names = &models.animations;
    for Received(event) in received.read() {
        let (user, ability, landed) = match event {
            ServerEvent::AbilityUsed { user, ability, .. } => (*user, ability, false),
            ServerEvent::AbilityLanded { user, ability, .. } => (*user, ability, true),
            _ => continue,
        };
        let Ok(mut rig) = rigs.get_mut(user) else {
            continue;
        };
        let Some(def) = data.abilities.get(ability) else {
            continue;
        };
        // Instant abilities swing as they start; cast ones release as they land.
        let clip = match (def.cast_time > 0.0, landed) {
            (false, false) => {
                let swings = &names.attacks[&rig.style];
                let clip = swings[rig.next_attack % swings.len()].clone();
                rig.next_attack += 1;
                clip
            }
            (true, true) => names.release.clone(),
            _ => continue,
        };
        let Some(set) = library.sets.get(&rig.file) else {
            continue;
        };
        if let Some(&(node, length)) = set.clips.get(&clip) {
            // An area spell lands on many targets at once: start it only once.
            if rig.one_shot.is_some_and(|(n, _)| n == node) && landed {
                continue;
            }
            rig.one_shot = Some((node, now + f64::from(length / names.attack_speed)));
            rig.playing = None;
        }
    }
}

/// Pick each character's animation from what it is doing.
fn choose_animations(
    models: Res<ModelLibrary>,
    library: Res<AnimationLibrary>,
    fixed: Res<Time<Fixed>>,
    mut rigs: Query<(
        &mut Rig,
        Option<&DisplayMotion>,
        Option<&ActionState>,
        Has<Defeated>,
        Has<Riding>,
        Has<FlameChange>,
    )>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let now = game_now(&fixed);
    let names = &models.animations;
    let blend = Duration::from_secs_f32(names.blend);
    let tick = 1.0 / fixed.timestep().as_secs_f32().max(1e-6);
    for (mut rig, motion, actions, defeated, riding, changing) in &mut rigs {
        let Some(set) = library.sets.get(&rig.file) else {
            continue;
        };
        let Some(animator) = rig.player else { continue };
        let Ok((mut player, mut transitions)) = players.get_mut(animator) else {
            continue;
        };
        if rig.one_shot.is_some_and(|(_, ends)| now >= ends) || defeated || riding {
            rig.one_shot = None;
        }
        // (clip, loops, speed)
        let (clip, loops, speed) = if defeated {
            (&names.defeated, false, 1.0)
        } else if riding {
            (&names.ride, true, 1.0)
        } else if let Some((node, _)) = rig.one_shot {
            if rig.playing != Some(node) {
                transitions
                    .play(&mut player, node, blend)
                    .set_speed(names.attack_speed);
                rig.playing = Some(node);
            }
            continue;
        } else if changing {
            (&names.lantern, true, 1.0)
        } else if actions.is_some_and(|a| a.cast_progress(now).is_some()) {
            (&names.cast, true, 1.0)
        } else if motion.is_some_and(|m| !m.current.grounded) {
            (&names.jump, true, 1.0)
        } else if motion.is_some_and(|m| {
            (m.current.position - m.previous.position).xz().length() * tick > names.run_above
        }) {
            (&names.run, true, 1.0)
        } else {
            (&names.idle[&rig.style], true, 1.0)
        };
        let Some(&(node, _)) = set.clips.get(clip) else {
            continue;
        };
        if rig.playing == Some(node) {
            continue;
        }
        let active = transitions.play(&mut player, node, blend).set_speed(speed);
        if loops {
            active.repeat();
        } else {
            active.set_repeat(RepeatAnimation::Never);
        }
        rig.playing = Some(node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::data::find_assets_dir;

    #[test]
    fn every_class_spec_and_townsperson_has_a_look() {
        let assets = find_assets_dir().unwrap();
        let data = GameData::load(&assets).unwrap();
        let zones = data.load_zones(&assets).unwrap();
        let models = ModelLibrary::load(&assets).unwrap();
        assert_eq!(
            models.check_references(&assets, &data, &zones),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_head_stays_round_after_the_body_is_stretched() {
        let assets = find_assets_dir().unwrap();
        let models = ModelLibrary::load(&assets).unwrap();
        // Hips, spine and chest stack; the head undoes them.
        let mut total = Vec3::ONE;
        for bone in ["hips", "spine", "chest", "head"] {
            total *= models.stretch(bone).unwrap();
        }
        let head = models.proportions.head;
        assert!(
            (total - Vec3::splat(head)).abs().max_element() < 1e-4,
            "{total}"
        );
    }

    #[test]
    fn a_missing_weapon_or_body_is_reported() {
        let assets = find_assets_dir().unwrap();
        let mut models = ModelLibrary::load(&assets).unwrap();
        models.weapons.remove("staff");
        models.bodies.remove("knight");
        let problems = models.validate();
        assert!(
            problems
                .iter()
                .any(|p| p.contains("no weapon called `staff`"))
        );
        assert!(
            problems
                .iter()
                .any(|p| p.contains("no body called `knight`"))
        );
    }
}
