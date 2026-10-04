// Toon (cel) shading for Lanternflame.
//
// Extends Bevy's StandardMaterial: we reuse its inputs (base colour,
// textures, emissive, shadows) but replace the realistic lighting with a
// few flat bands of light plus a soft rim highlight.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings as view_bindings,
    mesh_view_types,
    shadows,
}

const MESH_FLAGS_SHADOW_RECEIVER_BIT: u32 = 1u << 29u;

struct ToonSettings {
    // Colour multiplied into surfaces that face away from the light.
    shadow_color: vec4<f32>,
    // Rim highlight colour; alpha is strength.
    rim_color: vec4<f32>,
    // x: shadow edge, y: highlight edge, z: mid-tone brightness, w: edge softness.
    bands: vec4<f32>,
    // x: rim start (0..1), y: rim softness.
    rim: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> toon: ToonSettings;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    let base = pbr_input.material.base_color;

    let view_z = dot(vec4<f32>(
        view_bindings::view.view_from_world[0].z,
        view_bindings::view.view_from_world[1].z,
        view_bindings::view.view_from_world[2].z,
        view_bindings::view.view_from_world[3].z
    ), pbr_input.world_position);

    // How much direct light reaches this point (0 = none, 1 = full).
    var light_amount = 0.0;
    let n_lights = view_bindings::lights.n_directional_lights;
    for (var i: u32 = 0u; i < n_lights; i = i + 1u) {
        let light = &view_bindings::lights.directional_lights[i];
        let n_dot_l = max(dot(pbr_input.N, (*light).direction_to_light), 0.0);
        var shadow = 1.0;
        if ((pbr_input.flags & MESH_FLAGS_SHADOW_RECEIVER_BIT) != 0u
            && ((*light).flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
            shadow = shadows::fetch_directional_shadow(
                i, pbr_input.world_position, pbr_input.world_normal, view_z, pbr_input.frag_coord.xy);
        }
        light_amount = max(light_amount, min(n_dot_l, shadow));
    }

    // Snap the light into bands: shadow -> mid-tone -> fully lit.
    let soft = max(toon.bands.w, 0.001);
    let to_mid = smoothstep(toon.bands.x - soft, toon.bands.x + soft, light_amount);
    let to_full = smoothstep(toon.bands.y - soft, toon.bands.y + soft, light_amount);
    let lit = mix(0.0, toon.bands.z, to_mid) + (1.0 - toon.bands.z) * to_full;
    var color = mix(base.rgb * toon.shadow_color.rgb, base.rgb, lit);

    // Rim light on the lit side of silhouettes.
    let facing = 1.0 - max(dot(pbr_input.N, pbr_input.V), 0.0);
    let rim_soft = max(toon.rim.y, 0.001);
    let rim = smoothstep(toon.rim.x - rim_soft, toon.rim.x + rim_soft, facing) * to_mid;
    color += toon.rim_color.rgb * toon.rim_color.a * rim;

    // Glowing parts (lantern flames, magitech crystals).
    color += pbr_input.material.emissive.rgb;

    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(color, base.a));
    return out;
}
