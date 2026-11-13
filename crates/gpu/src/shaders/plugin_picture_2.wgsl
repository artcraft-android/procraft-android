// What a plugin's picture function is joined to, interface 2: a picture the app shows in place of
// one of its own (first: the colour panel's Classic square), drawn on the GPU as often as it changes.
// Kept as it is once released, as the filter's template is. The picture's text comes after all of
// this, so nothing it holds can reach back into it. Everything here named host_ is the app's, and
// the text can't touch it (plugin_filter.rs); `hue` and `picture_size` are what it is given, and
// `picture` is what it brings.

struct HostUniform {
    // (the hue 0..1, unused, unused, unused)
    a: vec4<f32>,
    // (the picture's width and height in pixels, unused, unused)
    size: vec4<f32>,
}

@group(0) @binding(0) var<uniform> host_u: HostUniform;

// What this pixel may still spend; -1 once it ran out. Every loop pays from it as it goes round.
var<private> host_budget: i32 = /*BUDGET*/;

@vertex
fn host_vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// The hue the picture is drawn for, 0 to 1 round the wheel from red.
fn hue() -> f32 {
    return host_u.a.x;
}

fn picture_size() -> vec2<f32> {
    return host_u.size.xy;
}

// The picture at pixel `pos`: `picture` is asked at its centre, 0..1 across and down, for a colour
// straight, and it is kept premultiplied. A pixel that ran out of its budget, or came back with what
// isn't a colour, is clear.
@fragment
fn host_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let at = pos.xy / host_u.size.xy;
    let made = picture(at);
    if (host_budget < 0 || !all(abs(made) < vec4<f32>(3.0e38))) {
        return vec4<f32>(0.0);
    }
    let colour = clamp(made, vec4<f32>(0.0), vec4<f32>(1.0));
    return vec4<f32>(colour.rgb * colour.a, colour.a);
}

// A sample spread over the picture that says only whether a pixel ran out: a picture that never
// ends is found here, before it is drawn whole.
@fragment
fn host_probe(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let made = picture(pos.xy / 16.0);
    return vec4<f32>(select(0.0, 1.0, host_budget < 0), made.a * 0.0, 0.0, 1.0);
}
