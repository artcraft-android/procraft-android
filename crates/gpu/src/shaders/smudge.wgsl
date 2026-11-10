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
// how much of each canvas pixel is selected (all of it without a selection): paint is moved only
// as far as that, as a brush lays it
@group(1) @binding(4) var selected: texture_2d<f32>;

fn selected_at(p: vec2<i32>) -> f32 {
    return textureSampleLevel(selected, shape_smp, (vec2<f32>(p) + 0.5) / vec2<f32>(st.size), 0.0).r;
}

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
        // mixed as paint is: blue and yellow make green, not grey
        carry[k] = paint_mix(under, carry[k], st.pull);
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

// how far a stamp's quad reaches past the stamp each way (pixels)
const QUAD_REACH: f32 = 0.70710678;

// half the stamp across each way (pixels)
fn stamp_half() -> vec2<f32> {
    return vec2<f32>(st.radius, max(st.radius * st.roundness, 1e-3));
}

fn mask(t: vec2<f32>) -> f32 {
    if (st.shape < 0) {
        let d = length(t) * st.radius;
        let rim = clamp(st.radius - d + 0.5, 0.0, 1.0);
        let soft = 1.0 - smoothstep(min(st.hardness, 0.999) * st.radius, st.radius + 0.5, d);
        return min(rim, soft);
    }
    // the shape fills the stamp's quad, as a brush stamp's does
    let half = stamp_half();
    let uv = t * vec2<f32>(st.flip_x, st.flip_y) * half / (half + QUAD_REACH) * 0.5 + 0.5;
    if (st.shape_sharp > 0.5) {
        // unfiltered: the texel under it
        let size = vec2<i32>(textureDimensions(shapes));
        let at = clamp(vec2<i32>(floor(uv * vec2<f32>(size))), vec2<i32>(0), size - 1);
        return textureLoad(shapes, at, st.shape, 0).r;
    }
    // (a brush file's own image is bigger than the brush images, by as many levels)
    let side = f32(textureDimensions(shapes).x);
    let lod = clamp(log2(0.5 * side / (st.radius + QUAD_REACH)), 0.0, st.shape_lod_cap + log2(side / 256.0));
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
    // the stamp's quad
    let half = stamp_half();
    let reach = (half + QUAD_REACH) / half;
    if (abs(t.x) > reach.x || abs(t.y) > reach.y) {
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
    if (st.mode == 0u && st.modulated == 1u) {
        // what is carried goes on as strongly as the strongest stamp has been at the pixel, over
        // the pixel as the stroke found it: the newest carried wins (the drag), and the stamp's
        // shape and grain say where and how much, without stamp after stamp evening them out
        let held = amount[k];
        let l = st.stride * st.size.y + k;
        if (held <= 0.0) {
            amount[l] = bitcast<f32>(work[k]);
        }
        let first = unpack4x8unorm(bitcast<u32>(amount[l]));
        // crisp: the shape and grain cut where it drags, each part all the way or not at all,
        // and the stamp's strength (opacity, pressure) how much of them is let through; at full
        // strength nearly all of it (a part half dragged is the soft mush a blur is)
        let through = shape * clamp(2.0 * g, 0.0, 1.0);
        // never at nothing: what the shape or grain leaves out stays out at any strength
        let cut = 0.12;
        // and the strength how much each part goes over (with how far, in the pull, making the
        // whole follow the slider evenly)
        let strongest = max(held, smoothstep(cut - 0.06, cut + 0.06, through) * pow(clamp(st.alpha, 0.0, 1.0), 0.15));
        amount[k] = strongest;
        out = paint_mix(first, carried(p), strongest);
    }
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
    // outside the selection the layer stays as it is
    out = mix(w, out, selected_at(p));
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
    out = mix(was, out, selected_at(p));
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

// Pigment mixing (Kubelka-Munk over 38 spectral bands), after spectral.js 3.0.0
// (c) 2025 Ronald van Wijnen, MIT License: https://github.com/rvanwijnen/spectral.js
const KM_BANDS: u32 = 38u;
var<private> KM_W = array<f32, 38>(1.00116073, 1.00116065, 1.00116032, 1.00115867, 1.0011526, 1.00113253, 1.00108501, 1.00099688, 1.00086525, 1.00069629, 1.00050496, 1.00030808, 1.00011967, 0.999952766, 0.999821837, 0.99973861, 0.999709552, 0.99973193, 0.999799436, 0.99990033, 1.00002041, 1.00014479, 1.00025998, 1.0003558, 1.00042754, 1.00047623, 1.00050721, 1.00052519, 1.0005351, 1.00054022, 1.00054273, 1.0005439, 1.00054448, 1.00054477, 1.0005449, 1.00054496, 1.00054499, 1.000545);
var<private> KM_C = array<f32, 38>(0.970585001, 0.970592498, 0.970625349, 0.970786806, 0.971368673, 0.973163231, 0.976740223, 0.981587605, 0.986280266, 0.989949148, 0.992492702, 0.99414568, 0.995183975, 0.99575675, 0.995912818, 0.995606158, 0.994597601, 0.992215715, 0.986236453, 0.967943337, 0.891285004, 0.536202478, 0.154108119, 0.0574575093, 0.0315349873, 0.022263392, 0.0182022841, 0.016299056, 0.0153656239, 0.0149111569, 0.014695434, 0.0145964147, 0.0145470157, 0.0145228772, 0.0145120341, 0.0145066941, 0.0145044507, 0.0145038009);
var<private> KM_M = array<f32, 38>(0.990673557, 0.990671525, 0.990662582, 0.990618108, 0.990451481, 0.989871081, 0.988286609, 0.984290693, 0.973934906, 0.941817838, 0.817390326, 0.432472805, 0.138453978, 0.0537347217, 0.0292174997, 0.0213136518, 0.020134953, 0.0241323096, 0.0372236145, 0.0760506553, 0.205375472, 0.541268903, 0.815841685, 0.912817704, 0.94633983, 0.959927696, 0.966260595, 0.96932597, 0.970854537, 0.971605067, 0.97196277, 0.972127272, 0.972209418, 0.972249578, 0.972267622, 0.972276509, 0.972280243, 0.972281325);
var<private> KM_Y = array<f32, 38>(0.0210523372, 0.0210564628, 0.0210746179, 0.0211649058, 0.0215027957, 0.0226738799, 0.025823565, 0.0334879386, 0.0519069664, 0.100749015, 0.2391299, 0.534804312, 0.797807579, 0.911449894, 0.953797963, 0.971241615, 0.979303124, 0.98338012, 0.985461247, 0.986435047, 0.986738251, 0.986617882, 0.986277777, 0.985860592, 0.985474928, 0.985176935, 0.984971574, 0.984846303, 0.984775352, 0.984738067, 0.984719648, 0.984711023, 0.984706683, 0.984704554, 0.984703596, 0.984703124, 0.984702926, 0.984702868);
var<private> KM_R = array<f32, 38>(0.0315605738, 0.0315520718, 0.0315148216, 0.0313318045, 0.0306729858, 0.0286480477, 0.0246450407, 0.0192960754, 0.0142066612, 0.0102942609, 0.00761914605, 0.00589804108, 0.00482332478, 0.00422987484, 0.00405991713, 0.00435336956, 0.0053434426, 0.0076917201, 0.0135969796, 0.0316975443, 0.107861196, 0.463812603, 0.847055405, 0.943185409, 0.968862151, 0.978030667, 0.982043644, 0.983923624, 0.984845484, 0.985294276, 0.985507295, 0.985605072, 0.98565385, 0.985677685, 0.985688392, 0.985693665, 0.98569588, 0.985696521);
var<private> KM_G = array<f32, 38>(0.00955607476, 0.00955815801, 0.00956732454, 0.00961291263, 0.00978370904, 0.0103786227, 0.0120026452, 0.0160977721, 0.0267061902, 0.059555544, 0.186039827, 0.57057982, 0.861467768, 0.94587909, 0.970465486, 0.97841363, 0.979589031, 0.975533537, 0.962288755, 0.923121575, 0.793434019, 0.459270136, 0.185574104, 0.088177496, 0.0543630229, 0.0406288447, 0.0342215204, 0.0311185791, 0.0295708898, 0.0288108739, 0.0284486271, 0.0282820302, 0.0281988376, 0.0281581655, 0.028139891, 0.0281308902, 0.0281271087, 0.0281260134);
var<private> KM_B = array<f32, 38>(0.979404753, 0.979400707, 0.979382903, 0.979294365, 0.978963015, 0.977814467, 0.974724321, 0.967198482, 0.949079658, 0.900850129, 0.763150445, 0.465922172, 0.20126328, 0.0877524413, 0.0457176793, 0.0284706051, 0.0205271768, 0.0165302792, 0.0145135107, 0.0136003509, 0.0133604259, 0.0135488943, 0.0139594356, 0.0144434256, 0.0148854441, 0.0152254297, 0.0154592848, 0.0156018026, 0.0156824871, 0.0157248764, 0.0157458109, 0.0157556123, 0.0157605444, 0.0157629638, 0.0157640526, 0.0157645892, 0.0157648148, 0.0157648801);
var<private> KM_CMF_X = array<f32, 38>(6.4691999e-05, 0.0002194099, 0.00112057435, 0.00376661341, 0.0118805536, 0.0232864424, 0.0345594182, 0.0372237901, 0.0324183761, 0.0212332056, 0.0104909908, 0.00329583758, 0.000507035163, 0.000948674206, 0.0062737181, 0.0168646242, 0.028689649, 0.0426748125, 0.0562547481, 0.0694703973, 0.0830531517, 0.0861260963, 0.0904661377, 0.0850038651, 0.0709066691, 0.0506288916, 0.0354739619, 0.0214682103, 0.0125164568, 0.00680458164, 0.00346456579, 0.00149760975, 0.000769700481, 0.000407368058, 0.000169010403, 9.5224515e-05, 4.90309873e-05, 1.99961492e-05);
var<private> KM_CMF_Y = array<f32, 38>(1.84428944e-06, 6.20532359e-06, 3.10096047e-05, 0.000104748385, 0.00035364053, 0.000951471406, 0.00228226317, 0.00420732904, 0.00668879837, 0.00988839602, 0.0152494514, 0.0214183109, 0.0334229302, 0.0513100135, 0.0704020839, 0.0878387073, 0.0942490536, 0.0979566703, 0.0941521857, 0.0867810237, 0.0788565339, 0.0635267026, 0.0537414168, 0.0426460644, 0.0316173493, 0.0208852059, 0.0138601101, 0.0081026402, 0.00463010226, 0.00249138001, 0.00125930337, 0.000541646522, 0.000277952892, 0.000147108067, 6.10327473e-05, 3.4387323e-05, 1.7705986e-05, 7.22097491e-06);
var<private> KM_CMF_Z = array<f32, 38>(0.000305017148, 0.00103680667, 0.00531313633, 0.0179543926, 0.0570775815, 0.113651619, 0.173358726, 0.196206576, 0.186082371, 0.139950475, 0.0891745294, 0.0478962114, 0.0281456254, 0.0161376623, 0.00775910192, 0.00429614837, 0.00200550921, 0.00086147111, 0.000369038718, 0.000191428729, 0.000149555586, 9.23109285e-05, 6.81349182e-05, 2.88263656e-05, 1.57671821e-05, 3.9406041e-06, 1.58401259e-06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
const KM_XYZ_RGB = mat3x3<f32>(3.24096994, -0.969243636, 0.0556300797, -1.53738318, 1.8759675, -0.203976959, -0.49861076, 0.0415550574, 1.05697151);

fn km_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

fn km_encoded(c: vec3<f32>) -> vec3<f32> {
    let l = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(l, vec3<f32>(1.0 / 2.4)) - 0.055, l * 12.92, l <= vec3<f32>(0.0031308));
}

// A colour's reflectance at band i, from how much white, cyan, magenta, yellow, red, green and blue
// make it up (its linear RGB taken apart into them, as spectral.js does).
fn km_reflectance(l: vec3<f32>, i: u32) -> f32 {
    let w = min(l.r, min(l.g, l.b));
    let d = l - w;
    let c = min(d.g, d.b);
    let m = min(d.r, d.b);
    let y = min(d.r, d.g);
    let r = max(0.0, min(d.r - d.b, d.r - d.g));
    let g = max(0.0, min(d.g - d.b, d.g - d.r));
    let b = max(0.0, min(d.b - d.g, d.b - d.r));
    return max(1e-6, w * KM_W[i] + c * KM_C[i] + m * KM_M[i] + y * KM_Y[i] + r * KM_R[i] + g * KM_G[i] + b * KM_B[i]);
}

// Two straight sRGB colours mixed as paint, `t` of the second: by Kubelka-Munk, each weighed by
// its share and its luminance, so blue and yellow make green rather than grey.
fn km_mix(a: vec3<f32>, b: vec3<f32>, t: f32) -> vec3<f32> {
    let la = km_linear(a);
    let lb = km_linear(b);
    let lum = vec3<f32>(0.2126, 0.7152, 0.0722);
    // shares as they are (spectral.js squares them, which here would also lengthen the drag)
    let ca = (1.0 - t) * max(dot(la, lum), 1e-6);
    let cb = t * max(dot(lb, lum), 1e-6);
    var xyz = vec3<f32>(0.0);
    for (var i = 0u; i < KM_BANDS; i = i + 1u) {
        let ra = km_reflectance(la, i);
        let rb = km_reflectance(lb, i);
        let ks = (ca * (1.0 - ra) * (1.0 - ra) / (2.0 * ra) + cb * (1.0 - rb) * (1.0 - rb) / (2.0 * rb)) / (ca + cb);
        // 1 + ks - sqrt(ks² + 2ks), written without the subtraction of two nearly equal large
        // numbers, which in 32 bits leaves a few percent where nothing should be reflected and
        // greys saturated colours out
        let r = 1.0 / (1.0 + ks + sqrt(ks * ks + 2.0 * ks));
        xyz = xyz + vec3<f32>(KM_CMF_X[i], KM_CMF_Y[i], KM_CMF_Z[i]) * r;
    }
    return km_encoded(KM_XYZ_RGB * xyz);
}

// Premultiplied colours mixed as paint, `t` of the second: the alpha as it was, the colours by
// km_mix weighed by how much of each is there.
fn paint_mix(a: vec4<f32>, b: vec4<f32>, t: f32) -> vec4<f32> {
    let alpha = mix(a.a, b.a, t);
    let wa = (1.0 - t) * a.a;
    let wb = t * b.a;
    if (wa + wb <= 1e-4 || a.a <= 1e-4 || b.a <= 1e-4) {
        return mix(a, b, t);
    }
    let rgb = km_mix(a.rgb / a.a, b.rgb / b.a, wb / (wa + wb));
    return vec4<f32>(rgb * alpha, alpha);
}
