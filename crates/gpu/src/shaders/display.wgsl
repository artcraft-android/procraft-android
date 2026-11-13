// The composited canvas onto the window: a quad placed by the view (move, zoom, rotate).

struct DisplayUniform {
    // the map from the canvas's 0..1 coordinates to clip space: (a, b, c, d) ...
    m0: vec4<f32>,
    // ... (e, f), then 1 when the target encodes sRGB itself, then unused
    m1: vec4<f32>,
    // the canvas's colours to the target's, in linear light: three rows, c0.w 1 to apply them
    c0: vec4<f32>,
    c1: vec4<f32>,
    c2: vec4<f32>,
    // how far the quad reaches past the canvas's sides (its 0..1 across and down), then 1 to
    // smooth the edge over that; all 0 for a quad drawn exactly
    edge: vec4<f32>,
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
    // pushed out past each side as far as the edge is smoothed (not at all for a quad drawn exactly)
    let uv = corners[vi % 6u] * (1.0 + 2.0 * du.edge.xy) - du.edge.xy;
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
    // how much of this target pixel the canvas covers, by how far in from each side its middle
    // is (in target pixels): a turned canvas's sides show no steps
    let across = length(vec2<f32>(dpdx(i.uv.x), dpdy(i.uv.x)));
    let down = length(vec2<f32>(dpdx(i.uv.y), dpdy(i.uv.y)));
    let inside = min(i.uv, vec2<f32>(1.0) - i.uv) / max(vec2<f32>(across, down), vec2<f32>(1e-9));
    let covered = clamp(inside + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
    let cover = select(1.0, covered.x * covered.y, du.edge.z > 0.5);
    return shown(textureSample(composite, smp, i.uv)) * cover;
}

// a canvas colour (premultiplied) as the target shows it
fn shown(c: vec4<f32>) -> vec4<f32> {
    // see-through parts (a hidden background) show what is drawn behind the canvas, the surround
    if (c.a <= 0.0) {
        return vec4<f32>(0.0);
    }
    var rgb = c.rgb / c.a;
    // in perceptual greys, as a soft proof to a grey profile (sRGB's tone curve) shows it: each
    // colour as the grey of its luminance (Rec. 709 on linear light, as the colour panel's HSY′
    // luma is, so colours of one Y′ show as one grey), keeping its CIE lightness
    if (du.m1.w > 0.5) {
        let y = dot(vec3<f32>(0.2126, 0.7152, 0.0722), srgb_to_linear(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0))));
        rgb = linear_to_srgb(vec3<f32>(clamp(y, 0.0, 1.0)));
    }
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
    return vec4<f32>(rgb * c.a, c.a);
}

// ---- a veil: part of the canvas as it was, over it but for a hole ---------------------------
// The texture bound as `composite` is the veil's picture, and du maps its 0..1 to clip space.

struct VeilUniform {
    // the veil's rectangle on the canvas: left, top, width, height (canvas pixels)
    rect: vec4<f32>,
    // the colour round the canvas as the target takes it, for the picture's see-through parts;
    // then how many points the hole's outline has
    surround: vec4<f32>,
    // the hole's outline in canvas pixels, two points an entry
    points: array<vec4<f32>, 32>,
}

@group(3) @binding(0) var<uniform> vu: VeilUniform;

fn hole_point(k: u32) -> vec2<f32> {
    let e = vu.points[k / 2u];
    return select(e.xy, e.zw, (k & 1u) == 1u);
}

@fragment
fn fs_veil(i: Out) -> @location(0) vec4<f32> {
    let c = textureSample(composite, smp, i.uv);
    let p = vu.rect.xy + i.uv * vu.rect.zw;
    // the canvas pixels a target pixel spans: the hole's edge is smoothed over one target pixel
    let across = max(length(dpdx(p)), 1e-4);
    let n = min(u32(vu.surround.w), 64u);
    var inside = false;
    var near = 1e9;
    for (var k = 0u; k < n; k++) {
        let a = hole_point(k);
        let b = hole_point((k + 1u) % n);
        if ((a.y > p.y) != (b.y > p.y)) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if (p.x < x) {
                inside = !inside;
            }
        }
        let ab = b - a;
        let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-12), 0.0, 1.0);
        near = min(near, length(p - a - ab * t));
    }
    // how much of this pixel is outside the hole (all of it with no hole)
    let cover = select(clamp(select(near, -near, inside) / across + 0.5, 0.0, 1.0), 1.0, n < 3u);
    if (cover <= 0.0) {
        return vec4<f32>(0.0);
    }
    let s = shown(c);
    // where the picture is see-through, what was round the canvas showed through it
    let rgb = s.rgb + vu.surround.rgb * (1.0 - s.a);
    return vec4<f32>(rgb * cover, cover);
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
