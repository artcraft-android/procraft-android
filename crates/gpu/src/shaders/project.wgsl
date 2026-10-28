// Carrying a stroke drawn on the screen onto a model's texture. First the model's distance from
// the camera is drawn for each screen pixel (the nearest surface wins); then the model is drawn
// in its texture's space, each texel taking the stroke where its own surface lands on the screen,
// if that surface is the nearest there. Surfaces sharing texels that both show keep the most of
// each (a blend by max). Last, texels just outside the part's islands (where its triangles lie in
// the texture) take the nearest island texel's stroke, so sampling across a seam finds paint.

struct Proj {
    // clip from world
    view_proj: mat4x4<f32>,
    // the screen's size (pixels), and 1 when strokes paint through the model
    screen: vec4<f32>,
    // the camera's place
    eye: vec4<f32>,
}

@group(0) @binding(0) var<uniform> proj: Proj;
@group(0) @binding(1) var stroke: texture_2d<f32>;
@group(0) @binding(2) var nearest: texture_2d<f32>;

struct In {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct DistanceOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) distance: f32,
}

@vertex
fn vs_distance(v: In) -> DistanceOut {
    var o: DistanceOut;
    o.clip = proj.view_proj * vec4<f32>(v.position, 1.0);
    o.distance = o.clip.w;
    return o;
}

@fragment
fn fs_distance(i: DistanceOut) -> @location(0) vec4<f32> {
    return vec4<f32>(i.distance, 0.0, 0.0, 1.0);
}

struct ProjectOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) screen: vec4<f32>,
    @location(1) world: vec3<f32>,
    @location(2) normal: vec3<f32>,
}

@vertex
fn vs_project(v: In) -> ProjectOut {
    var o: ProjectOut;
    // texture rows run down from v = 1
    o.clip = vec4<f32>(v.uv.x * 2.0 - 1.0, v.uv.y * 2.0 - 1.0, 0.0, 1.0);
    o.screen = proj.view_proj * vec4<f32>(v.position, 1.0);
    o.world = v.position;
    o.normal = v.normal;
    return o;
}

@fragment
fn fs_project(i: ProjectOut) -> @location(0) vec4<f32> {
    // behind the camera, off the screen or hidden: left as it is (another surface sharing these
    // texels may take the stroke)
    if (i.screen.w <= 0.0) {
        discard;
    }
    let ndc = i.screen.xy / i.screen.w;
    let at = vec2<f32>((ndc.x * 0.5 + 0.5) * proj.screen.x, (0.5 - ndc.y * 0.5) * proj.screen.y);
    if (at.x < 0.0 || at.y < 0.0 || at.x >= proj.screen.x || at.y >= proj.screen.y) {
        discard;
    }
    let p = vec2<i32>(at);
    // a small share of the distance for precision
    let front = textureLoad(nearest, p, 0).r;
    if (proj.screen.z < 0.5 && i.screen.w > front * 1.002 + 1e-4) {
        discard;
    }
    // a surface turned away takes less: none at grazing angles (either side of an open surface)
    let facing = abs(dot(normalize(i.normal), normalize(proj.eye.xyz - i.world)));
    let weight = 1.0 - saturate(6.0 * (1.0 - facing) - 4.6);
    return textureLoad(stroke, p, 0) * weight;
}

// ---- the part's islands: 1 wherever its triangles lie in the texture -------------------------

@vertex
fn vs_islands(v: In) -> @builtin(position) vec4<f32> {
    return vec4<f32>(v.uv.x * 2.0 - 1.0, v.uv.y * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_islands() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0);
}

// ---- dilation: the projected stroke (binding 1) grown past the islands (binding 2) -----------

