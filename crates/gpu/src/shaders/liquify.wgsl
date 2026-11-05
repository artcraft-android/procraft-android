// Liquify. The picture shows through a field of offsets (canvas pixels: each pixel shows the copy
// from that far away). The brush works in steps: a step's stamps add up what they do at each
// pixel, then the field is carried along by that, so what was moved before moves on with the rest
// and the picture behaves like something soft, however often the brush goes over it.

struct LiquifyUniform {
    // a stamp's (centre x, y, radius, amount); a step's (how much of the field stays, ...)
    stamp: vec4<f32>,
    // a stamp's (way x, y, mode, how narrow a lobe is); a step's (.., .., .., selected only)
    way: vec4<f32>,
    // (canvas width, height, field pixels a canvas pixel across, down)
    canvas: vec4<f32>,
    // the step's part of the field (x, y, width, height, field pixels)
    area: vec4<f32>,
}

@group(0) @binding(0) var<uniform> lu: LiquifyUniform;
// what the stamps add up to: (offset x, y, how much is rebuilt)
@group(1) @binding(0) var sums: texture_2d<f32>;
@group(1) @binding(1) var field: texture_2d<f32>;
@group(1) @binding(2) var chosen: texture_2d<f32>;

// how far a stamp moves things: what is behind a push comes on by this part of the brush's move;
// a twirl turns at this many radians a second, a pinch draws in by this part of the distance
const PUSH: f32 = 0.75;
const TURN: f32 = 4.0;
const DRAW: f32 = 1.5;
// rebuilding: a second of it at the centre leaves this, to this power, of the offsets; those under
// the least offset (pixels) go altogether
const REBUILD_KEEP: f32 = 0.75;
const REBUILD_POWER: f32 = 25.0;
const LEAST_OFFSET: f32 = 0.25;
const NEAREST: f32 = 0.3;

struct StampOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
}

// a square around the stamp, in the step's part of the field
@vertex
fn vs_stamp(@builtin(vertex_index) vi: u32) -> StampOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let q = corners[vi % 6u];
    let p = (lu.stamp.xy + q * lu.stamp.z) * lu.canvas.zw - lu.area.xy;
    var out: StampOut;
    out.pos = vec4<f32>(p.x / lu.area.z * 2.0 - 1.0, 1.0 - p.y / lu.area.w * 2.0, 0.0, 1.0);
    out.local = q * lu.stamp.z;
    return out;
}

// all of it at the centre, none at the rim, half at half way, flat at both ends
fn falloff(d: f32) -> f32 {
    let x = clamp(d, 0.0, 1.0);
    if (x < 0.5) {
        return 1.0 - 2.0 * x * x;
    }
    return 2.0 * (1.0 - x) * (1.0 - x);
}

// what one stamp does at its own pixel
@fragment
fn fs_stamp(in: StampOut) -> @location(0) vec4<f32> {
    let r = lu.stamp.z;
    let away = in.local;
    let dist = length(away);
    if (dist >= r) {
        discard;
    }
    // no nearer than a part of a pixel of the field: a tiny stamp only just touches it
    let f = falloff(max(dist, NEAREST / max(lu.canvas.z, lu.canvas.w)) / r) * lu.stamp.w;
    let way = lu.way.xy;
    var moved = vec2<f32>(0.0);
    var rebuilt = 0.0;
    switch i32(lu.way.z + 0.5) {
        // push: what is behind the brush comes on
        case 0: { moved = way * (-PUSH * f); }
        // twirl right and left: turned about the centre
        case 1: { moved = vec2<f32>(away.y, -away.x) * (TURN * f); }
        case 2: { moved = vec2<f32>(-away.y, away.x) * (TURN * f); }
        // pinch and expand: drawn in, or out, a lobe only towards its way
        case 3: { moved = away * (DRAW * f); }
        case 4: { moved = away * (-DRAW * f); }
        case 5: {
            let narrow = lu.way.w;
            let towards = dot(away / max(dist, 1e-4), way);
            moved = away * (DRAW * f * clamp((towards - narrow) / (1.0 - narrow), 0.0, 1.0));
        }
        // edge: drawn in towards the brush's line, more the further from it
        case 6: {
            let across = vec2<f32>(-way.y, way.x);
            let side = dot(across, away);
            moved = across * (sign(side) * 0.5 * abs(side) * f / r);
        }
        // reconstruct
        default: { rebuilt = f; }
    }
    return vec4<f32>(moved, rebuilt, 0.0);
}

@vertex
fn vs_step(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// the field at `p` (field pixels), between its pixels
fn field_at(p: vec2<f32>) -> vec2<f32> {
    let last = vec2<i32>(textureDimensions(field)) - 1;
    let at = p - 0.5;
    let below = floor(at);
    let t = at - below;
    let a = clamp(vec2<i32>(below), vec2<i32>(0), last);
    let b = clamp(vec2<i32>(below) + 1, vec2<i32>(0), last);
    let top = mix(textureLoad(field, a, 0).xy, textureLoad(field, vec2<i32>(b.x, a.y), 0).xy, t.x);
    let bottom = mix(textureLoad(field, vec2<i32>(a.x, b.y), 0).xy, textureLoad(field, b, 0).xy, t.x);
    return mix(top, bottom, t.y);
}

// the field after a step: where the stamps send this pixel, the offset from there on top
@fragment
fn fs_step(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let sum = textureLoad(sums, vec2<i32>(pos.xy), 0);
    let here = pos.xy + lu.area.xy;
    var taken = 1.0;
    if (lu.way.w > 0.5) {
        taken = textureLoad(chosen, vec2<i32>(here / lu.canvas.zw), 0).r;
    }
    let moved = sum.xy * taken;
    var offset = (moved + field_at(here + moved * lu.canvas.zw)) * lu.stamp.x;
    if (sum.z > 0.0) {
        offset *= pow(REBUILD_KEEP, REBUILD_POWER * sum.z * taken);
        if (length(offset) < LEAST_OFFSET) {
            offset = vec2<f32>(0.0);
        }
    }
    return vec4<f32>(offset, 0.0, 1.0);
}
