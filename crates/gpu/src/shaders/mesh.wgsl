// A 3D model drawn textured and lit: each part's albedo (premultiplied, over the part's base
// colour where nothing is painted) shaded physically, by up to four lights about it and by an
// environment (its irradiance for diffuse light, a prefiltered radiance cube for reflections,
// with the split-sum table), in the camera's space. Behind it, optionally, the environment's
// irradiance as a backdrop.

struct View {
    // clip from world, and the camera's space from world
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    // each light's place in the camera's space, and its radiance
    lights: array<vec4<f32>, 4>,
    radiances: array<vec4<f32>, 4>,
    // the environment's irradiance as spherical harmonics (bands 0 to 2)
    sh: array<vec4<f32>, 9>,
    // exposure, the radiance cube's last level, and the target's size
    params: vec4<f32>,
    // the projection's x and y scales (for the backdrop's rays)
    projection: vec4<f32>,
    // the textures' colours to the target's, in linear light: three rows, c0.w 1 to apply them
    c0: vec4<f32>,
    c1: vec4<f32>,
    c2: vec4<f32>,
    // how many lights there are
    counts: vec4<f32>,
}

struct Part {
    base: vec4<f32>,
    // roughness and metallic
    material: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var radiance: texture_cube<f32>;
@group(0) @binding(2) var radiance_smp: sampler;
@group(0) @binding(3) var brdf: texture_2d<f32>;
@group(0) @binding(4) var brdf_smp: sampler;
@group(1) @binding(0) var albedo: texture_2d<f32>;
@group(1) @binding(1) var albedo_smp: sampler;
@group(1) @binding(2) var<uniform> part: Part;
// roughness and metalness painted over the material's (premultiplied greys)
@group(1) @binding(3) var roughness_map: texture_2d<f32>;
@group(1) @binding(4) var metallic_map: texture_2d<f32>;

const PI: f32 = 3.14159265;

struct In {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct Out {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) eye: vec3<f32>,
}

@vertex
fn vs_main(v: In) -> Out {
    var o: Out;
    o.clip = view.view_proj * vec4<f32>(v.position, 1.0);
    o.normal = (view.view * vec4<f32>(v.normal, 0.0)).xyz;
    // texture rows run down, files' v up
    o.uv = vec2<f32>(v.uv.x, 1.0 - v.uv.y);
    o.eye = (view.view * vec4<f32>(v.position, 1.0)).xyz;
    return o;
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
        e += view.sh[i].xyz * b[i];
    }
    return max(e, vec3<f32>(0.0));
}

// the light one source sends back toward the eye
fn direct(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, radiance_in: vec3<f32>, albedo_lin: vec3<f32>, f0: vec3<f32>, r: f32, metallic: f32) -> vec3<f32> {
    let h = normalize(v + l);
    let nl = max(dot(n, l), 0.0);
    let nh = max(dot(n, h), 0.0);
    let hv = max(dot(h, v), 0.0);
    let nv = clamp(dot(n, v), 1e-5, 1.0);
    let a = r * r;
    let a2 = a * a;
    let f = f0 + (1.0 - f0) * pow(1.0 - hv, 5.0);
    let d = a2 / max(PI * pow(nh * nh * (a2 - 1.0) + 1.0, 2.0), 1e-6);
    let k = (r + 1.0) * (r + 1.0) / 8.0;
    let g = (nl / max(nl * (1.0 - k) + k, 1e-5)) * (nv / max(nv * (1.0 - k) + k, 1e-5));
    let spec = d * g * f / max(4.0 * nv * nl, 1e-6);
    let kd = (1.0 - f) * (1.0 - metallic);
    return radiance_in * nl * (kd * albedo_lin + spec);
}

// exposure, to display values, into the target's colours
fn finish(color: vec3<f32>) -> vec4<f32> {
    var rgb = clamp(pow(max(color * view.params.x, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2)), vec3<f32>(0.0), vec3<f32>(1.0));
    // into the target's colour space (another profile's primaries), clipped to it
    if (view.c0.w > 0.5) {
        let lin = srgb_to_linear(rgb);
        let moved = vec3<f32>(dot(view.c0.xyz, lin), dot(view.c1.xyz, lin), dot(view.c2.xyz, lin));
        rgb = linear_to_srgb(clamp(moved, vec3<f32>(0.0), vec3<f32>(1.0)));
    }
    return vec4<f32>(rgb, 1.0);
}

@fragment
fn fs_main(i: Out) -> @location(0) vec4<f32> {
    let painted = textureSample(albedo, albedo_smp, i.uv);
    let color = painted.rgb + part.base.rgb * (1.0 - painted.a);
    let albedo_lin = pow(max(color, vec3<f32>(0.0)), vec3<f32>(2.2));
    // a painted channel's grey over the material's value, as far as it covers
    let rm = textureSample(roughness_map, albedo_smp, i.uv);
    let mm = textureSample(metallic_map, albedo_smp, i.uv);
    let grey = vec3<f32>(0.2126, 0.7152, 0.0722);
    let roughness = part.material.x * (1.0 - rm.a) + dot(rm.rgb, grey);
    let metallic = part.material.y * (1.0 - mm.a) + dot(mm.rgb, grey);
    let v = normalize(-i.eye);
    // a normal turned away from the eye (the far side of an open surface, or a smooth one's
    // outline) mirrored across the view plane: facing the eye, its slant kept
    var n = normalize(i.normal);
    let away = dot(n, v);
    if (away < 0.0) {
        n = normalize(n - 2.0 * away * v);
    }
    let r = max(roughness, 0.045);
    let f0 = mix(vec3<f32>(0.04), albedo_lin, metallic);
    var lo = vec3<f32>(0.0);
    let count = i32(view.counts.x);
    for (var k = 0; k < 4; k++) {
        if (k < count) {
            lo += direct(n, v, normalize(view.lights[k].xyz - i.eye), view.radiances[k].xyz, albedo_lin, f0, r, metallic);
        }
    }
    // the environment: diffuse light from its irradiance, reflections from the prefiltered cube
    let nv = clamp(dot(n, v), 1e-5, 1.0);
    let reflected = textureSampleLevel(radiance, radiance_smp, reflect(-v, n), r * view.params.y).rgb;
    let split = textureSample(brdf, brdf_smp, vec2<f32>(nv, r)).rg;
    let fr = f0 + (max(vec3<f32>(1.0 - r), f0) - f0) * pow(1.0 - nv, 5.0);
    let ambient = (1.0 - fr) * (1.0 - metallic) * albedo_lin * irradiance(n) + reflected * (fr * split.x + split.y);
    return finish(lo + ambient);
}

// ---- the backdrop: the environment's irradiance seen past the model -------------------------

@vertex
fn vs_backdrop(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(vi & 1u) * 4.0 - 1.0;
    let y = f32(vi >> 1u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 1.0, 1.0);
}

@fragment
fn fs_backdrop(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let ndc = vec2<f32>(pos.x / view.params.z * 2.0 - 1.0, 1.0 - pos.y / view.params.w * 2.0);
    let ray = normalize(vec3<f32>(ndc.x / view.projection.x, ndc.y / view.projection.y, -1.0));
    return finish(irradiance(ray));
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
