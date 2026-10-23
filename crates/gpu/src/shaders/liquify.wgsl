// Liquify: brush stamps drawn into a field of offsets (canvas pixels: each pixel shows the copy
// from that far back), added up as the brush moves; reconstruct pulls the offsets back to none.

struct LiquifyUniform {
    // (centre x, y, radius, pressure)
    stamp: vec4<f32>,
    // (the brush's move since its last stamp x, y, mode, seed)
    move_mode: vec4<f32>,
    // (canvas width, height, distortion, unused)
    canvas: vec4<f32>,
    pad: vec4<f32>,
}

@group(0) @binding(0) var<uniform> lu: LiquifyUniform;

struct StampOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
}

// a square around the stamp, in canvas pixels
@vertex
fn vs_liquify(@builtin(vertex_index) vi: u32) -> StampOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let q = corners[vi % 6u];
    let p = lu.stamp.xy + q * lu.stamp.z;
    var out: StampOut;
    out.pos = vec4<f32>(p.x / lu.canvas.x * 2.0 - 1.0, 1.0 - p.y / lu.canvas.y * 2.0, 0.0, 1.0);
    out.local = q * lu.stamp.z;
    return out;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233)) + lu.move_mode.w) * 43758.5453);
}

// what one stamp adds to the offsets at its own pixel (or, for reconstruct, how much of them goes)
@fragment
fn fs_liquify(in: StampOut) -> @location(0) vec4<f32> {
    let r = lu.stamp.z;
    let d = length(in.local) / r;
    if (d >= 1.0) {
        discard;
    }
    // soft towards the rim
    let f = (1.0 - d * d) * (1.0 - d * d) * lu.stamp.w;
    let mode = i32(lu.move_mode.z + 0.5);
    let away = in.local;
    var add = vec2<f32>(0.0);
    switch mode {
        // push: what is under the brush goes with it
        case 0: { add = lu.move_mode.xy * f; }
        // twirl right and left: turned about the centre
        case 1, 2: {
            let turn = select(-1.0, 1.0, mode == 1) * f * 0.12;
            let c = cos(turn);
            let s = sin(turn);
            add = away - vec2<f32>(away.x * c - away.y * s, away.x * s + away.y * c);
        }
        // pinch: drawn in towards the centre; expand: pushed out
        case 3: { add = -away * f * 0.08; }
        case 4: { add = away * f * 0.08; }
        // crystals: scattered
        case 5: {
            let a = hash(floor(in.pos.xy / 6.0)) * 6.2831853;
            add = vec2<f32>(cos(a), sin(a)) * f * r * 0.04;
        }
        // edge: pushed out most near the rim
        case 6: { add = normalize(away + vec2<f32>(1e-4)) * d * f * r * 0.05; }
        // reconstruct: the alpha says how much of the offsets goes
        default: { return vec4<f32>(0.0, 0.0, 0.0, clamp(f * 0.2, 0.0, 1.0)); }
    }
    // distortion scatters the stroke a little
    let jitter = (vec2<f32>(hash(in.pos.xy), hash(in.pos.yx)) - 0.5) * lu.canvas.z * f * r * 0.05;
    return vec4<f32>(add + jitter, 0.0, 1.0);
}
