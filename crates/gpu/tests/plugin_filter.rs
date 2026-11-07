//! Plugin filters on a real GPU (skipped where the machine has none). Those that throw a filter
//! made to run for ever or to cost all it may are ignored unless asked for: the app's guards are
//! what stops them, and a guard broken here would stop a whole machine's graphics driver.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use procraft_engine::document::Document;
use procraft_engine::geom::Vec2;
use procraft_engine::stroke::Stamp;
use procraft_gpu::{Canvas, FilterProgram, FilterRun, Gpu, StrokeStyle, StrokeTool, read_filter};

fn gpu() -> Option<Gpu> {
    match Gpu::headless() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipped: {e}");
            None
        }
    }
}

fn program(gpu: &Gpu, text: &str, keys: &[&str]) -> Arc<FilterProgram> {
    let keys: Vec<String> = keys.iter().map(|k| (*k).to_owned()).collect();
    gpu.make_filter(read_filter(text, &keys).unwrap()).unwrap()
}

/// A canvas of `size` with one black dot on the layer, whose id comes with it.
fn with_a_dot(gpu: &Gpu, size: u32) -> (Canvas, Document) {
    let mut doc = Document::new(size, size);
    let mut canvas = Canvas::new(gpu, &doc).unwrap();
    canvas.render(gpu, &mut doc, None);
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0));
    let middle = size as f32 / 2.0;
    canvas.add_stamps(&[Stamp::round(Vec2::new(middle, middle), middle / 2.0, 1.0)]);
    canvas.end_stroke();
    canvas.render(gpu, &mut doc, None);
    (canvas, doc)
}

fn layer(canvas: &mut Canvas, gpu: &Gpu, doc: &Document) -> Vec<u8> {
    canvas.read_layer(gpu, doc.current()).unwrap()
}

