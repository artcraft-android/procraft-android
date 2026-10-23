// The predicted tail of the stroke being drawn: stamps straight onto the window over the canvas,
// never into the layer. They cover the gap between the latest input and where the pen already is.

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
    _p0: u32,
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
}

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) q: vec2<f32>,
    // radius in screen pixels
    @location(1) radius: f32,
    @location(2) alpha: f32,
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
    // one screen pixel to spare for anti-aliasing
    let grow = (rs + 1.0) / rs;
    let q = corners[vi % 6u] * grow;
    let local = vec2<f32>(q.x * r, q.y * r * inst.roundness);
    let c = cos(inst.angle);
    let s = sin(inst.angle);
    let p = inst.center + vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
    var o: Out;
    o.pos = vec4<f32>(ou.m0.x * p.x + ou.m0.z * p.y + ou.m1.x, ou.m0.y * p.x + ou.m0.w * p.y + ou.m1.y, 0.0, 1.0);
    o.q = q;
    o.radius = rs;
    o.alpha = inst.alpha;
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
    var a = 0.0;
    if (ou.shape < 0) {
        let d = length(i.q) * r;
        let rim = clamp(r - d + 0.5, 0.0, 1.0);
        let soft = 1.0 - smoothstep(min(ou.m1.w, 0.999) * r, r + 0.5, d);
        a = min(rim, soft);
    } else {
        let uv = i.q * 0.5 + 0.5;
        let lod = max(log2(256.0 / max(2.0 * r, 1.0)), 0.0);
        let inside = all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0));
        a = select(0.0, textureSampleLevel(shapes, shape_smp, uv, ou.shape, lod).r, inside);
    }
    let small = min(r * 2.0, 1.0);
    a = a * small * small * i.alpha * ou.color.a;
    var rgb = ou.color.rgb;
    if (ou.srgb == 1u) {
        rgb = srgb_to_linear(rgb);
    }
    return vec4<f32>(rgb * a, a);
}
