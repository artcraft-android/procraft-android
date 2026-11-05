//! What a large canvas does on this device's GPU, to run on a tablet: its limits and memory, then
//! a smudge, wet paint with a grain and both ways a crop takes a layer across, at the sides in
//! `LARGE_SIDES` (comma separated; 2048, 2300 and 3000 if unset), on `LARGE_LAYERS` painted layers
//! (5 if unset). Built with `cargo ndk -t arm64-v8a test -p procraft-gpu --test large_canvas
//! --no-run`, pushed to /data/local/tmp and run with `--ignored --nocapture --test-threads=1`.
//!
//! Keep it well under the device's free memory: a process started from `adb shell` can't be
//! killed and GPU memory is charged to no process, so going past it panics the kernel (2300 with
//! 100 layers did on an 11.5 GB tablet: the crop step takes about three canvases' layers).

#![allow(clippy::unwrap_used)]

use procraft_engine::brush::{GrainMode, Rendering};
use procraft_engine::document::Document;
use procraft_engine::geom::Vec2;
use procraft_engine::stroke::Stamp;
use procraft_gpu::{Canvas, Gpu, StrokeStyle, StrokeTool};

fn number_list(name: &str, default: &[u32]) -> Vec<u32> {
    std::env::var(name).map_or_else(|_| default.to_vec(), |v| v.split(',').filter_map(|n| n.parse().ok()).collect())
}

fn memory(gpu: &Gpu) -> String {
    gpu.flush();
    gpu.device.generate_allocator_report().map_or_else(
        || "no report".to_owned(),
        |r| format!("{} MB in use, {} MB held", r.total_allocated_bytes >> 20, r.total_reserved_bytes >> 20),
    )
}

/// A canvas `side` square with a dot on each of `layers` layers, and the one on top.
fn painted(gpu: &Gpu, side: u32, layers: u32) -> (Document, Canvas) {
    let mut doc = Document::new(side, side);
    let mut canvas = Canvas::new(gpu, &doc).unwrap();
    let middle = side as f32 / 2.0;
    for k in 0..layers {
        if k > 0 {
            doc.add_layer();
        }
        canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.1, 0.2, 0.3], 1.0));
        canvas.add_stamps(&[Stamp::round(Vec2::new(middle, middle), 300.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(gpu, &mut doc, None);
    }
    (doc, canvas)
}

#[test]
#[ignore = "for a device: see the top of this file"]
fn a_large_canvas_smudges_and_crops_with_no_gpu_error() {
    let Ok(gpu) = Gpu::headless() else { return };
    let (limits, info) = (gpu.device.limits(), gpu.adapter.get_info());
    println!(
        "{} ({:?}, driver {}): storage binding {}, buffer {}, texture side {}",
        info.name,
        info.backend,
        info.driver_info,
        limits.max_storage_buffer_binding_size,
        limits.max_buffer_size,
        limits.max_texture_dimension_2d
    );
    let layers = number_list("LARGE_LAYERS", &[5]).first().copied().unwrap_or(5);
    let wet = StrokeStyle {
        wet_pull: Some(0.5),
        load: 0.5,
        grain: Some(0),
        grain_mode: GrainMode::Texturized,
        grain_depth: 1.0,
        rendering: Rendering::Gradual,
        ..StrokeStyle::new(StrokeTool::Paint, [0.2, 0.4, 0.8], 1.0)
    };
    let smudge = StrokeStyle::new(StrokeTool::Smudge, [0.0; 3], 1.0);
    for side in number_list("LARGE_SIDES", &[2048, 2300, 3000]) {
        let row = u64::from((side * 4).div_ceil(256) * 256);
        let layer_bytes = row * u64::from(side);
        // what each binds at most, as a stroke's own buffers
        for (name, style, bound) in [("smudge", smudge, layer_bytes), ("wet paint", wet, 2 * layer_bytes)] {
            let (mut doc, mut canvas) = painted(&gpu, side, layers);
            let id = doc.current();
            let before = canvas.read_layer(&gpu, id).unwrap();
            let middle = side as f32 / 2.0;
            let stamps: Vec<Stamp> =
                (0..40).map(|k| Stamp::round(Vec2::new(middle + k as f32 * 8.0, middle), 120.0, 1.0)).collect();
            canvas.begin_stroke(id, style);
            canvas.add_stamps(&stamps);
            canvas.render(&gpu, &mut doc, None);
            let during = memory(&gpu);
            canvas.end_stroke();
            canvas.render(&gpu, &mut doc, None);
            let refused = canvas.take_smudge_refused();
            let changed = canvas.read_layer(&gpu, id).unwrap() != before;
            println!("{side} {name}: refused {refused}, changed {changed}, {during}");
            assert_eq!(gpu.errors(), 0, "{side} {name}");
            assert!(!gpu.is_lost(), "{side} {name}");
            assert_eq!(refused, bound > limits.max_storage_buffer_binding_size, "{side} {name}");
            assert_eq!(changed, !refused, "{side} {name}");
        }
        let (doc, canvas) = painted(&gpu, side, layers);
        let id = doc.current();
        let painted_in = |c: &Canvas| {
            let pixels = c.read_layer(&gpu, id).unwrap();
            pixels.as_chunks::<4>().0.iter().filter(|p| p[3] != 0).count()
        };
        let smaller = doc.resized(side - 300, side - 200);
        let mut moved = Canvas::new(&gpu, &smaller).unwrap();
        assert!(moved.copy_layer_from(&gpu, &canvas, id, (150, 100)));
        let mut turned = Canvas::new(&gpu, &smaller).unwrap();
        assert!(turned.resample_layer_from(&gpu, &canvas, id, [0.9, 0.1, -0.1, 0.9, 40.0, 40.0]));
        let (by_copy, by_drawing) = (painted_in(&moved), painted_in(&turned));
        println!("{side} crop: {by_copy} and {by_drawing} pixels across, {}", memory(&gpu));
        assert_eq!(gpu.errors(), 0, "{side} crop");
        assert!(!gpu.is_lost(), "{side} crop");
        assert!(by_copy > 0 && by_drawing > 0, "{side} crop");
    }
}
