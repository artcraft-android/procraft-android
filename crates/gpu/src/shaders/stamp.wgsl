// Brush stamps into the stroke texture: one instanced quad per stamp, turned and squashed as the
// stamp says, with the round tip or a shape image, and a moving grain if the brush has one.
// Written premultiplied; the pipeline blends them "over" (light glaze) or keeps the maximum
// (uniform glaze).

struct StampUniform {
    canvas_size: vec2<f32>,
    hardness: f32,
    // -1: the round tip; otherwise a layer of `shapes`
    shape: i32,
    // straight sRGB colour
    color: vec4<f32>,
    // -1: no moving grain; otherwise a layer of `grains`
    grain: i32,
    grain_scale: f32,
    grain_depth: f32,
    grain_brightness: f32,
    grain_contrast: f32,
    // the grain turned, radians, and how it combines with the shape (a blend mode's number)
    grain_rotation: f32,
    grain_blend: u32,
    // 1: the grain read unfiltered (sharp)
    grain_sharp: u32,
    // the secondary colour (straight sRGB), mixed in as far as each stamp says
    secondary: vec4<f32>,
    // 1: the shape read unfiltered (sharp)
    shape_sharp: u32,
}

@group(0) @binding(0) var<uniform> su: StampUniform;
@group(1) @binding(0) var shapes: texture_2d_array<f32>;
@group(1) @binding(1) var grains: texture_2d_array<f32>;
@group(1) @binding(2) var shape_smp: sampler;
@group(1) @binding(3) var grain_smp: sampler;
// how much of each canvas pixel is selected (all of it without a selection)
@group(1) @binding(4) var selected: texture_2d<f32>;

struct StampIn {
    @location(0) center: vec2<f32>,
    @location(1) radius: f32,
    @location(2) alpha: f32,
    @location(3) angle: f32,
    @location(4) roundness: f32,
    @location(5) grain_offset: vec2<f32>,
    @location(6) flip: vec2<f32>,
    @location(7) shift: vec4<f32>,
    // a moving grain's depth and tile size as multiples of the brush's; the secondary colour's share
    @location(8) look: vec4<f32>,
    // how far the paint fills a moving grain in, and room for more
    @location(9) more: vec4<f32>,
}

struct StampOut {
    @builtin(position) pos: vec4<f32>,
    // position in the stamp's own frame, in radii (the shape spans -1..1)
    @location(0) q: vec2<f32>,
    @location(1) radius: f32,
    @location(2) alpha: f32,
    // where a moving grain is read from: the stamp's middle in it, plus the point's place from
    // there on the canvas (the grain never turns or squashes with the stamp)
    @location(3) grain_at: vec2<f32>,
    @location(4) flip: vec2<f32>,
    @location(5) shift: vec3<f32>,
    // how hard the paint is pressed into a moving grain, and how much that counts
    @location(8) grain_press: f32,
    @location(10) grain_bleed: f32,
    @location(6) look: vec4<f32>,
    // what light pressure lets through: the most a capped stroke builds up to
    @location(7) transfer: f32,
}

@vertex
fn vs_stamp(@builtin(vertex_index) vi: u32, inst: StampIn) -> StampOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let r = max(inst.radius, 0.25);
    // one pixel to spare for anti-aliasing
    let grow = (r + 1.0) / r;
    let q = corners[vi % 6u] * grow;
    let local = vec2<f32>(q.x * r, q.y * r * inst.roundness);
    let c = cos(inst.angle);
    let s = sin(inst.angle);
    let p = inst.center + vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
    var o: StampOut;
    o.pos = vec4<f32>(p.x / su.canvas_size.x * 2.0 - 1.0, 1.0 - p.y / su.canvas_size.y * 2.0, 0.0, 1.0);
    o.q = q;
    o.radius = r;
    o.alpha = inst.alpha;
    o.grain_at = inst.grain_offset + (p - inst.center);
    o.flip = inst.flip;
    o.shift = inst.shift.xyz;
    o.look = inst.look;
    o.grain_press = inst.more.x;
    o.grain_bleed = inst.more.y;
    o.transfer = inst.more.z;
    return o;
}

// hue (0..1), saturation and lightness of a colour, and back
fn to_hsl(c: vec3<f32>) -> vec3<f32> {
    let hi = max(c.r, max(c.g, c.b));
    let lo = min(c.r, min(c.g, c.b));
    let l = (hi + lo) * 0.5;
    let d = hi - lo;
    if (d <= 1e-5) {
        return vec3<f32>(0.0, 0.0, l);
    }
    let s = d / (1.0 - abs(2.0 * l - 1.0) + 1e-5);
    var h = 0.0;
    if (hi == c.r) {
        h = (c.g - c.b) / d;
    } else if (hi == c.g) {
        h = (c.b - c.r) / d + 2.0;
    } else {
        h = (c.r - c.g) / d + 4.0;
    }
    return vec3<f32>(fract(h / 6.0), clamp(s, 0.0, 1.0), l);
}

