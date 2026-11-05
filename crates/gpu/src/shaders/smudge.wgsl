// Smudge: the brush carries colour. A grid fixed to the canvas's axes, a pixel a cell, moving
// with the brush a whole pixel at a time, holds what it carries (so nothing is resampled as it
// goes); for each stamp, in order, it picks up some of what is under it (cs_carry) and lays what
// it carries back down under the stamp's shape (cs_deposit). The layer being smudged is a buffer
// of packed premultiplied RGBA8 pixels, read and written in place.

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
    // the cells carried over from the stamp before: those within this many of the middle (-1 at
    // the stroke's first stamp); the rest load what is under them, as a stroke's start does
    kept: i32,
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
    // the mix's grade (-1..1, folded into the mixtures) and the stamp's blur (0..1, see cs_blur_down)
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
    // 1: paint laid in place with a texturized grain, which cs_synthesize lays over the stroke once
    grain_once: f32,
    // where the grid's middle cell is on the canvas (a whole pixel when a cell is one), canvas
    // pixels a cell (1 unless the stroke's first stamp was wider than the grid), and this stamp's
    // cells: those within `half` of the middle
    anchor: vec2<f32>,
    scale: f32,
    half: i32,
    // wet paint (see cs_wet_plots): the stamps in the pass, their plots' texels a side, this
    // stamp's place in the pass (for the plots pass: which carried-over plot it reads), and the
    // texels a side of the plot the pass starts from
    count: u32,
    plot: u32,
    index: u32,
    carried: u32,
    // 1: wet paint builds up by what is left at each pixel (the modulated mode; see `amount`)
    modulated: u32,
    // how the moving grain combines with the shape (a blend mode), for paint laid in place
    grain_blend: u32,
    // a dual brush's paint laid in place: the dual mode + 1 (0: none, the paint laid alone) and
    // the second's threshold (negative: none), for cs_synthesize
    dual: u32,
    dual_threshold: f32,
    // 1: the stamp only adds to the paint laid so far (`amount`'s second half), which
    // cs_synthesize lays over the layer as it was; 2: wet paint, which also keeps what it lays in
    // `grained`, so that the brush goes on mixing with the stroke as it was laid
    laid_apart: u32,
    // the coarsest mip level the shape is read from
    shape_lod_cap: f32,
    // 1: wet paint mixes with the layer as it was before the stroke (a glaze), not as it goes
    source_before: u32,
    // the grain's levels, as the stamp shader takes them
    grain_brightness: f32,
    grain_contrast: f32,
    // how far paint laid in place pools, as cs_synthesize lays it (see the composite shader)
    wet_edges: f32,
    // wet paint with luminance blending: 1 when its colours mix squared, 2 when they do as it goes
    // over the layer too (paint that mixes only with the layer as it was)
    luminance: u32,
    // pixels a side of the image the grain is read from
    grain_side: f32,
    // how far paint laid apart burns the paint it goes over, and by what blend mode
    burnt: f32,
    burnt_blend: u32,
    // -1 where the stamp's shape is mirrored on that axis, else 1
    flip_x: f32,
    flip_y: f32,
}

// a pass of wet paint's stamps: middle, radius and turn; roundness, pull, mixture and final
// mixture
struct WetStamp {
    at: vec4<f32>,
    mixing: vec4<f32>,
    // what the stamp paints with, its colour dynamics in it
    paint: vec4<f32>,
}

struct WetPass {
    stamps: array<WetStamp, 64>,
}

@group(0) @binding(0) var<uniform> st: SmudgeStamp;
@group(0) @binding(1) var<storage, read_write> work: array<u32>;
@group(0) @binding(2) var<storage, read_write> carry: array<vec4<f32>>;
// wet paint laid apart: the stroke as it was laid, without its grain or its second
@group(0) @binding(3) var<storage, read_write> grained: array<u32>;
// wet paint in the modulated mode: how much each pixel has taken so far
@group(0) @binding(4) var<storage, read_write> amount: array<f32>;
@group(0) @binding(5) var<uniform> wet_pass: WetPass;
// a dual brush's paint laid in place: its second's coverage, and the layer as it was
@group(0) @binding(6) var second_tex: texture_2d<f32>;
@group(0) @binding(7) var before_tex: texture_2d<f32>;
@group(1) @binding(0) var shapes: texture_2d_array<f32>;
@group(1) @binding(1) var grains: texture_2d_array<f32>;
@group(1) @binding(2) var shape_smp: sampler;
@group(1) @binding(3) var grain_smp: sampler;

