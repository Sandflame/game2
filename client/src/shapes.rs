//! Small mesh builders for character pieces (ears, horns, tails, gear):
//! flat-shaded blades, tubes along curves, and splitting a textured mesh
//! by colour (so skin and hair can be recoloured on their own).

use std::collections::HashMap;
use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

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

/// A blade along +Y with a diamond cross-section: (height, half width) at
/// each step. `thick` is the half thickness as a fraction of the width.
pub fn blade(profile: &[(f32, f32)], thick: f32) -> Mesh {
    let mut p = Vec::new();
    for &(y, w) in profile {
        let t = (w * thick).max(0.0);
        p.extend([[w, y, 0.0], [0.0, y, t], [-w, y, 0.0], [0.0, y, -t]]);
    }
    let mut idx = vec![0, 2, 1, 0, 3, 2];
    for i in 0..profile.len().saturating_sub(1) as u32 {
        for k in 0..4 {
            let a = i * 4 + k;
            let b = i * 4 + (k + 1) % 4;
            idx.extend([a, b, b + 4, a, b + 4, a + 4]);
        }
    }
    finish(p, idx)
}

/// A tube along a path, `sides` around, with a radius at each point.
pub fn tube(path: &[Vec3], radii: &[f32], sides: u32) -> Mesh {
    let mut p = Vec::new();
    let mut normal = Vec3::X;
    for (i, (&c, &r)) in path.iter().zip(radii).enumerate() {
        let next = match path.get(i + 1) {
            Some(&next) => next,
            None if i > 0 => c + (c - path[i - 1]),
            None => c + Vec3::Y,
        };
        let prev = if i == 0 { c - (next - c) } else { path[i - 1] };
        let tangent = (next - prev).normalize_or(Vec3::Y);
        normal = (normal - tangent * normal.dot(tangent)).normalize_or(Vec3::Z);
        let bi = tangent.cross(normal);
        for k in 0..sides {
            let a = k as f32 / sides as f32 * TAU;
            p.push((c + (normal * a.cos() + bi * a.sin()) * r).to_array());
        }
    }
    let mut idx = Vec::new();
    for i in 0..path.len().saturating_sub(1) as u32 {
        for k in 0..sides {
            let a = i * sides + k;
            let b = i * sides + (k + 1) % sides;
            idx.extend([a, b, b + sides, a, b + sides, a + sides]);
        }
    }
    finish(p, idx)
}

/// Points along a curve: starts at `start` going `dir`, turning by `bend`
/// radians around `axis` after each `step`.
pub fn curve(start: Vec3, dir: Vec3, axis: Vec3, bend: f32, step: f32, n: usize) -> Vec<Vec3> {
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

/// Radii shrinking from `r0` to a point over `n` steps.
pub fn taper(n: usize, r0: f32) -> Vec<f32> {
    (0..=n)
        .map(|i| r0 * (1.0 - i as f32 / n.max(1) as f32).max(0.02))
        .collect()
}

/// What part of a character a triangle of its texture belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    Skin,
    Hair,
    Other,
}

/// One piece of a split mesh and the average colour (sRGB 0–1) of its
/// texture, so a tint can turn that colour into another.
pub struct Piece {
    pub mesh: Mesh,
    pub average: Vec3,
}

/// Is this texture colour (sRGB 0–255) the pack's skin? Light, warm,
/// peachy colours.
pub fn is_skin([r, g, b]: [u8; 3]) -> bool {
    let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
    r > 190 && g > 130 && b > 95 && r > g && g > b && (40..=140).contains(&(r - b))
}

