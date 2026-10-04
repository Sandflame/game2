//! Spell visuals: when an ability lands, look up its `vfx` name in
//! `assets/data/client/vfx.ron` and play its look: a short glowing shape,
//! particles (`particles.rs`) and a sound (`audio.rs`). Other parts of the
//! client play looks by name too (`play_look`), e.g. markers going off.

use std::collections::HashMap;

use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use serde::Deserialize;
use shared::components::Zone;
use shared::data::{DataError, Problems, Validate, load_ron};
use shared::gamedata::{GameData, Zones};
use shared::protocol::ServerEvent;

use crate::audio::{SoundLibrary, Sounds};
use crate::particles::{ParticleEffects, ParticleLibrary, play_particles};
use crate::session::Received;
use crate::world::CurrentZone;

/// How long each style plays, in seconds.
const PROJECTILE_FLIGHT: f32 = 0.22;
const EFFECT_LIFE: f32 = 0.45;
const RISE_LIFE: f32 = 0.8;
/// Height above the feet that effects aim at.
const CHEST_HEIGHT: f32 = 1.1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum VfxStyle {
    Projectile,
    Burst,
    Slash,
    Ring,
    Rise,
    /// Only particles (and sound), no glowing shape.
    Particles,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VfxDef {
    pub style: VfxStyle,
    pub color: (f32, f32, f32),
    pub size: f32,
    /// Particle presets (`particles.ron`) to play, tinted `color`.
    #[serde(default)]
    pub particles: Vec<String>,
    /// Sound (`sounds.ron`) to play.
    #[serde(default)]
    pub sound: Option<String>,
}

/// Every visual effect, by name (`assets/data/client/vfx.ron`).
#[derive(Resource, Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct VfxLibrary(pub HashMap<String, VfxDef>);

impl Validate for VfxLibrary {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        for (name, def) in &self.0 {
            p.positive(&format!("{name}.size"), def.size);
        }
        p.0
    }
}

/// Looks the client plays by name (besides abilities' `vfx`, each class's
/// `flame_<flame>`, and each zone's `ambience`).
pub const BUILT_IN_LOOKS: &[&str] = &[
    "marker_dodge",
    "marker_stack",
    "marker_spread",
    "portal",
    "defeated",
    "revived",
    "victory",
    "level_up",
];

impl VfxLibrary {
    pub fn load(assets_dir: &std::path::Path) -> Result<Self, DataError> {
        load_ron(&assets_dir.join("data").join("client").join("vfx.ron"))
    }

    /// Check that every look the game needs exists, and that looks only
    /// name particles and sounds that exist.
    pub fn check_references(
        &self,
        data: &GameData,
        zones: &Zones,
        particles: &ParticleLibrary,
        sounds: &SoundLibrary,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        let mut need = |name: &str, user: String| {
            if !name.is_empty() && !self.0.contains_key(name) {
                problems.push(format!("vfx.ron: no look called `{name}` (used by {user})"));
            }
        };
        for ability in data.abilities.values() {
            need(&ability.vfx, format!("ability `{}`", ability.id));
        }
        for (id, class) in &data.classes {
            need(&format!("flame_{}", class.flame), format!("class `{id}`"));
        }
        for (id, zone) in &zones.0 {
            need(&zone.ambience, format!("zone `{id}`"));
        }
        for name in BUILT_IN_LOOKS {
            need(name, "the client".to_owned());
        }
        for (name, look) in &self.0 {
            for preset in &look.particles {
                if !particles.0.contains_key(preset) {
                    problems.push(format!(
                        "vfx.ron: `{name}` uses particles `{preset}`, missing from particles.ron"
                    ));
                }
            }
            if let Some(sound) = &look.sound
                && !sounds.sounds.contains_key(sound)
            {
                problems.push(format!(
                    "vfx.ron: `{name}` uses sound `{sound}`, missing from sounds.ron"
                ));
            }
        }
        problems.sort();
        problems
    }
}

pub struct VfxPlugin;

impl Plugin for VfxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (start_effects, looks_for_events, animate_effects).chain(),
        );
    }
}

/// One glowing shape playing.
#[derive(Component)]
struct Effect {
    style: VfxStyle,
    size: f32,
    from: Vec3,
    to: Vec3,
    age: f32,
    life: f32,
    material: Handle<StandardMaterial>,
    /// Projectiles play their look's landing (burst, particles, sound)
    /// when they arrive.
    then_land: Option<String>,
}

fn glow_material(
    materials: &mut Assets<StandardMaterial>,
    def: &VfxDef,
) -> Handle<StandardMaterial> {
    let (r, g, b) = def.color;
    materials.add(StandardMaterial {
        base_color: Color::linear_rgba(r.min(1.0), g.min(1.0), b.min(1.0), 0.8),
        emissive: LinearRgba::rgb(r, g, b) * 1.5,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    })
}

