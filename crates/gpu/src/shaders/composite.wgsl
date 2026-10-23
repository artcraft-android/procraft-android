// Layer compositing, blend modes and stroke commits. Every texture here is the canvas's size, so
// pixels are read with textureLoad at the fragment's own position: no filtering, no coordinates.
// Colours are premultiplied; blend modes work on straight colour (separable and non-separable
// modes as usually defined for compositing).

struct LayerUniform {
    opacity: f32,
    // 0: no stroke on this layer, 1: the stroke paints, 2: it erases, 3: it paints alpha-locked
    stroke_mode: u32,
    stroke_opacity: f32,
    blend: u32,
    // 1: multiply by the clip base's alpha
    clipped: u32,
    // the stroke's texturized grain: -1 for none, else a layer of `grains`
    grain: i32,
    grain_scale: f32,
    grain_depth: f32,
    // premultiplied colour for fs_solid
    solid: vec4<f32>,
    // the stroke's wet edges (0..1, negative as a preview draws them) and burnt edges (0..1), its
    // alpha threshold (negative for none), and its grain's brightness and contrast (-1..1)
    wet: f32,
    burnt: f32,
    threshold: f32,
    grain_brightness: f32,
    grain_contrast: f32,
    // 1: the layer shows through its mask (mask_tex); 1: the stroke being drawn is on the mask
    masked: f32,
    stroke_on_mask: f32,
    // how the stroke blends into the layer: a blend mode's number (0: normal)
    stroke_blend: u32,
    // an onion skin's colour (rgb) and how far towards it the layer goes (a)
    tint: vec4<f32>,
    // the stroke's texturized grain turned, radians, and how it and the burnt edges combine with
    // the stroke (blend modes' numbers)
    grain_rotation: f32,
    grain_blend: u32,
    burnt_blend: u32,
    // 1: the grain read unfiltered (sharp)
    grain_sharp: f32,
}

@group(0) @binding(0) var layer_tex: texture_2d<f32>;
@group(0) @binding(1) var stroke_tex: texture_2d<f32>;
@group(0) @binding(2) var clip_tex: texture_2d<f32>;
@group(0) @binding(3) var backdrop_tex: texture_2d<f32>;
@group(0) @binding(4) var grains: texture_2d_array<f32>;
@group(0) @binding(5) var grain_smp: sampler;
@group(0) @binding(6) var mask_tex: texture_2d<f32>;
@group(1) @binding(0) var<uniform> lu: LayerUniform;

// one triangle covering the target
@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// the stroke's coverage at p moved by o, held at the edges
fn stroke_alpha(p: vec2<i32>, o: vec2<i32>) -> f32 {
    let dims = vec2<i32>(textureDimensions(stroke_tex));
    return textureLoad(stroke_tex, clamp(p + o, vec2<i32>(0), dims - 1), 0).a;
}

// a grain's value made lighter or darker and of more or less contrast
fn grain_look(g: f32) -> f32 {
    return clamp((g - 0.5) * (1.0 + lu.grain_contrast) + 0.5 + lu.grain_brightness, 0.0, 1.0);
}

// the stroke at a pixel, and how much its burnt edges show there
struct StrokePixel {
    s: vec4<f32>,
    burn: f32,
}

// the stroke at a pixel: its build-up, its edges, the paper grain fixed to the canvas, its
// threshold and its opacity
fn stroke_pixel(p: vec2<i32>) -> StrokePixel {
    var s = textureLoad(stroke_tex, p, 0);
    if (lu.wet < 0.0 && s.a > 0.0) {
        // a preview: how much of an edge the pixel is (its coverage above its neighbours' a few
        // pixels away), the edges a little stronger and the inside as it is (as measured: at
        // full, a wide brush's rim 4% stronger, its middle and a thin line unchanged)
        let d = 3;
        let around = 0.25 * (stroke_alpha(p, vec2<i32>(d, 0)) + stroke_alpha(p, vec2<i32>(-d, 0)) + stroke_alpha(p, vec2<i32>(0, d)) + stroke_alpha(p, vec2<i32>(0, -d)));
        let edge = clamp((s.a - around) * 2.0, 0.0, 1.0);
        let wet_a = min(s.a * (1.0 - lu.wet * 0.1 * edge), 1.0);
        s = s * (wet_a / s.a);
    } else if (lu.wet > 0.0 && s.a > 0.0) {
        // on the canvas the paint pools: the coverage taken through a damped wave, which rises to
        // full where the stroke is about 0.7 (a ring where it fades out) and settles back to
        // about 0.78 where it is full; faint paint fades
        let x = 4.5 * s.a;
        let pooled = 1.0 - abs(sin(x) / x);
        s = s * (mix(s.a, pooled, lu.wet) / s.a);
    }
    let built = s.a;
    if (lu.grain >= 0) {
        let at = vec2<f32>(p) + 0.5;
        let gc = cos(lu.grain_rotation);
        let gs = sin(lu.grain_rotation);
        let uv = vec2<f32>(at.x * gc - at.y * gs, at.x * gs + at.y * gc) / lu.grain_scale;
        let sharp = lu.grain_sharp > 0.5;
        let lod = select(max(log2(256.0 / lu.grain_scale), 0.0), 0.0, sharp);
        var raw = textureSampleLevel(grains, grain_smp, uv, lu.grain, lod).r;
        if (sharp) {
            raw = sharp_grain(raw, uv);
        }
        let g = grain_look(raw);
        // the coverage and the grain combined by the grain's blend mode, as deep as it says, and
        // only where the stroke is
        let a = grain_combine(s.a, g, lu.grain_depth, lu.grain_blend);
        s = s * (a / max(s.a, 1e-6));
    }
    // burnt edges show where the stroke is thin: where its paper grain took paint away, or
    // without one wherever it is less than full
    var burn = 0.0;
    if (lu.burnt > 0.0 && built > 0.0) {
        burn = lu.burnt * select(1.0 - s.a, abs(built - s.a), lu.grain >= 0);
    }
    if (lu.threshold >= 0.0 && s.a > 0.0) {
        let on = select(0.0, 1.0, s.a > lu.threshold);
        s = s * (on / s.a);
    }
    return StrokePixel(s * lu.stroke_opacity, burn);
}

