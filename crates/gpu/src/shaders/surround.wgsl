// Grid lines over the colour around the canvas, fixed to the window.

struct SurroundUniform {
    // (spacing, origin x, origin y, line width), in target pixels
    v0: vec4<f32>,
    // (line colour, 1 when the target encodes sRGB itself)
    v1: vec4<f32>,
}

@group(0) @binding(0) var<uniform> su: SurroundUniform;

@vertex
fn vs_surround(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_surround(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = pos.xy - su.v0.yz;
    let s = su.v0.x;
    // how far this pixel's centre is from the nearest line across and down
    let d = abs(p - round(p / s) * s);
    if (p.y < 0.0 || min(d.x, d.y) >= su.v0.w * 0.5) {
        discard;
    }
    var rgb = su.v1.rgb;
    if (su.v1.w > 0.5) {
        rgb = srgb_to_linear(rgb);
    }
    return vec4<f32>(rgb, 1.0);
}
