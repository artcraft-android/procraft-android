
// A capped stamp (the uniformed glaze): its paint scaled by its transfer over what is there, kept
// by what the stamp leaves, so the stroke builds up towards the transfer and never past it.
// Appended to stamp.wgsl in a module that enables dual-source blending.

struct CappedOut {
    @location(0) @blend_src(0) paint: vec4<f32>,
    @location(0) @blend_src(1) keep: vec4<f32>,
}

@fragment
fn fs_stamp_capped(i: StampOut) -> CappedOut {
    let s = stamp_paint(i);
    var o: CappedOut;
    o.paint = s * clamp(i.transfer, 0.0, 1.0);
    o.keep = vec4<f32>(s.a);
    return o;
}
