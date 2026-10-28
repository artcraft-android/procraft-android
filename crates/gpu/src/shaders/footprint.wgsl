// Which tiles of a texture a stroke reaches: a tile is marked when any texel in it holds
// anything. Each group of 8 x 8 texels lies within one tile (tiles are a multiple of 8 across and
// the groups start on a multiple of 8), so it marks its tile once at most.

struct Region {
    // the first texel looked at (a multiple of 8 each way), and how many from there
    origin: vec2<u32>,
    size: vec2<u32>,
    // a tile's side (texels), and how many tiles across the texture
    tile: u32,
    across: u32,
    pad: vec2<u32>,
}

@group(0) @binding(0) var<uniform> region: Region;
@group(0) @binding(1) var stroke: texture_2d<f32>;
@group(0) @binding(2) var<storage, read_write> marks: array<atomic<u32>>;

var<workgroup> reached: atomic<u32>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32) {
    if (local == 0u) {
        atomicStore(&reached, 0u);
    }
    workgroupBarrier();
    let p = region.origin + id.xy;
    if (all(id.xy < region.size) && any(textureLoad(stroke, vec2<i32>(p), 0) != vec4<f32>(0.0))) {
        atomicOr(&reached, 1u);
    }
    workgroupBarrier();
    if (local == 0u && atomicLoad(&reached) != 0u) {
        let t = (p.y / region.tile) * region.across + p.x / region.tile;
        atomicOr(&marks[t / 32u], 1u << (t % 32u));
    }
}
