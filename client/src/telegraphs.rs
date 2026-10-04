//! Drawing ground markers (telegraphs): a translucent shape on the ground
//! with a bright edge, and an inner fill that grows until the attack goes
//! off. The rules half creates and removes the markers; this only draws.

use std::f32::consts::FRAC_PI_2;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use shared::telegraphs::{MarkerShape, Placement, Telegraph};

use crate::hud::game_now;

pub struct TelegraphsPlugin;

impl Plugin for TelegraphsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (dress_new_markers, update_markers).chain());
    }
}

/// The drawn parts of a marker.
#[derive(Component)]
struct MarkerVisual {
    progress_mesh: Handle<Mesh>,
    fill_material: Handle<StandardMaterial>,
    base_alpha: f32,
}

/// Small heights so the layers don't flicker against the ground or each other.
const FILL_HEIGHT: f32 = 0.03;
const EDGE_HEIGHT: f32 = 0.04;
const PROGRESS_HEIGHT: f32 = 0.05;
/// Width of the bright edge lines, in metres.
const EDGE_WIDTH: f32 = 0.15;

/// Marker colours: orange-red to dodge, gold to stack on, violet to spread from.
fn marker_color(placement: Placement) -> Color {
    match placement {
        Placement::StackOnTarget => Color::srgb(1.0, 0.82, 0.25),
        Placement::SpreadOnEveryone => Color::srgb(0.80, 0.45, 1.0),
        _ => Color::srgb(1.0, 0.42, 0.15),
    }
}

/// A flat mesh of the marker's shape, `scale` (0–1) of its full size.
/// Built in 2D and laid on the ground, pointing towards -Z.
fn shape_mesh(shape: MarkerShape, scale: f32) -> Mesh {
    let scale = scale.max(0.001);
    let flat = Quat::from_rotation_x(-FRAC_PI_2);
    let mesh: Mesh = match shape {
        MarkerShape::Circle { radius } => Circle::new(radius * scale).mesh().resolution(48).build(),
        MarkerShape::Donut { inner, outer } => {
            // The fill grows from the outside edge inwards.
            let reach = inner + (outer - inner) * (1.0 - scale);
            Annulus::new(reach.min(outer - 0.001), outer)
                .mesh()
                .resolution(64)
                .build()
        }
        MarkerShape::Cone { radius, angle } => {
            CircularSector::new(radius * scale, angle.to_radians() / 2.0)
                .mesh()
                .resolution(48)
                .build()
        }
        MarkerShape::Line { length, width } => {
            let length = length * scale;
            Rectangle::new(width, length)
                .mesh()
                .build()
                .translated_by(Vec3::new(0.0, length / 2.0, 0.0))
        }
    };
    mesh.rotated_by(flat)
}

/// The bright outline pieces of a shape (rings for circles and donuts;
/// cones and lines rely on their brighter fill).
fn edge_meshes(shape: MarkerShape) -> Vec<Mesh> {
    let flat = Quat::from_rotation_x(-FRAC_PI_2);
    let ring = |outer: f32| {
        Annulus::new((outer - EDGE_WIDTH).max(0.01), outer)
            .mesh()
            .resolution(64)
            .build()
            .rotated_by(flat)
    };
    match shape {
        MarkerShape::Circle { radius } => vec![ring(radius)],
        MarkerShape::Donut { inner, outer } => vec![ring(inner + EDGE_WIDTH), ring(outer)],
        MarkerShape::Cone { .. } | MarkerShape::Line { .. } => Vec::new(),
    }
}

fn translucent(
    materials: &mut Assets<StandardMaterial>,
    color: Color,
    alpha: f32,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color.with_alpha(alpha),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        cull_mode: None,
        ..default()
    })
}

/// Give new markers their shapes.
fn dress_new_markers(
    mut commands: Commands,
    new: Query<(Entity, &Telegraph), Added<Telegraph>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, marker) in &new {
        let color = marker_color(marker.placement);
        let has_edges = !edge_meshes(marker.shape).is_empty();
        let base_alpha = if has_edges { 0.18 } else { 0.26 };
        let fill_material = translucent(&mut materials, color, base_alpha);
        let edge_material = translucent(&mut materials, color.lighter(0.15), 0.9);
        let progress_material = translucent(&mut materials, color, 0.32);
        let progress_mesh = meshes.add(shape_mesh(marker.shape, 0.0));

        commands.entity(entity).insert((
            Transform::from_translation(marker.origin)
                .with_rotation(Quat::from_rotation_y(marker.yaw)),
            Visibility::default(),
            MarkerVisual {
                progress_mesh: progress_mesh.clone(),
                fill_material: fill_material.clone(),
                base_alpha,
            },
        ));
        commands.spawn((
            Mesh3d(meshes.add(shape_mesh(marker.shape, 1.0))),
            MeshMaterial3d(fill_material),
            Transform::from_xyz(0.0, FILL_HEIGHT, 0.0),
            NotShadowCaster,
            ChildOf(entity),
        ));
        for edge in edge_meshes(marker.shape) {
            commands.spawn((
                Mesh3d(meshes.add(edge)),
                MeshMaterial3d(edge_material.clone()),
                Transform::from_xyz(0.0, EDGE_HEIGHT, 0.0),
                NotShadowCaster,
                ChildOf(entity),
            ));
        }
        commands.spawn((
            Mesh3d(progress_mesh),
            MeshMaterial3d(progress_material),
            Transform::from_xyz(0.0, PROGRESS_HEIGHT, 0.0),
            NotShadowCaster,
            ChildOf(entity),
        ));
    }
}

/// Follow moving markers, grow the progress fill, and pulse just before
/// the attack lands.
fn update_markers(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    mut markers: Query<(&Telegraph, &MarkerVisual, &mut Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let now = game_now(&fixed);
    for (marker, visual, mut transform) in &mut markers {
        transform.translation = marker.origin;
        transform.rotation = Quat::from_rotation_y(marker.yaw);
        let progress = marker.progress(now);
        if let Some(mut mesh) = meshes.get_mut(&visual.progress_mesh) {
            *mesh = shape_mesh(marker.shape, progress);
        }
        let urgent = ((progress - 0.75) / 0.25).clamp(0.0, 1.0);
        let pulse = 1.0 + urgent * 0.8 * (0.5 + 0.5 * (time.elapsed_secs() * 18.0).sin());
        if let Some(mut material) = materials.get_mut(&visual.fill_material) {
            material.base_color.set_alpha(visual.base_alpha * pulse);
        }
    }
}
