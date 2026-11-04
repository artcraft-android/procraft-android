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
    // 1: wet paint with a texturized grain, laid over the stroke once: `raw` holds the stroke
    // without it and `orig` the layer as it was
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
    // cs_synthesize lays over the layer as it was
    laid_apart: u32,
    spare1: u32,
    spare2: u32,
    spare3: u32,
}

// a pass of wet paint's stamps: middle, radius and turn; roundness, pull, mixture and final
// mixture
struct WetStamp {
    at: vec4<f32>,
    mixing: vec4<f32>,
}

struct WetPass {
    stamps: array<WetStamp, 64>,
}

@group(0) @binding(0) var<uniform> st: SmudgeStamp;
@group(0) @binding(1) var<storage, read_write> work: array<u32>;
@group(0) @binding(2) var<storage, read_write> carry: array<vec4<f32>>;
// wet paint with a texturized grain: the stroke without it, then the layer as it was
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
    let k = u32(cy) * st.stride + u32(cx);
    // the brush picks up the stroke as it is without the grain laid over it
    if (st.grain_once > 0.5) {
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
    var g = textureSampleLevel(grains, grain_smp, uv, st.grain, select(max(log2(256.0 / scale), 0.0), 0.0, sharp)).r;
    if (sharp) {
        g = sharp_grain(g, uv);
    }
    return g;
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
    let w = select(unpack4x8unorm(work[k]), unpack4x8unorm(grained[k]), once);
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
        if (st.laid_apart == 1u) {
            // how much of the paint shows there so far, put together with the rest later
            let l = st.stride * st.size.y + k;
            amount[l] = amount[l] + weight * (1.0 - amount[l]);
            return;
        }
        out = mix(w, laid, weight);
    }
    if (st.preserve_alpha == 1u) {
        out = out * (w.a / max(out.a, 1e-6));
    }
    // rounded to 8 bits up or down at random, in proportion to the remainder, so that faint stamps
    // add up as they should instead of rounding away
    let h = dither(u32(p.x), u32(p.y), bitcast<u32>(st.center.x) ^ (bitcast<u32>(st.center.y) * 2654435761u));
    if (once) {
        // the stroke kept without the grain; shown with the grain over its change to the layer
        grained[k] = pack4x8unorm(clamp(out + (h - 0.5) / 255.0, vec4<f32>(0.0), vec4<f32>(1.0)));
        let was = unpack4x8unorm(grained[st.stride * st.size.y + k]);
        let shown = was + (out - was) * grain_at(vec2<f32>(p) + 0.5);
        work[k] = pack4x8unorm(clamp(shown + (h - 0.5) / 255.0, vec4<f32>(0.0), vec4<f32>(1.0)));
        return;
    }
    work[k] = pack4x8unorm(clamp(out + (h - 0.5) / 255.0, vec4<f32>(0.0), vec4<f32>(1.0)));
}

// A dual brush's paint laid in place, put together where its stamps or its second's went: the
// paint over the layer as it was, as far as how much of it was laid and the second's coverage
// (cut at its threshold) combine by the dual mode (with a texturized grain laid over it once).
@compute @workgroup_size(8, 8)
fn cs_synthesize(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = st.bbox.xy + vec2<i32>(id.xy);
    if (p.x >= st.bbox.z || p.y >= st.bbox.w || p.x < 0 || p.y < 0 || p.x >= i32(st.size.x) || p.y >= i32(st.size.y)) {
        return;
    }
    let k = u32(p.y) * st.stride + u32(p.x);
    var a = amount[st.stride * st.size.y + k];
    if (st.dual != 0u) {
        var b = textureLoad(second_tex, p, 0).a;
        if (st.dual_threshold >= 0.0) {
            b = select(0.0, 1.0, b > st.dual_threshold);
        }
        a = dual_combine(a, b, st.dual - 1u);
    }
    if (st.grain_once > 0.5) {
        a = a * grain_at(vec2<f32>(p) + 0.5);
    }
    let was = textureLoad(before_tex, p, 0);
    var out = mix(was, vec4<f32>(st.paint.rgb, 1.0), a);
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
    let under_first = layer_at(plot_point(wet_pass.stamps[0], t));
    var c = under_first;
    if (st.kept >= 0) {
        c = plot_at(st.index * CARRIED, st.carried, t);
    }
    var out = c;
    let paint = vec4<f32>(st.paint.rgb, 1.0);
    for (var k = 0u; k < st.count; k++) {
        let w = wet_pass.stamps[k];
        c = mix(layer_at(plot_point(w, t)), c, w.mixing.y);
        c = mix(c, paint, w.mixing.z);
        out = mix(under_first, c, w.mixing.w);
        carry[2u * CARRIED + k * n * n + id.y * n + id.x] = out;
    }
    carry[(1u - st.index) * CARRIED + id.y * n + id.x] = out;
}
