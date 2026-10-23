// The composited canvas onto the window: a quad placed by the view (move, zoom, rotate).

struct DisplayUniform {
    // the map from the canvas's 0..1 coordinates to clip space: (a, b, c, d) ...
    m0: vec4<f32>,
    // ... (e, f), then 1 when the target encodes sRGB itself
    m1: vec4<f32>,
    // the canvas's colours to the target's, in linear light: three rows, c0.w 1 to apply them
    c0: vec4<f32>,
    c1: vec4<f32>,
    c2: vec4<f32>,
}

@group(0) @binding(0) var composite: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(1) @binding(0) var<uniform> du: DisplayUniform;
// a colour table taking the canvas's colours to the target's (the display only): red along z,
// green along y, blue along x; c1.w 1 to use it
@group(2) @binding(0) var lut: texture_3d<f32>;
@group(2) @binding(1) var lut_smp: sampler;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_display(@builtin(vertex_index) vi: u32) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let uv = corners[vi % 6u];
    var o: Out;
    o.pos = vec4<f32>(du.m0.x * uv.x + du.m0.z * uv.y + du.m1.x, du.m0.y * uv.x + du.m0.w * uv.y + du.m1.y, 0.0, 1.0);
    o.uv = uv;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_display(i: Out) -> @location(0) vec4<f32> {
    let c = textureSample(composite, smp, i.uv);
    // transparent pixels (a hidden background) show as white for now
    var rgb = c.rgb + (1.0 - c.a) * vec3<f32>(1.0);
    // into the target's colour space (another profile's primaries), clipped to it
    if (du.c0.w > 0.5) {
        let lin = srgb_to_linear(rgb);
        let moved = vec3<f32>(dot(du.c0.xyz, lin), dot(du.c1.xyz, lin), dot(du.c2.xyz, lin));
        rgb = linear_to_srgb(clamp(moved, vec3<f32>(0.0), vec3<f32>(1.0)));
    }
    if (du.c1.w > 0.5) {
        // between the table's first and last points' centres
        let n = f32(textureDimensions(lut).x);
        let at = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * ((n - 1.0) / n) + 0.5 / n;
        rgb = textureSampleLevel(lut, lut_smp, at.zyx, 0.0).rgb;
    }
    if (du.m1.z > 0.5) {
        rgb = srgb_to_linear(rgb);
    }
    return vec4<f32>(rgb, 1.0);
}

// ---- thumbnails: part of a layer box-filtered down to the target ----------------------------
// du.m0 = (target width, target height, source pixels per target pixel in x, in y)
// du.m1 = the part: (left, top, width, height) in 0..1 of the layer

@vertex
fn vs_thumb(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_thumb(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = du.m1.xy + pos.xy / du.m0.xy * du.m1.zw;
    // one to one: the pixel itself
    if (du.m0.z <= 1.0 && du.m0.w <= 1.0) {
        return textureSampleLevel(composite, smp, uv, 0.0);
    }
    // a 4×4 grid of bilinear taps across the source pixels this pixel covers
    let step = du.m1.zw / du.m0.xy / 4.0;
    var sum = vec4<f32>(0.0);
    for (var j = 0; j < 4; j++) {
        for (var i = 0; i < 4; i++) {
            let offset = (vec2<f32>(f32(i), f32(j)) - 1.5) * step;
            sum += textureSampleLevel(composite, smp, uv + offset, 0.0);
        }
    }
    return sum / 16.0;
}
