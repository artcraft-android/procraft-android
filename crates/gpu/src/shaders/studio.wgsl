// The Lighting Studio's scene around a model: a round floor of grid lines under it, fading toward
// its rim, and each light as a small glowing cube in the light's colour.

struct Studio {
    view_proj: mat4x4<f32>,
    // the floor's height, its radius, a grid cell's side, and the line width (in cells)
    floor: vec4<f32>,
    // each light's place and the cube's side
    cubes: array<vec4<f32>, 4>,
    // each light's colour (sRGB 0..1)
    colors: array<vec4<f32>, 4>,
    // the target's colours: x 1 when it encodes sRGB itself
    output: vec4<f32>,
}

@group(0) @binding(0) var<uniform> studio: Studio;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

fn out_color(rgb: vec3<f32>, a: f32) -> vec4<f32> {
    var c = rgb;
    if (studio.output.x > 0.5) {
        c = srgb_to_linear(c);
    }
    return vec4<f32>(c * a, a);
}

// ---- the floor ------------------------------------------------------------------------------

struct FloorOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) at: vec2<f32>,
}

@vertex
fn vs_floor(@builtin(vertex_index) vi: u32) -> FloorOut {
    // two triangles over the floor's square
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let r = studio.floor.y;
    let xz = corners[vi] * r;
    var o: FloorOut;
    o.clip = studio.view_proj * vec4<f32>(xz.x, studio.floor.x, xz.y, 1.0);
    o.at = xz;
    return o;
}

@fragment
fn fs_floor(i: FloorOut) -> @location(0) vec4<f32> {
    let r = studio.floor.y;
    let cell = studio.floor.z;
    let p = i.at / cell;
    // how far from the nearest line across and along, in pixels (for even lines at any slant)
    let d = abs(p - round(p)) / max(fwidth(p), vec2<f32>(1e-4));
    let line = 1.0 - clamp(min(d.x, d.y) - 0.5, 0.0, 1.0);
    // full in the middle, gone at the rim
    let rim = length(i.at) / r;
    let fade = 1.0 - smoothstep(0.45, 1.0, rim);
    let a = line * fade * 0.85;
    if (a <= 0.003) {
        discard;
    }
    return out_color(vec3<f32>(0.93, 0.95, 1.0), a);
}

// ---- the lights' cubes ----------------------------------------------------------------------

struct CubeOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) local: vec3<f32>,
}

@vertex
fn vs_cube(@builtin(vertex_index) vi: u32, @builtin(instance_index) light: u32) -> CubeOut {
    // six faces of two triangles: a face's normal axis and its corners
    let face = vi / 6u;
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let corner = corners[vi % 6u];
    var n = vec3<f32>(0.0);
    var p = vec3<f32>(0.0);
    switch face {
        case 0u: { n = vec3<f32>(1.0, 0.0, 0.0); p = vec3<f32>(1.0, corner.x, -corner.y); }
        case 1u: { n = vec3<f32>(-1.0, 0.0, 0.0); p = vec3<f32>(-1.0, corner.x, corner.y); }
        case 2u: { n = vec3<f32>(0.0, 1.0, 0.0); p = vec3<f32>(corner.x, 1.0, -corner.y); }
        case 3u: { n = vec3<f32>(0.0, -1.0, 0.0); p = vec3<f32>(corner.x, -1.0, corner.y); }
        case 4u: { n = vec3<f32>(0.0, 0.0, 1.0); p = vec3<f32>(corner.x, corner.y, 1.0); }
        default: { n = vec3<f32>(0.0, 0.0, -1.0); p = vec3<f32>(-corner.x, corner.y, -1.0); }
    }
    let cube = studio.cubes[light];
    // turned a little about the upright and leaning, so three faces show
    let yaw = 0.6;
    let lean = 0.35;
    let ry = mat3x3<f32>(cos(yaw), 0.0, -sin(yaw), 0.0, 1.0, 0.0, sin(yaw), 0.0, cos(yaw));
    let rx = mat3x3<f32>(1.0, 0.0, 0.0, 0.0, cos(lean), sin(lean), 0.0, -sin(lean), cos(lean));
    let turn = ry * rx;
    var o: CubeOut;
    o.clip = studio.view_proj * vec4<f32>(cube.xyz + turn * p * (cube.w * 0.5), 1.0);
    o.normal = turn * n;
    o.color = studio.colors[light].rgb;
    o.local = p;
    return o;
}

@fragment
fn fs_cube(i: CubeOut) -> @location(0) vec4<f32> {
    // a frosted, glowing block: mostly its own colour, the top brightest, the edges softened
    let n = normalize(i.normal);
    let shade = 0.82 + 0.18 * n.y;
    let edge = max(max(abs(i.local.x) * abs(i.local.y), abs(i.local.y) * abs(i.local.z)), abs(i.local.x) * abs(i.local.z));
    let soft = 1.0 - 0.12 * smoothstep(0.75, 1.0, edge);
    let c = mix(vec3<f32>(1.0), i.color, 0.85) * shade * soft;
    return out_color(clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
