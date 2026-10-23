// A model's albedo and its smaller levels: the albedo copied into the first level, then each
// level the average of the four texels under each of its own (premultiplied, so a plain average).

@group(0) @binding(0) var source: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(vi & 1u) * 4.0 - 1.0;
    let y = f32(vi >> 1u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_copy(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(pos.xy), 0);
}

@fragment
fn fs_halve(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy) * 2;
    let last = vec2<i32>(textureDimensions(source)) - 1;
    let a = textureLoad(source, min(p, last), 0);
    let b = textureLoad(source, min(p + vec2<i32>(1, 0), last), 0);
    let c = textureLoad(source, min(p + vec2<i32>(0, 1), last), 0);
    let d = textureLoad(source, min(p + vec2<i32>(1, 1), last), 0);
    return (a + b + c + d) * 0.25;
}
