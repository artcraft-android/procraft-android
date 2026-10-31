// Where a layer is painted, or where it differs from another, without reading all of it back:
// each pixel of the target says whether a stretch of the layer has any alpha (or differs). Across
// rows: target pixel (y, k) for row y's pixels k * STRETCH up to the next stretch; across
// columns: target pixel (x, k) likewise down column x.

const STRETCH: i32 = 512;

@group(0) @binding(0) var layer: texture_2d<f32>;
// what the layer is compared with (the changed passes only)
@group(0) @binding(1) var other: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(vi & 1u) * 4.0 - 1.0;
    let y = f32(vi >> 1u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

@fragment
fn fs_rows(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let y = i32(pos.x);
    let start = i32(pos.y) * STRETCH;
    let end = min(start + STRETCH, i32(textureDimensions(layer).x));
    for (var x = start; x < end; x++) {
        if (textureLoad(layer, vec2<i32>(x, y), 0).a > 0.0) {
            return vec4<f32>(1.0);
        }
    }
    return vec4<f32>(0.0);
}

@fragment
fn fs_columns(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(pos.x);
    let start = i32(pos.y) * STRETCH;
    let end = min(start + STRETCH, i32(textureDimensions(layer).y));
    for (var y = start; y < end; y++) {
        if (textureLoad(layer, vec2<i32>(x, y), 0).a > 0.0) {
            return vec4<f32>(1.0);
        }
    }
    return vec4<f32>(0.0);
}

@fragment
fn fs_changed_rows(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let y = i32(pos.x);
    let start = i32(pos.y) * STRETCH;
    let end = min(start + STRETCH, i32(textureDimensions(layer).x));
    for (var x = start; x < end; x++) {
        let p = vec2<i32>(x, y);
        if (any(textureLoad(layer, p, 0) != textureLoad(other, p, 0))) {
            return vec4<f32>(1.0);
        }
    }
    return vec4<f32>(0.0);
}

@fragment
fn fs_changed_columns(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = i32(pos.x);
    let start = i32(pos.y) * STRETCH;
    let end = min(start + STRETCH, i32(textureDimensions(layer).y));
    for (var y = start; y < end; y++) {
        let p = vec2<i32>(x, y);
        if (any(textureLoad(layer, p, 0) != textureLoad(other, p, 0))) {
            return vec4<f32>(1.0);
        }
    }
    return vec4<f32>(0.0);
}
