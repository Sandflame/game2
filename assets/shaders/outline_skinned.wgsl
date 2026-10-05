// Sample only: the game's outline, plus support for animated (skinned) meshes.

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
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> outline: OutlineSettings;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
#ifdef SKINNED
    let world_from_local = skinning::skin_model(vertex.joint_indices, vertex.joint_weights, vertex.instance_index);
#else
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
#endif
    let p = vertex.position;
    let world_position = (world_from_local * vec4<f32>(p, 1.0)).xyz;
    let world_push = normalize((world_from_local * vec4<f32>(vertex.normal, 0.0)).xyz);
    let distance_to_camera = clamp(distance(view.world_position.xyz, world_position), 2.0, 60.0);
    let inflated = world_position + world_push * outline.params.x * distance_to_camera;
    out.world_position = vec4<f32>(inflated, 1.0);
    out.position = position_world_to_clip(inflated);
    out.world_normal = world_push;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return outline.color;
}
