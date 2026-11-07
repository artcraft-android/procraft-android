// The predicted tail of the stroke being drawn: stamps straight onto the window over the canvas,
// never into the layer. They cover the gap between the latest input and where the pen already is.

// how far a stamp's quad reaches past the stamp each way (canvas pixels): its shape fills all of it
const QUAD_REACH: f32 = 0.70710678;

struct OverlayUniform {
    // canvas pixels to clip space: (a, b, c, d) ...
    m0: vec4<f32>,
    // ... (e, f), screen pixels per canvas pixel, the round tip's hardness
    m1: vec4<f32>,
    // straight colour and the stroke's opacity
    color: vec4<f32>,
    // -1: the round tip; otherwise a layer of `shapes`
    shape: i32,
    // 1 when the target encodes sRGB itself
    srgb: u32,
    // the coarsest mip level the shape is read from, as for the stamps
    shape_lod_cap: f32,
    _p1: u32,
}

@group(0) @binding(0) var<uniform> ou: OverlayUniform;
@group(1) @binding(0) var shapes: texture_2d_array<f32>;
@group(1) @binding(1) var grains: texture_2d_array<f32>;
@group(1) @binding(2) var shape_smp: sampler;
@group(1) @binding(3) var grain_smp: sampler;

struct StampIn {
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,
    @location(2) alpha: f32,
    @location(3) angle: f32,
    @location(4) roundness: f32,
    @location(5) grain_offset: vec2<f32>,
    @location(6) flip: vec2<f32>,
}

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) q: vec2<f32>,
    // radius in screen pixels
    @location(1) radius: f32,
    @location(2) alpha: f32,
    // how much of its quad the stamp itself is across each way, mirrored as the stamp says
    @location(3) fill: vec2<f32>,
}

@vertex
fn vs_overlay(@builtin(vertex_index) vi: u32, inst: StampIn) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let zoom = max(ou.m1.z, 1e-3);
    let r = max(inst.radius, 0.25);
    let rs = r * zoom;
    let half = vec2<f32>(r, max(r * inst.roundness, 1e-3));
    // one screen pixel to spare for anti-aliasing, or as far as a shape's quad reaches
    let q = corners[vi % 6u] * (half + max(1.0 / zoom, QUAD_REACH)) / half;
    let local = q * half;
    let c = cos(inst.angle);
    let s = sin(inst.angle);
    let p = inst.center + vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
    var o: Out;
    o.pos = vec4<f32>(ou.m0.x * p.x + ou.m0.z * p.y + ou.m1.x, ou.m0.y * p.x + ou.m0.w * p.y + ou.m1.y, 0.0, 1.0);
    o.q = q;
    o.radius = rs;
    o.alpha = inst.alpha;
    o.fill = inst.flip * half / (half + QUAD_REACH);
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_overlay(i: Out) -> @location(0) vec4<f32> {
    let r = i.radius;
    let zoom = max(ou.m1.z, 1e-3);
    var a = 0.0;
    if (ou.shape < 0) {
        let d = length(i.q) * r;
        let rim = clamp(r - d + 0.5, 0.0, 1.0);
        let soft = 1.0 - smoothstep(min(ou.m1.w, 0.999) * r, r + 0.5, d);
        // as a stamp's: under a canvas pixel across, only its area of one
        let small = min(2.0 * r / zoom, 1.0);
        a = min(rim, soft) * small * small;
    } else {
        // as a stamp's: filling its quad, at the same mip level
        let uv = i.q * i.fill * 0.5 + 0.5;
        let side = f32(textureDimensions(shapes).x);
        let lod = clamp(log2(0.5 * side / (r / zoom + QUAD_REACH)), 0.0, ou.shape_lod_cap + log2(side / 256.0));
        let inside = all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0));
        a = select(0.0, textureSampleLevel(shapes, shape_smp, uv, ou.shape, lod).r, inside);
    }
    a = a * i.alpha * ou.color.a;
    var rgb = ou.color.rgb;
    if (ou.srgb == 1u) {
        rgb = srgb_to_linear(rgb);
    }
    return vec4<f32>(rgb * a, a);
}
