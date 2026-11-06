// Each channel rounded to one of a few steps. No loop, so a whole layer goes to the GPU at once.
fn filter(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
    let top = v.steps - 1.0;
    return vec4<f32>(round(colour.rgb * top) / top, colour.a);
}
