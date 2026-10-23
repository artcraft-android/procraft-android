// Frosted panels: what is behind a panel, drawn small, blurred, and put back in its place for a
// translucent panel to be drawn over.

struct BackdropUniform {
    // blur: (step x, step y in 0..1 of the source, sigma in texels, radius in texels)
    // frost: the region (left, top, right, bottom) in target pixels
    v0: vec4<f32>,
    // frost: (corner radius, target width, target height, target pixels per source texel)
    v1: vec4<f32>,
    // frost: (how much it shows, 0..1, as the panel fades in and out; unused)
    v2: vec4<f32>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(1) @binding(0) var<uniform> u: BackdropUniform;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// One direction of a Gaussian blur, from a source the size of the target.
@fragment
fn fs_blur(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = pos.xy / vec2<f32>(textureDimensions(source));
    let sigma = max(u.v0.z, 0.5);
    let radius = i32(u.v0.w);
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var i = -radius; i <= radius; i++) {
        let x = f32(i);
        let w = exp(-0.5 * x * x / (sigma * sigma));
        sum += textureSampleLevel(source, smp, uv + u.v0.xy * x, 0.0) * w;
        total += w;
    }
    return sum / total;
}

@vertex
fn vs_frost(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    // a pixel beyond the region, for its smoothed edge
    let p = mix(u.v0.xy - 1.0, u.v0.zw + 1.0, corners[vi % 6u]);
    return vec4<f32>(p.x / u.v1.y * 2.0 - 1.0, 1.0 - p.y / u.v1.z * 2.0, 0.0, 1.0);
}

// The blurred scene inside the rounded region, its edge smoothed over a pixel.
@fragment
fn fs_frost(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let centre = (u.v0.xy + u.v0.zw) * 0.5;
    let half = max((u.v0.zw - u.v0.xy) * 0.5, vec2<f32>(0.0));
    let r = clamp(u.v1.x, 0.0, min(half.x, half.y));
    let q = abs(pos.xy - centre) - (half - vec2<f32>(r));
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    let cover = clamp(0.5 - d, 0.0, 1.0) * clamp(u.v2.x, 0.0, 1.0);
    let uv = pos.xy / (u.v1.w * vec2<f32>(textureDimensions(source)));
    let c = textureSampleLevel(source, smp, uv, 0.0);
    return vec4<f32>(c.rgb * cover, cover);
}
