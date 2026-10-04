// Ground markers (telegraphs). The mesh is a flat quad whose UVs are its
// own position on the ground in metres (x right, y = z, so "ahead" is -y).
// Everything is worked out here from the shape: a crisp outer line, a soft
// glow just inside the edge, gentle ripples, the fill that grows until the
// attack lands, and a moving hint (chevrons or inward rings).

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
    // x: progress 0..1, y: outer line width in metres, z: fade in/out
    // (0..1), w: hint pattern (0 none, 1 chevrons forward, 2 rings inward).
    state: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> marker: MarkerSettings;

const PI: f32 = 3.14159265;
// How far the soft inner glow reaches from the edge (metres).
const GLOW_REACH: f32 = 1.1;
// Width of the bright front of the growing fill (metres).
const FRONT_WIDTH: f32 = 0.12;
// Ripples: spacing (metres) and speed.
const RIPPLE_SPACING: f32 = 2.6;
const RIPPLE_SPEED: f32 = 0.9;
// Hint pattern: spacing (metres), speed, and how far chevrons bend.
const HINT_SPACING: f32 = 2.2;
const HINT_SPEED: f32 = 1.6;
const CHEVRON_BEND: f32 = 0.55;

struct Shape {
    // Signed distance to the edge in metres (negative inside).
    d: f32,
    // How far through the shape the fill has to travel (0..1).
    through: f32,
    // The fill's full travel in metres.
    extent: f32,
    // Distance travelled "forwards" in metres, and sideways from the middle.
    along: f32,
    across: f32,
}

fn shape_of(p: vec2<f32>) -> Shape {
    let kind = marker.shape.x;
    let r = length(p);
    var s: Shape;
    s.along = r;
    s.across = 0.0;
    if kind < 0.5 {
        let radius = marker.shape.y;
        s.d = r - radius;
        s.through = r / radius;
        s.extent = radius;
    } else if kind < 1.5 {
        let inner = marker.shape.y;
        let outer = marker.shape.z;
        // Donuts fill from the outside in, towards the safe middle.
        s.d = max(r - outer, inner - r);
        s.through = (outer - r) / (outer - inner);
        s.extent = outer - inner;
    } else if kind < 2.5 {
        let radius = marker.shape.y;
        let half = marker.shape.z;
        // Angle away from straight ahead (-y).
        let angle = atan2(p.x, -p.y);
        let off = abs(angle) - half;
        var side: f32;
        if off > 0.0 {
            side = select(r, r * sin(off), off < PI / 2.0);
        } else {
            side = r * sin(off);
        }
        if half >= PI - 0.001 {
            side = -1000.0;
        }
        s.d = max(r - radius, side);
        s.through = r / radius;
        s.extent = radius;
        s.across = angle * r;
    } else {
        let length_ = marker.shape.y;
        let width = marker.shape.z;
        let along = -p.y;
        s.d = max(max(-along, along - length_), abs(p.x) - width / 2.0);
        s.through = along / length_;
        s.extent = length_;
        s.along = along;
        s.across = p.x;
    }
    return s;
}

// A soft band that is 1 where `x` is within `width` of 0, anti-aliased.
fn band(x: f32, width: f32, aa: f32) -> f32 {
    return 1.0 - smoothstep(width - aa, width + aa, abs(x));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let s = shape_of(in.uv);
    let progress = marker.state.x;
    let line_width = marker.state.y;
    let appear = marker.state.z;
    let hint = marker.state.w;
    let time = globals.time;
    let through = clamp(s.through, 0.0, 1.0);

    // Smooth edges at any zoom. (Screen-space rates are all measured here,
    // before any pixel is thrown away.)
    let aa = max(fwidth(s.d), 0.002);
    let t_aa = max(fwidth(s.through), 0.0005);
    // How finely the screen samples this spot; patterns fade out where
    // they would turn into noise.
    let detail = clamp(1.0 - fwidth(s.along) * 2.5, 0.0, 1.0);
    let inside = 1.0 - smoothstep(-aa, aa, s.d);
    if inside <= 0.0 {
        discard;
    }

    // Base: faint in the middle, glowing towards the edge.
    let glow = exp(s.d / GLOW_REACH);
    var alpha = 0.10 + 0.20 * glow;
    var brightness = 1.0 + 0.4 * glow;

    // Gentle ripples drifting outwards.
    let ripple = 0.5 + 0.5 * sin((s.along / RIPPLE_SPACING - time * RIPPLE_SPEED) * 2.0 * PI);
    alpha += 0.035 * ripple * detail;

    // The fill growing until the attack lands: deeper colour behind a
    // bright front, a little brighter just behind the front.
    let filled = 1.0 - smoothstep(progress - t_aa, progress + t_aa, s.through);
    let trail = smoothstep(progress - 0.3, progress, through) * filled;
    alpha += 0.12 * filled + 0.08 * trail;
    let front_width = FRONT_WIDTH / max(s.extent, 0.01);
    let front = band(s.through - progress, front_width, t_aa) * step(0.01, progress)
        * step(progress, 0.995);
    let front_halo = exp(-abs(s.through - progress) / (front_width * 4.0)) * filled;
    alpha += 0.45 * front + 0.12 * front_halo;
    brightness += 1.2 * front + 0.3 * front_halo;

    // A hint of what to do.
    if hint > 0.5 {
        var phase: f32;
        if hint < 1.5 {
            // Chevrons rushing the way the attack goes.
            phase = (s.along - abs(s.across) * CHEVRON_BEND) / HINT_SPACING - time * HINT_SPEED;
        } else {
            // Rings drawing inwards.
            phase = length(in.uv) / HINT_SPACING + time * HINT_SPEED;
        }
        let v = fract(phase);
        let stripe = smoothstep(0.0, 0.12, v) * (1.0 - smoothstep(0.22, 0.34, v));
        alpha += 0.10 * stripe * detail * (1.0 - 0.5 * filled);
        brightness += 0.25 * stripe * detail;
    }

    // The crisp outer line, with a soft glow just inside it.
    let edge_line = band(s.d + line_width * 0.5, line_width * 0.5, aa);
    let edge_glow = exp(s.d / 0.25);
    alpha = max(alpha, 0.9 * edge_line);
    alpha += 0.15 * edge_glow;
    brightness += 1.3 * edge_line + 0.4 * edge_glow;

    // Pulse in the last fifth, so the moment it lands is clear.
    let urgent = clamp((progress - 0.8) / 0.2, 0.0, 1.0);
    let pulse = urgent * (0.5 + 0.5 * sin(time * 16.0));
    alpha *= 1.0 + 0.45 * pulse;
    brightness += 0.6 * pulse;

    // Appearing: the shape spreads out from where the fill starts.
    let grow = 1.0 - smoothstep(appear * 1.1 - 0.05, appear * 1.1, through);
    let shown = select(grow, 1.0, appear >= 1.0) * min(appear * 2.0, 1.0);

    // The line and front lean towards white so they read on any ground.
    let whiten = clamp(0.35 * edge_line + 0.4 * front, 0.0, 1.0);
    let color = mix(marker.color.rgb, vec3(1.0), whiten) * brightness;
    return vec4(color, clamp(alpha * inside * shown, 0.0, 1.0));
}
