// Smudge: the brush carries colour. A small grid in the stamp's own frame holds what it carries;
// for each stamp, in order, it picks up some of what is under it (cs_carry) and lays what it
// carries back down under the stamp's shape (cs_deposit). The layer being smudged is a buffer of
// packed premultiplied RGBA8 pixels, read and written in place.

struct SmudgeStamp {
    center: vec2<f32>,
    radius: f32,
    angle: f32,
    roundness: f32,
    // the stamp's alpha times the tool's strength
    alpha: f32,
    // how much of what it carries the brush keeps at each stamp (the rest is picked up)
    pull: f32,
    hardness: f32,
    shape: i32,
    // 1 for the stroke's first stamp: the brush starts loaded with what is under it
    first: u32,
    preserve_alpha: u32,
    // u32 words per buffer row
    stride: u32,
    // the pixels this stamp can touch: x0, y0, x1, y1
    bbox: vec4<i32>,
    size: vec2<u32>,
    grid: u32,
    // 1: stamps over a pixel don't build up past the strongest of them (the uniform glaze)
    uniform_glaze: u32,
    // wet mix: the paint (straight colour), how much of it the brush mixes into what it carries,
    // and how much of what it carries it lays down
    paint: vec4<f32>,
    // 0 smudges; 1 paints wet
    mode: u32,
    mixture: f32,
    final_mixture: f32,
    // the mix's grade and blur (0..1), folded into the mixtures for now
    grade: f32,
    blur: f32,
    // the brush's grain: a layer of `grains` (-1: none), the size of a tile in pixels (negative
    // for a grain moving with the stamp, else fixed to the canvas) and how strongly it shows
    grain: i32,
    grain_scale: f32,
    grain_depth: f32,
    // where a moving grain is read from at the stamp's middle (see the stamp shader), and the
    // grain turned (radians)
    grain_offset: vec2<f32>,
    grain_rotation: f32,
    // 1: the grain read unfiltered (sharp)
    grain_sharp: f32,
    // 1: the shape read unfiltered (sharp)
    shape_sharp: f32,
    // how hard the paint is pressed into a moving grain, and how much that counts
    grain_press: f32,
    grain_bleed: f32,
    // 1: wet paint with a texturized grain, laid over the stroke once: `raw` holds the stroke
    // without it and `orig` the layer as it was
    grain_once: f32,
}

@group(0) @binding(0) var<uniform> st: SmudgeStamp;
@group(0) @binding(1) var<storage, read_write> work: array<u32>;
@group(0) @binding(2) var<storage, read_write> carry: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> raw: array<u32>;
@group(0) @binding(4) var<storage, read> orig: array<u32>;
@group(1) @binding(0) var shapes: texture_2d_array<f32>;
@group(1) @binding(1) var grains: texture_2d_array<f32>;
@group(1) @binding(2) var shape_smp: sampler;
@group(1) @binding(3) var grain_smp: sampler;

fn pixel(x: i32, y: i32) -> vec4<f32> {
    let cx = clamp(x, 0, i32(st.size.x) - 1);
    let cy = clamp(y, 0, i32(st.size.y) - 1);
    let k = u32(cy) * st.stride + u32(cx);
    // the brush picks up the stroke as it is without the grain laid over it
    if (st.grain_once > 0.5) {
        return unpack4x8unorm(raw[k]);
    }
    return unpack4x8unorm(work[k]);
}

// the layer at a canvas position (pixel centres at +0.5), bilinear
fn layer_at(q: vec2<f32>) -> vec4<f32> {
    let p = q - 0.5;
    let i = vec2<i32>(floor(p));
    let f = p - floor(p);
    let top = mix(pixel(i.x, i.y), pixel(i.x + 1, i.y), f.x);
    let bottom = mix(pixel(i.x, i.y + 1), pixel(i.x + 1, i.y + 1), f.x);
    return mix(top, bottom, f.y);
}

// stamp frame (-1..1, before roundness and rotation) to canvas
fn to_canvas(t: vec2<f32>) -> vec2<f32> {
    let local = vec2<f32>(t.x * st.radius, t.y * st.radius * st.roundness);
    let c = cos(st.angle);
    let s = sin(st.angle);
    return st.center + vec2<f32>(local.x * c - local.y * s, local.x * s + local.y * c);
}

@compute @workgroup_size(8, 8)
fn cs_carry(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= st.grid || id.y >= st.grid) {
        return;
    }
    let t = (vec2<f32>(id.xy) + 0.5) / f32(st.grid) * 2.0 - 1.0;
    let under = layer_at(to_canvas(t));
    let k = id.y * st.grid + id.x;
    if (st.first == 1u) {
        carry[k] = under;
    } else {
        carry[k] = mix(under, carry[k], st.pull);
    }
    if (st.mode == 1u) {
        // wet: the brush's paint mixed into what it carries
        carry[k] = mix(carry[k], vec4<f32>(st.paint.rgb, 1.0), st.mixture);
    }
}

fn carried(t: vec2<f32>) -> vec4<f32> {
    let g = i32(st.grid);
    let p = (t * 0.5 + 0.5) * f32(st.grid) - 0.5;
    let i = vec2<i32>(floor(p));
    let f = p - floor(p);
    let x0 = clamp(i.x, 0, g - 1);
    let x1 = clamp(i.x + 1, 0, g - 1);
    let y0 = clamp(i.y, 0, g - 1);
    let y1 = clamp(i.y + 1, 0, g - 1);
    let top = mix(carry[y0 * g + x0], carry[y0 * g + x1], f.x);
    let bottom = mix(carry[y1 * g + x0], carry[y1 * g + x1], f.x);
    return mix(top, bottom, f.y);
}

