// The colour triangle's picture: the pure hue at the right corner, white at the top left and black
// at the bottom left, the triangle centred across the square, with a pixel's soft edge round it.
// The app draws it on the GPU for each hue; where touches land is the plugin's code (src/lib.rs).

// Its height across the square: an equilateral triangle of side 1.
const TRIANGLE: f32 = 0.8660254;

// How much of the pure hue, white and black make point `p`: they add up to 1, and one below 0 is
// outside.
fn shares(p: vec2<f32>) -> vec3<f32> {
    let left = (1.0 - TRIANGLE) / 2.0;
    let pure = vec2<f32>(left + TRIANGLE, 0.5);
    let white = vec2<f32>(left, 0.0);
    let black = vec2<f32>(left, 1.0);
    let det = (white.y - black.y) * (pure.x - black.x) + (black.x - white.x) * (pure.y - black.y);
    let first = ((white.y - black.y) * (p.x - black.x) + (black.x - white.x) * (p.y - black.y)) / det;
    let second = ((black.y - pure.y) * (p.x - black.x) + (pure.x - black.x) * (p.y - black.y)) / det;
    return vec3<f32>(first, second, 1.0 - first - second);
}

// Hue `h`, saturation `s` and brightness `v` (each 0..1) as red, green and blue.
fn hsv_rgb(h: f32, s: f32, v: f32) -> vec3<f32> {
    let n = (vec3<f32>(5.0, 3.0, 1.0) + vec3<f32>(fract(h) * 6.0)) % vec3<f32>(6.0);
    let k = clamp(min(n, vec3<f32>(4.0) - n), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec3<f32>(v) - v * s * k;
}

fn picture(p: vec2<f32>) -> vec4<f32> {
    let w = shares(p);
    // how far in from the nearest side, in pixels
    let inside = min(w.x, min(w.y, w.z)) * TRIANGLE * picture_size().x;
    let edge = clamp(inside + 0.5, 0.0, 1.0);
    let c = max(w, vec3<f32>(0.0));
    let v = clamp(c.x + c.y, 0.0, 1.0);
    var s = 0.0;
    if (v > 0.000001) {
        s = clamp(c.x / v, 0.0, 1.0);
    }
    return vec4<f32>(hsv_rgb(hue(), s, v), edge);
}