fn stroke_at(p: vec2<i32>) -> vec4<f32> {
    return stroke_pixel(p).s;
}

// burnt edges: the stroke's result `r` blended once more with the paint that was under it, by
// `w` (nothing shows where the layer was empty)
fn burnt(under: vec4<f32>, r: vec4<f32>, w: f32) -> vec4<f32> {
    if (w <= 0.0) {
        return r;
    }
    let b = blend(under, r, lu.burnt_blend);
    return mix(r, vec4<f32>(unpremultiply(b) * r.a, r.a), clamp(w, 0.0, 1.0));
}

// a layer with the stroke in progress on it applied
fn with_stroke(c0: vec4<f32>, p: vec2<i32>) -> vec4<f32> {
    var c = c0;
    if (lu.stroke_mode != 0u) {
        let px = stroke_pixel(p);
        let s = px.s;
        if (lu.stroke_mode == 1u) {
            c = burnt(c0, blend(c, s, lu.stroke_blend), px.burn);
        } else if (lu.stroke_mode == 2u) {
            c = c * (1.0 - s.a);
        } else {
            // recolour, keeping the layer's alpha
            let painted = burnt(c0, blend(c, s, lu.stroke_blend), px.burn);
            c = painted * (c.a / max(painted.a, 1e-6));
        }
    }
    return c;
}

// a layer as it composites: stroke, mask, clipping, opacity
fn layer_color(p: vec2<i32>) -> vec4<f32> {
    var c = textureLoad(layer_tex, p, 0);
    if (lu.stroke_on_mask < 0.5) {
        c = with_stroke(c, p);
    }
    if (lu.masked > 0.5) {
        // the mask shows the layer as much as it is light (and opaque)
        var m = textureLoad(mask_tex, p, 0);
        if (lu.stroke_on_mask > 0.5) {
            m = with_stroke(m, p);
        }
        c = c * clamp(dot(m.rgb, vec3<f32>(0.299, 0.587, 0.114)), 0.0, 1.0);
    }
    if (lu.clipped != 0u) {
        c = c * textureLoad(clip_tex, p, 0).a;
    }
    if (lu.tint.a > 0.0) {
        c = vec4<f32>(mix(c.rgb, lu.tint.rgb * c.a, lu.tint.a), c.a);
    }
    return c * lu.opacity;
}

// Normal layers: blended "over" by the pipeline
@fragment
fn fs_layer(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return layer_color(vec2<i32>(pos.xy));
}

// other modes: read what is below and write the result
@fragment
fn fs_blend(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy);
    return blend(textureLoad(backdrop_tex, p, 0), layer_color(p), lu.blend);
}

// the background colour (written, not blended)
@fragment
fn fs_solid() -> @location(0) vec4<f32> {
    return lu.solid;
}

// the finished stroke at its opacity: blended "over" into the layer to paint, or as
// "destination out" to erase
@fragment
fn fs_commit(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return stroke_at(vec2<i32>(pos.xy));
}

// the finished stroke on an alpha-locked layer, or blended by a mode (written to a scratch
// texture, copied back)
@fragment
fn fs_commit_locked(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy);
    return with_stroke(textureLoad(layer_tex, p, 0), p);
}

// ---- blending -------------------------------------------------------------------------------

fn unpremultiply(c: vec4<f32>) -> vec3<f32> {
    if (c.a <= 0.0) {
        return vec3<f32>(0.0);
    }
    return clamp(c.rgb / c.a, vec3<f32>(0.0), vec3<f32>(1.0));
}

// source `s` over backdrop `b` (both premultiplied) with blend function `mode`
fn blend(b: vec4<f32>, s: vec4<f32>, mode: u32) -> vec4<f32> {
    // shade: the darker of the backdrop and the source laid over it, premultiplied and alpha too
    if (mode == 26u) {
        return min(b, s + b * (1.0 - s.a));
    }
    // behind (a brush's): the source laid under the backdrop
    if (mode == 29u) {
        return b + s * (1.0 - b.a);
    }
    let cb = unpremultiply(b);
    let cs = unpremultiply(s);
    let mixed = clamp(blend_rgb(cb, cs, mode), vec3<f32>(0.0), vec3<f32>(1.0));
    let rgb = s.a * (1.0 - b.a) * cs + s.a * b.a * mixed + (1.0 - s.a) * b.rgb;
    return vec4<f32>(rgb, s.a + b.a * (1.0 - s.a));
}
