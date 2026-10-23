// Blend functions on straight colour, shared by the shaders that blend (prepended to them):
// the separable and non-separable modes as usually defined for compositing, numbered as the
// layers' blend modes are.

// How much of a grain the paint takes when pressed into it at `press` (0..1): what stands above
// a level that sinks as the pen presses harder, nothing at no pressure and all of it at full,
// the grain's highest parts first.
const PRESS_SOFT: f32 = 0.08;
fn pressed_in(grain: f32, press: f32) -> f32 {
    let level = mix(1.0 + PRESS_SOFT, PRESS_SOFT, clamp(press, 0.0, 1.0));
    return smoothstep(level - PRESS_SOFT, level + PRESS_SOFT, grain);
}

// a grain read unfiltered: the grain's own images are a quarter of the size of what they stand
// for, so the finer texels a pixel would land on (four to each of ours, tied to the grain so they
// move with it) are made up around the value read
fn sharp_grain(v: f32, uv: vec2<f32>) -> f32 {
    let cell = floor(uv * 1024.0);
    let n = fract(sin(dot(cell, vec2<f32>(12.9898, 78.233))) * 43758.547);
    return clamp(v + (n - 0.5) * 0.22, 0.0, 1.0);
}

fn blend_rgb(cb: vec3<f32>, cs: vec3<f32>, mode: u32) -> vec3<f32> {
    switch (mode) {
        case 1u: { return cb * cs; }
        case 2u: { return min(cb, cs); }
        case 3u: { return vec3<f32>(burn(cb.x, cs.x), burn(cb.y, cs.y), burn(cb.z, cs.z)); }
        case 4u: { return max(cb + cs - 1.0, vec3<f32>(0.0)); }
        case 5u: { return select(cb, cs, lum(cs) < lum(cb)); }
        case 6u: { return max(cb, cs); }
        case 7u: { return cb + cs - cb * cs; }
        case 8u: { return vec3<f32>(dodge(cb.x, cs.x), dodge(cb.y, cs.y), dodge(cb.z, cs.z)); }
        case 9u: { return min(cb + cs, vec3<f32>(1.0)); }
        case 10u: { return select(cb, cs, lum(cs) > lum(cb)); }
        case 11u: { return vec3<f32>(hard(cs.x, cb.x), hard(cs.y, cb.y), hard(cs.z, cb.z)); }
        case 12u: { return vec3<f32>(soft(cb.x, cs.x), soft(cb.y, cs.y), soft(cb.z, cs.z)); }
        case 13u: { return vec3<f32>(hard(cb.x, cs.x), hard(cb.y, cs.y), hard(cb.z, cs.z)); }
        case 14u: { return vec3<f32>(vivid(cb.x, cs.x), vivid(cb.y, cs.y), vivid(cb.z, cs.z)); }
        case 15u: { return clamp(cb + 2.0 * cs - 1.0, vec3<f32>(0.0), vec3<f32>(1.0)); }
        case 16u: { return vec3<f32>(pin(cb.x, cs.x), pin(cb.y, cs.y), pin(cb.z, cs.z)); }
        case 17u: { return select(vec3<f32>(0.0), vec3<f32>(1.0), cb + cs >= vec3<f32>(1.0)); }
        case 18u: { return abs(cb - cs); }
        case 19u: { return cb + cs - 2.0 * cb * cs; }
        case 20u: { return max(cb - cs, vec3<f32>(0.0)); }
        case 21u: { return vec3<f32>(divide(cb.x, cs.x), divide(cb.y, cs.y), divide(cb.z, cs.z)); }
        case 22u: { return set_lum(set_sat(cs, sat(cb)), lum(cb)); }
        case 23u: { return set_lum(set_sat(cb, sat(cs)), lum(cb)); }
        case 24u: { return set_lum(cs, lum(cb)); }
        case 25u: { return set_lum(cb, lum(cs)); }
        // shade, on straight colour (the layers' own is in `blend`)
        case 26u: { return min(cb, cs); }
        default: { return cs; }
    }
}

