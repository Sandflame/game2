//! Drawing ground markers (telegraphs) with their own shader
//! (`assets/shaders/marker.wgsl`): a translucent shape with a bright rim and
//! flowing stripes, and a fill that grows until the attack goes off. When a
//! marker goes off, dust and light burst over its whole area. The rules half
//! creates and removes the markers; this only draws.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use shared::components::Zone;
use shared::telegraphs::{MarkerShape, Placement, Telegraph, covers};

use crate::camera::CameraShake;
use crate::hud::game_now;
use crate::vfx::Looks;
use crate::world::CurrentZone;

const MARKER_SHADER: &str = "shaders/marker.wgsl";

pub struct TelegraphsPlugin;

impl Plugin for TelegraphsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<MarkerMaterial>::default())
            .init_resource::<KnownMarkers>()
            .add_systems(
                Update,
                (
                    dress_new_markers,
                    update_markers,
                    markers_going_off,
                    fade_spent_markers,
                )
                    .chain(),
            );
    }
}

#[derive(ShaderType, Debug, Clone, Copy)]
pub struct MarkerSettings {
    pub color: LinearRgba,
    /// x: shape kind, y/z: its sizes (see `marker.wgsl`).
    pub shape: Vec4,
    /// x: progress, y: rim width, z: fade.
    pub state: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct MarkerMaterial {
    #[uniform(0)]
    pub settings: MarkerSettings,
}

impl Material for MarkerMaterial {
    fn fragment_shader() -> ShaderRef {
        MARKER_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
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
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The drawn marker.
#[derive(Component)]
struct MarkerVisual {
    material: Handle<MarkerMaterial>,
    /// Seconds since it appeared (for fading in).
    age: f32,
}

/// A marker that just went off: it flashes and fades away.
#[derive(Component)]
struct SpentMarker {
    material: Handle<MarkerMaterial>,
    age: f32,
}

/// What each marker looked like last frame, so a burst can be shown over
/// its area once the rules half removes it.
#[derive(Resource, Default)]
struct KnownMarkers(HashMap<Entity, (Telegraph, String)>);

/// Just above the ground so it doesn't flicker against it.
const MARKER_HEIGHT: f32 = 0.04;
/// Width of the bright rim, in metres.
const RIM_WIDTH: f32 = 0.18;
/// Extra quad around the shape so its soft edge isn't cut off.
const MARGIN: f32 = 0.2;
/// Seconds to fade in.
const FADE_IN: f32 = 0.15;
/// Seconds a marker that went off takes to fade away.
const FADE_OUT: f32 = 0.4;
/// At most this many dust bursts when a marker goes off.
const MAX_BURSTS: f32 = 24.0;
/// Bursts are never closer together than this (metres).
const MIN_BURST_SPACING: f32 = 2.5;
/// How much the camera shakes when a marker goes off (0–1).
const MARKER_SHAKE: f32 = 0.6;
/// A marker removed this close to (or after) its time went off; one
/// removed earlier was cleared (e.g. a wipe) and shows nothing.
const WENT_OFF_TOLERANCE: f64 = 0.1;

/// Marker colours: orange-red to dodge, gold to stack on, violet to spread from.
fn marker_color(placement: Placement) -> LinearRgba {
    match placement {
        Placement::StackOnTarget => LinearRgba::rgb(1.0, 0.70, 0.12),
        Placement::SpreadOnEveryone => LinearRgba::rgb(0.65, 0.25, 1.0),
        _ => LinearRgba::rgb(1.0, 0.25, 0.06),
    }
}

/// The look (`vfx.ron`) played when a marker goes off.
fn burst_look(placement: Placement) -> &'static str {
    match placement {
        Placement::StackOnTarget => "marker_stack",
        Placement::SpreadOnEveryone => "marker_spread",
        _ => "marker_dodge",
    }
}

/// The shape as the shader's numbers.
fn shape_numbers(shape: MarkerShape) -> Vec4 {
    match shape {
        MarkerShape::Circle { radius } => Vec4::new(0.0, radius, 0.0, 0.0),
        MarkerShape::Donut { inner, outer } => Vec4::new(1.0, inner, outer, 0.0),
        MarkerShape::Cone { radius, angle } => {
            Vec4::new(2.0, radius, angle.to_radians() / 2.0, 0.0)
        }
        MarkerShape::Line { length, width } => Vec4::new(3.0, length, width, 0.0),
    }
}

/// The ground rectangle (min x, min z, max x, max z) the shape fits in,
/// with the marker pointing towards -Z.
fn bounds(shape: MarkerShape) -> (f32, f32, f32, f32) {
    let (x0, z0, x1, z1) = match shape {
        MarkerShape::Circle { radius: r } | MarkerShape::Donut { outer: r, .. } => (-r, -r, r, r),
        MarkerShape::Cone { radius, angle } => {
            let half = angle.to_radians() / 2.0;
            if half <= std::f32::consts::FRAC_PI_2 {
                let side = radius * half.sin();
                (-side, -radius, side, 0.0)
            } else {
                (-radius, -radius, radius, radius)
            }
        }
        MarkerShape::Line { length, width } => (-width / 2.0, -length, width / 2.0, 0.0),
    };
    (x0 - MARGIN, z0 - MARGIN, x1 + MARGIN, z1 + MARGIN)
}

/// A flat rectangle whose UVs are its own ground position in metres.
fn quad_mesh(shape: MarkerShape) -> Mesh {
    let (x0, z0, x1, z1) = bounds(shape);
    let corners = [[x0, z0], [x1, z0], [x1, z1], [x0, z1]];
    let positions: Vec<[f32; 3]> = corners.iter().map(|[x, z]| [*x, 0.0, *z]).collect();
    let uvs: Vec<[f32; 2]> = corners.to_vec();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]))
}

