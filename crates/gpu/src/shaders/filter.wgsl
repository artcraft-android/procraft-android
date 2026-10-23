// Adjustments: a layer redrawn from its copy through a filter, kept to the selection. The last
// pass of each mixes the filtered colour with the copy by how much the selection takes.
// Premultiplied throughout.

struct FilterUniform {
    // the filter's own values
    a: vec4<f32>,
    b: vec4<f32>,
    // (the target's width, height, 1 with a selection, unused)
    size: vec4<f32>,
    c: vec4<f32>,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var chosen: texture_2d<f32>;
@group(1) @binding(0) var<uniform> fu: FilterUniform;
// the layer's copy, for the last pass's mix (bound to the copy itself when the source is the copy)
@group(2) @binding(0) var before: texture_2d<f32>;
// 256 entries: the red, green and blue curves and the curve for all three (curves), or a
// gradient by brightness (gradient map)
@group(2) @binding(1) var table: texture_2d<f32>;

@vertex
fn vs_filter(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// how much of target pixel i the filter takes: all, or what the selection (or the painted-on mask)
// takes
fn taken(i: vec2<i32>) -> f32 {
    if (fu.size.z > 0.5) {
        return textureLoad(chosen, i, 0).r;
    }
    return 1.0;
}

// the filtered colour mixed with the copy by the selection
fn kept(i: vec2<i32>, filtered: vec4<f32>) -> vec4<f32> {
    return mix(textureLoad(before, i, 0), filtered, taken(i));
}

// luma and the two chroma differences (Rec. 601)
fn to_yuv(c: vec3<f32>) -> vec3<f32> {
    let y = dot(c, vec3<f32>(0.299, 0.587, 0.114));
    return vec3<f32>(y, (c.b - y) * 0.565, (c.r - y) * 0.713);
}

fn from_yuv(c: vec3<f32>) -> vec3<f32> {
    let r = c.x + c.z / 0.713;
    let b = c.x + c.y / 0.565;
    let g = (c.x - 0.299 * r - 0.114 * b) / 0.587;
    return vec3<f32>(r, g, b);
}

// the chroma turned by a.x (radians) and scaled by a.y, a.z added to the luma
@fragment
fn fs_hsb(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let c = textureLoad(src, i, 0);
    if (c.a <= 0.0) {
        return kept(i, c);
    }
    let yuv = to_yuv(c.rgb / c.a);
    let cs = cos(fu.a.x);
    let sn = sin(fu.a.x);
    let uv = vec2<f32>(yuv.y * cs - yuv.z * sn, yuv.y * sn + yuv.z * cs) * fu.a.y;
    let rgb = clamp(from_yuv(vec3<f32>(yuv.x + fu.a.z, uv)), vec3<f32>(0.0), vec3<f32>(1.0));
    return kept(i, vec4<f32>(rgb * c.a, c.a));
}

// half the size: each target pixel the mean of the 2 × 2 under it
@fragment
fn fs_down(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = pos.xy / fu.size.xy;
    return textureSampleLevel(src, smp, uv, 0.0);
}

// a Gaussian along a.xy (one pixel), sigma a.z, out to a.w pixels either side
@fragment
fn fs_blur(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let dims = vec2<i32>(textureDimensions(src));
    let step = vec2<i32>(fu.a.xy);
    let sigma = max(fu.a.z, 0.01);
    let reach = i32(fu.a.w);
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var k = -reach; k <= reach; k++) {
        let w = exp(-f32(k * k) / (2.0 * sigma * sigma));
        let j = clamp(i + step * k, vec2<i32>(0), dims - 1);
        sum += textureLoad(src, j, 0) * w;
        total += w;
    }
    return sum / total;
}

// back to full size from the small blurred texture, kept to the selection
@fragment
fn fs_up(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let uv = pos.xy / fu.size.xy;
    return kept(i, textureSampleLevel(src, smp, uv, 0.0));
}

// copy pixel i moved by o, held at the edges
fn near(i: vec2<i32>, o: vec2<i32>) -> vec4<f32> {
    let dims = vec2<i32>(textureDimensions(src));
    return textureLoad(src, clamp(i + o, vec2<i32>(0), dims - 1), 0);
}

// a premultiplied colour kept valid
fn valid(c: vec4<f32>) -> vec4<f32> {
    let a = clamp(c.a, 0.0, 1.0);
    return vec4<f32>(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(a)), a);
}

