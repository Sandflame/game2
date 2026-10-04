//! Simple spell visuals: when an ability lands, look up its `vfx` name in
//! `assets/data/client/vfx.ron` and play a short effect. (Milestone 4b
//! replaces these with proper particles.)

use std::collections::HashMap;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use serde::Deserialize;
use shared::data::{DataError, Problems, Validate, load_ron};
use shared::gamedata::GameData;
use shared::protocol::ServerEvent;

use crate::session::Received;

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
}

#[derive(Debug, Clone, Deserialize)]
pub struct VfxDef {
    pub style: VfxStyle,
    pub color: (f32, f32, f32),
    pub size: f32,
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

impl VfxLibrary {
    pub fn load(assets_dir: &std::path::Path) -> Result<Self, DataError> {
        load_ron(&assets_dir.join("data").join("client").join("vfx.ron"))
    }
}

pub struct VfxPlugin;

impl Plugin for VfxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (start_effects, animate_effects).chain());
    }
}

/// One effect playing.
#[derive(Component)]
struct Effect {
    style: VfxStyle,
    size: f32,
    from: Vec3,
    to: Vec3,
    age: f32,
    life: f32,
    material: Handle<StandardMaterial>,
    /// Projectiles burst when they arrive.
    then_burst: Option<VfxDef>,
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
        VfxStyle::Projectile | VfxStyle::Burst | VfxStyle::Rise => {
            Sphere::new(1.0).mesh().uv(20, 12)
        }
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
    }
}

fn spawn_effect(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    def: &VfxDef,
    from: Vec3,
    to: Vec3,
) {
    let material = glow_material(materials, def);
    let (life, then_burst) = match def.style {
        VfxStyle::Projectile => (
            PROJECTILE_FLIGHT,
            Some(VfxDef {
                style: VfxStyle::Burst,
                size: def.size * 3.0,
                ..def.clone()
            }),
        ),
        VfxStyle::Rise => (RISE_LIFE, None),
        _ => (EFFECT_LIFE, None),
    };
    commands.spawn((
        Mesh3d(meshes.add(effect_mesh(def.style))),
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
            then_burst,
        },
    ));
}

fn start_effects(
    mut commands: Commands,
    mut received: MessageReader<Received>,
    data: Res<GameData>,
    library: Res<VfxLibrary>,
    characters: Query<&Transform>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
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
        let Some(def) = data
            .abilities
            .get(ability)
            .and_then(|a| library.0.get(&a.vfx))
        else {
            continue;
        };
        let (Ok(user_at), Ok(target_at)) = (characters.get(*user), characters.get(*target)) else {
            continue;
        };
        let lift = Vec3::Y * CHEST_HEIGHT;
        let (from, to) = match def.style {
            VfxStyle::Projectile => (user_at.translation + lift, target_at.translation + lift),
            VfxStyle::Ring => (user_at.translation, user_at.translation),
            _ => (target_at.translation + lift, target_at.translation + lift),
        };
        spawn_effect(&mut commands, &mut meshes, &mut materials, def, from, to);
    }
}

fn animate_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut effects: Query<(Entity, &mut Effect, &mut Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, mut effect, mut transform) in &mut effects {
        effect.age += time.delta_secs();
        let t = (effect.age / effect.life).clamp(0.0, 1.0);
        if t >= 1.0 {
            if let Some(burst) = effect.then_burst.take() {
                let at = effect.to;
                spawn_effect(&mut commands, &mut meshes, &mut materials, &burst, at, at);
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
            VfxStyle::Burst => {
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
        if let Some(mut material) = materials.get_mut(&effect.material) {
            material.base_color.set_alpha(0.8 * fade);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::data::find_assets_dir;

    #[test]
    fn every_ability_vfx_has_a_look() {
        let assets = find_assets_dir().unwrap();
        let library = VfxLibrary::load(&assets).unwrap();
        let data = GameData::load(&assets).unwrap();
        for ability in data.abilities.values() {
            assert!(
                ability.vfx.is_empty() || library.0.contains_key(&ability.vfx),
                "ability `{}` uses vfx `{}`, which is missing from vfx.ron",
                ability.id,
                ability.vfx
            );
        }
    }
}
