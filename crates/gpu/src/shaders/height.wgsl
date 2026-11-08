// Merges a stroke's paint height into the layer's when the stroke ends: at each pixel the layer
// keeps whichever height is taller, so paint laid over old impasto builds on its ridges instead of
// flattening them.

@group(0) @binding(0) var layer_height: texture_2d<f32>;
@group(0) @binding(1) var stroke_height: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// the height a premultiplied pixel holds, and its coverage
fn height_of(c: vec4<f32>) -> f32 {
    return c.r / max(c.a, 1e-4);
}

@fragment
fn fs_merge(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy);
    let a = textureLoad(layer_height, p, 0);
    let b = textureLoad(stroke_height, p, 0);
    // the paint the stroke dragged from behind it: its ridges follow the stroke a little way
    let dims = vec2<i32>(textureDimensions(layer_height));
    let drag = vec2<i32>(round(b.gb / max(b.a, 1e-3)));
    let behind = textureLoad(layer_height, clamp(p - drag, vec2<i32>(0), dims - 1), 0);
    let dragged = behind * 0.8;
    let top = select(a, b, height_of(b) > height_of(a));
    return select(top, dragged, height_of(dragged) > height_of(top));
}
