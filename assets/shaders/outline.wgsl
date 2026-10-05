// Black outlines using the "inverted hull" trick: draw a slightly
// inflated copy of the mesh with its front faces hidden, so only a thin
// rim shows around the real object.

#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
    mesh_view_bindings::view,
}
#ifdef SKINNED
#import bevy_pbr::skinning
#endif

struct OutlineSettings {
    color: vec4<f32>,
    // x: thickness (grows with camera distance so lines stay similar on screen)
    // y: how to inflate. 0 = along normals (smooth shapes),
    //    1 = box corners, 2 = cylinder sides and caps.
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> outline: OutlineSettings;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
#ifdef SKINNED
    // Animated models: follow the skeleton.
    let world_from_local = skinning::skin_model(vertex.joint_indices, vertex.joint_weights, vertex.instance_index);
#else
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
#endif

    // Direction to push this vertex outwards, in the mesh's own space.
    // Flat-shaded boxes and cylinders have split normals at their edges,
    // which would tear the hull apart, so they use their shape instead.
    var push = vertex.normal;
    let mode = outline.params.y;
    let p = vertex.position;
    if mode > 1.5 {
        let side = select(vec2<f32>(0.0), normalize(p.xz), length(p.xz) > 1e-5);
        push = vec3<f32>(side.x, sign(p.y), side.y);
    } else if mode > 0.5 {
        push = sign(p);
    }

    let world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(p, 1.0));
    let world_push = (world_from_local * vec4<f32>(push, 0.0)).xyz;
    let distance_to_camera = clamp(distance(view.world_position.xyz, world_position.xyz), 2.0, 60.0);
    let inflated = world_position.xyz + world_push * outline.params.x * distance_to_camera;

    out.world_position = vec4<f32>(inflated, 1.0);
    out.position = position_world_to_clip(inflated);
    out.world_normal = normalize(world_push + vec3<f32>(0.0, 1e-5, 0.0));
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return outline.color;
}
