//! Colour triangle: the colour panel's Classic tab picks saturation and brightness in a triangle,
//! between the pure hue, white and black, instead of a square. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! On the picker hook: its picture is `picture.wgsl`, which the app draws on the GPU for each hue,
//! and this says where a touch lands (the saturation and brightness at a point) and where the knob
//! sits. Points are 0..1 across and down the square.

// no std inside the app; the computer's own for its tests
#![cfg_attr(target_arch = "wasm32", no_std)]

/// Its height across the square: an equilateral triangle of side 1.
const HEIGHT: f32 = 0.866_025_4;

/// The corners: the pure hue at the right, white at the top left, black at the bottom left, the
/// triangle centred across the square.
fn corners() -> [[f32; 2]; 3] {
    let left = (1.0 - HEIGHT) / 2.0;
    [[left + HEIGHT, 0.5], [left, 0.0], [left, 1.0]]
}

/// How much of the pure hue, white and black make (x, y): they add up to 1, and one below 0 is
/// outside.
fn shares(x: f32, y: f32) -> [f32; 3] {
    let [p, w, k] = corners();
    let det = (w[1] - k[1]) * (p[0] - k[0]) + (k[0] - w[0]) * (p[1] - k[1]);
    let a = ((w[1] - k[1]) * (x - k[0]) + (k[0] - w[0]) * (y - k[1])) / det;
    let b = ((k[1] - p[1]) * (x - k[0]) + (p[0] - k[0]) * (y - k[1])) / det;
    [a, b, 1.0 - a - b]
}

/// The saturation and brightness at (x, y); a point outside is taken at the edge.
fn pick(x: f32, y: f32) -> (f32, f32) {
    let parts = shares(x, y).map(|part| part.max(0.0));
    let sum: f32 = parts.iter().sum();
    let [a, b, _] = if sum > 0.0 { parts.map(|part| part / sum) } else { [0.0, 0.0, 1.0] };
    let v = (a + b).clamp(0.0, 1.0);
    (if v > 1e-6 { (a / v).clamp(0.0, 1.0) } else { 0.0 }, v)
}

/// Where saturation `s` and brightness `v` sit.
fn point(s: f32, v: f32) -> (f32, f32) {
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let [p, w, k] = corners();
    let (a, b, c) = (v * s, v * (1.0 - s), 1.0 - v);
    (a * p[0] + b * w[0] + c * k[0], a * p[1] + b * w[1] + c * k[1])
}

kit::picker!(pick, point);

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_comes_back_where_it_sits() {
        for (s, v) in [(1.0, 1.0), (0.0, 1.0), (0.5, 0.5), (0.25, 0.8), (0.9, 0.1)] {
            let (x, y) = point(s, v);
            let (bs, bv) = pick(x, y);
            assert!((bs - s).abs() < 1e-3 && (bv - v).abs() < 1e-3, "({s}, {v}) came back ({bs}, {bv})");
        }
        // the corners, and past an edge at the edge
        let [p, w, _] = corners();
        assert_eq!(pick(p[0], p[1]), (1.0, 1.0));
        assert_eq!(pick(w[0], w[1]), (0.0, 1.0));
        assert_eq!(pick(1.0, 0.5), (1.0, 1.0));
    }
}
