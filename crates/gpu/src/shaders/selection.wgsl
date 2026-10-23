// How the selection shows over the canvas, fixed to the window: faint diagonal bands, dark and
// light by turns, drifting over what is not selected; or what is selected multiplied by a colour
// (a tint) or covered by one.

struct SelectionUniform {
    // window pixel to the canvas's unit square: (a, b, c, d) of x' = a·x + c·y + e, y' = b·x + d·y + f
    m0: vec4<f32>,
    // (e, f, the bands' period across the window in pixels, their drift in pixels)
    m1: vec4<f32>,
    // (colour for a tint or fill, the bands' coverage)
    v: vec4<f32>,
    // (0 bands, 1 tint, 2 fill; 1 when the target encodes sRGB itself; unused)
    mode: vec4<f32>,
}

@group(0) @binding(0) var mask: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(1) @binding(0) var<uniform> su: SelectionUniform;

@vertex
fn vs_selection(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_selection(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = pos.xy;
    let u = vec2<f32>(su.m0.x * p.x + su.m0.z * p.y + su.m1.x, su.m0.y * p.x + su.m0.w * p.y + su.m1.y);
    if (any(u < vec2<f32>(0.0)) || any(u > vec2<f32>(1.0))) {
        discard;
    }
    let selected = textureSampleLevel(mask, smp, u, 0.0).r;
    var c = su.v.rgb;
    if (su.mode.y > 0.5) {
        c = srgb_to_linear(c);
    }
    let mode = i32(su.mode.x + 0.5);
    if (mode == 1) {
        // multiplied (the pipeline blends by multiplying)
        return vec4<f32>(mix(vec3<f32>(1.0), c, selected), 1.0);
    }
    if (mode == 2) {
        return vec4<f32>(c * selected, selected);
    }
    // bands rising to the right, dark and light by turns, with soft edges
    let band = fract((p.x + p.y + su.m1.w) / su.m1.z);
    let light = smoothstep(0.09, 0.25, band) - smoothstep(0.59, 0.75, band);
    let a = (1.0 - selected) * su.v.w;
    return vec4<f32>(vec3<f32>(light * a), a);
}