fn at(px: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * size + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

fn drew(run: Option<FilterRun>) -> Duration {
    match run {
        Some(FilterRun::Drew(took)) => took,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_filter_redraws_the_layer_from_its_copy() {
    let Some(gpu) = gpu() else { return };
    let (mut canvas, mut doc) = with_a_dot(&gpu, 64);
    let before = layer(&mut canvas, &gpu, &doc);
    assert_eq!(at(&before, 64, 32, 32), [0, 0, 0, 255]);
    // red in place of black, as far as the value says; each draw starts from the copy
    let red = program(
        &gpu,
        "fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
            return vec4<f32>(v.amount, colour.g, colour.b, colour.a);
        }",
        &["amount"],
    );
    canvas.begin_transform(doc.current());
    canvas.set_plugin_filter(&red, &[1.0], false);
    canvas.render(&gpu, &mut doc, None);
    drew(canvas.take_filter_run());
    assert_eq!(at(&layer(&mut canvas, &gpu, &doc), 64, 32, 32), [255, 0, 0, 255]);
    canvas.set_plugin_filter(&red, &[0.5], false);
    canvas.render(&gpu, &mut doc, None);
    assert!(at(&layer(&mut canvas, &gpu, &doc), 64, 32, 32)[0].abs_diff(128) <= 1);
    // Cancel: as it was
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(layer(&mut canvas, &gpu, &doc), before);
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_filter_is_kept_to_the_alpha_lock_and_sees_its_neighbours() {
    let Some(gpu) = gpu() else { return };
    let (mut canvas, mut doc) = with_a_dot(&gpu, 64);
    let clears = program(
        &gpu,
        "fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> { return vec4<f32>(1.0, 0.0, 0.0, 0.5); }",
        &[],
    );
    canvas.begin_transform(doc.current());
    canvas.set_plugin_filter(&clears, &[], false);
    canvas.render(&gpu, &mut doc, None);
    // straight in, premultiplied in the layer
    let [r, g, b, a] = at(&layer(&mut canvas, &gpu, &doc), 64, 32, 32);
    assert!((a as i32 - 128).abs() <= 1 && (r as i32 - 128).abs() <= 1 && g == 0 && b == 0, "{r} {g} {b} {a}");
    // the alpha locked: what was clear stays clear, and the dot stays whole
    canvas.set_plugin_filter(&clears, &[], true);
    canvas.render(&gpu, &mut doc, None);
    let locked = layer(&mut canvas, &gpu, &doc);
    assert_eq!(at(&locked, 64, 32, 32), [255, 0, 0, 255]);
    assert_eq!(at(&locked, 64, 1, 1)[3], 0);
    // the pixel to the left, past the canvas's edge the edge's own
    let left = program(
        &gpu,
        "fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> { return layer_at(at - vec2<f32>(canvas_size().x / 2.0, 0.0)); }",
        &[],
    );
    canvas.set_plugin_filter(&left, &[], false);
    canvas.render(&gpu, &mut doc, None);
    let moved = layer(&mut canvas, &gpu, &doc);
    assert_eq!(at(&moved, 64, 32, 32)[3], 0);
    assert_eq!(at(&moved, 64, 60, 32), [0, 0, 0, 255]);
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_filter_is_kept_to_the_selection() {
    let Some(gpu) = gpu() else { return };
    let (mut canvas, mut doc) = with_a_dot(&gpu, 64);
    let red = program(
        &gpu,
        "fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> { return vec4<f32>(1.0, 0.0, 0.0, 1.0); }",
        &[],
    );
    // the left half chosen: the right half never knows
    let coverage: Vec<u8> = (0..64 * 64).map(|i| if i % 64 < 32 { 255 } else { 0 }).collect();
    canvas.set_selection(&gpu, Some(&coverage));
    let before = layer(&mut canvas, &gpu, &doc);
    canvas.begin_transform(doc.current());
    canvas.set_plugin_filter(&red, &[], false);
    canvas.render(&gpu, &mut doc, None);
    drew(canvas.take_filter_run());
    let px = layer(&mut canvas, &gpu, &doc);
    assert_eq!(at(&px, 64, 10, 32), [255, 0, 0, 255]);
    assert_eq!(at(&px, 64, 40, 32), at(&before, 64, 40, 32));
    assert_eq!(at(&px, 64, 60, 2), at(&before, 64, 60, 2));
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(gpu.errors(), 0);
}

#[test]
fn a_filter_with_a_loop_goes_in_pieces_and_comes_out_whole() {
    let Some(gpu) = gpu() else { return };
    let size = 700;
    let (mut canvas, mut doc) = with_a_dot(&gpu, size);
    // ten rounds of nothing much: a loop, so the pieces are small and the probe looks first
    let code = "fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
        var total = 0.0;
        for (var i = 0; i < 10; i += 1) { total += 0.1; }
        return vec4<f32>(total, 0.0, 0.0, 1.0);
    }";
    let filter = program(&gpu, code, &[]);
    canvas.begin_transform(doc.current());
    canvas.set_plugin_filter(&filter, &[], false);
    canvas.render(&gpu, &mut doc, None);
    drew(canvas.take_filter_run());
    let px = layer(&mut canvas, &gpu, &doc);
    for y in (0..size).step_by(13).chain([size - 1]) {
        for x in (0..size).step_by(29).chain([size - 1]) {
            assert_eq!(at(&px, size, x, y), [255, 0, 0, 255], "{x} {y}");
        }
    }
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(gpu.errors(), 0);
}

/// What a sample with no loop costs a large layer: run on a device with `--ignored --nocapture`.
#[test]
#[ignore]
fn extract_lines_follows_a_finger() {
    let Some(gpu) = gpu() else { return };
    let (mut canvas, mut doc) = with_a_dot(&gpu, 2048);
    let code = include_str!("../../../plugins/extract-lines/filter.wgsl");
    let filter = program(&gpu, code, &["cut", "soft", "keep_colour"]);
    canvas.begin_transform(doc.current());
    for round in 0..4 {
        canvas.set_plugin_filter(&filter, &[0.15 + round as f32 * 0.1, 0.1, 0.0], false);
        canvas.render(&gpu, &mut doc, None);
        let took = drew(canvas.take_filter_run());
        println!("extract lines, 2048 px, round {round}: {took:?}");
    }
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
}

/// Only on a machine whose graphics driver may be stopped by a broken guard.
#[test]
#[ignore]
fn a_filter_that_never_ends_is_stopped_before_it_meets_the_layer() {
    let Some(gpu) = gpu() else { return };
    let (mut canvas, mut doc) = with_a_dot(&gpu, 2048);
    let before = layer(&mut canvas, &gpu, &doc);
    let forever = program(
        &gpu,
        "fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
            var x = colour.r + at.x;
            loop { x = fract(x * 1.37 + sin(x * 3.1) * 0.5); if (x > 2.0) { break; } }
            return vec4<f32>(x);
        }",
        &[],
    );
    canvas.begin_transform(doc.current());
    let started = Instant::now();
    canvas.set_plugin_filter(&forever, &[], false);
    canvas.render(&gpu, &mut doc, None);
    let took = started.elapsed();
    let run = canvas.take_filter_run();
    println!("an endless filter: {run:?} after {took:?}");
    assert_eq!(run, Some(FilterRun::Spent));
    assert!(took < Duration::from_secs(1), "{took:?}");
    assert_eq!(layer(&mut canvas, &gpu, &doc), before);
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
    assert!(!gpu.is_lost());
}

/// Only on a machine whose graphics driver may be stopped by a broken guard.
#[test]
#[ignore]
fn a_filter_costing_all_it_may_never_holds_the_gpu_for_long() {
    let Some(gpu) = gpu() else { return };
    let (mut canvas, mut doc) = with_a_dot(&gpu, 2048);
    // just under the budget on every pixel, so the probe lets it by
    let heavy = program(
        &gpu,
        "fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
            var x = colour.r + at.x * 0.001;
            for (var i = 0; i < 2500; i += 1) { x = fract(x * 1.37 + sin(x * 3.1) * 0.5); }
            return vec4<f32>(x, x, x, 1.0);
        }",
        &[],
    );
    canvas.begin_transform(doc.current());
    for round in 0..3 {
        let started = Instant::now();
        canvas.set_plugin_filter(&heavy, &[], false);
        canvas.render(&gpu, &mut doc, None);
        let run = canvas.take_filter_run();
        println!("a filter at its budget, round {round}: {run:?} after {:?}", started.elapsed());
        drew(run);
    }
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
    assert!(!gpu.is_lost());
}
