// Liquify: brush stamps drawn into a field of offsets (canvas pixels: each pixel shows the copy
// from that far back), added up as the brush moves; reconstruct pulls the offsets back to none.

struct LiquifyUniform {
    // (centre x, y, radius, pressure)
    stamp: vec4<f32>,
    // (the brush's move since its last stamp x, y, or how far a twirl, pinch or expand had gone
    // before this stamp and goes after it; mode, seed)
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

// `v` turned by `a`
fn turned(v: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(v.x * c - v.y * s, v.x * s + v.y * c);
}

// how far from the centre (a part of the radius) what `a` of drawing in has brought to `d` came
// from (drawing out, below zero): the brush's falloff followed back in small steps, none past 1,
// where they would overshoot the centre and fold the picture, however long it has been held
fn traced(d: f32, a: f32) -> f32 {
    var x = d;
    let steps = i32(clamp(ceil(abs(a)), 16.0, 64.0));
    let h = clamp(a, -64.0, 64.0) / f32(steps);
    for (var i = 0; i < steps; i += 1) {
        let k = max(1.0 - x * x, 0.0);
        x += h * x * k * k;
    }
    return x;
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
        // twirl right and left: turned about the centre, from how far it had turned to how far it
        // now does, so stamps at one place add up to the whole turn
        case 1, 2: {
            let way = select(1.0, -1.0, mode == 1) * f;
            add = turned(away, lu.move_mode.x * way) - turned(away, lu.move_mode.y * way);
        }
        // pinch: drawn in towards the centre; expand: pushed out; likewise
        case 3, 4: {
            let way = select(-1.0, 1.0, mode == 3) * lu.stamp.w;
            add = away * (traced(d, lu.move_mode.x * way) - traced(d, lu.move_mode.y * way)) / max(d, 1e-4);
        }
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
