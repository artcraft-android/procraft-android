// Brush stamps into the stroke texture: one instanced quad per stamp, turned and squashed as the
// stamp says, with the round tip or a shape image, and a moving grain if the brush has one.
// Written premultiplied; the pipeline blends them "over" (light glaze) or keeps the maximum
// (uniform glaze).

// how far a stamp's quad reaches past the stamp each way (pixels): its shape fills all of it
const QUAD_REACH: f32 = 0.70710678;

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
    // the coarsest mip level the shape is read from: a small stamp's thin shape keeps its strength
    shape_lod_cap: f32,
    // pixels a side of the image a moving grain is read from
    grain_side: f32,
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
    // the tint: the hue's turn, the saturation's keep and add, the brightness's keep
    @location(7) tint: vec4<f32>,
    // a moving grain's depth and tile size as multiples of the brush's; the secondary colour's share
    @location(8) look: vec4<f32>,
    // how far the paint fills a moving grain in, and room for more
    @location(9) more: vec4<f32>,
    // the tint's brightness add
    @location(10) brightness_add: f32,
    // how far the grain's patch under the stamp turns, where it is read from staying put
    @location(11) grain_turn: f32,
}

struct StampOut {
    @builtin(position) pos: vec4<f32>,
    // position in the stamp's own frame, in radii (the shape spans -1..1)
    @location(0) q: vec2<f32>,
    @location(1) radius: f32,
    @location(2) alpha: f32,
    // where a moving grain is read from: the stamp's middle in it, plus the point's place from
    // there on the canvas, turned as far as the stamp says
    @location(3) grain_at: vec2<f32>,
    // how much of its quad the stamp itself is across each way, mirrored as the stamp says
    @location(4) fill: vec2<f32>,
    // the stamp's colour: the brush's tinted, mixed with the secondary
    @location(5) color: vec3<f32>,
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
    // a shape's quad keeps its reach however small the stamp; the round tip thins out by its area
    let r = max(inst.radius, select(0.25, 0.01, su.shape >= 0));
    let half = vec2<f32>(r, max(r * inst.roundness, 1e-3));
    // one pixel to spare for anti-aliasing
    let q = corners[vi % 6u] * (half + 1.0) / half;
    let local = q * half;
    let c = cos(inst.angle);
    let s = sin(inst.angle);
    let p = inst.center + vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
    var o: StampOut;
    o.pos = vec4<f32>(p.x / su.canvas_size.x * 2.0 - 1.0, 1.0 - p.y / su.canvas_size.y * 2.0, 0.0, 1.0);
    o.q = q;
    o.radius = r;
    o.alpha = inst.alpha;
    let gc = cos(inst.grain_turn);
    let gs = sin(inst.grain_turn);
    let from_middle = p - inst.center;
    o.grain_at = inst.grain_offset + vec2<f32>(from_middle.x * gc - from_middle.y * gs, from_middle.x * gs + from_middle.y * gc);
    o.fill = inst.flip * half / (half + QUAD_REACH);
    o.color = mix(tinted(su.color.rgb, inst.tint, inst.brightness_add), su.secondary.rgb, inst.look.z);
    o.look = inst.look;
    o.grain_press = inst.more.x;
    o.grain_bleed = inst.more.y;
    o.transfer = inst.more.z;
    return o;
}

// hue (0..1), saturation and brightness of a colour, and back
fn to_hsb(c: vec3<f32>) -> vec3<f32> {
    let hi = max(c.r, max(c.g, c.b));
    let lo = min(c.r, min(c.g, c.b));
    let d = hi - lo;
    if (d <= 1e-5) {
        return vec3<f32>(0.0, 0.0, hi);
    }
    var h = 0.0;
    if (hi == c.r) {
        h = (c.g - c.b) / d;
    } else if (hi == c.g) {
        h = (c.b - c.r) / d + 2.0;
    } else {
        h = (c.r - c.g) / d + 4.0;
    }
    return vec3<f32>(fract(h / 6.0), d / hi, hi);
}

fn from_hsb(c: vec3<f32>) -> vec3<f32> {
    let k = fract(vec3<f32>(c.x) + vec3<f32>(5.0, 3.0, 1.0) / 6.0) * 6.0;
    return c.z - c.z * c.y * clamp(min(k, 4.0 - k), vec3<f32>(0.0), vec3<f32>(1.0));
}

// `c` tinted: its hue turned, its saturation and brightness each kept by so much and added to
fn tinted(c: vec3<f32>, t: vec4<f32>, brightness_add: f32) -> vec3<f32> {
    if (all(t == vec4<f32>(0.0, 1.0, 0.0, 1.0)) && brightness_add == 0.0) {
        return c;
    }
    let hsb = to_hsb(c);
    let s = clamp(hsb.y * t.y + t.z, 0.0, 1.0);
    let b = clamp(hsb.z * t.w + brightness_add, 0.0, 1.0);
    return from_hsb(vec3<f32>(fract(hsb.x + t.x), s, b));
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
        // the shape fills the quad it is drawn on, which shows on a stamp a few pixels across
        let uv = i.q * i.fill * 0.5 + 0.5;
        // the mip level that matches the quad's size on the canvas
        // (a brush file's own image is bigger than the brush images, by as many levels)
        let side = f32(textureDimensions(shapes).x);
        let lod = clamp(log2(0.5 * side / (r + QUAD_REACH)), 0.0, su.shape_lod_cap + log2(side / 256.0));
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
        // a brush file's own image has the fine detail itself; ours only stands in for it
        let own = su.grain_side > 256.0;
        let g_lod = max(log2(su.grain_side / scale), 0.0);
        var raw = textureSampleLevel(grains, grain_smp, g_uv, su.grain, select(g_lod, 0.0, sharp && !own)).r;
        if (sharp) {
            if (own) {
                // unfiltered: the texel under it at its level
                let l = i32(round(g_lod));
                raw = textureLoad(grains, vec2<i32>(fract(g_uv) * vec2<f32>(textureDimensions(grains, l))), su.grain, l).r;
            } else {
                raw = sharp_grain(raw, g_uv);
            }
        }
        let g = clamp((raw - 0.5) * (1.0 + su.grain_contrast) + 0.5 + su.grain_brightness, 0.0, 1.0);
        // combined with the shape by the grain's blend mode, as deep as it says, inside the shape
        a = grain_combine(a, g, clamp(su.grain_depth * i.look.x, 0.0, 1.0), su.grain_blend);
    }
    // the pressure's bleed, a contrast on the shape and its grain: none of the faint parts, all
    // of the strong ones, and more of both the harder the pen presses
    if (i.grain_bleed > 0.0) {
        let b = 8.0 * i.grain_bleed;
        a = clamp((1.0 + b) * a - b / sqrt(max(b * i.grain_press, 1e-4)), 0.0, 1.0);
    }
    // a leaning pen shades it across its width, strongest at its leading edge
    if (i.look.w > 0.0) {
        a = a * (1.0 - i.look.w * clamp(0.5 - 0.5 * i.q.x, 0.0, 1.0));
    }
    // the round tip's rim takes a pixel as covered to its middle: a tip under a pixel across
    // covers only its area of one (a shape's quad reaches past it, which thins it as far)
    if (su.shape < 0) {
        let small = min(r * 2.0, 1.0);
        a = a * small * small;
    }
    // paint keeps inside the selection
    let inside_selection = textureSampleLevel(selected, shape_smp, i.pos.xy / su.canvas_size, 0.0).r;
    a = a * i.alpha * inside_selection;
    return vec4<f32>(i.color * a, a);
}