/// Split a textured mesh by the colour under each triangle's middle.
/// `classify` gets the triangle's middle (in the mesh's space) and its
/// colour. Pieces with no triangles are left out.
pub fn split_by_colour(
    mesh: &Mesh,
    image: &Image,
    classify: impl Fn(Vec3, [u8; 3]) -> Part,
) -> HashMap<Part, Piece> {
    let mut pieces = HashMap::new();
    let (Some(VertexAttributeValues::Float32x3(positions)), Some(uvs), Some(data)) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_UV_0),
        image.data.as_ref(),
    ) else {
        return pieces;
    };
    let VertexAttributeValues::Float32x2(uvs) = uvs else {
        return pieces;
    };
    let (width, height) = (image.width() as usize, image.height() as usize);
    if width == 0 || height == 0 || data.len() < width * height * 4 {
        return pieces;
    }
    let sample = |uv: Vec2| -> [u8; 3] {
        let x = ((uv.x.rem_euclid(1.0) * width as f32) as usize).min(width - 1);
        let y = ((uv.y.rem_euclid(1.0) * height as f32) as usize).min(height - 1);
        let at = (y * width + x) * 4;
        [data[at], data[at + 1], data[at + 2]]
    };
    let indices: Vec<u32> = match mesh.indices() {
        Some(indices) => indices.iter().map(|i| i as u32).collect(),
        None => (0..positions.len() as u32).collect(),
    };
    let mut lists: HashMap<Part, (Vec<u32>, Vec3, f32)> = HashMap::new();
    for tri in indices.chunks_exact(3) {
        let [a, b, c] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        if a.max(b).max(c) >= positions.len().min(uvs.len()) {
            continue;
        }
        let middle =
            (Vec3::from(positions[a]) + Vec3::from(positions[b]) + Vec3::from(positions[c])) / 3.0;
        let uv = (Vec2::from(uvs[a]) + Vec2::from(uvs[b]) + Vec2::from(uvs[c])) / 3.0;
        let colour = sample(uv);
        let part = classify(middle, colour);
        let entry = lists.entry(part).or_insert((Vec::new(), Vec3::ZERO, 0.0));
        entry.0.extend_from_slice(tri);
        entry.1 += Vec3::new(
            f32::from(colour[0]),
            f32::from(colour[1]),
            f32::from(colour[2]),
        ) / 255.0;
        entry.2 += 1.0;
    }
    for (part, (list, total, count)) in lists {
        let mut piece = mesh.clone();
        piece.insert_indices(Indices::U32(list));
        pieces.insert(
            part,
            Piece {
                mesh: piece,
                average: total / count.max(1.0),
            },
        );
    }
    pieces
}

/// The tint that turns a texture's `average` colour into `wanted` (both
/// sRGB 0–1). The material's base colour multiplies the texture.
pub fn tint_towards(average: Vec3, wanted: Vec3) -> Color {
    let to_linear = |c: Vec3| {
        let l = Color::srgb(c.x, c.y, c.z).to_linear();
        Vec3::new(l.red, l.green, l.blue)
    };
    let ratio = to_linear(wanted) / to_linear(average).max(Vec3::splat(0.02));
    Color::LinearRgba(LinearRgba::rgb(ratio.x, ratio.y, ratio.z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    #[test]
    fn the_packs_skin_colours_count_as_skin() {
        for skin in [[243, 176, 136], [247, 196, 161], [245, 188, 151]] {
            assert!(is_skin(skin), "{skin:?}");
        }
        // Hair, beard and dark details don't.
        for other in [[131, 67, 49], [42, 38, 41], [130, 120, 113], [20, 20, 20]] {
            assert!(!is_skin(other), "{other:?}");
        }
    }

    #[test]
    fn a_mesh_splits_by_the_colour_under_each_triangle() {
        // A 2x1 texture: skin on the left, dark hair on the right.
        let image = Image::new(
            Extent3d {
                width: 2,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![240, 180, 140, 255, 40, 30, 30, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [5.0, 0.0, 0.0],
                [6.0, 0.0, 0.0],
                [5.0, 1.0, 0.0],
            ],
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![
                [0.1, 0.5],
                [0.2, 0.5],
                [0.1, 0.5],
                [0.8, 0.5],
                [0.9, 0.5],
                [0.8, 0.5],
            ],
        );
        mesh.insert_indices(Indices::U32(vec![0, 1, 2, 3, 4, 5]));
        let pieces = split_by_colour(&mesh, &image, |_, c| {
            if is_skin(c) { Part::Skin } else { Part::Hair }
        });
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[&Part::Skin].mesh.indices().unwrap().len(), 3);
        assert!((pieces[&Part::Hair].average.x - 40.0 / 255.0).abs() < 1e-4);
    }

    #[test]
    fn tints_turn_one_colour_into_another() {
        let average = Vec3::new(0.9, 0.7, 0.6);
        let tint = tint_towards(average, average).to_linear();
        assert!((tint.red - 1.0).abs() < 1e-4 && (tint.blue - 1.0).abs() < 1e-4);
        let darker = tint_towards(average, Vec3::new(0.45, 0.35, 0.3)).to_linear();
        assert!(darker.red < 1.0 && darker.green < 1.0);
    }
}
