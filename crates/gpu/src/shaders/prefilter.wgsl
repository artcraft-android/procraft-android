// An environment's light prefiltered into a cube for rough reflections: each level of the cube is
// the panorama seen through a GGX lobe as rough as that level's share of the levels, from
// importance samples read at a level of the panorama as coarse as each sample's share of the
// sphere (so few samples stay smooth).

struct Prefilter {
    // the cube face (0..5: +x, -x, +y, -y, +z, -z), its side at this level (texels), the
    // roughness, and the panorama's texel count
    face: u32,
    side: f32,
    roughness: f32,
    texels: f32,
}

@group(0) @binding(0) var<uniform> pf: Prefilter;
@group(0) @binding(1) var panorama: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;

const PI: f32 = 3.14159265;
const SAMPLES: u32 = 96u;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(vi & 1u) * 4.0 - 1.0;
    let y = f32(vi >> 1u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

// the direction through a face's point (s, t in -1..1, t down the face), as cube maps lay them out
fn face_direction(face: u32, s: f32, t: f32) -> vec3<f32> {
    switch face {
        case 0u: { return vec3<f32>(1.0, -t, -s); }
        case 1u: { return vec3<f32>(-1.0, -t, s); }
        case 2u: { return vec3<f32>(s, 1.0, t); }
        case 3u: { return vec3<f32>(s, -1.0, -t); }
        case 4u: { return vec3<f32>(s, -t, 1.0); }
        default: { return vec3<f32>(-s, -t, -1.0); }
    }
}

// where a direction falls in the panorama (its middle looking along -z, its top straight up)
fn panorama_at(d: vec3<f32>) -> vec2<f32> {
    let u = atan2(d.x, -d.z) / (2.0 * PI) + 0.5;
    let v = acos(clamp(d.y, -1.0, 1.0)) / PI;
    return vec2<f32>(u, v);
}

fn hammersley(i: u32, n: u32) -> vec2<f32> {
    return vec2<f32>(f32(i) / f32(n), f32(reverseBits(i)) * 2.3283064e-10);
}

@fragment
fn fs_prefilter(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let st = pos.xy / pf.side * 2.0 - 1.0;
    let n = normalize(face_direction(pf.face, st.x, st.y));
    if (pf.roughness <= 0.0) {
        return vec4<f32>(textureSampleLevel(panorama, smp, panorama_at(n), 0.0).rgb, 1.0);
    }
    // a frame about the normal (the view taken along it)
    let up = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.y) < 0.999);
    let tx = normalize(cross(up, n));
    let ty = cross(n, tx);
    let a = pf.roughness * pf.roughness;
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var i = 0u; i < SAMPLES; i++) {
        let xi = hammersley(i, SAMPLES);
        let phi = 2.0 * PI * xi.x;
        let cos_t = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
        let sin_t = sqrt(max(1.0 - cos_t * cos_t, 0.0));
        let h = normalize(tx * (sin_t * cos(phi)) + ty * (sin_t * sin(phi)) + n * cos_t);
        let l = 2.0 * dot(n, h) * h - n;
        let nl = dot(n, l);
        if (nl > 0.0) {
            // the sample's share of the sphere against a texel's: the level to read it at
            let nh = max(dot(n, h), 0.0);
            let d = a * a / max(PI * pow(nh * nh * (a * a - 1.0) + 1.0, 2.0), 1e-6);
            let pdf = d / 4.0;
            let sample_share = 1.0 / (f32(SAMPLES) * pdf + 1e-4);
            let texel_share = 4.0 * PI / pf.texels;
            let lod = max(0.5 * log2(sample_share / texel_share) + 1.0, 0.0);
            sum += textureSampleLevel(panorama, smp, panorama_at(l), lod).rgb * nl;
            weight += nl;
        }
    }
    return vec4<f32>(sum / max(weight, 1e-4), 1.0);
}
