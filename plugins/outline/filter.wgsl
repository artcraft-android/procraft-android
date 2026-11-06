// A pixel is on the line if what is near it is more solid than it is. A loop and the pixels around
// it: the app draws this in bands, and each round of the loop spends some of the pixel's budget.
fn filter(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
    let reach = i32(v.reach);
    var near = 0.0;
    for (var y = -reach; y <= reach; y += 1) {
        for (var x = -reach; x <= reach; x += 1) {
            let step = vec2<f32>(f32(x), f32(y));
            if (length(step) <= v.reach) {
                near = max(near, pixel(at + step).a);
            }
        }
    }
    return vec4<f32>(0.0, 0.0, 0.0, max(near - colour.a, 0.0));
}
