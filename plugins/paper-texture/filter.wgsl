// Paper behind the layer's paint: a white, warmer as asked, with a grain made from noise at each
// pixel's place (a coarse cloud, fibres stretched across, and each pixel's own tooth). Nothing is
// read but the layer's own pixel, and there is no loop, so a whole layer goes to the GPU at once.

// A number from 0 to 1 for each whole place.
fn paper_hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + vec2<f32>(33.33));
    return fract((r.x + r.y) * r.x);
}

// The hash smoothed between whole places.
fn paper_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = paper_hash(i);
    let b = paper_hash(i + vec2<f32>(1.0, 0.0));
    let c = paper_hash(i + vec2<f32>(0.0, 1.0));
    let d = paper_hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
    let p = at / max(v.scale, 1.0);
    let cloud = paper_noise(p * 0.25);
    let fibre = paper_noise(p * vec2<f32>(1.0, 0.25) + vec2<f32>(17.0, 3.0));
    let tooth = paper_hash(floor(at));
    let grain = 0.45 * cloud + 0.35 * fibre + 0.2 * tooth;
    let white = mix(vec3<f32>(1.0), vec3<f32>(1.0, 0.96, 0.88), v.warmth);
    let paper = white * (1.0 - v.grain * 0.35 * grain);
    // the paint over the paper, a little of the grain showing through it
    let painted = colour.rgb * (1.0 - v.grain * 0.15 * grain);
    return vec4<f32>(mix(paper, painted, colour.a), 1.0);
}