// how far past an island's edge texels take its stroke
const DILATE: i32 = 2;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(vi & 1u) * 4.0 - 1.0;
    let y = f32(vi >> 1u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_dilate(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy);
    if (textureLoad(nearest, p, 0).r > 0.5) {
        return textureLoad(stroke, p, 0);
    }
    let size = vec2<i32>(textureDimensions(stroke));
    var best = vec4<f32>(0.0);
    var best_d = 1000000;
    for (var dy = -DILATE; dy <= DILATE; dy++) {
        for (var dx = -DILATE; dx <= DILATE; dx++) {
            let q = p + vec2<i32>(dx, dy);
            let d = dx * dx + dy * dy;
            if (all(q >= vec2<i32>(0)) && all(q < size) && d < best_d && textureLoad(nearest, q, 0).r > 0.5) {
                best = textureLoad(stroke, q, 0);
                best_d = d;
            }
        }
    }
    return best;
}

// ---- a stroke's roughness or metalness: the colour's coverage (binding 1, in the texture's
// space) carrying the brush's amount, through its source seen from the model's three axes; and
// a stroke's grain, read the same way -----------------------------------------------------------

struct Material {
    // grain space from the model's (columns): framing units about the model's middle, or about
    // the camera and turned with it when the grain follows the camera
    space: mat4x4<f32>,
    // the amount, one source tile's side (grain space units), and 1 when there is a source
    look: vec4<f32>,
}

@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var source: texture_2d<f32>;
@group(1) @binding(2) var source_sampler: sampler;

struct MaterialOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) framed: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

@vertex
fn vs_material(v: In) -> MaterialOut {
    var o: MaterialOut;
    o.clip = vec4<f32>(v.uv.x * 2.0 - 1.0, v.uv.y * 2.0 - 1.0, 0.0, 1.0);
    o.framed = (material.space * vec4<f32>(v.position, 1.0)).xyz;
    o.normal = (material.space * vec4<f32>(v.normal, 0.0)).xyz;
    return o;
}

// how sharply the three views part where the surface turns from one axis to another
const AXIS_SHARPNESS: f32 = 10.0;

// the source where a surface at `p` facing `n` is, read along the axis it faces most: each
// axis's reading weighted by how squarely the surface faces it, the weights' squares summing
// to one; seen from the x side across z and up y (z turned about for the side facing +x), from
// above across x and z (z turned about from above), from the front across x and up y (x turned
// about from behind)
fn from_three_axes(p: vec3<f32>, n: vec3<f32>, tile: f32) -> f32 {
    let along = pow(abs(n), vec3<f32>(AXIS_SHARPNESS));
    let weights = saturate(along * inverseSqrt(max(dot(along, along), 1e-12)));
    let q = p / tile;
    let across_x = vec2<f32>(select(q.z, -q.z, n.x >= 0.0), q.y);
    let across_y = vec2<f32>(q.x, select(-q.z, q.z, n.y < 0.0));
    let across_z = vec2<f32>(select(-q.x, q.x, n.z > 0.0), q.y);
    let seen = vec3<f32>(
        textureSampleLevel(source, source_sampler, across_x, 0.0).r,
        textureSampleLevel(source, source_sampler, across_y, 0.0).r,
        textureSampleLevel(source, source_sampler, across_z, 0.0).r,
    );
    return dot(seen, weights);
}

@fragment
fn fs_material(i: MaterialOut) -> @location(0) vec4<f32> {
    let cover = textureLoad(stroke, vec2<i32>(i.clip.xy), 0).a;
    var amount = material.look.x;
    if (material.look.z > 0.5) {
        let n = normalize(i.normal);
        amount = amount * from_three_axes(i.framed, n, max(material.look.y, 1e-4));
    }
    let v = clamp(amount, 0.0, 1.0) * cover;
    return vec4<f32>(v, v, v, cover);
}

// each texel the grain where its surface is (the source read along the model's three axes),
// what a stroke's grain fixed to the canvas becomes on a model
@fragment
fn fs_grain(i: MaterialOut) -> @location(0) vec4<f32> {
    let g = saturate(from_three_axes(i.framed, normalize(i.normal), max(material.look.y, 1e-4)));
    return vec4<f32>(g, g, g, 1.0);
}
