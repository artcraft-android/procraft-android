// A layer of the canvas from before a crop seen through it: each new pixel's middle mapped onto
// the old canvas (`m`), bilinear between the four old pixels around it, clear past the old edges;
// worked in steps of 0..255 and rounded as the CPU's crop does.

struct Crop {
    // x' = a x + c y + e, y' = b x + d y + f
    abcd: vec4<f32>,
    ef: vec4<f32>,
}

@group(0) @binding(0) var old: texture_2d<f32>;
@group(0) @binding(1) var<uniform> m: Crop;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(vi & 1u) * 4.0 - 1.0;
    let y = f32(vi >> 1u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn tap(x: i32, y: i32) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(old));
    if (x < 0 || y < 0 || x >= size.x || y >= size.y) {
        return vec4<f32>(0.0);
    }
    return floor(textureLoad(old, vec2<i32>(x, y), 0) * 255.0 + 0.5);
}

@fragment
fn fs_crop(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<f32>(m.abcd.x * pos.x + m.abcd.z * pos.y + m.ef.x, m.abcd.y * pos.x + m.abcd.w * pos.y + m.ef.y);
    let f = p - vec2<f32>(0.5);
    let o = floor(f);
    let t = f - o;
    let i = vec2<i32>(o);
    let a = tap(i.x, i.y);
    let b = tap(i.x + 1, i.y);
    let c = tap(i.x, i.y + 1);
    let d = tap(i.x + 1, i.y + 1);
    let top = a + (b - a) * t.x;
    let bottom = c + (d - c) * t.x;
    // halves away from zero, as the CPU rounds (WGSL's round goes to even)
    return clamp(floor(top + (bottom - top) * t.y + 0.5), vec4<f32>(0.0), vec4<f32>(255.0)) / 255.0;
}