// a Laplacian sharpen: the pixel pushed away from its four neighbours by a.x
@fragment
fn fs_sharpen(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let s = fu.a.x;
    let around = near(i, vec2<i32>(1, 0)) + near(i, vec2<i32>(-1, 0)) + near(i, vec2<i32>(0, 1)) + near(i, vec2<i32>(0, -1));
    return kept(i, valid((1.0 + 4.0 * s) * textureLoad(src, i, 0) - s * around));
}

// a pseudo-random gradient direction for lattice point p, varied by seed
fn gradient(p: vec2<f32>, seed: f32) -> vec2<f32> {
    let h = fract(sin(dot(p, vec2<f32>(127.1, 311.7)) + seed * 74.7) * 43758.5453);
    let t = h * 6.2831853;
    return vec2<f32>(cos(t), sin(t));
}

// smooth gradient noise at p, about -0.7..0.7
fn lattice_noise(p: vec2<f32>, seed: f32) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = dot(gradient(i, seed), f);
    let b = dot(gradient(i + vec2<f32>(1.0, 0.0), seed), f - vec2<f32>(1.0, 0.0));
    let c = dot(gradient(i + vec2<f32>(0.0, 1.0), seed), f - vec2<f32>(0.0, 1.0));
    let d = dot(gradient(i + vec2<f32>(1.0, 1.0), seed), f - vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// one octave of noise of the kind b.x (clouds signed, billows and ridges 0..1)
fn shaped_noise(p: vec2<f32>, seed: f32) -> f32 {
    let n = lattice_noise(p, seed);
    if (fu.b.x > 1.5) {
        let r = 1.0 - abs(n) * 1.4;
        return r * r;
    }
    if (fu.b.x > 0.5) {
        return abs(n) * 1.4;
    }
    return n;
}

// fractal noise at p for channel seed s: a.y frequency per pixel, a.z octaves, a.w turbulence
fn fractal(p: vec2<f32>, s: f32) -> f32 {
    var total = 0.0;
    var weight = 1.0;
    var sum = 0.0;
    var freq = fu.a.y;
    let octaves = i32(fu.a.z);
    for (var o = 0; o < octaves; o++) {
        total += shaped_noise(p * freq, f32(o) + s) * weight;
        sum += weight;
        weight *= 0.5 + fu.a.w;
        freq *= 2.0;
    }
    return total / max(sum, 1e-4);
}

// fractal noise over the colour: a.x amount; b.x the kind, b.y 1 for each channel its own, b.z 1
// to add it (else mixed in)
@fragment
fn fs_noise(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let c = textureLoad(src, i, 0);
    let one = fractal(pos.xy, 0.0);
    var n = vec3<f32>(one);
    if (fu.b.y > 0.5) {
        n = vec3<f32>(one, fractal(pos.xy, 17.0), fractal(pos.xy, 31.0));
    }
    // billows and ridges are 0..1: centred, so adding them both lightens and darkens
    if (fu.b.x > 0.5) {
        n = (n - 0.5) * 1.4;
    }
    if (fu.b.z > 0.5) {
        return kept(i, valid(vec4<f32>(c.rgb + n * fu.a.x * c.a, c.a)));
    }
    let grey = clamp(n * 0.5 + 0.5, vec3<f32>(0.0), vec3<f32>(1.0));
    return kept(i, valid(vec4<f32>(mix(c.rgb, grey * c.a, clamp(fu.a.x, 0.0, 1.0)), c.a)));
}

// samples along a line from the pixel: a.xy the line (pixels), a.z how many, a.w 1 to centre it
@fragment
fn fs_streak(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let count = max(i32(fu.a.z), 1);
    let start = select(vec2<f32>(0.0), -0.5 * fu.a.xy, fu.a.w > 0.5);
    var sum = vec4<f32>(0.0);
    for (var k = 0; k < count; k++) {
        let t = (f32(k) + 0.5) / f32(count);
        let p = pos.xy + start + fu.a.xy * t;
        sum += textureSampleLevel(src, smp, p / fu.size.xy, 0.0);
    }
    return kept(i, sum / f32(count));
}

// each pixel streaked towards (b.xy, pixels) by a.x of the way there
@fragment
fn fs_zoom(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let towards = (fu.b.xy - pos.xy) * fu.a.x;
    let count = clamp(i32(length(towards)), 1, 96);
    var sum = vec4<f32>(0.0);
    for (var k = 0; k < count; k++) {
        let t = (f32(k) + 0.5) / f32(count);
        sum += textureSampleLevel(src, smp, (pos.xy + towards * t) / fu.size.xy, 0.0);
    }
    return kept(i, sum / f32(count));
}

// red and blue pulled apart, smeared over a.y of the pull: from the centre (b.xy, pixels) by a.x of
// the distance and less within a.z of the half diagonal of it; or (b.z below 0) by b.xy everywhere,
// the fringes beyond the picture a.w transparent
@fragment
fn fs_chroma(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    var away = fu.b.xy;
    if (fu.b.z >= 0.0) {
        let reach = 0.5 * length(fu.size.xy);
        let near_centre = smoothstep(0.0, max(fu.a.z * reach, 1e-3), length(pos.xy - fu.b.xy));
        away = (pos.xy - fu.b.xy) * fu.a.x * select(1.0, near_centre, fu.a.z > 0.0);
    }
    let taps = 1 + i32(fu.a.y * 8.0);
    var red = vec4<f32>(0.0);
    var blue = vec4<f32>(0.0);
    for (var k = 0; k < taps; k++) {
        let t = 1.0 - fu.a.y * f32(k) / f32(max(taps, 1));
        red += textureSampleLevel(src, smp, (pos.xy - away * t) / fu.size.xy, 0.0);
        blue += textureSampleLevel(src, smp, (pos.xy + away * t) / fu.size.xy, 0.0);
    }
    red /= f32(taps);
    blue /= f32(taps);
    let mid = textureLoad(src, i, 0);
    let fringe = max(red.a, blue.a);
    let a = max(mid.a, fringe * (1.0 - fu.a.w * select(0.0, 1.0, fu.b.z < 0.0)));
    return kept(i, valid(vec4<f32>(red.r, mid.g, blue.b, a)));
}

// how much ink a halftone screen at `angle` (radians) with cells `pitch` pixels puts at p, for an
// ink coverage of `ink`
fn screen_dot(p: vec2<f32>, angle: f32, pitch: f32, ink: f32) -> f32 {
    let cs = cos(angle);
    let sn = sin(angle);
    let q = vec2<f32>(p.x * cs + p.y * sn, -p.x * sn + p.y * cs) / pitch;
    let d = length(fract(q) - 0.5) * pitch;
    // a dot whose area is the ink's share of the cell
    let r = sqrt(ink / 3.1415927) * pitch;
    return clamp(r - d + 0.5, 0.0, 1.0);
}

// a halftone with cells a.x pixels across, of the kind a.y: full colour (cyan, magenta, yellow and
// black screens), screen print (the colour itself in dots as big as it is dark), newspaper (black
// dots as big as the grey is dark)
@fragment
fn fs_halftone(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let c = textureLoad(src, i, 0);
    if (c.a <= 0.0) {
        return kept(i, c);
    }
    let rgb = c.rgb / c.a;
    let pitch = fu.a.x;
    let deg = 0.017453292;
    let kind = i32(fu.a.y + 0.5);
    if (kind == 2) {
        let dot_ink = screen_dot(pos.xy, 45.0 * deg, pitch, 1.0 - dot(rgb, vec3<f32>(0.299, 0.587, 0.114)));
        return kept(i, vec4<f32>(vec3<f32>(1.0 - dot_ink) * c.a, c.a));
    }
    if (kind == 1) {
        let top = max(rgb.r, max(rgb.g, rgb.b));
        let ink = rgb / max(top, 1e-4);
        let cover = screen_dot(pos.xy, 45.0 * deg, pitch, clamp(1.0 - dot(rgb, vec3<f32>(0.299, 0.587, 0.114)) + 0.25 * top, 0.0, 1.0));
        return kept(i, vec4<f32>(mix(vec3<f32>(1.0), ink * top, cover) * c.a, c.a));
    }
    let k = 1.0 - max(rgb.r, max(rgb.g, rgb.b));
    let under = max(1.0 - k, 1e-4);
    let cmy = (vec3<f32>(1.0) - rgb - vec3<f32>(k)) / under;
    let dc = screen_dot(pos.xy, 15.0 * deg, pitch, cmy.x);
    let dm = screen_dot(pos.xy, 75.0 * deg, pitch, cmy.y);
    let dy = screen_dot(pos.xy, 0.0, pitch, cmy.z);
    let dk = screen_dot(pos.xy, 45.0 * deg, pitch, k);
    let printed = (vec3<f32>(1.0) - vec3<f32>(dc, dm, dy)) * (1.0 - dk);
    return kept(i, vec4<f32>(printed * c.a, c.a));
}

// the blurred copy's light over the copy: a.x how much, from brightness a.y up over a ramp a.z
// (0..1) long, made stronger by a.w (burn, -0.3..1)
@fragment
fn fs_bloom(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let glow = textureSampleLevel(src, smp, pos.xy / fu.size.xy, 0.0);
    let c = textureLoad(before, i, 0);
    let low = fu.a.y * (1.0 - clamp(fu.a.z, 0.0, 1.0));
    let bright = smoothstep(low, fu.a.y + 0.02, dot(glow.rgb, vec3<f32>(0.299, 0.587, 0.114)));
    let strength = fu.a.x * (1.0 + 1.5 * fu.a.w);
    let light = glow.rgb * bright * strength;
    // screened over what is there
    let rgb = c.rgb + light * (1.0 - c.rgb);
    return kept(i, valid(vec4<f32>(rgb, max(c.a, glow.a * bright * strength))));
}

// a colour through the table at v (0..1)
fn look(v: f32) -> vec4<f32> {
    return textureLoad(table, vec2<i32>(i32(clamp(v, 0.0, 1.0) * 255.0 + 0.5), 0), 0);
}

// curves: each channel through the curve for all three, then its own
@fragment
fn fs_curves(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let c = textureLoad(src, i, 0);
    if (c.a <= 0.0) {
        return kept(i, c);
    }
    let rgb = c.rgb / c.a;
    let r = look(look(rgb.r).a).r;
    let g = look(look(rgb.g).a).g;
    let b = look(look(rgb.b).a).b;
    return kept(i, vec4<f32>(vec3<f32>(r, g, b) * c.a, c.a));
}

// gradient map: the gradient's colour at the pixel's brightness, a.x of the way
@fragment
fn fs_gradient(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let c = textureLoad(src, i, 0);
    if (c.a <= 0.0) {
        return kept(i, c);
    }
    let rgb = c.rgb / c.a;
    let mapped = look(dot(rgb, vec3<f32>(0.299, 0.587, 0.114))).rgb;
    return kept(i, vec4<f32>(mix(rgb, mapped, fu.a.x) * c.a, c.a));
}

// how much of each tone a channel value is: shadows, midtones, highlights
fn tones(v: f32) -> vec3<f32> {
    let sh = clamp(0.5 - 4.0 * (v - 1.0 / 3.0), 0.0, 1.0);
    let mid = clamp(0.5 + 4.0 * (v - 1.0 / 3.0), 0.0, 1.0) * clamp(0.5 - 4.0 * (v - 2.0 / 3.0), 0.0, 1.0);
    let hi = clamp(0.5 + 4.0 * (v - 2.0 / 3.0), 0.0, 1.0);
    return vec3<f32>(sh, mid, hi);
}

// colour balance: each channel shifted by its shadows (a), midtones (b) and highlights (c) values,
// as much as it is of each tone; the brightness kept
@fragment
fn fs_balance(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let c = textureLoad(src, i, 0);
    if (c.a <= 0.0) {
        return kept(i, c);
    }
    let rgb = c.rgb / c.a;
    var shifted = vec3<f32>(0.0);
    for (var k = 0; k < 3; k++) {
        let w = tones(rgb[k]);
        shifted[k] = clamp(rgb[k] + 0.7 * (w.x * fu.a[k] + w.y * fu.b[k] + w.z * fu.c[k]), 0.0, 1.0);
    }
    let kept_y = to_yuv(rgb).x;
    let moved = to_yuv(shifted);
    let out = clamp(from_yuv(vec3<f32>(kept_y, moved.y, moved.z)), vec3<f32>(0.0), vec3<f32>(1.0));
    return kept(i, vec4<f32>(out * c.a, c.a));
}

// liquify: the copy from as far back as the field's offsets say, a.x of them
@fragment
fn fs_liquified(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let offset = textureLoad(table, i, 0).xy * fu.a.x;
    return kept(i, textureSampleLevel(src, smp, (pos.xy - offset) / fu.size.xy, 0.0));
}

// clone: the copy from a.xy (pixels) away
@fragment
fn fs_clone(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    return kept(i, textureSampleLevel(src, smp, (pos.xy + fu.a.xy) / fu.size.xy, 0.0));
}

// a pseudo-random value 0..1 for cell p, varied by seed
fn cell_random(p: vec2<f32>, seed: f32) -> f32 {
    return fract(sin(dot(p, vec2<f32>(269.5, 183.3)) + seed * 91.3) * 43758.5453);
}

// the copy at canvas point p
fn copy_at(p: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(src, smp, p / fu.size.xy, 0.0);
}

// glitch: a.x the kind (artifact, wave, signal, diverge), a.y how much, a.zw and b.x the kind's
// values, b.y the zoom, b.z a seed
@fragment
fn fs_glitch(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    let p = pos.xy;
    let kind = i32(fu.a.x + 0.5);
    let amount = fu.a.y;
    let span = min(fu.size.x, fu.size.y);
    let scale = 0.3 + 0.7 * fu.b.y;
    let seed = fu.b.z;
    var out = textureLoad(src, i, 0);
    if (kind == 0) {
        // blocks twice as wide as tall, some copied from a few blocks away, and finer pieces
        let h = span * (0.008 + 0.12 * fu.a.w) * scale;
        let block = vec2<f32>(2.0 * h, h);
        let cell = floor(p / block);
        if (cell_random(cell, seed) < fu.a.z * amount * 0.6) {
            let jump = vec2<f32>(cell_random(cell + 1.3, seed), cell_random(cell + 7.1, seed)) - 0.5;
            out = copy_at(p + round(jump * 12.0 * amount) * block);
        }
        let piece = block / 3.0;
        let small = floor(p / piece);
        if (cell_random(small, seed + 3.0) < fu.a.z * amount * 0.15) {
            let jump = vec2<f32>(cell_random(small + 2.9, seed), cell_random(small + 5.3, seed)) - 0.5;
            out = copy_at(p + round(jump * 8.0) * piece);
        }
    } else if (kind == 1) {
        // rows bent sideways along two waves
        let k = 6.2831853 / (span * (1.25 - fu.a.w) * scale);
        let dx = amount * fu.a.z * span * 0.15 * (sin(k * p.y + seed) + 0.5 * sin(2.3 * k * p.y + 1.7 + seed));
        out = copy_at(p + vec2<f32>(dx, 0.0));
    } else if (kind == 2) {
        // bands moved sideways over the picture, their red and blue a little apart
        let h = span * (0.01 + 0.15 * fu.a.w) * scale;
        let band = floor(p.y / h);
        if (cell_random(vec2<f32>(band, 0.5), seed) < fu.a.z * amount * 0.9) {
            let dx = (cell_random(vec2<f32>(band, 3.1), seed) - 0.5) * span * 0.16 * amount;
            let moved = copy_at(p + vec2<f32>(dx, 0.0));
            let r = copy_at(p + vec2<f32>(dx * 1.04, 0.0)).r;
            let b = copy_at(p + vec2<f32>(dx * 0.96, 0.0)).b;
            out = max(out, vec4<f32>(r, moved.g, b, moved.a));
        }
    } else {
        // each channel moved its own way, block by block
        let c = span * (0.04 + 0.3 * fu.b.y);
        let cell = floor(p / c);
        let shift = vec3<f32>(fu.a.z, fu.a.w, fu.b.x) * c * 0.45 * amount;
        let along = vec3<f32>(cell_random(cell, seed), cell_random(cell + 2.0, seed), cell_random(cell + 4.0, seed));
        let across = vec3<f32>(cell_random(cell + 1.0, seed), cell_random(cell + 3.0, seed), cell_random(cell + 5.0, seed)) - 0.5;
        let red = copy_at(p - vec2<f32>(0.4 + 0.6 * along.x, 0.6 * across.x) * shift.x);
        let green = copy_at(p - vec2<f32>(0.4 + 0.6 * along.y, 0.6 * across.y) * shift.y);
        let blue = copy_at(p - vec2<f32>(0.4 + 0.6 * along.z, 0.6 * across.z) * shift.z);
        out = vec4<f32>(red.r, green.g, blue.b, max(max(red.a, green.a), blue.a));
    }
    return kept(i, valid(out));
}