fn mask(t: vec2<f32>) -> f32 {
    if (st.shape < 0) {
        let d = length(t) * st.radius;
        let rim = clamp(st.radius - d + 0.5, 0.0, 1.0);
        let soft = 1.0 - smoothstep(min(st.hardness, 0.999) * st.radius, st.radius + 0.5, d);
        return min(rim, soft);
    }
    let uv = t * 0.5 + 0.5;
    if (st.shape_sharp > 0.5) {
        // unfiltered: the texel under it
        let size = vec2<i32>(textureDimensions(shapes));
        let at = clamp(vec2<i32>(floor(uv * vec2<f32>(size))), vec2<i32>(0), size - 1);
        return textureLoad(shapes, at, st.shape, 0).r;
    }
    let lod = max(log2(256.0 / max(2.0 * st.radius, 1.0)), 0.0);
    return textureSampleLevel(shapes, shape_smp, uv, st.shape, lod).r;
}

// how much of the stamp the grain lets through at canvas point `p` (stamp frame point `t`)
fn grain_at(p: vec2<f32>) -> f32 {
    if (st.grain < 0) {
        return 1.0;
    }
    let scale = abs(st.grain_scale);
    var at = p;
    if (st.grain_scale < 0.0) {
        // a moving grain: read as far from where the stamp's middle is in it as the pixel is from
        // the middle on the canvas, so a rolling grain stays put as the stamps move over it
        at = p - st.center + st.grain_offset;
    }
    let gc = cos(st.grain_rotation);
    let gs = sin(st.grain_rotation);
    let uv = vec2<f32>(at.x * gc - at.y * gs, at.x * gs + at.y * gc) / scale;
    let sharp = st.grain_sharp > 0.5;
    var g = textureSampleLevel(grains, grain_smp, uv, st.grain, select(max(log2(256.0 / scale), 0.0), 0.0, sharp)).r;
    if (sharp) {
        g = sharp_grain(g, uv);
    }
    if (st.grain_bleed > 0.0) {
        // pressed in as far as the pressure's bleed says
        g = mix(g, pressed_in(g, st.grain_press), st.grain_bleed);
    }
    return mix(1.0, g, st.grain_depth);
}

@compute @workgroup_size(8, 8)
fn cs_deposit(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = st.bbox.xy + vec2<i32>(id.xy);
    if (p.x >= st.bbox.z || p.y >= st.bbox.w || p.x < 0 || p.y < 0 || p.x >= i32(st.size.x) || p.y >= i32(st.size.y)) {
        return;
    }
    // into the stamp's frame
    let d = vec2<f32>(p) + 0.5 - st.center;
    let c = cos(st.angle);
    let s = sin(st.angle);
    let local = vec2<f32>(d.x * c + d.y * s, -d.x * s + d.y * c);
    let t = vec2<f32>(local.x / st.radius, local.y / (st.radius * st.roundness));
    if (abs(t.x) > 1.0 || abs(t.y) > 1.0) {
        return;
    }
    let shape = mask(t);
    if (shape * st.alpha <= 0.0) {
        return;
    }
    // the grain lets each stamp through as far as it stands: stamps over a pixel build up past
    // it towards full, the grain showing while they are few or light (unless it is laid over the
    // stroke once)
    let once = st.grain_once > 0.5;
    let g = select(grain_at(vec2<f32>(p) + 0.5), 1.0, once);
    let m = shape * st.alpha;
    let k = u32(p.y) * st.stride + u32(p.x);
    let w = select(unpack4x8unorm(work[k]), unpack4x8unorm(raw[k]), once);
    // smudging: the grain lets less of what is carried through
    var out = mix(w, carried(t), m * g);
    if (st.mode == 1u) {
        // wet: what it carries (its paint and what it picked up, alpha and all), as much as the
        // final mixture lays down, over what is there, as far as the stamp covers
        let laid = carried(t) * st.final_mixture;
        out = mix(w, laid + w * (1.0 - laid.a), m * g);
    }
    if (st.preserve_alpha == 1u) {
        out = out * (w.a / max(out.a, 1e-6));
    }
    // rounded to 8 bits up or down at random, in proportion to the remainder, so that faint stamps
    // add up as they should instead of rounding away
    let h = dither(u32(p.x), u32(p.y), bitcast<u32>(st.center.x) ^ (bitcast<u32>(st.center.y) * 2654435761u));
    if (once) {
        // the stroke kept without the grain; shown with the grain over its change to the layer
        raw[k] = pack4x8unorm(clamp(out + (h - 0.5) / 255.0, vec4<f32>(0.0), vec4<f32>(1.0)));
        let was = unpack4x8unorm(orig[k]);
        let shown = was + (out - was) * grain_at(vec2<f32>(p) + 0.5);
        work[k] = pack4x8unorm(clamp(shown + (h - 0.5) / 255.0, vec4<f32>(0.0), vec4<f32>(1.0)));
        return;
    }
    work[k] = pack4x8unorm(clamp(out + (h - 0.5) / 255.0, vec4<f32>(0.0), vec4<f32>(1.0)));
}

// a number in 0..1 that looks random, from three
fn dither(a: u32, b: u32, c: u32) -> f32 {
    var h = a * 747796405u + b * 2891336453u + c * 277803737u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    h = h ^ (h >> 16u);
    return f32(h >> 8u) / 16777216.0;
}
