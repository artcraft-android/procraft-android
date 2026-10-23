// A layer redrawn from its copy: through a projective map (each layer pixel looks up where it
// came from in the copy), or as a mesh of triangles each carrying where its corners came from
// (a warp). Either way the copy is filtered there (nearest, bilinear or bicubic). With a
// selection only what it selects moves, over what it leaves where it was. Premultiplied
// throughout.

struct TransformUniform {
    // the map from layer pixel coordinates to copy pixel coordinates, row by row (3 × 3)
    r0: vec4<f32>,
    r1: vec4<f32>,
    r2: vec4<f32>,
    // (filter: 0 nearest, 1 bilinear, 2 bicubic, the copy's width, height, 1 with a selection)
    p: vec4<f32>,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
// how much of each pixel the selection takes (read only with a selection)
@group(0) @binding(2) var chosen: texture_2d<f32>;
@group(1) @binding(0) var<uniform> tu: TransformUniform;

@vertex
fn vs_transform(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// how much of copy pixel i moves: all of it, or what the selection takes
fn moving(i: vec2<i32>) -> f32 {
    if (tu.p.w > 0.5) {
        return textureLoad(chosen, i, 0).r;
    }
    return 1.0;
}

// what moves of a copy pixel, transparent outside the copy
fn texel(i: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(i32(tu.p.y), i32(tu.p.z));
    if (i.x < 0 || i.y < 0 || i.x >= size.x || i.y >= size.y) {
        return vec4<f32>(0.0);
    }
    return textureLoad(src, i, 0) * moving(i);
}

// what stays where it was: the copy where the selection does not take it
@fragment
fn fs_static(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy);
    return textureLoad(src, i, 0) * (1.0 - moving(i));
}

// Catmull-Rom weights for the four taps around a fraction t
fn cubic(t: f32) -> vec4<f32> {
    let t2 = t * t;
    let t3 = t2 * t;
    return vec4<f32>(
        -0.5 * t3 + t2 - 0.5 * t,
        1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
        0.5 * t3 - 0.5 * t2,
    );
}

// the copy filtered at copy coordinate s (pixel centres at +0.5)
fn filtered(s: vec2<f32>) -> vec4<f32> {
    let kind = i32(tu.p.x);
    if (kind == 0) {
        return texel(vec2<i32>(floor(s)));
    }
    let c = s - 0.5;
    let base = vec2<i32>(floor(c));
    let f = c - floor(c);
    if (kind == 1) {
        let top = mix(texel(base), texel(base + vec2<i32>(1, 0)), f.x);
        let bottom = mix(texel(base + vec2<i32>(0, 1)), texel(base + vec2<i32>(1, 1)), f.x);
        return mix(top, bottom, f.y);
    }
    let wx = cubic(f.x);
    let wy = cubic(f.y);
    var sum = vec4<f32>(0.0);
    for (var j = 0; j < 4; j++) {
        var row = vec4<f32>(0.0);
        for (var i = 0; i < 4; i++) {
            row += wx[i] * texel(base + vec2<i32>(i - 1, j - 1));
        }
        sum += wy[j] * row;
    }
    // the cubic can overshoot: keep it a valid premultiplied colour
    let a = clamp(sum.a, 0.0, 1.0);
    return vec4<f32>(clamp(sum.rgb, vec3<f32>(0.0), vec3<f32>(a)), a);
}

@fragment
fn fs_transform(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let h = vec3<f32>(pos.x, pos.y, 1.0);
    let w = dot(tu.r2.xyz, h);
    if (w <= 1e-6) {
        return vec4<f32>(0.0);
    }
    return filtered(vec2<f32>(dot(tu.r0.xyz, h), dot(tu.r1.xyz, h)) / w);
}

struct MeshOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) origin: vec2<f32>,
}

// a mesh corner: where it goes in the layer and where it came from in the copy (pixels)
@vertex
fn vs_mesh(@location(0) at: vec2<f32>, @location(1) origin: vec2<f32>) -> MeshOut {
    let size = tu.p.yz;
    var out: MeshOut;
    out.pos = vec4<f32>(at.x / size.x * 2.0 - 1.0, 1.0 - at.y / size.y * 2.0, 0.0, 1.0);
    out.origin = origin;
    return out;
}

@fragment
fn fs_mesh(in: MeshOut) -> @location(0) vec4<f32> {
    return filtered(in.origin);
}