fn effect_mesh(style: VfxStyle) -> Mesh {
    match style {
        VfxStyle::Slash => Torus::new(0.9, 1.0)
            .mesh()
            .minor_resolution(6)
            .major_resolution(32)
            .build(),
        VfxStyle::Ring => Torus::new(0.94, 1.0)
            .mesh()
            .minor_resolution(6)
            .major_resolution(48)
            .build(),
        _ => Sphere::new(1.0).mesh().uv(20, 12),
    }
}

/// Everything needed to play a look from any system.
#[derive(SystemParam)]
pub struct Looks<'w, 's> {
    commands: Commands<'w, 's>,
    library: Res<'w, VfxLibrary>,
    particles: Res<'w, ParticleEffects>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    sounds: Sounds<'w, 's>,
}

impl Looks<'_, '_> {
    /// The mesh store (so systems using `Looks` can still make meshes).
    pub fn meshes(&mut self) -> &mut Assets<Mesh> {
        &mut self.meshes
    }

    /// Play the look called `name` from one character's feet (`from`) to
    /// another's (`to`). Use the same point twice for looks in one place.
    pub fn play(&mut self, name: &str, from: Vec3, to: Vec3) {
        let Some(def) = self.library.0.get(name).cloned() else {
            return;
        };
        let lift = Vec3::Y * CHEST_HEIGHT;
        if def.style == VfxStyle::Projectile {
            self.spawn_shape(&def, from + lift, to + lift, Some(name.to_owned()));
            return;
        }
        let (shape_at, particles_at) = match def.style {
            VfxStyle::Ring => (from, from),
            VfxStyle::Rise | VfxStyle::Particles => (to + lift, to),
            _ => (to + lift, to + lift),
        };
        if def.style != VfxStyle::Particles {
            self.spawn_shape(&def, shape_at, shape_at, None);
        }
        self.finish(name, &def, particles_at);
    }

    /// Only the look's particles, at `at` (no shape, no sound).
    pub fn particles(&mut self, name: &str, at: Vec3) {
        play_particles(&mut self.commands, &self.particles, name, at);
    }

    /// Particles that never stop (portals, ambience), attached to `parent`
    /// at `offset` so they go away with it.
    pub fn attach_lasting_particles(&mut self, name: &str, offset: Vec3, parent: Entity) {
        for particles in play_particles(&mut self.commands, &self.particles, name, offset) {
            self.commands.entity(particles).insert(ChildOf(parent));
        }
    }

    /// Only the look's sound.
    pub fn sound(&mut self, name: &str) {
        if let Some(sound) = self.library.0.get(name).and_then(|d| d.sound.clone()) {
            self.sounds.play(&sound);
        }
    }

    /// A projectile arrived: a burst three times its size, plus particles
    /// and sound.
    fn land(&mut self, name: &str, at: Vec3) {
        let Some(def) = self.library.0.get(name).cloned() else {
            return;
        };
        let burst = VfxDef {
            style: VfxStyle::Burst,
            size: def.size * 3.0,
            ..def.clone()
        };
        self.spawn_shape(&burst, at, at, None);
        self.finish(name, &def, at);
    }

    fn finish(&mut self, name: &str, def: &VfxDef, particles_at: Vec3) {
        play_particles(&mut self.commands, &self.particles, name, particles_at);
        if let Some(sound) = &def.sound {
            self.sounds.play(sound);
        }
    }