fn burn(b: f32, s: f32) -> f32 {
    if (b >= 1.0) { return 1.0; }
    if (s <= 0.0) { return 0.0; }
    return 1.0 - min(1.0, (1.0 - b) / s);
}

fn dodge(b: f32, s: f32) -> f32 {
    if (b <= 0.0) { return 0.0; }
    if (s >= 1.0) { return 1.0; }
    return min(1.0, b / (1.0 - s));
}

// hard light of `s` over `b` (overlay is hard light with the two swapped)
fn hard(b: f32, s: f32) -> f32 {
    if (s <= 0.5) { return b * 2.0 * s; }
    let t = 2.0 * s - 1.0;
    return b + t - b * t;
}

fn soft(b: f32, s: f32) -> f32 {
    if (s <= 0.5) {
        return b - (1.0 - 2.0 * s) * b * (1.0 - b);
    }
    var d = sqrt(b);
    if (b <= 0.25) {
        d = ((16.0 * b - 12.0) * b + 4.0) * b;
    }
    return b + (2.0 * s - 1.0) * (d - b);
}

fn vivid(b: f32, s: f32) -> f32 {
    if (s <= 0.5) { return burn(b, 2.0 * s); }
    return dodge(b, 2.0 * (s - 0.5));
}

fn pin(b: f32, s: f32) -> f32 {
    if (s <= 0.5) { return min(b, 2.0 * s); }
    return max(b, 2.0 * s - 1.0);
}

fn divide(b: f32, s: f32) -> f32 {
    if (s <= 0.0) { return select(0.0, 1.0, b > 0.0); }
    return min(1.0, b / s);
}

fn lum(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.3, 0.59, 0.11));
}

fn clip_color(c: vec3<f32>) -> vec3<f32> {
    let l = lum(c);
    let n = min(c.x, min(c.y, c.z));
    let x = max(c.x, max(c.y, c.z));
    var r = c;
    if (n < 0.0) {
        r = l + (r - l) * l / max(l - n, 1e-6);
    }
    if (x > 1.0) {
        r = l + (r - l) * (1.0 - l) / max(x - l, 1e-6);
    }
    return r;
}

fn set_lum(c: vec3<f32>, l: f32) -> vec3<f32> {
    return clip_color(c + (l - lum(c)));
}

fn sat(c: vec3<f32>) -> f32 {
    return max(c.x, max(c.y, c.z)) - min(c.x, min(c.y, c.z));
}

fn set_sat(c: vec3<f32>, s: f32) -> vec3<f32> {
    let n = min(c.x, min(c.y, c.z));
    let x = max(c.x, max(c.y, c.z));
    if (x <= n) {
        return vec3<f32>(0.0);
    }
    return (c - n) * s / (x - n);
}

// one value blended with another: a grey through the colour blend
fn blend_value(b: f32, s: f32, mode: u32) -> f32 {
    return blend_rgb(vec3<f32>(b), vec3<f32>(s), mode).x;
}

// coverage `a` and grain `g` combined by the grain's blend mode `mode`, as deep as `depth`: two
// height-like modes of the grain's own (27: the coverage raised twelvefold, less the grain; 28:
// the same or as much again over the grain's light), normal laying the coverage over the grain,
// and every other mode by the blend mode's own rule
fn grain_combine(a: f32, g: f32, depth: f32, mode: u32) -> f32 {
    let h = 12.0 * a * depth;
    if (mode == 27u) {
        return clamp(h - g, 0.0, 1.0);
    }
    if (mode == 28u) {
        return clamp(max(h * (1.0 - g), clamp(h - g, 0.0, 1.0)), 0.0, 1.0);
    }
    var blended = g + a * (1.0 - g);
    if (mode != 0u) {
        blended = clamp(blend_value(a, g, mode), 0.0, 1.0);
    }
    return mix(a, blended, depth);
}
