//! Particles (sparks, embers, dust, motes) drawn on the graphics card with
//! `bevy_hanabi`. Presets come from `assets/data/client/particles.ron`;
//! each look in `vfx.ron` lists the presets it plays, tinted its colour.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_hanabi::prelude::*;
use serde::Deserialize;
use shared::data::{DataError, Problems, Validate, load_ron};

use crate::vfx::VfxLibrary;

/// Extra time a burst is kept after its longest-lived particle, so it is
/// never cut off.
const CLEANUP_MARGIN: f32 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum ParticleShape {
    /// Flying out in every direction.
    Sphere,
    /// Rising from a disc on the ground.
    Up,
    /// Starting on a ring and flying outwards along the ground.
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
pub enum Blend {
    /// Adds light: sparks and magic (shines with bloom).
    #[default]
    Glow,
    /// Covers what is behind it: dust, leaves.
    Solid,
}

/// One particle preset.
#[derive(Debug, Clone, Deserialize)]
pub struct ParticlePreset {
    /// Particles in one burst.
    #[serde(default)]
    pub count: f32,
    /// Or: particles per second, forever (portals, ambience).
    #[serde(default)]
    pub rate: Option<f32>,
    pub lifetime: (f32, f32),
    pub speed: (f32, f32),
    pub shape: ParticleShape,
    pub radius: f32,
    /// Multiply `radius` by the look's `size` (rings, big areas).
    #[serde(default)]
    pub scale_with_size: bool,
    #[serde(default)]
    pub gravity: f32,
    #[serde(default)]
    pub drag: f32,
    /// Particle size (start, end) in metres.
    pub size: (f32, f32),
    /// Brightness multiplier; above 1 glows.
    #[serde(default = "one")]
    pub glow: f32,
    #[serde(default)]
    pub blend: Blend,
}

fn one() -> f32 {
    1.0
}

impl ParticlePreset {
    /// How long one burst takes to finish.
    fn duration(&self) -> f32 {
        self.lifetime.1 + CLEANUP_MARGIN
    }

    fn capacity(&self) -> u32 {
        let wanted = match self.rate {
            Some(rate) => rate * self.lifetime.1 * 1.5,
            None => self.count,
        };
        (wanted.ceil() as u32).max(1)
    }
}

#[derive(Resource, Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct ParticleLibrary(pub HashMap<String, ParticlePreset>);

impl Validate for ParticleLibrary {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        for (name, preset) in &self.0 {
            match preset.rate {
                Some(rate) => p.positive(&format!("{name}.rate"), rate),
                None => p.positive(&format!("{name}.count"), preset.count),
            }
            p.positive(&format!("{name}.lifetime"), preset.lifetime.0);
            if preset.lifetime.1 < preset.lifetime.0 {
                p.push(format!(
                    "{name}.lifetime: the longest is shorter than the shortest"
                ));
            }
            if preset.speed.1 < preset.speed.0 {
                p.push(format!(
                    "{name}.speed: the fastest is slower than the slowest"
                ));
            }
            p.non_negative(&format!("{name}.radius"), preset.radius);
            p.non_negative(&format!("{name}.size"), preset.size.0.min(preset.size.1));
        }
        p.0
    }
}

impl ParticleLibrary {
    pub fn load(assets_dir: &std::path::Path) -> Result<Self, DataError> {
        load_ron(&assets_dir.join("data").join("client").join("particles.ron"))
    }
}

/// A ready-to-play particle effect: one preset tinted for one look.
#[derive(Clone)]
struct Prepared {
    handle: Handle<EffectAsset>,
    /// `None` for effects that never stop.
    duration: Option<f32>,
}

/// Every look's particle effects, built once at startup.
#[derive(Resource, Default)]
pub struct ParticleEffects(HashMap<String, Vec<Prepared>>);

/// A burst that removes itself once its particles are gone.
#[derive(Component)]
struct ParticleTimer(f32);

pub struct ParticlesPlugin;

impl Plugin for ParticlesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(HanabiPlugin)
            .init_resource::<ParticleEffects>()
            .add_systems(Startup, (prepare_effects, warm_up_effects).chain())
            .add_systems(Update, clean_up_bursts);
    }
}

fn prepare_effects(
    looks: Res<VfxLibrary>,
    presets: Res<ParticleLibrary>,
    mut assets: ResMut<Assets<EffectAsset>>,
    mut prepared: ResMut<ParticleEffects>,
) {
    for (name, look) in &looks.0 {
        let effects = look
            .particles
            .iter()
            .filter_map(|preset_name| {
                let preset = presets.0.get(preset_name)?;
                let color = Vec3::new(look.color.0, look.color.1, look.color.2);
                let asset = build_effect(preset, color, look.size);
                Some(Prepared {
                    handle: assets.add(asset),
                    duration: preset.rate.is_none().then(|| preset.duration()),
                })
            })
            .collect::<Vec<_>>();
        if !effects.is_empty() {
            prepared.0.insert(name.clone(), effects);
        }
    }
}

/// Where effects are played once at start-up, out of sight, so the graphics
/// card prepares them before they are needed in a fight.
const WARM_UP_AT: Vec3 = Vec3::new(0.0, -500.0, 0.0);

fn warm_up_effects(mut commands: Commands, prepared: Res<ParticleEffects>) {
    for effects in prepared.0.values() {
        for effect in effects.iter().filter(|e| e.duration.is_some()) {
            commands.spawn((
                ParticleEffect::new(effect.handle.clone()),
                Transform::from_translation(WARM_UP_AT),
                ParticleTimer(0.0),
            ));
        }
    }
}

