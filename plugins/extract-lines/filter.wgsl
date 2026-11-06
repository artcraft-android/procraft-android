// Brightness to alpha: how dark a pixel is becomes how much of it stays.
fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
    let dark = 1.0 - dot(colour.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let soft = max(v.soft, 0.001);
    let ink = smoothstep(v.cut - soft, v.cut + soft, dark);
    let shade = select(vec3<f32>(0.0), colour.rgb, v.keep_colour > 0.5);
    return vec4<f32>(shade, ink * colour.a);
}
