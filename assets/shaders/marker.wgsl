// Ground markers (telegraphs). The mesh is a flat quad whose UVs are its
// own position on the ground in metres (x right, y = z, so "ahead" is -y).
// Everything else is worked out here from the shape: what is inside, a
// bright rim, gently moving stripes, and the fill that grows until the
// attack lands.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::globals,
}

struct MarkerSettings {
    color: vec4<f32>,
    // x: shape (0 circle, 1 donut, 2 cone, 3 line), then its sizes:
    //   circle (radius), donut (inner, outer), cone (radius, half angle
    //   in radians), line (length, width).
    shape: vec4<f32>,
    // x: progress 0..1, y: rim width in metres, z: fade in/out (0..1).
    state: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> marker: MarkerSettings;

const PI: f32 = 3.14159265;

// Signed distance to the shape's edge (negative inside) and how far
// through the shape the point is in the direction the fill grows (0..1).
fn shape_info(p: vec2<f32>) -> vec2<f32> {
    let kind = marker.shape.x;
    let r = length(p);
    if kind < 0.5 {
        let radius = marker.shape.y;
        return vec2(r - radius, r / radius);
    } else if kind < 1.5 {
        let inner = marker.shape.y;
        let outer = marker.shape.z;
        // Donuts fill from the outside in, towards the safe middle.
        return vec2(max(r - outer, inner - r), (outer - r) / (outer - inner));
    } else if kind < 2.5 {
        let radius = marker.shape.y;
        let half = marker.shape.z;
        // Angle away from straight ahead (-y).
        let angle = abs(atan2(p.x, -p.y));
        var side: f32;
        if angle > half {
            side = select(r, r * sin(angle - half), angle - half < PI / 2.0);
        } else {
            side = -r * sin(half - angle);
        }
        if half >= PI - 0.001 {
            side = -1000.0;
        }
        return vec2(max(r - radius, side), r / radius);
    } else {
        let length_ = marker.shape.y;
        let width = marker.shape.z;
        let along = -p.y;
        let d = max(max(-along, along - length_), abs(p.x) - width / 2.0);
        return vec2(d, along / length_);
    }
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.uv;
    let info = shape_info(p);
    let d = info.x;
    let through = clamp(info.y, 0.0, 1.0);
    let progress = marker.state.x;
    let rim = marker.state.y;
    let time = globals.time;

    // Smooth edges without jaggies, whatever the zoom.
    let aa = max(fwidth(d), 0.001);
    let inside = 1.0 - smoothstep(-aa, aa, d);
    if inside <= 0.0 {
        discard;
    }

    // Base fill with slow stripes flowing outwards.
    let stripes = 0.5 + 0.5 * sin(through * 40.0 - time * 3.0);
    var alpha = 0.16 + 0.05 * stripes;

    // The fill that grows until the attack lands, with a bright front.
    let filled = 1.0 - smoothstep(progress - 0.01, progress, through);
    alpha += 0.20 * filled;
    let front = exp(-pow((through - progress) * 30.0, 2.0)) * step(0.02, progress);
    alpha += 0.35 * front;

    // The bright rim.
    let edge = smoothstep(-rim, -rim * 0.4, d);
    alpha = max(alpha, 0.85 * edge);

    // Pulse urgently in the last quarter.
    let urgent = clamp((progress - 0.75) / 0.25, 0.0, 1.0);
    alpha *= 1.0 + urgent * 0.7 * (0.5 + 0.5 * sin(time * 18.0));

    var color = marker.color.rgb * (1.0 + 0.8 * edge + 1.2 * front);
    return vec4(color, clamp(alpha * inside * marker.state.z, 0.0, 1.0));
}