/// Turn a preset into a GPU particle effect.
fn build_effect(preset: &ParticlePreset, color: Vec3, look_size: f32) -> EffectAsset {
    let writer = ExprWriter::new();
    let radius = if preset.scale_with_size {
        preset.radius * look_size
    } else {
        preset.radius
    }
    .max(0.02);

    let zero = writer.lit(Vec3::ZERO);
    let position: Box<dyn Modifier> = match preset.shape {
        ParticleShape::Sphere => Box::new(SetPositionSphereModifier {
            center: zero.clone().expr(),
            radius: writer.lit(radius).expr(),
            dimension: ShapeDimension::Volume,
        }),
        ParticleShape::Up | ParticleShape::Flat => Box::new(SetPositionCircleModifier {
            center: zero.clone().expr(),
            axis: writer.lit(Vec3::Y).expr(),
            radius: writer.lit(radius).expr(),
            dimension: if preset.shape == ParticleShape::Flat {
                ShapeDimension::Surface
            } else {
                ShapeDimension::Volume
            },
        }),
    };

    let speed = writer
        .lit(preset.speed.0)
        .uniform(writer.lit(preset.speed.1));
    let velocity: Box<dyn Modifier> = match preset.shape {
        ParticleShape::Sphere | ParticleShape::Flat => Box::new(SetVelocitySphereModifier {
            center: zero.expr(),
            speed: speed.expr(),
        }),
        ParticleShape::Up => {
            // Mostly upwards, with a little sideways wander.
            let wander = (writer.rand(VectorType::VEC3F) * writer.lit(2.0) - writer.lit(1.0))
                * writer.lit(Vec3::new(0.3, 0.0, 0.3));
            let direction = (wander + writer.lit(Vec3::Y)).normalized();
            Box::new(SetAttributeModifier::new(
                Attribute::VELOCITY,
                (direction * speed).expr(),
            ))
        }
    };

    let lifetime = writer
        .lit(preset.lifetime.0)
        .uniform(writer.lit(preset.lifetime.1))
        .expr();
    let init_lifetime = SetAttributeModifier::new(Attribute::LIFETIME, lifetime);
    let init_age = SetAttributeModifier::new(Attribute::AGE, writer.lit(0.0).expr());
    let accel = AccelModifier::new(writer.lit(Vec3::Y * preset.gravity).expr());
    let drag = LinearDragModifier::new(writer.lit(preset.drag).expr());

    let mut module = writer.finish();
    let round = RoundModifier::constant(&mut module, 1.0);

    let bright = color * preset.glow;
    let mut colors = bevy_hanabi::Gradient::new();
    colors.add_key(0.0, bright.extend(1.0));
    colors.add_key(0.6, bright.extend(0.9));
    colors.add_key(1.0, bright.extend(0.0));
    let mut sizes = bevy_hanabi::Gradient::new();
    sizes.add_key(0.0, Vec3::splat(preset.size.0));
    sizes.add_key(1.0, Vec3::splat(preset.size.1));

    let spawner = match preset.rate {
        Some(rate) => SpawnerSettings::rate(rate.into()),
        None => SpawnerSettings::once(preset.count.into()),
    };
    let alpha = match preset.blend {
        Blend::Glow => bevy_hanabi::AlphaMode::Add,
        Blend::Solid => bevy_hanabi::AlphaMode::Blend,
    };
    EffectAsset::new(preset.capacity(), spawner, module)
        .with_alpha_mode(alpha)
        .add_modifier(ModifierContext::Init, position)
        .add_modifier(ModifierContext::Init, velocity)
        .init(init_lifetime)
        .init(init_age)
        .update(accel)
        .update(drag)
        .render(ColorOverLifetimeModifier {
            gradient: colors,
            blend: ColorBlendMode::Overwrite,
            mask: ColorBlendMask::RGBA,
        })
        .render(SizeOverLifetimeModifier {
            gradient: sizes,
            screen_space_size: false,
        })
        .render(OrientModifier::new(OrientMode::FaceCameraPosition))
        .render(round)
}

/// Play a look's particles at `at`. Bursts clean themselves up; effects
/// that never stop are returned so the caller can attach them to something.
pub fn play_particles(
    commands: &mut Commands,
    effects: &ParticleEffects,
    look: &str,
    at: Vec3,
) -> Vec<Entity> {
    let Some(list) = effects.0.get(look) else {
        return Vec::new();
    };
    let mut forever = Vec::new();
    for effect in list {
        // The global position is set too: bursts fire on their very first
        // frame, before Bevy would normally have worked it out.
        let mut entity = commands.spawn((
            ParticleEffect::new(effect.handle.clone()),
            Transform::from_translation(at),
            GlobalTransform::from_translation(at),
        ));
        match effect.duration {
            Some(duration) => {
                entity.insert(ParticleTimer(duration));
            }
            None => forever.push(entity.id()),
        }
    }
    forever
}

/// Bursts count down once they have actually fired (the first time an
/// effect is used, the graphics card may need a moment to get it ready).
fn clean_up_bursts(
    mut commands: Commands,
    time: Res<Time>,
    mut bursts: Query<(Entity, &mut ParticleTimer, Option<&EffectSpawner>)>,
) {
    for (entity, mut timer, spawner) in &mut bursts {
        if !spawner.is_some_and(|s| s.has_completed()) {
            continue;
        }
        timer.0 -= time.delta_secs();
        if timer.0 <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
