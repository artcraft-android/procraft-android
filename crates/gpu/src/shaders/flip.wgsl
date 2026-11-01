// A layer mirrored: each pixel taken from its mirror across the middle, left to right or top to
// bottom; or inverted: each premultiplied colour taken from its coverage.

@group(0) @binding(0) var layer: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(vi & 1u) * 4.0 - 1.0;
    let y = f32(vi >> 1u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_across(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(layer));
    let p = vec2<i32>(pos.xy);
    return textureLoad(layer, vec2<i32>(size.x - 1 - p.x, p.y), 0);
}

@fragment
fn fs_invert(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(layer, vec2<i32>(pos.xy), 0);
    return vec4<f32>(max(vec3<f32>(c.a) - c.rgb, vec3<f32>(0.0)), c.a);
}

@fragment
fn fs_down(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(layer));
    let p = vec2<i32>(pos.xy);
    return textureLoad(layer, vec2<i32>(p.x, size.y - 1 - p.y), 0);
}