fn from_hsl(c: vec3<f32>) -> vec3<f32> {
    let k = vec3<f32>(0.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3<f32>(c.x) + k) * 6.0 - 3.0);
    let rgb = clamp(p - 1.0, vec3<f32>(0.0), vec3<f32>(1.0));
    let chroma = (1.0 - abs(2.0 * c.z - 1.0)) * c.y;
    return c.z + chroma * (rgb - 0.5);
}

// the shape image's texel under `uv`, unfiltered
fn shape_texel(uv: vec2<f32>, layer: i32) -> f32 {
    let size = vec2<i32>(textureDimensions(shapes));
    let at = clamp(vec2<i32>(floor(uv * vec2<f32>(size))), vec2<i32>(0), size - 1);
    return textureLoad(shapes, at, layer, 0).r;
}

@fragment
fn fs_stamp(i: StampOut) -> @location(0) vec4<f32> {
    return stamp_paint(i);
}

// a stamp's premultiplied paint at a pixel
fn stamp_paint(i: StampOut) -> vec4<f32> {
    let r = i.radius;
    var a = 0.0;
    if (su.shape < 0) {
        let d = length(i.q) * r;
        // a one-pixel anti-aliased rim, and the brush's own falloff from hardness·r outwards
        let rim = clamp(r - d + 0.5, 0.0, 1.0);
        let soft = 1.0 - smoothstep(min(su.hardness, 0.999) * r, r + 0.5, d);
        a = min(rim, soft);
    } else {
        let uv = i.q * i.flip * 0.5 + 0.5;
        // the mip level that matches the stamp's size on the canvas
        let lod = max(log2(256.0 / max(2.0 * r, 1.0)), 0.0);
        let inside = all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0));
        var s = textureSampleLevel(shapes, shape_smp, uv, su.shape, lod).r;
        if (su.shape_sharp == 1u) {
            s = shape_texel(uv, su.shape);
        }
        a = select(0.0, s, inside);
    }
    if (su.grain >= 0) {
        let scale = su.grain_scale * i.look.y;
        let gc = cos(su.grain_rotation);
        let gs = sin(su.grain_rotation);
        let at = i.grain_at;
        let g_uv = vec2<f32>(at.x * gc - at.y * gs, at.x * gs + at.y * gc) / scale;
        let sharp = su.grain_sharp == 1u;
        let g_lod = select(max(log2(256.0 / scale), 0.0), 0.0, sharp);
        var raw = textureSampleLevel(grains, grain_smp, g_uv, su.grain, g_lod).r;
        if (sharp) {
            raw = sharp_grain(raw, g_uv);
        }
        var g = clamp((raw - 0.5) * (1.0 + su.grain_contrast) + 0.5 + su.grain_brightness, 0.0, 1.0);
        if (i.grain_bleed > 0.0) {
            // pressed in as far as the pressure's bleed says
            g = mix(g, pressed_in(g, i.grain_press), i.grain_bleed);
        }
        // combined with the shape by the grain's blend mode, as deep as it says, inside the shape
        a = grain_combine(a, g, clamp(su.grain_depth * i.look.x, 0.0, 1.0), su.grain_blend);
    }
    // a leaning pen shades it across its width, strongest at its leading edge
    if (i.look.w > 0.0) {
        a = a * (1.0 - i.look.w * clamp(0.5 - 0.5 * i.q.x, 0.0, 1.0));
    }
    // a stamp smaller than a pixel covers only part of it
    let small = min(r * 2.0, 1.0);
    // paint keeps inside the selection
    let inside_selection = textureSampleLevel(selected, shape_smp, i.pos.xy / su.canvas_size, 0.0).r;
    a = a * small * small * i.alpha * inside_selection;
    var color = mix(su.color.rgb, su.secondary.rgb, i.look.z);
    if (any(i.shift != vec3<f32>(0.0))) {
        let hsl = to_hsl(color);
        color = from_hsl(vec3<f32>(fract(hsl.x + i.shift.x + 1.0), clamp(hsl.y + i.shift.y, 0.0, 1.0), clamp(hsl.z + i.shift.z, 0.0, 1.0)));
    }
    return vec4<f32>(color * a, a);
}
