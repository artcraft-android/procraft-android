// The selection shown over its part of a model: the part drawn again, reading the selection's
// coverage from the texture's space. As on a flat canvas: faint bands fixed to the window over
// what is not selected, or a colour over what is.

struct Look {
    view_proj: mat4x4<f32>,
    // the bands' period and drift (pixels), their coverage, and 0 for bands or 1 for a colour
    look: vec4<f32>,
    // the colour (sRGB), and 1 when the target encodes sRGB itself
    colour: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: Look;
@group(0) @binding(1) var mask: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;

struct In {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_over(v: In) -> Out {
    var o: Out;
    o.clip = u.view_proj * vec4<f32>(v.position, 1.0);
    o.uv = v.uv;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_over(i: Out) -> @location(0) vec4<f32> {
    // texture rows run down from v = 1
    let at = fract(vec2<f32>(i.uv.x, 1.0 - i.uv.y));
    let selected = textureSampleLevel(mask, smp, at, 0.0).r;
    if (u.look.w > 0.5) {
        var c = u.colour.rgb;
        if (u.colour.w > 0.5) {
            c = srgb_to_linear(c);
        }
        let a = selected * u.look.z;
        return vec4<f32>(c * a, a);
    }
    // bands rising to the right, dark and light by turns, with soft edges
    let p = i.clip.xy;
    let band = fract((p.x + p.y + u.look.y) / u.look.x);
    let light = smoothstep(0.09, 0.25, band) - smoothstep(0.59, 0.75, band);
    let a = (1.0 - selected) * u.look.z;
    return vec4<f32>(vec3<f32>(light * a), a);
}