/// Give new markers their look.
fn dress_new_markers(
    mut commands: Commands,
    new: Query<(Entity, &Telegraph), Added<Telegraph>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<MarkerMaterial>>,
) {
    for (entity, marker) in &new {
        let material = materials.add(MarkerMaterial {
            settings: MarkerSettings {
                color: marker_color(marker.placement),
                shape: shape_numbers(marker.shape),
                state: Vec4::new(0.0, RIM_WIDTH, 0.0, 0.0),
            },
        });
        commands.entity(entity).insert((
            Transform::from_translation(marker.origin)
                .with_rotation(Quat::from_rotation_y(marker.yaw)),
            Visibility::default(),
            MarkerVisual {
                material: material.clone(),
                age: 0.0,
            },
        ));
        commands.spawn((
            Mesh3d(meshes.add(quad_mesh(marker.shape))),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, MARKER_HEIGHT, 0.0),
            NotShadowCaster,
            ChildOf(entity),
        ));
    }
}

/// Follow moving markers, grow the fill, and remember each marker.
fn update_markers(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    mut known: ResMut<KnownMarkers>,
    mut markers: Query<(Entity, &Telegraph, &Zone, &mut MarkerVisual, &mut Transform)>,
    mut materials: ResMut<Assets<MarkerMaterial>>,
) {
    let now = game_now(&fixed);
    for (entity, marker, zone, mut visual, mut transform) in &mut markers {
        transform.translation = marker.origin;
        transform.rotation = Quat::from_rotation_y(marker.yaw);
        visual.age += time.delta_secs();
        if let Some(mut material) = materials.get_mut(&visual.material) {
            material.settings.state.x = marker.progress(now);
            material.settings.state.z = (visual.age / FADE_IN).min(1.0);
        }
        known.0.insert(entity, (marker.clone(), zone.0.clone()));
    }
}

