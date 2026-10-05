//! Toon-shaded materials and black outlines. Every visible object in the
//! game is built from parts spawned through [`ToonAssets::spawn_part`].

use bevy::ecs::system::SystemParam;
use bevy::light::NotShadowCaster;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Face, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

const TOON_SHADER: &str = "shaders/toon.wgsl";
const OUTLINE_SHADER: &str = "shaders/outline.wgsl";

/// Outline thickness as a fraction of the distance to the camera.
pub const OUTLINE_THICKNESS: f32 = 0.0022;
pub const OUTLINE_COLOR: LinearRgba = LinearRgba::new(0.02, 0.015, 0.03, 1.0);

pub type ToonMaterial = ExtendedMaterial<StandardMaterial, ToonExtension>;

pub struct ToonPlugin;

impl Plugin for ToonPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<ToonMaterial>::default(),
            MaterialPlugin::<OutlineMaterial>::default(),
        ));
    }
}

/// Look of the toon lighting. Shared by all toon materials for a
/// consistent style.
#[derive(ShaderType, Reflect, Debug, Clone, Copy)]
pub struct ToonSettings {
    pub shadow_color: LinearRgba,
    pub rim_color: LinearRgba,
    pub bands: Vec4,
    pub rim: Vec4,
}

impl Default for ToonSettings {
    fn default() -> Self {
        Self {
            // Shadows lean cool and purple, a common anime look.
            shadow_color: LinearRgba::rgb(0.42, 0.40, 0.62),
            rim_color: LinearRgba::new(1.0, 0.95, 0.85, 0.25),
            // shadow edge, highlight edge, mid-tone, softness
            bands: Vec4::new(0.08, 0.55, 0.82, 0.02),
            // rim start, rim softness
            rim: Vec4::new(0.62, 0.04, 0.0, 0.0),
        }
    }
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct ToonExtension {
    #[uniform(100)]
    pub settings: ToonSettings,
}

impl MaterialExtension for ToonExtension {
    fn fragment_shader() -> ShaderRef {
        TOON_SHADER.into()
    }
}

/// How an outline hull is inflated; must match `outline.wgsl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outline {
    /// No outline (ground, very large surfaces).
    None,
    /// Rounded shapes: spheres, capsules.
    Smooth,
    /// Boxes centred on their origin.
    Box,
    /// Upright cylinders centred on their origin.
    Cylinder,
}

#[derive(ShaderType, Debug, Clone, Copy)]
pub struct OutlineSettings {
    pub color: LinearRgba,
    pub params: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct OutlineMaterial {
    #[uniform(0)]
    pub settings: OutlineSettings,
}

impl Material for OutlineMaterial {
    fn vertex_shader() -> ShaderRef {
        OUTLINE_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        OUTLINE_SHADER.into()
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Hide the near side of the inflated copy so only the rim shows.
        descriptor.primitive.cull_mode = Some(Face::Front);
        Ok(())
    }
}

/// Everything needed to build toon-shaded objects, bundled for convenience.
#[derive(SystemParam)]
pub struct ToonAssets<'w> {
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub materials: ResMut<'w, Assets<ToonMaterial>>,
    pub outlines: ResMut<'w, Assets<OutlineMaterial>>,
}

impl ToonAssets<'_> {
    /// A plain toon material of one colour.
    pub fn material(&mut self, color: Color) -> Handle<ToonMaterial> {
        self.glowing(color, LinearRgba::BLACK)
    }

    /// A toon material for large flat ground. Rim light is turned off
    /// because it would brighten the whole floor towards the horizon.
    pub fn ground(&mut self, color: Color) -> Handle<ToonMaterial> {
        let mut material = self.toon_material(color, LinearRgba::BLACK);
        material.extension.settings.rim_color.alpha = 0.0;
        self.materials.add(material)
    }

    /// A toon material that also glows (needs bloom on the camera to shine).
    pub fn glowing(&mut self, color: Color, emissive: LinearRgba) -> Handle<ToonMaterial> {
        let material = self.toon_material(color, emissive);
        self.materials.add(material)
    }

    fn toon_material(&self, color: Color, emissive: LinearRgba) -> ToonMaterial {
        ToonMaterial {
            base: StandardMaterial {
                base_color: color,
                emissive,
                perceptual_roughness: 1.0,
                ..default()
            },
            extension: ToonExtension::default(),
        }
    }

    /// Spawn one visible part with an optional outline, as a child of `parent`.
    pub fn spawn_part(
        &mut self,
        commands: &mut Commands,
        parent: Entity,
        mesh: impl Into<Mesh>,
        material: Handle<ToonMaterial>,
        outline: Outline,
        transform: Transform,
    ) -> Entity {
        let mesh = self.meshes.add(mesh);
        let part = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                transform,
                ChildOf(parent),
            ))
            .id();
        let mode = match outline {
            Outline::None => return part,
            Outline::Smooth => 0.0,
            Outline::Box => 1.0,
            Outline::Cylinder => 2.0,
        };
        let outline_material = self.outlines.add(OutlineMaterial {
            settings: OutlineSettings {
                color: OUTLINE_COLOR,
                params: Vec4::new(OUTLINE_THICKNESS, mode, 0.0, 0.0),
            },
        });
        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(outline_material),
            NotShadowCaster,
            ChildOf(part),
        ));
        part
    }
}
