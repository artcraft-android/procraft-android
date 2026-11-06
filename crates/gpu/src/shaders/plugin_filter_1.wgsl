// What a plugin filter's one function is joined to, interface 1. Kept as it is once released: a new
// interface gets a file of its own, and filters written to this one go on being built with this.
// `/*VALUES*/` becomes the fields of `HostValues`, `/*READ*/` the values from the uniform; the
// filter's text comes after all of this, so nothing it holds can reach back into it. Everything
// here named host_ is the app's, and a filter can't touch it (plugin_filter.rs); `v`, `layer_at` and
// `canvas_size` are what it is given, and `pixel` is what it brings.

struct HostUniform {
    // the controls' values, in the manifest's order
    a: vec4<f32>,
    b: vec4<f32>,
    // (the canvas's width, height, 1 with a selection, 1 with the alpha locked)
    size: vec4<f32>,
    // (the probe's width and height, unused, unused)
    c: vec4<f32>,
}

@group(0) @binding(0) var host_copy: texture_2d<f32>;
@group(0) @binding(2) var host_chosen: texture_2d<f32>;
@group(1) @binding(0) var<uniform> host_u: HostUniform;

struct HostValues { /*VALUES*/ }
var<private> v: HostValues;

// What this pixel may still spend; -1 once it ran out. Every loop pays from it as it goes round.
var<private> host_budget: i32 = /*BUDGET*/;

@vertex
fn host_vs(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn host_straight(c: vec4<f32>) -> vec4<f32> {
    if (c.a <= 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(c.rgb / c.a, c.a);
}

// The layer as it was at `at`, in canvas pixels, colour straight: past an edge, the edge's.
fn layer_at(at: vec2<f32>) -> vec4<f32> {
    let most = vec2<i32>(host_u.size.xy) - vec2<i32>(1);
    let i = clamp(vec2<i32>(floor(at)), vec2<i32>(0), most);
    return host_straight(textureLoad(host_copy, i, 0));
}

fn canvas_size() -> vec2<f32> {
    return host_u.size.xy;
}

// What pixel i becomes, premultiplied. A pixel that ran out of its budget, or came back with what
// isn't a colour, stays as it was; the alpha stays where it is locked, and the selection takes
// only what it holds.
fn host_filtered(i: vec2<i32>) -> vec4<f32> {
    v = HostValues(/*READ*/);
    let was = textureLoad(host_copy, i, 0);
    let made = pixel(host_straight(was), vec2<f32>(i) + vec2<f32>(0.5));
    if (host_budget < 0 || !all(abs(made) < vec4<f32>(3.0e38))) {
        return was;
    }
    let colour = clamp(made, vec4<f32>(0.0), vec4<f32>(1.0));
    let alpha = select(colour.a, was.a, host_u.size.w > 0.5);
    var taken = 1.0;
    if (host_u.size.z > 0.5) {
        taken = textureLoad(host_chosen, i, 0).r;
    }
    return mix(was, vec4<f32>(colour.rgb * alpha, alpha), taken);
}

@fragment
fn host_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return host_filtered(vec2<i32>(pos.xy));
}

// A sample of the layer, spread over it, that says only whether a pixel ran out: a filter that
// never ends is found here, at the cost of a few hundred pixels, before it meets the layer.
@fragment
fn host_probe(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let i = vec2<i32>(pos.xy * host_u.size.xy / host_u.c.xy);
    let made = host_filtered(i);
    return vec4<f32>(select(0.0, 1.0, host_budget < 0), made.a * 0.0, 0.0, 1.0);
}