/// Points spread over a marker's area for the dust bursts.
pub fn burst_points(marker: &Telegraph) -> Vec<Vec3> {
    let (x0, z0, x1, z1) = bounds(marker.shape);
    let area = (x1 - x0) * (z1 - z0);
    let spacing = (area / MAX_BURSTS).sqrt().max(MIN_BURST_SPACING);
    let rotation = Quat::from_rotation_y(marker.yaw);
    let mut points = Vec::new();
    let mut z = z0 + spacing / 2.0;
    while z < z1 {
        let mut x = x0 + spacing / 2.0;
        while x < x1 {
            let point = marker.origin + rotation * Vec3::new(x, 0.0, z);
            if covers(marker.shape, marker.origin, marker.yaw, point, 0.0) {
                points.push(point);
            }
            x += spacing;
        }
        z += spacing;
    }
    if points.is_empty() {
        points.push(marker.origin);
    }
    points
}

/// Dust and light over the area of markers that just went off.
fn markers_going_off(
    mut commands: Commands,
    fixed: Res<Time<Fixed>>,
    current: Res<CurrentZone>,
    mut known: ResMut<KnownMarkers>,
    mut removed: RemovedComponents<Telegraph>,
    mut shake: ResMut<CameraShake>,
    mut materials: ResMut<Assets<MarkerMaterial>>,
    mut looks: Looks,
) {
    let now = game_now(&fixed);
    for entity in removed.read() {
        let Some((marker, zone)) = known.0.remove(&entity) else {
            continue;
        };
        if current.0.as_deref() != Some(zone.as_str()) || now + WENT_OFF_TOLERANCE < marker.resolves
        {
            continue;
        }
        // A flash of the whole shape that fades out.
        let material = materials.add(MarkerMaterial {
            settings: MarkerSettings {
                color: marker_color(marker.placement) * 2.0,
                shape: shape_numbers(marker.shape),
                state: Vec4::new(1.0, RIM_WIDTH, 1.0, 0.0),
            },
        });
        commands.spawn((
            Mesh3d(looks.meshes().add(quad_mesh(marker.shape))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(marker.origin + Vec3::Y * MARKER_HEIGHT)
                .with_rotation(Quat::from_rotation_y(marker.yaw)),
            NotShadowCaster,
            SpentMarker { material, age: 0.0 },
        ));
        let look = burst_look(marker.placement);
        for point in burst_points(&marker) {
            looks.particles(look, point);
        }
        looks.sound(look);
        shake.add(MARKER_SHAKE);
    }
}

fn fade_spent_markers(
    mut commands: Commands,
    time: Res<Time>,
    mut spent: Query<(Entity, &mut SpentMarker)>,
    mut materials: ResMut<Assets<MarkerMaterial>>,
) {
    for (entity, mut marker) in &mut spent {
        marker.age += time.delta_secs();
        let left = 1.0 - marker.age / FADE_OUT;
        if left <= 0.0 {
            commands.entity(entity).despawn();
        } else if let Some(mut material) = materials.get_mut(&marker.material) {
            material.settings.state.z = left * left;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(shape: MarkerShape) -> Telegraph {
        Telegraph {
            shape,
            placement: Placement::Caster,
            origin: Vec3::new(3.0, 0.0, -2.0),
            yaw: 0.7,
            follow: None,
            caster: Entity::PLACEHOLDER,
            ability: String::new(),
            starts: 0.0,
            resolves: 1.0,
        }
    }

    #[test]
    fn bursts_cover_the_area_and_stay_inside() {
        for shape in [
            MarkerShape::Circle { radius: 6.0 },
            MarkerShape::Donut {
                inner: 5.0,
                outer: 30.0,
            },
            MarkerShape::Cone {
                radius: 18.0,
                angle: 100.0,
            },
            MarkerShape::Line {
                length: 34.0,
                width: 5.0,
            },
            MarkerShape::Circle { radius: 0.5 },
        ] {
            let m = marker(shape);
            let points = burst_points(&m);
            assert!(!points.is_empty(), "{shape:?}");
            assert!(
                points.len() as f32 <= MAX_BURSTS * 1.5,
                "{shape:?}: {}",
                points.len()
            );
            if points.len() > 1 {
                for p in &points {
                    assert!(m.covers(*p, 0.0), "{shape:?}: {p} outside");
                }
            }
        }
    }
}
