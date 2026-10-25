// The brush studio's 3D preview: a sphere on the pad, lit as a model is by an environment (its
// irradiance for diffuse light, a prefiltered radiance cube for reflections, with the split-sum
// table), showing the pad's canvas where the screen sees it: what is painted there over the
// sphere's own plain material, in its colour and with the brush's metalness and roughness (each
// through its source, tiled across the canvas). Around the sphere, the pad's own colour.

struct Sphere {
    // the environment's irradiance as spherical harmonics (bands 0 to 2)
    sh: array<vec4<f32>, 9>,
    // the sphere's middle and radius (target pixels), and the radiance cube's last level
    circle: vec4<f32>,
    // target pixels to canvas pixels: x by the first row, y by the second (x, y, 1 weights)
    to_canvas_x: vec4<f32>,
    to_canvas_y: vec4<f32>,
    // the canvas's size, and the metalness and roughness sources' tiles (canvas pixels)
    canvas: vec4<f32>,
    // the sphere's own colour (as stored) and roughness
    base: vec4<f32>,
    // the brush's metalness and roughness, and whether each goes through a source
    brush: vec4<f32>,
    // around the sphere (as stored)
    behind: vec4<f32>,
    // the textures' colours to the target's, in linear light: three rows, c0.w 1 to apply them
    c0: vec4<f32>,
    c1: vec4<f32>,
    c2: vec4<f32>,
}

@group(0) @binding(0) var<uniform> s: Sphere;
@group(0) @binding(1) var radiance: texture_cube<f32>;
@group(0) @binding(2) var radiance_smp: sampler;
@group(0) @binding(3) var brdf: texture_2d<f32>;
@group(0) @binding(4) var brdf_smp: sampler;
@group(0) @binding(5) var painted_tex: texture_2d<f32>;
@group(0) @binding(6) var painted_smp: sampler;
@group(0) @binding(7) var metallic_src: texture_2d<f32>;
@group(0) @binding(8) var roughness_src: texture_2d<f32>;
@group(0) @binding(9) var source_smp: sampler;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(vi & 1u) * 4.0 - 1.0;
    let y = f32(vi >> 1u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn irradiance(n: vec3<f32>) -> vec3<f32> {
    let b = array<f32, 9>(
        0.282095,
        0.488603 * n.y,
        0.488603 * n.z,
        0.488603 * n.x,
        1.092548 * n.x * n.y,
        1.092548 * n.y * n.z,
        0.315392 * (3.0 * n.z * n.z - 1.0),
        1.092548 * n.x * n.z,
        0.546274 * (n.x * n.x - n.y * n.y),
    );
    var e = vec3<f32>(0.0);
    for (var i = 0; i < 9; i++) {
        e += s.sh[i].xyz * b[i];
    }
    return max(e, vec3<f32>(0.0));
}

// the stored colour into the target's colour space (another profile's primaries), clipped to it
fn moved(rgb: vec3<f32>) -> vec3<f32> {
    if (s.c0.w < 0.5) {
        return rgb;
    }
    let lin = srgb_to_linear(rgb);
    let m = vec3<f32>(dot(s.c0.xyz, lin), dot(s.c1.xyz, lin), dot(s.c2.xyz, lin));
    return linear_to_srgb(clamp(m, vec3<f32>(0.0), vec3<f32>(1.0)));
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let behind = moved(s.behind.rgb);
    let d = (pos.xy - s.circle.xy) / s.circle.z;
    let dist = length(d);
    // how much of this pixel the sphere covers: its outline smoothed over a pixel
    let cover = clamp(0.5 - (dist - 1.0) * s.circle.z, 0.0, 1.0);
    if (cover <= 0.0) {
        return vec4<f32>(behind, 1.0);
    }
    // just outside, the outline's own normal
    let e = d / max(dist, 1.0);
    let n = vec3<f32>(e.x, -e.y, sqrt(max(1.0 - dot(e, e), 0.0)));
    let v = vec3<f32>(0.0, 0.0, 1.0);
    // the canvas under this pixel, over the sphere's own colour
    let at = vec2<f32>(dot(s.to_canvas_x.xyz, vec3<f32>(pos.xy, 1.0)), dot(s.to_canvas_y.xyz, vec3<f32>(pos.xy, 1.0)));
    let painted = textureSampleLevel(painted_tex, painted_smp, at / s.canvas.xy, 0.0);
    let color = painted.rgb + s.base.rgb * (1.0 - painted.a);
    let albedo_lin = pow(max(color, vec3<f32>(0.0)), vec3<f32>(2.2));
    // the paint's metalness and roughness through their sources, over the sphere's own
    let metal_through = select(1.0, textureSampleLevel(metallic_src, source_smp, at / s.canvas.z, 0.0).r, s.brush.z > 0.5);
    let rough_through = select(1.0, textureSampleLevel(roughness_src, source_smp, at / s.canvas.w, 0.0).r, s.brush.w > 0.5);
    let metallic = s.brush.x * metal_through * painted.a;
    let roughness = mix(s.base.w, s.brush.y * rough_through, painted.a);
    let r = max(roughness, 0.045);
    let f0 = mix(vec3<f32>(0.04), albedo_lin, metallic);
    let nv = clamp(dot(n, v), 1e-5, 1.0);
    let reflected = textureSampleLevel(radiance, radiance_smp, reflect(-v, n), r * s.circle.w).rgb;
    let split = textureSampleLevel(brdf, brdf_smp, vec2<f32>(nv, r), 0.0).rg;
    let fr = f0 + (max(vec3<f32>(1.0 - r), f0) - f0) * pow(1.0 - nv, 5.0);
    let lit = (1.0 - fr) * (1.0 - metallic) * albedo_lin * irradiance(n) + reflected * (fr * split.x + split.y);
    let rgb = clamp(pow(max(lit, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(mix(behind, moved(rgb), cover), 1.0);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}