fn pixel(x: i32, y: i32) -> vec4<f32> {
    let cx = clamp(x, 0, i32(st.size.x) - 1);
    let cy = clamp(y, 0, i32(st.size.y) - 1);
    // a glaze picks up the layer as it was before the stroke
    if (st.source_before == 1u) {
        return textureLoad(before_tex, vec2<i32>(cx, cy), 0);
    }
    let k = u32(cy) * st.stride + u32(cx);
    // the brush picks up the stroke as it is without the grain laid over it, or its second
    if (st.laid_apart == 2u) {
        return unpack4x8unorm(grained[k]);
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

// the grid's middle cell
fn middle() -> i32 {
    return i32(st.grid) / 2;
}

// this stamp's cell `id` across its window, as a cell of the grid
fn window_cell(id: vec2<u32>) -> vec2<i32> {
    return vec2<i32>(id) + vec2<i32>(middle() - st.half);
}

@compute @workgroup_size(8, 8)
fn cs_carry(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = u32(2 * st.half + 1);
    if (id.x >= n || id.y >= n) {
        return;
    }
    let c = window_cell(id.xy);
    // the cell's middle on the canvas: a pixel's middle when a cell is one
    let under = layer_at(st.anchor + (vec2<f32>(c - vec2<i32>(middle())) + 0.5) * st.scale);
    let k = c.y * i32(st.grid) + c.x;
    let off = abs(c - vec2<i32>(middle()));
    if (max(off.x, off.y) > st.kept) {
        carry[k] = under;
    } else {
        carry[k] = mix(under, carry[k], st.pull);
    }
}

// The stamp's blur: what the brush lays down this stamp blurred (a Gaussian as wide in canvas
// pixels as the blur says: at 1, 32 pixels a side with a sigma of about 21, growing with the
// blur squared), kept to the stamp's own patch. What it carries stays as it was, so the blur
// never builds up from stamp to stamp. The carry buffer holds three grids: the carry, the blur
// down its columns, then across its rows, which the deposit reads.

// how far the blur reaches and its sigma, in cells `cell` canvas pixels wide
fn blur_reach(cell: f32) -> vec2<f32> {
    let a = clamp(st.blur, 0.0, 1.0);
    let reach = clamp(ceil(32.0 * a), 1.0, 32.0);
    let sigma = a / 3.0 * ceil(64.0 * a);
    return vec2<f32>(min(ceil(reach / max(cell, 1e-3)), f32(st.grid)), sigma / max(cell, 1e-3));
}

fn gauss(j: i32, sigma: f32) -> f32 {
    return exp(-0.5 * f32(j * j) / max(sigma * sigma, 1e-6));
}

@compute @workgroup_size(8, 8)
fn cs_blur_down(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = u32(2 * st.half + 1);
    if (id.x >= n || id.y >= n) {
        return;
    }
    let g = i32(st.grid);
    let c = window_cell(id.xy);
    let r = blur_reach(st.scale);
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    for (var j = -i32(r.x); j <= i32(r.x); j++) {
        let y = clamp(c.y + j, middle() - st.half, middle() + st.half);
        let w = gauss(j, r.y);
        sum += carry[y * g + c.x] * w;
        weight += w;
    }
    carry[g * g + c.y * g + c.x] = sum / weight;
}

@compute @workgroup_size(8, 8)
fn cs_blur_across(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = u32(2 * st.half + 1);
    if (id.x >= n || id.y >= n) {
        return;
    }
    let g = i32(st.grid);
    let c = window_cell(id.xy);
    let r = blur_reach(st.scale);
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    for (var j = -i32(r.x); j <= i32(r.x); j++) {
        let x = clamp(c.x + j, middle() - st.half, middle() + st.half);
        let w = gauss(j, r.y);
        sum += carry[g * g + c.y * g + x] * w;
        weight += w;
    }
    carry[2 * g * g + c.y * g + c.x] = sum / weight;
}

// what the brush lays down at canvas pixel `p`: what it carries there (blurred, when it blurs),
// the cell itself when a cell is a pixel
fn carried(p: vec2<i32>) -> vec4<f32> {
    let g = i32(st.grid);
    let base = select(0, 2 * g * g, st.blur > 0.0);
    let at = (vec2<f32>(p) + 0.5 - st.anchor) / st.scale + f32(middle()) - 0.5;
    let i = vec2<i32>(floor(at));
    let f = at - floor(at);
    let least = middle() - st.half;
    let most = middle() + st.half;
    let x0 = clamp(i.x, least, most);
    let x1 = clamp(i.x + 1, least, most);
    let y0 = clamp(i.y, least, most);
    let y1 = clamp(i.y + 1, least, most);
    let top = mix(carry[base + y0 * g + x0], carry[base + y0 * g + x1], f.x);
    let bottom = mix(carry[base + y1 * g + x0], carry[base + y1 * g + x1], f.x);
    return mix(top, bottom, f.y);
}

fn mask(t: vec2<f32>) -> f32 {
    if (st.shape < 0) {
        let d = length(t) * st.radius;
        let rim = clamp(st.radius - d + 0.5, 0.0, 1.0);
        let soft = 1.0 - smoothstep(min(st.hardness, 0.999) * st.radius, st.radius + 0.5, d);
        return min(rim, soft);
    }
    // the shape fills the stamp and half a pixel round it, as a brush stamp's does
    let uv = t * vec2<f32>(st.flip_x, st.flip_y) * (st.radius / (st.radius + 0.5)) * 0.5 + 0.5;
    if (st.shape_sharp > 0.5) {
        // unfiltered: the texel under it
        let size = vec2<i32>(textureDimensions(shapes));
        let at = clamp(vec2<i32>(floor(uv * vec2<f32>(size))), vec2<i32>(0), size - 1);
        return textureLoad(shapes, at, st.shape, 0).r;
    }
    let lod = clamp(log2(256.0 / max(2.0 * st.radius + 1.0, 1.0)), 0.0, st.shape_lod_cap);
    return textureSampleLevel(shapes, shape_smp, uv, st.shape, lod).r;
}

// how much of the stamp the grain lets through at canvas point `p`
fn grain_at(p: vec2<f32>) -> f32 {
    let g = grain_plain(p);
    if (st.grain_bleed > 0.0) {
        // pressed in as far as the pressure's bleed says
        return mix(1.0, mix(g, pressed_in(g, st.grain_press), min(st.grain_bleed, 1.0)), st.grain_depth);
    }
    return mix(1.0, g, st.grain_depth);
}

// the grain at canvas point `p` as it is, before its depth and bleed
fn grain_plain(p: vec2<f32>) -> f32 {
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
    // a brush file's own image has the fine detail itself; ours only stands in for it
    let own = st.grain_side > 256.0;
    let level = max(log2(st.grain_side / scale), 0.0);
    var g = textureSampleLevel(grains, grain_smp, uv, st.grain, select(level, 0.0, sharp && !own)).r;
    if (sharp) {
        if (own) {
            // unfiltered: the texel under it at its level
            let l = i32(round(level));
            g = textureLoad(grains, vec2<i32>(fract(uv) * vec2<f32>(textureDimensions(grains, l))), st.grain, l).r;
        } else {
            g = sharp_grain(g, uv);
        }
    }
    return clamp((g - 0.5) * (1.0 + st.grain_contrast) + 0.5 + st.grain_brightness, 0.0, 1.0);
}

// how much a wet stamp lays at canvas point `p` (stamp frame point `t`): its shape and its grain
// combined by the grain's blend mode (unless the grain is laid over the stroke once), then the
// bleed as a contrast on that (none of the faint parts, all of the strong ones, more of both the
// harder the pressure's bleed), times the stamp's alpha
fn wet_weight(p: vec2<f32>, t: vec2<f32>) -> f32 {
    var through = mask(t);
    if (st.grain >= 0 && st.grain_once <= 0.5) {
        through = grain_combine(through, grain_plain(p), st.grain_depth, st.grain_blend);
    }
    let b = 8.0 * st.grain_bleed;
    let bled = (1.0 + b) * through - b / sqrt(max(b * st.grain_press, 1e-4));
    return clamp(bled, 0.0, 1.0) * st.alpha;
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
    // the stamp and half a pixel round it
    let reach = (st.radius + 0.5) / st.radius;
    if (abs(t.x) > reach || abs(t.y) > reach) {
        return;
    }
    let shape = mask(t);
    if (shape * st.alpha <= 0.0) {
        return;
    }
    // the grain lets each stamp through as far as it stands: stamps over a pixel build up past
    // it towards full, the grain showing while they are few or light
    let g = grain_at(vec2<f32>(p) + 0.5);
    let m = shape * st.alpha;
    let k = u32(p.y) * st.stride + u32(p.x);
    let w = select(unpack4x8unorm(work[k]), unpack4x8unorm(grained[k]), st.laid_apart == 2u);
    // smudging: the grain lets less of what is carried through
    var out = mix(w, carried(p), m * g);
    if (st.mode == 1u) {
        // wet: the stamp's plot mixed in, alpha and all (a stamp can lay bare paper), as far as
        // its weight there
        // the stamp's plot, or (dry paint laid in place, no plots) the paint itself
        var laid = vec4<f32>(st.paint.rgb, 1.0);
        if (st.count > 0u) {
            laid = plot_at(2u * CARRIED + st.index * st.plot * st.plot, st.plot, t);
        }
        var weight = wet_weight(vec2<f32>(p) + 0.5, t);
        if (st.modulated == 1u) {
            // only as far as the pixel has room left, which fills as it takes paint
            let held = amount[k];
            weight = weight * (1.0 - held);
            amount[k] = held + weight * (1.0 - held);
        }
        if (st.laid_apart != 0u) {
            // how much of the paint shows there so far, put together with the rest later
            let l = st.stride * st.size.y + k;
            amount[l] = amount[l] + weight * (1.0 - amount[l]);
        }
        out = from_mix(mix(to_mix(w), to_mix(laid), weight));
    }
    if (st.preserve_alpha == 1u) {
        out = out * (w.a / max(out.a, 1e-6));
    }
    // rounded to 8 bits up or down at random, in proportion to the remainder, so that faint stamps
    // add up as they should instead of rounding away
    let h = dither(u32(p.x), u32(p.y), bitcast<u32>(st.center.x) ^ (bitcast<u32>(st.center.y) * 2654435761u));
    if (st.laid_apart == 2u) {
        // wet paint kept as it was laid; shown by cs_synthesize
        grained[k] = pack4x8unorm(clamp(out + (h - 0.5) / 255.0, vec4<f32>(0.0), vec4<f32>(1.0)));
        return;
    }
    work[k] = pack4x8unorm(clamp(out + (h - 0.5) / 255.0, vec4<f32>(0.0), vec4<f32>(1.0)));
}

// Paint laid in place with a texturized grain or a second, put together where its stamps or its
// second's went: the paint over the layer as it was, as far as how much of it was laid combines
// with the grain by its mode, then the second's coverage (cut at its threshold) by the dual mode. Wet
// paint keeps what it laid: its change to the layer, scaled from how much was laid to that. Only
// this far: more, and the change's own rounding shows as speckle.
const LAID_MOST_SCALE: f32 = 4.0;

@compute @workgroup_size(8, 8)
fn cs_synthesize(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = st.bbox.xy + vec2<i32>(id.xy);
    if (p.x >= st.bbox.z || p.y >= st.bbox.w || p.x < 0 || p.y < 0 || p.x >= i32(st.size.x) || p.y >= i32(st.size.y)) {
        return;
    }
    let k = u32(p.y) * st.stride + u32(p.x);
    let laid = amount[st.stride * st.size.y + k];
    var a = laid;
    if (st.wet_edges > 0.0 && laid > 0.0) {
        // the paint pools, settling back to about 0.78 where it is full
        let x = 4.5 * laid;
        a = mix(laid, 1.0 - abs(sin(x) / x), st.wet_edges);
    }
    if (st.grain_once > 0.5) {
        a = grain_combine(a, grain_plain(vec2<f32>(p) + 0.5), st.grain_depth, st.grain_blend);
    }
    if (st.dual != 0u) {
        var b = textureLoad(second_tex, p, 0).a;
        if (st.dual_threshold >= 0.0) {
            b = select(0.0, 1.0, b > st.dual_threshold);
        }
        a = dual_rise(dual_combine(a, b, st.dual - 1u), laid, st.dual - 1u);
    }
    let was = textureLoad(before_tex, p, 0);
    var out = mix(was, vec4<f32>(st.paint.rgb, 1.0), a);
    if (st.laid_apart == 2u && laid > 0.0) {
        // what a second adds past that is laid as plain paint, as it is where none was laid at all
        let scale = min(a / laid, LAID_MOST_SCALE);
        var kept = over_layer(was, unpack4x8unorm(grained[k]), scale);
        if (st.burnt > 0.0) {
            // burnt edges show where the stroke is thin: where its grain or its second took paint
            // away, or without either wherever it is less than full. The paint itself, blended
            // once more with what was under it (nothing shows where the layer was empty)
            let full = select(1.0, laid, st.grain_once > 0.5 || st.dual != 0u);
            let own = clamp(over_layer(was, unpack4x8unorm(grained[k]), 1.0 / laid), vec4<f32>(0.0), vec4<f32>(1.0));
            let burnt = vec4<f32>(unpremultiply(blend(was, own, st.burnt_blend)) * own.a, own.a);
            kept = over_layer(was, mix(own, burnt, clamp(st.burnt * abs(full - a), 0.0, 1.0)), laid * scale);
        }
        out = mix(kept, vec4<f32>(st.paint.rgb, 1.0), max(a - laid * scale, 0.0));
    }
    if (st.preserve_alpha == 1u) {
        out = out * (was.a / max(out.a, 1e-6));
    }
    // dithered the same each time it is put together again
    let h = dither(u32(p.x), u32(p.y), 0u);
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

// Wet paint, a display frame's stamps at a time (`st.count` of them, listed in `wet_pass`). Each
// stamp has a plot, a square of `st.plot` texels over its own turned quad (and a texel more round
// it), holding what it lays down. A thread a texel walks the pass's stamps in order with what the
// brush carries there: it picks up what is under that stamp (the layer as the pass found it) as
// far as the pull leaves room, takes in fresh paint by the mixture, and the stamp's plot is that
// laid over what was under the pass's first stamp by the final mixture. The last stamp's plot is
// what the next pass starts from; the stroke's first starts from what is under it. The carry
// buffer holds two carried-over plots (read one, write the other) and then the pass's plots.

// room for a carried-over plot: the largest plot's texels (128 a side)
const CARRIED: u32 = 16384u;

// premultiplied paint as it mixes: with luminance blending its colour squared, its alpha as it is
fn to_mix(c: vec4<f32>) -> vec4<f32> {
    if (st.luminance == 0u || c.a <= 0.0) {
        return c;
    }
    let plain = c.rgb / c.a;
    return vec4<f32>(plain * plain * c.a, c.a);
}

// paint that has mixed, back as the layer keeps it
fn from_mix(c: vec4<f32>) -> vec4<f32> {
    if (st.luminance == 0u || c.a <= 0.0) {
        return c;
    }
    return vec4<f32>(sqrt(max(c.rgb, vec3<f32>(0.0)) / c.a) * c.a, c.a);
}

// what paint laid apart shows over the layer `was`: `t` of the way to `laid`
fn over_layer(was: vec4<f32>, laid: vec4<f32>, t: f32) -> vec4<f32> {
    if (st.luminance == 2u) {
        return from_mix(mix(to_mix(was), to_mix(laid), t));
    }
    return was + (laid - was) * t;
}

// the canvas point under stamp `w` at its point `t` (-1..1 across each way)
fn plot_point(w: WetStamp, t: vec2<f32>) -> vec2<f32> {
    let l = vec2<f32>(t.x * w.at.z, t.y * w.at.z * w.mixing.x);
    let c = cos(w.at.w);
    let s = sin(w.at.w);
    return w.at.xy + vec2<f32>(l.x * c - l.y * s, l.x * s + l.y * c);
}

// the stamp point a plot's texel `i` stands for, in a plot `n` texels a side
fn plot_t(i: vec2<f32>, n: u32) -> vec2<f32> {
    return ((i + 0.5) / f32(n) * 2.0 - 1.0) * (1.0 + 1.0 / f32(n));
}

// what the plot at `base` (`n` texels a side) holds at stamp point `t`, bilinear
fn plot_at(base: u32, n: u32, t: vec2<f32>) -> vec4<f32> {
    let at = (t / (1.0 + 1.0 / f32(n)) * 0.5 + 0.5) * f32(n) - 0.5;
    let i = vec2<i32>(floor(at));
    let f = at - floor(at);
    let last = i32(n) - 1;
    let x0 = u32(clamp(i.x, 0, last));
    let x1 = u32(clamp(i.x + 1, 0, last));
    let y0 = u32(clamp(i.y, 0, last));
    let y1 = u32(clamp(i.y + 1, 0, last));
    let top = mix(carry[base + y0 * n + x0], carry[base + y0 * n + x1], f.x);
    let bottom = mix(carry[base + y1 * n + x0], carry[base + y1 * n + x1], f.x);
    return mix(top, bottom, f.y);
}

@compute @workgroup_size(8, 8)
fn cs_wet_plots(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = st.plot;
    if (id.x >= n || id.y >= n) {
        return;
    }
    let t = plot_t(vec2<f32>(id.xy), n);
    // (what the brush carries stays as it mixes from pass to pass; only the plots go back)
    let under_first = to_mix(layer_at(plot_point(wet_pass.stamps[0], t)));
    var c = under_first;
    if (st.kept >= 0) {
        c = plot_at(st.index * CARRIED, st.carried, t);
    }
    var out = c;
    for (var k = 0u; k < st.count; k++) {
        let w = wet_pass.stamps[k];
        c = mix(to_mix(layer_at(plot_point(w, t))), c, w.mixing.y);
        c = mix(c, to_mix(w.paint), w.mixing.z);
        out = mix(under_first, c, w.mixing.w);
        carry[2u * CARRIED + k * n * n + id.y * n + id.x] = from_mix(out);
    }
    carry[(1u - st.index) * CARRIED + id.y * n + id.x] = out;
}