    fn spawn_shape(&mut self, def: &VfxDef, from: Vec3, to: Vec3, then_land: Option<String>) {
        let material = glow_material(&mut self.materials, def);
        let life = match def.style {
            VfxStyle::Projectile => PROJECTILE_FLIGHT,
            VfxStyle::Rise => RISE_LIFE,
            _ => EFFECT_LIFE,
        };
        self.commands.spawn((
            Mesh3d(self.meshes.add(effect_mesh(def.style))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(from).with_scale(Vec3::ZERO),
            NotShadowCaster,
            Effect {
                style: def.style,
                size: def.size,
                from,
                to,
                age: 0.0,
                life,
                material,
                then_land,
            },
        ));
    }
}

fn start_effects(
    mut received: MessageReader<Received>,
    data: Res<GameData>,
    current: Res<CurrentZone>,
    characters: Query<(&Transform, &Zone)>,
    mut looks: Looks,
) {
    for Received(event) in received.read() {
        let ServerEvent::AbilityLanded {
            user,
            ability,
            target,
        } = event
        else {
            continue;
        };
        let Some(ability) = data.abilities.get(ability) else {
            continue;
        };
        // Ground-marker attacks show their hit over the marker's area
        // instead (`telegraphs.rs`), even on players who dodged.
        if ability.telegraph.is_some() {
            continue;
        }
        let (Ok((user_at, zone)), Ok((target_at, _))) =
            (characters.get(*user), characters.get(*target))
        else {
            continue;
        };
        // Only what happens in the zone being shown.
        if current.0.as_deref() != Some(zone.0.as_str()) {
            continue;
        }
        looks.play(&ability.vfx, user_at.translation, target_at.translation);
    }
}

/// Looks for things that happen to characters and fights (not abilities).
fn looks_for_events(
    mut received: MessageReader<Received>,
    data: Res<GameData>,
    zones: Res<Zones>,
    current: Res<CurrentZone>,
    characters: Query<(&Transform, &Zone)>,
    mut looks: Looks,
) {
    // Where a character is, if they are in the zone being shown.
    let here = |entity: &Entity| {
        characters
            .get(*entity)
            .ok()
            .filter(|(_, zone)| current.0.as_deref() == Some(zone.0.as_str()))
            .map(|(transform, _)| transform.translation)
    };
    for Received(event) in received.read() {
        match event {
            ServerEvent::ClassChanged { user, class } => {
                if let (Some(at), Some(class)) = (here(user), data.classes.get(class)) {
                    looks.play(&format!("flame_{}", class.flame), at, at);
                }
            }
            ServerEvent::Defeated { entity } => {
                if let Some(at) = here(entity) {
                    looks.play("defeated", at, at);
                }
            }
            ServerEvent::Revived { entity } => {
                if let Some(at) = here(entity) {
                    looks.play("revived", at, at);
                }
            }
            ServerEvent::LevelUp { entity, .. } => {
                if let Some(at) = here(entity) {
                    looks.play("level_up", at, at);
                }
            }
            ServerEvent::EncounterWon { zone, .. } if current.0.as_deref() == Some(zone) => {
                let boss_at = zones
                    .get(zone)
                    .and_then(|level| level.encounter.as_ref())
                    .and_then(|id| data.encounters.get(id))
                    .map(|encounter| encounter.boss_position);
                if let Some(at) = boss_at {
                    looks.play("victory", at, at);
                }
            }
            _ => {}
        }
    }
}

fn animate_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut effects: Query<(Entity, &mut Effect, &mut Transform)>,
    mut looks: Looks,
) {
    for (entity, mut effect, mut transform) in &mut effects {
        effect.age += time.delta_secs();
        let t = (effect.age / effect.life).clamp(0.0, 1.0);
        if t >= 1.0 {
            if let Some(name) = effect.then_land.take() {
                looks.land(&name, effect.to);
            }
            commands.entity(entity).despawn();
            continue;
        }
        let size = effect.size;
        let fade = 1.0 - t * t;
        match effect.style {
            VfxStyle::Projectile => {
                transform.translation = effect.from.lerp(effect.to, t);
                transform.scale = Vec3::splat(size);
            }
            VfxStyle::Burst | VfxStyle::Particles => {
                transform.translation = effect.to;
                transform.scale = Vec3::splat(size * (0.3 + 0.7 * t.sqrt()));
            }
            VfxStyle::Slash => {
                transform.translation = effect.to;
                // A thin ring standing up, swinging round and widening.
                transform.rotation = Quat::from_rotation_z(0.6)
                    * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2 - t * 1.2);
                transform.scale =
                    Vec3::new(size * (0.6 + 0.4 * t), size * 0.2, size * (0.6 + 0.4 * t));
            }
            VfxStyle::Ring => {
                transform.translation = effect.from + Vec3::Y * 0.1;
                transform.scale = Vec3::new(size * t.sqrt(), 0.5, size * t.sqrt());
            }
            VfxStyle::Rise => {
                transform.translation = effect.to + Vec3::Y * (t * 1.2 - 0.8);
                transform.scale = Vec3::new(size * 0.6, size * (0.6 + t), size * 0.6);
            }
        }
        if let Some(mut material) = looks.materials.get_mut(&effect.material) {
            material.base_color.set_alpha(0.8 * fade);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::data::find_assets_dir;

    #[test]
    fn every_look_particle_and_sound_exists() {
        let assets = find_assets_dir().unwrap();
        let data = GameData::load(&assets).unwrap();
        let zones = data.load_zones(&assets).unwrap();
        let library = VfxLibrary::load(&assets).unwrap();
        let particles = ParticleLibrary::load(&assets).unwrap();
        let sounds = SoundLibrary::load(&assets).unwrap();
        let problems = library.check_references(&data, &zones, &particles, &sounds);
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn every_sound_file_exists() {
        let assets = find_assets_dir().unwrap();
        let sounds = SoundLibrary::load(&assets).unwrap();
        for (name, sound) in &sounds.sounds {
            let path = assets.join(&sound.file);
            assert!(
                path.exists(),
                "sound `{name}`: {} is missing",
                path.display()
            );
        }
    }
}
