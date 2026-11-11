//! The canvas on a real GPU (skipped where the machine has none).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use procraft_engine::brush::{Brush, Rendering, Shape, Source, Taper};
use procraft_engine::document::Document;
use procraft_engine::geom::{Homography, Vec2};
use procraft_engine::input::{Pointer, Sample};
use procraft_engine::profile::Profile;
use procraft_engine::stroke::{Stamp, StrokeBuilder, Tint};
use procraft_engine::textures::TEXTURE_SIZE;
use procraft_engine::view::View;
use procraft_gpu::{Backdrop, Canvas, Gpu, Resample, StrokeStyle, StrokeTool, SurroundGrid, Target, wgpu};

fn gpu() -> Option<Gpu> {
    match Gpu::headless() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipped: {e}");
            None
        }
    }
}

/// Fails the test if the GPU reported validation errors by its end.
struct NoErrors<'a>(&'a Gpu);

impl Drop for NoErrors<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert_eq!(self.0.errors(), 0, "GPU validation errors");
        }
    }
}

fn style(tool: StrokeTool, opacity: f32) -> StrokeStyle {
    StrokeStyle::new(tool, [0.0; 3], opacity)
}

fn pixel(px: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

fn dot(at: Vec2, radius: f32) -> Stamp {
    Stamp::round(at, radius, 1.0)
}

#[test]
fn paint_undo_redo() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 64, 32, 32), [255, 255, 255, 255]);

    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(32.0, 32.0), 8.0)]);
    canvas.render(&gpu, &mut doc, None);
    // drawn live, before the stroke ends
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 64, 32, 32), [0, 0, 0, 255]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_composite(&gpu).unwrap();
    assert_eq!(pixel(&px, 64, 32, 32), [0, 0, 0, 255]);
    assert_eq!(pixel(&px, 64, 2, 2), [255, 255, 255, 255]);
    assert_eq!(pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 64, 32, 32), [0, 0, 0, 255]);
    assert!(canvas.can_undo());

    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 64, 32, 32), [255, 255, 255, 255]);
    assert!(canvas.can_redo());
    canvas.redo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 64, 32, 32), [0, 0, 0, 255]);
}

#[test]
fn steps_run_together_over_one_another_undo_and_redo_whole() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(96, 96);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    let stroke = |canvas: &mut Canvas, colour: [f32; 3], at: Vec2, radius: f32| {
        canvas.begin_stroke(layer, StrokeStyle::new(StrokeTool::Paint, colour, 1.0));
        canvas.add_stamps(&[dot(at, radius)]);
        canvas.end_stroke();
    };
    stroke(&mut canvas, [0.2, 0.2, 0.9], Vec2::new(30.0, 30.0), 20.0);
    canvas.render(&gpu, &mut doc, None);
    let before = canvas.read_layer(&gpu, layer).unwrap();
    // three strokes, each over the last, made one step
    canvas.mark_steps();
    stroke(&mut canvas, [0.9, 0.1, 0.1], Vec2::new(40.0, 40.0), 22.0);
    stroke(&mut canvas, [0.1, 0.8, 0.2], Vec2::new(50.0, 45.0), 18.0);
    stroke(&mut canvas, [0.9, 0.9, 0.1], Vec2::new(45.0, 55.0), 25.0);
    canvas.collapse_steps();
    canvas.render(&gpu, &mut doc, None);
    let after = canvas.read_layer(&gpu, layer).unwrap();
    assert!(after != before);
    // undone and redone twice: each time all three or none, every pixel as it was
    for _ in 0..2 {
        canvas.undo();
        canvas.render(&gpu, &mut doc, None);
        assert!(canvas.read_layer(&gpu, layer).unwrap() == before, "undone");
        canvas.redo();
        canvas.render(&gpu, &mut doc, None);
        assert!(canvas.read_layer(&gpu, layer).unwrap() == after, "redone");
    }
    // and the first stroke undone after them
    canvas.undo();
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert!(canvas.read_layer(&gpu, layer).unwrap().iter().all(|&v| v == 0));
}

#[test]
fn a_whole_layer_kept_in_a_step_let_go_undoes_as_it_should() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(80, 80);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    let stroke = |canvas: &mut Canvas, colour: [f32; 3], at: Vec2| {
        canvas.begin_stroke(layer, StrokeStyle::new(StrokeTool::Paint, colour, 1.0));
        canvas.add_stamps(&[dot(at, 15.0)]);
        canvas.end_stroke();
    };
    stroke(&mut canvas, [0.9, 0.1, 0.1], Vec2::new(25.0, 25.0));
    canvas.render(&gpu, &mut doc, None);
    let red = canvas.read_layer(&gpu, layer).unwrap();
    // cleared and undone: the redo keeps the whole layer
    canvas.clear_layer(layer);
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert!(canvas.read_layer(&gpu, layer).unwrap() == red);
    // another stroke drops that redo; cleared again, the whole layer kept in its texture
    stroke(&mut canvas, [0.1, 0.2, 0.9], Vec2::new(55.0, 50.0));
    canvas.render(&gpu, &mut doc, None);
    let both = canvas.read_layer(&gpu, layer).unwrap();
    canvas.clear_layer(layer);
    canvas.render(&gpu, &mut doc, None);
    assert!(canvas.read_layer(&gpu, layer).unwrap().iter().all(|&v| v == 0));
    for _ in 0..2 {
        canvas.undo();
        canvas.render(&gpu, &mut doc, None);
        assert!(canvas.read_layer(&gpu, layer).unwrap() == both, "undone");
        canvas.redo();
        canvas.render(&gpu, &mut doc, None);
        assert!(canvas.read_layer(&gpu, layer).unwrap().iter().all(|&v| v == 0), "redone");
    }
    canvas.undo();
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert!(canvas.read_layer(&gpu, layer).unwrap() == red);
}

#[test]
fn stroke_opacity_caps_build_up() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 0.5));
    // forty overlapping stamps: the stroke still only reaches its opacity
    let stamps: Vec<Stamp> = (0..40).map(|i| dot(Vec2::new(30.0 + i as f32 * 0.1, 32.0), 10.0)).collect();
    canvas.add_stamps(&stamps);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let [r, g, b, a] = pixel(&canvas.read_composite(&gpu).unwrap(), 64, 32, 32);
    assert!((126..=129).contains(&r) && r == g && g == b && a == 255, "got {r} {g} {b} {a}");
}

#[test]
fn erase_and_cancel() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(32.0, 32.0), 12.0)]);
    canvas.end_stroke();
    canvas.begin_stroke(doc.current(), style(StrokeTool::Erase, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(32.0, 32.0), 4.0)]);
    canvas.render(&gpu, &mut doc, None);
    // the eraser shows while it is drawn
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 64, 32, 32), [255, 255, 255, 255]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    assert_eq!(pixel(&layer, 64, 32, 32)[3], 0);
    assert_eq!(pixel(&layer, 64, 32, 24), [0, 0, 0, 255]);

    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(5.0, 5.0), 3.0)]);
    canvas.cancel_stroke();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 64, 5, 5), [255, 255, 255, 255]);
}

#[test]
fn a_built_stroke_with_taper_and_clear() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(200, 100);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let brush = Brush {
        streamline: 0.0,
        pen_taper: Taper { start: 0.0, end: 0.3, size: 1.0, opacity: 0.0 },
        ..Brush::round_pen()
    };
    let mut stroke = StrokeBuilder::new(brush, 12.0, Pointer::Stylus);
    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
    let samples: Vec<Sample> =
        (0..=20).map(|i| Sample::at(Vec2::new(10.0 + i as f32 * 9.0, 50.0), 1.0, i as f64 * 8.0)).collect();
    canvas.add_stamps(&stroke.add(&samples));
    canvas.replace_stamps(&stroke.finish());
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_composite(&gpu).unwrap();
    // full width in the middle, tapered to nothing at the end
    assert_eq!(pixel(&px, 200, 60, 55), [0, 0, 0, 255]);
    assert_eq!(pixel(&px, 200, 188, 55), [255, 255, 255, 255]);

    canvas.clear_layer(doc.current());
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 200, 60, 50), [255, 255, 255, 255]);
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 200, 60, 50), [0, 0, 0, 255]);
}

#[test]
fn draws_to_a_target_through_the_view() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(100, 100);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(25.0, 25.0), 10.0)]);
    canvas.end_stroke();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 200, height: 100, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    // the canvas fits the middle 100×100 of a 200×100 target
    let to_screen = View::fit((100, 100), (200, 100)).to_screen();
    canvas.render(
        &gpu,
        &mut doc,
        Some(Target {
            view: &view,
            format,
            size: (200, 100),
            to_screen,
            surround: [0.2, 0.2, 0.2],
            grid: None,
            selection: None,
            backdrop: None,
            convert: None,
            lut: None,
            crisp_from: f32::INFINITY,
            grey: false,
        }),
    );
    let px = read(&gpu, &target, 200, 100);
    assert_eq!(pixel(&px, 200, 10, 50), [51, 51, 51, 255], "surround");
    assert_eq!(pixel(&px, 200, 75, 25), [0, 0, 0, 255], "the dot");
    assert_eq!(pixel(&px, 200, 125, 75), [255, 255, 255, 255], "canvas");
}

#[test]
fn zoomed_in_to_its_mark_the_canvas_shows_each_pixel_as_a_square() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(4, 4);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    // the top row: two pixels painted, one partly, one not
    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(0.0, 0.0), 2.5)]);
    canvas.end_stroke();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 100, height: 100, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    // 25 target pixels to a canvas pixel
    let to_screen = View::fit((4, 4), (100, 100)).to_screen();
    let mut row = |crisp_from: f32| {
        canvas.render(
            &gpu,
            &mut doc,
            Some(Target {
                view: &view,
                format,
                size: (100, 100),
                to_screen,
                surround: [0.2, 0.2, 0.2],
                grid: None,
                selection: None,
                backdrop: None,
                convert: None,
                lut: None,
                crisp_from,
                grey: false,
            }),
        );
        let px = read(&gpu, &target, 100, 100);
        (0..100).map(|x| pixel(&px, 100, x, 12)).collect::<Vec<_>>()
    };
    let crisp = row(25.0);
    for square in crisp.chunks(25) {
        assert!(square.iter().all(|p| Some(p) == square.first()), "{square:?}");
    }
    assert_ne!(crisp.first(), crisp.last());
    // short of the mark, smoothed between them
    let smooth = row(26.0);
    assert!(smooth.chunks(25).any(|square| square.iter().any(|p| Some(p) != square.first())), "{smooth:?}");
}

#[test]
fn the_surround_grid_is_fixed_to_the_target() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(10, 10);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 100, height: 100, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    // a tiny canvas in the top-left corner, the rest surround with a line every 20 px from y = 10
    let to_screen = View::fit((10, 10), (10, 10)).to_screen();
    let grid = SurroundGrid { spacing: 20.0, origin: [0.0, 10.0], top: 10.0, width: 2.0, color: [1.0, 0.0, 0.0] };
    canvas.render(
        &gpu,
        &mut doc,
        Some(Target {
            view: &view,
            format,
            size: (100, 100),
            to_screen,
            surround: [0.2, 0.2, 0.2],
            grid: Some(grid),
            selection: None,
            backdrop: None,
            convert: None,
            lut: None,
            crisp_from: f32::INFINITY,
            grey: false,
        }),
    );
    let px = read(&gpu, &target, 100, 100);
    assert_eq!(pixel(&px, 100, 40, 30), [255, 0, 0, 255], "on a line across and down");
    assert_eq!(pixel(&px, 100, 39, 55), [255, 0, 0, 255], "on a line down, the pixel left of it");
    assert_eq!(pixel(&px, 100, 50, 45), [51, 51, 51, 255], "between lines");
    assert_eq!(pixel(&px, 100, 50, 5), [51, 51, 51, 255], "above the origin");
    assert_eq!(pixel(&px, 100, 5, 5), [255, 255, 255, 255], "the canvas covers the grid");
    // a line wider than its pixels smooths into the ones beside it
    let grid = SurroundGrid { spacing: 20.0, origin: [41.0, 0.0], top: 0.0, width: 8.0 / 3.0, color: [1.0, 0.0, 0.0] };
    canvas.render(
        &gpu,
        &mut doc,
        Some(Target {
            view: &view,
            format,
            size: (100, 100),
            to_screen,
            surround: [0.2, 0.2, 0.2],
            grid: Some(grid),
            selection: None,
            backdrop: None,
            convert: None,
            lut: None,
            crisp_from: f32::INFINITY,
            grey: false,
        }),
    );
    let px = read(&gpu, &target, 100, 100);
    assert_eq!(pixel(&px, 100, 40, 50), [255, 0, 0, 255]);
    assert_eq!(pixel(&px, 100, 41, 50), [255, 0, 0, 255]);
    for x in [39, 42] {
        let side = pixel(&px, 100, x, 50);
        // a third of the way from the surround (51) to the line
        assert!((side[0] as i32 - 119).abs() <= 2 && (side[1] as i32 - 34).abs() <= 2, "{x}: {side:?}");
    }
    assert_eq!(pixel(&px, 100, 43, 50), [51, 51, 51, 255]);
}

#[test]
fn a_frosted_region_shows_what_is_behind_it_blurred() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(100, 100);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 400, height: 100, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    // the white canvas on the left quarter, the dark surround right of it from x = 100, and a
    // region across that edge
    let to_screen = View::fit((100, 100), (100, 100)).to_screen();
    let regions = [[40.0, 10.0, 300.0, 90.0, 12.0, 1.0]];
    canvas.render(
        &gpu,
        &mut doc,
        Some(Target {
            view: &view,
            format,
            size: (400, 100),
            to_screen,
            surround: [0.0, 0.0, 0.0],
            grid: None,
            selection: None,
            backdrop: Some(Backdrop { regions: &regions, windows: &[], sigma: 24.0 }),
            convert: None,
            lut: None,
            crisp_from: f32::INFINITY,
            grey: false,
        }),
    );
    let px = read(&gpu, &target, 400, 100);
    let red = |x: u32, y: u32| pixel(&px, 400, x, y)[0];
    // outside the region: as it was
    assert_eq!(red(20, 50), 255, "canvas left of the region");
    assert_eq!(red(350, 50), 0, "surround right of the region");
    assert_eq!(red(70, 5), 255, "canvas above the region");
    // inside: the edge spread out, falling from white to dark across it
    let (a, b, c, d) = (red(50, 50), red(90, 50), red(110, 50), red(150, 50));
    assert!(a > b && b > c && c > d, "{a} {b} {c} {d}");
    assert!((150..250).contains(&b) && (5..105).contains(&c), "{b} {c}");
    assert!(red(250, 50) < 3 && a > 240, "far from the edge: {}", red(250, 50));
    // the rounded corner leaves the canvas as it was
    assert_eq!(red(41, 11), 255);
}

/// A canvas with a hard edge at x = 100 shown in a 400 x 100 target, under `backdrop`.
fn frosted_edge(gpu: &Gpu, regions: &[[f32; 6]], windows: &[[f32; 6]]) -> Vec<u8> {
    let mut doc = Document::new(100, 100);
    doc.background = [1.0, 1.0, 1.0];
    let mut canvas = Canvas::new(gpu, &doc).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("glass target"),
        size: wgpu::Extent3d { width: 400, height: 100, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    canvas.render(
        gpu,
        &mut doc,
        Some(Target {
            view: &view,
            format,
            size: (400, 100),
            to_screen: View::fit((100, 100), (100, 100)).to_screen(),
            surround: [0.0, 0.0, 0.0],
            grid: None,
            selection: None,
            backdrop: Some(Backdrop { regions, windows, sigma: 24.0 }),
            convert: None,
            lut: None,
            crisp_from: f32::INFINITY,
            grey: false,
        }),
    );
    read(gpu, &target, 400, 100)
}

#[test]
fn a_window_in_the_glass_only_softens_what_is_behind() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // frost across the edge, and a window in it lower down
    let frost = [[40.0, 0.0, 300.0, 100.0, 0.0, 1.0]];
    let window = [[40.0, 50.0, 300.0, 100.0, 0.0, 1.0]];
    let px = frosted_edge(&gpu, &frost, &window);
    let red = |x: u32, y: u32| pixel(&px, 400, x, y)[0];
    // the frost spreads the edge far; through the window it stays near the edge
    assert!((30..240).contains(&red(120, 20)), "frost 20 past the edge: {}", red(120, 20));
    assert!(red(120, 75) < 10, "window 20 past the edge: {}", red(120, 75));
    assert!(red(80, 75) > 245, "window 20 short of the edge: {}", red(80, 75));
    // softened all the same: a pixel beside the edge is neither
    assert!((20..235).contains(&red(99, 75)), "at the edge: {}", red(99, 75));
    // a window with no frost anywhere still draws
    let alone = frosted_edge(&gpu, &[], &window);
    assert!((20..235).contains(&pixel(&alone, 400, 99, 75)[0]));
    assert_eq!(pixel(&alone, 400, 99, 25)[0], 255, "above the window: as it was");
}

#[test]
fn a_window_in_the_glass_shows_a_slanted_edge_without_steps() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // the white canvas turned 30 degrees on black, its edge running down through a window; the
    // frost's 24 px blur draws the scene four times smaller, whose texels would show as steps
    let mut doc = Document::new(100, 100);
    doc.background = [1.0, 1.0, 1.0];
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("slanted glass"),
        size: wgpu::Extent3d { width: 400, height: 200, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let turned =
        View::centred(procraft_engine::geom::Vec2::new(50.0, 50.0), 1.6, 30f32.to_radians(), (400, 200)).unwrap();
    let window = [[0.0, 0.0, 400.0, 200.0, 0.0, 1.0]];
    canvas.render(
        &gpu,
        &mut doc,
        Some(Target {
            view: &view,
            format,
            size: (400, 200),
            to_screen: turned.to_screen(),
            surround: [0.0, 0.0, 0.0],
            grid: None,
            selection: None,
            backdrop: Some(Backdrop { regions: &[], windows: &window, sigma: 24.0 }),
            convert: None,
            lut: None,
            crisp_from: f32::INFINITY,
            grey: false,
        }),
    );
    let px = read(&gpu, &target, 400, 200);
    // where each row first turns light, going in from the left: along a straight edge it moves
    // a little each row, never in jumps of a texel's size
    let crossing = |y: u32| (0..400u32).find(|&x| pixel(&px, 400, x, y)[0] >= 128);
    let rows: Vec<u32> = (60..140).filter_map(crossing).collect();
    assert!(rows.len() > 70, "the edge crosses the rows looked at");
    let jumps: Vec<u32> = rows.windows(2).map(|w| w[0].abs_diff(w[1])).collect();
    let most = jumps.iter().copied().max().unwrap_or(0);
    assert!(most <= 2, "the edge jumps {most} px between rows: {jumps:?}");
}

/// What the glass over the bars costs a frame: the canvas shown with and without it, at the
/// tablet's size, waiting for the GPU each time. Run with `--ignored --nocapture`.
#[test]
#[ignore]
fn glass_cost() {
    let Some(gpu) = gpu() else { return };
    let (w, h) = (2560u32, 1600u32);
    let mut doc = Document::new(2732, 2048);
    doc.background = [0.8, 0.7, 0.6];
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("glass cost"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let to_screen = View::fit((2732, 2048), (w, h)).to_screen();
    // the top bar and the side bar, 2 pixels a point, and its two tracks
    let regions = [[0.0, 0.0, 2560.0, 88.0, 0.0, 1.0], [-40.0, 260.0, 102.0, 1298.0, 40.0, 1.0]];
    let windows = [[10.0, 272.0, 90.0, 612.0, 26.0, 1.0], [10.0, 756.0, 90.0, 1096.0, 26.0, 1.0]];
    let sigma = 47.0;
    let mut frames = |backdrop: Option<Backdrop<'_>>| {
        let mut each: Vec<f64> = (0..240)
            .map(|_| {
                let t = std::time::Instant::now();
                canvas.render(
                    &gpu,
                    &mut doc,
                    Some(Target {
                        view: &view,
                        format,
                        size: (w, h),
                        to_screen,
                        surround: [0.15, 0.15, 0.15],
                        grid: None,
                        selection: None,
                        backdrop,
                        convert: None,
                        lut: None,
                        crisp_from: f32::INFINITY,
                        grey: false,
                    }),
                );
                gpu.flush();
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        each.drain(..40);
        each.sort_by(f64::total_cmp);
        (each[each.len() / 2], each[each.len() * 9 / 10])
    };
    let glass = Some(Backdrop { regions: &regions, windows: &windows, sigma });
    for round in 0..3 {
        let (none, none_90) = frames(None);
        let (with, with_90) = frames(glass);
        println!(
            "round {round}: no glass {none:.3} ms (90% {none_90:.3}), glass {with:.3} ms (90% {with_90:.3}), +{:.3} ms",
            with - none
        );
    }
}

#[test]
fn the_canvas_is_shown_in_the_targets_colour_space() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(10, 10);
    doc.background = [1.0, 0.0, 0.0];
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 10, height: 10, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    // an sRGB canvas on a Display P3 screen: its red is P3's (234, 51, 35)
    let convert = Some(Profile::Srgb.matrix_to(Profile::DisplayP3));
    canvas.render(
        &gpu,
        &mut doc,
        Some(Target {
            view: &view,
            format,
            size: (10, 10),
            to_screen: View::fit((10, 10), (10, 10)).to_screen(),
            surround: [0.0; 3],
            grid: None,
            selection: None,
            backdrop: None,
            convert,
            lut: None,
            crisp_from: f32::INFINITY,
            grey: false,
        }),
    );
    let px = read(&gpu, &target, 10, 10);
    let [r, g, b, _] = pixel(&px, 10, 5, 5);
    assert!(r.abs_diff(234) <= 1 && g.abs_diff(51) <= 1 && b.abs_diff(35) <= 1, "{r} {g} {b}");
}

#[test]
fn a_colour_table_moves_the_canvas_colours() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(10, 10);
    doc.background = [1.0, 0.25, 0.0];
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 10, height: 10, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    // a table turning each colour round (red to green to blue to red) and inverting it
    let grid = 17;
    let mut values = Vec::new();
    for r in 0..grid {
        for g in 0..grid {
            for b in 0..grid {
                let v = |i: usize| 1.0 - i as f32 / (grid - 1) as f32;
                values.extend([v(b), v(r), v(g)]);
            }
        }
    }
    let lut = canvas.display_lut(&gpu, &procraft_engine::cms::Lut { grid, outputs: 3, values });
    canvas.render(
        &gpu,
        &mut doc,
        Some(Target {
            view: &view,
            format,
            size: (10, 10),
            to_screen: View::fit((10, 10), (10, 10)).to_screen(),
            surround: [0.0; 3],
            grid: None,
            selection: None,
            backdrop: None,
            convert: None,
            lut: Some(&lut),
            crisp_from: f32::INFINITY,
            grey: false,
        }),
    );
    let px = read(&gpu, &target, 10, 10);
    let [r, g, b, _] = pixel(&px, 10, 5, 5);
    // (1, 0.25, 0) becomes (1 - 0, 1 - 1, 1 - 0.25)
    assert!(r >= 254 && g <= 1 && b.abs_diff(191) <= 2, "{r} {g} {b}");
}

fn read(gpu: &Gpu, texture: &wgpu::Texture, width: u32, height: u32) -> Vec<u8> {
    let padded = (width * 4).div_ceil(256) * 256;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(padded * height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    gpu.queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    mapped.chunks(padded as usize).flat_map(|row| row[..(width * 4) as usize].to_vec()).collect()
}

#[test]
fn the_predicted_tail_shows_but_never_paints() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(100, 100);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 100, height: 100, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let to_screen = View::fit((100, 100), (100, 100)).to_screen();
    let frame = |canvas: &mut Canvas, doc: &mut Document| {
        canvas.render(
            &gpu,
            doc,
            Some(Target {
                view: &view,
                format,
                size: (100, 100),
                to_screen,
                surround: [0.0; 3],
                grid: None,
                selection: None,
                backdrop: None,
                convert: None,
                lut: None,
                crisp_from: f32::INFINITY,
                grey: false,
            }),
        );
        read(&gpu, &target, 100, 100)
    };
    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(20.0, 50.0), 5.0)]);
    canvas.set_prediction(&[dot(Vec2::new(80.0, 50.0), 5.0)]);
    let px = frame(&mut canvas, &mut doc);
    assert_eq!(pixel(&px, 100, 20, 50), [0, 0, 0, 255], "the stroke");
    assert_eq!(pixel(&px, 100, 80, 50), [0, 0, 0, 255], "the predicted tail");
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 100, 80, 50), [255, 255, 255, 255], "not in the canvas");
    canvas.end_stroke();
    let px = frame(&mut canvas, &mut doc);
    assert_eq!(pixel(&px, 100, 80, 50), [255, 255, 255, 255], "gone when the stroke ends");
    assert_eq!(pixel(&px, 100, 20, 50), [0, 0, 0, 255]);
}

#[test]
fn a_predicted_tail_shows_a_tiny_stamp_as_strong_as_the_stamp_that_replaces_it() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 100, height: 100, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let to_screen = View::fit((100, 100), (100, 100)).to_screen();
    // the round tip and a shape, the same stamp laid at one place and predicted at another, thinned
    // by a taper so that the round tip's rim doesn't take it past a pixel
    for shape in [None, Some(0)] {
        for radius in [0.3, 0.6, 3.0] {
            let thin = |x: f32| Stamp { taper: 0.0, ..dot(Vec2::new(x, 50.5), radius) };
            let mut doc = Document::new(100, 100);
            let mut canvas = Canvas::new(&gpu, &doc).unwrap();
            canvas.begin_stroke(doc.current(), StrokeStyle { shape, ..style(StrokeTool::Paint, 1.0) });
            canvas.add_stamps(&[thin(20.5)]);
            canvas.set_prediction(&[thin(80.5)]);
            canvas.render(
                &gpu,
                &mut doc,
                Some(Target {
                    view: &view,
                    format,
                    size: (100, 100),
                    to_screen,
                    surround: [0.0; 3],
                    grid: None,
                    selection: None,
                    backdrop: None,
                    convert: None,
                    lut: None,
                    crisp_from: f32::INFINITY,
                    grey: false,
                }),
            );
            let px = read(&gpu, &target, 100, 100);
            let (laid, predicted) = (pixel(&px, 100, 20, 50)[0], pixel(&px, 100, 80, 50)[0]);
            assert!(laid < 250 && laid.abs_diff(predicted) < 8, "{shape:?} {radius}: {laid} {predicted}");
            canvas.end_stroke();
        }
    }
}

#[test]
fn a_round_tip_under_a_pixel_across_lays_only_its_area_of_one() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a taper's thinnest tip fades out, where over a pixel across the tip keeps its strength
    let middle = |radius: f32| {
        let mut doc = Document::new(16, 16);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
        canvas.add_stamps(&[Stamp { taper: 0.0, ..dot(Vec2::new(8.5, 8.5), radius) }]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        pixel(&layer, 16, 8, 8)[3]
    };
    let (thinnest, thin, full) = (middle(0.25), middle(0.4), middle(1.0));
    assert!(thinnest < 70 && thin > thinnest + 30 && full > 240, "{thinnest} {thin} {full}");
}

#[test]
fn paint_laid_in_place_shows_no_predicted_tail_over_the_canvas() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(100, 100);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let style = StrokeStyle {
        rendering: Rendering::Gradual,
        laid_in_place: true,
        ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
    };
    canvas.begin_stroke(doc.current(), style);
    assert!(!canvas.shows_tip());
    canvas.add_stamps(&[dot(Vec2::new(20.0, 50.0), 5.0)]);
    canvas.set_prediction(&[dot(Vec2::new(80.0, 50.0), 5.0)]);
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 100, 80, 50), [255, 255, 255, 255]);
    canvas.end_stroke();
}

#[test]
fn a_dual_brush_shows_no_tip_its_second_hasnt_reached() {
    let Some(gpu) = gpu() else { return };
    let doc = Document::new(100, 100);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let paint = style(StrokeTool::Paint, 1.0);
    canvas.begin_dual_stroke(
        doc.current(),
        paint,
        procraft_gpu::SecondStroke { style: paint, mode: 0, threshold: -1.0 },
    );
    assert!(!canvas.shows_tip());
    canvas.end_stroke();
}

#[test]
fn a_stroke_tip_shows_until_the_next_one_replaces_it() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(100, 100);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let at = |canvas: &mut Canvas, doc: &mut Document, x: u32| {
        canvas.render(&gpu, doc, None);
        pixel(&canvas.read_composite(&gpu).unwrap(), 100, x, 50)
    };
    let (black, white) = ([0, 0, 0, 255], [255, 255, 255, 255]);
    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
    assert!(canvas.shows_tip());
    canvas.add_stamps(&[dot(Vec2::new(20.0, 50.0), 5.0)]);
    canvas.set_tip(&[dot(Vec2::new(40.0, 50.0), 5.0)]);
    assert_eq!(at(&mut canvas, &mut doc, 40), black, "the tip");
    // the next tip takes the last one's place; stamps kept stay
    canvas.set_tip(&[dot(Vec2::new(60.0, 50.0), 5.0)]);
    assert_eq!(at(&mut canvas, &mut doc, 40), white, "the old tip gone");
    assert_eq!(at(&mut canvas, &mut doc, 60), black);
    canvas.add_stamps(&[dot(Vec2::new(80.0, 50.0), 5.0)]);
    assert_eq!(at(&mut canvas, &mut doc, 60), white, "stamps drawn for good take the tip off");
    assert_eq!(at(&mut canvas, &mut doc, 80), black);
    // a tip still showing as the stroke ends is not kept
    canvas.set_tip(&[dot(Vec2::new(60.0, 50.0), 5.0)]);
    canvas.end_stroke();
    assert_eq!(at(&mut canvas, &mut doc, 60), white);
    assert_eq!(at(&mut canvas, &mut doc, 20), black);
    canvas.undo();
    assert_eq!(at(&mut canvas, &mut doc, 20), white);
}

#[test]
fn a_transform_moves_the_layer_and_undoes_as_one_step() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    canvas.begin_stroke(layer, style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(16.0, 16.0), 4.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);

    let at = |x: f32, y: f32| {
        [Vec2::new(x, y), Vec2::new(x + 64.0, y), Vec2::new(x + 64.0, y + 64.0), Vec2::new(x, y + 64.0)]
    };
    // moved 20 right and 10 down: each layer pixel comes from 20 left and 10 up
    let to_source = Homography::quad_to_quad(at(20.0, 10.0), at(0.0, 0.0)).unwrap().to_f32();
    canvas.begin_transform(layer);
    assert!(canvas.is_transforming());
    canvas.set_transform(to_source, Resample::Nearest);
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, layer).unwrap();
    assert_eq!(pixel(&px, 64, 36, 26), [0, 0, 0, 255]);
    assert_eq!(pixel(&px, 64, 16, 16)[3], 0);
    // redrawn from the copy each time, so moving it again does not lose what left the canvas
    canvas.set_transform(Homography::quad_to_quad(at(60.0, 0.0), at(0.0, 0.0)).unwrap().to_f32(), Resample::Bilinear);
    canvas.set_transform(to_source, Resample::Bicubic);
    canvas.end_transform(true);
    canvas.render(&gpu, &mut doc, None);
    assert!(!canvas.is_transforming());
    let px = canvas.read_layer(&gpu, layer).unwrap();
    assert_eq!(pixel(&px, 64, 36, 26), [0, 0, 0, 255]);
    assert_eq!(pixel(&px, 64, 16, 16)[3], 0);

    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, layer).unwrap();
    assert_eq!(pixel(&px, 64, 16, 16), [0, 0, 0, 255]);
    assert_eq!(pixel(&px, 64, 36, 26)[3], 0);
    canvas.redo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_layer(&gpu, layer).unwrap(), 64, 36, 26), [0, 0, 0, 255]);

    // cancelled: put back, with no new step
    canvas.undo();
    canvas.begin_transform(layer);
    canvas.set_transform(to_source, Resample::Nearest);
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, layer).unwrap();
    assert_eq!(pixel(&px, 64, 16, 16), [0, 0, 0, 255]);
    assert_eq!(pixel(&px, 64, 36, 26)[3], 0);
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_layer(&gpu, layer).unwrap(), 64, 16, 16)[3], 0);
}

#[test]
fn a_warp_mesh_redraws_the_layer_from_its_copy() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    canvas.begin_stroke(layer, style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(16.0, 16.0), 4.0)]);
    canvas.end_stroke();
    canvas.begin_transform(layer);
    // the copy's top-left 32 × 32 drawn 20 right and 10 down, as two triangles
    let corner = |x: f32, y: f32| [x + 20.0, y + 10.0, x, y];
    let mesh = [
        corner(0.0, 0.0),
        corner(32.0, 0.0),
        corner(32.0, 32.0),
        corner(0.0, 0.0),
        corner(32.0, 32.0),
        corner(0.0, 32.0),
    ];
    canvas.set_transform_mesh(&mesh, Resample::Nearest);
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, layer).unwrap();
    assert_eq!(pixel(&px, 64, 36, 26), [0, 0, 0, 255]);
    assert_eq!(pixel(&px, 64, 16, 16)[3], 0);
    canvas.end_transform(true);
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_layer(&gpu, layer).unwrap(), 64, 16, 16), [0, 0, 0, 255]);
}

#[test]
fn a_selection_keeps_paint_inside_and_stripes_the_rest() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    // the left half selected
    let coverage: Vec<u8> = (0..64 * 64).map(|i| if i % 64 < 32 { 255 } else { 0 }).collect();
    canvas.set_selection(&gpu, Some(&coverage));
    assert!(canvas.has_selection());
    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(32.0, 32.0), 12.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, doc.current()).unwrap();
    assert_eq!(pixel(&px, 64, 26, 32), [0, 0, 0, 255]);
    assert_eq!(pixel(&px, 64, 38, 32)[3], 0);

    // white canvas shown 1:1: bands of grey over the right half only
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let stripes = procraft_gpu::Stripes { period: 8.0, drift: 0.0, coverage: 0.5 };
    let to_screen = procraft_engine::geom::Affine::IDENTITY;
    canvas.render(
        &gpu,
        &mut doc,
        Some(Target {
            view: &view,
            format,
            size: (64, 64),
            to_screen,
            surround: [0.0; 3],
            grid: None,
            selection: Some(procraft_gpu::SelectionLook::Stripes(stripes)),
            backdrop: None,
            convert: None,
            lut: None,
            crisp_from: f32::INFINITY,
            grey: false,
        }),
    );
    let shown = read(&gpu, &texture, 64, 64);
    let row: Vec<u8> = (32..48).map(|x| pixel(&shown, 64, x, 50)[0]).collect();
    // over white the dark bands show at half coverage, the light ones not at all
    assert!(row.contains(&255) && row.iter().any(|&v| (120..=135).contains(&v)), "{row:?}");
    assert!((0..16).all(|x| pixel(&shown, 64, x, 50)[0] == 255));
    canvas.set_selection(&gpu, None);
    assert!(!canvas.has_selection());
}

#[test]
fn adjustments_redraw_the_layer_and_keep_to_the_selection() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    // a red square across the middle
    let mut red = StrokeStyle::new(StrokeTool::Paint, [1.0, 0.0, 0.0], 1.0);
    red.hardness = 1.0;
    canvas.begin_stroke(layer, red);
    canvas.add_stamps(&[dot(Vec2::new(32.0, 32.0), 20.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);

    // no chroma left: the grey of red's luma
    canvas.begin_transform(layer);
    canvas.set_filter(procraft_gpu::Filter::Hsb { hue: 0.0, saturation: 0.0, brightness: 0.0 });
    canvas.render(&gpu, &mut doc, None);
    let [r, g, b, a] = pixel(&canvas.read_layer(&gpu, layer).unwrap(), 64, 32, 32);
    assert!(r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1 && r.abs_diff(76) <= 2 && a == 255, "{r} {g} {b}");
    // half a turn: the opposite of red
    canvas.set_filter(procraft_gpu::Filter::Hsb { hue: std::f32::consts::PI, saturation: 1.0, brightness: 0.0 });
    canvas.render(&gpu, &mut doc, None);
    let [r, g, b, _] = pixel(&canvas.read_layer(&gpu, layer).unwrap(), 64, 32, 32);
    assert!(g > r && b > r, "{r} {g} {b}");
    // put back
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_layer(&gpu, layer).unwrap(), 64, 32, 32), [255, 0, 0, 255]);

    // blurred, with the left half selected: only the left half softens
    let coverage: Vec<u8> = (0..64 * 64).map(|i| if i % 64 < 32 { 255 } else { 0 }).collect();
    canvas.set_selection(&gpu, Some(&coverage));
    canvas.begin_transform(layer);
    canvas.set_filter(procraft_gpu::Filter::Blur { sigma: 6.0 });
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, layer).unwrap();
    // the left edge of the disc spreads outwards, the right edge stays sharp
    assert!(pixel(&px, 64, 9, 32)[3] > 10, "{:?}", pixel(&px, 64, 9, 32));
    assert_eq!(pixel(&px, 64, 53, 32)[3], 0);
    assert_eq!(pixel(&px, 64, 50, 32), [255, 0, 0, 255]);
    canvas.end_transform(true);
    canvas.render(&gpu, &mut doc, None);
    assert!(canvas.can_undo());
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_layer(&gpu, layer).unwrap(), 64, 9, 32)[3], 0);
    // a wide blur runs at a smaller size
    canvas.begin_transform(layer);
    canvas.set_filter(procraft_gpu::Filter::Blur { sigma: 60.0 });
    canvas.end_transform(true);
    canvas.render(&gpu, &mut doc, None);
}

#[test]
fn a_narrow_blur_is_a_gaussian_of_every_pixel_either_side() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (w, h) = (48u32, 40u32);
    let mut doc = Document::new(w, h);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    // noise, premultiplied: every pixel unlike its neighbours
    let mut seed = 7u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 24) as u8
    };
    let picture: Vec<u8> = (0..w * h)
        .flat_map(|_| {
            let a = next();
            let c = [next(), next(), next()].map(|v| (u16::from(v) * u16::from(a) / 255) as u8);
            [c[0], c[1], c[2], a]
        })
        .collect();
    canvas.upload_layer(&gpu, layer, &picture).unwrap();
    // each pass a Gaussian out to three sigmas either side, the edges held, kept to 8 bits between
    let pass = |src: &[u8], across: bool, sigma: f32| -> Vec<u8> {
        let reach = (sigma * 3.0).ceil().min(64.0) as i32;
        let weights: Vec<f32> = (-reach..=reach).map(|k| (-((k * k) as f32) / (2.0 * sigma * sigma)).exp()).collect();
        let total: f32 = weights.iter().sum();
        let mut out = vec![0u8; src.len()];
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                for c in 0..4 {
                    let mut sum = 0.0;
                    for (k, wt) in (-reach..=reach).zip(&weights) {
                        let (sx, sy) = if across {
                            ((x + k).clamp(0, w as i32 - 1), y)
                        } else {
                            (x, (y + k).clamp(0, h as i32 - 1))
                        };
                        sum += f32::from(src[((sy * w as i32 + sx) * 4 + c) as usize]) * wt;
                    }
                    out[((y * w as i32 + x) * 4 + c) as usize] = (sum / total).round() as u8;
                }
            }
        }
        out
    };
    for sigma in [0.7, 2.5, 6.0, 7.9] {
        canvas.begin_transform(layer);
        canvas.set_filter(procraft_gpu::Filter::Blur { sigma });
        canvas.render(&gpu, &mut doc, None);
        let blurred = canvas.read_layer(&gpu, layer).unwrap();
        let expected = pass(&pass(&picture, true, sigma), false, sigma);
        let most = blurred.iter().zip(&expected).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
        assert!(most <= 2, "sigma {sigma}: off by {most}");
        canvas.end_transform(false);
        canvas.render(&gpu, &mut doc, None);
    }
}

#[test]
fn every_adjustment_draws_and_changes_the_layer() {
    use procraft_gpu::Filter;
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    let mut grey = StrokeStyle::new(StrokeTool::Paint, [0.5, 0.5, 0.5], 1.0);
    grey.hardness = 1.0;
    canvas.begin_stroke(layer, grey);
    canvas.add_stamps(&[dot(Vec2::new(32.0, 32.0), 14.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let original = canvas.read_layer(&gpu, layer).unwrap();
    let filters = [
        Filter::Sharpen { amount: 1.0 },
        Filter::Noise { amount: 1.0, frequency: 0.45, octaves: 3, turbulence: 0.2, kind: 0, multi: false, added: true },
        Filter::Noise { amount: 1.0, frequency: 0.2, octaves: 2, turbulence: 0.2, kind: 1, multi: true, added: false },
        Filter::Noise { amount: 1.0, frequency: 0.2, octaves: 1, turbulence: 0.2, kind: 2, multi: false, added: true },
        Filter::Motion { along: [20.0, 0.0] },
        Filter::Zoom { centre: [0.0, 0.0], amount: 0.3, toward: [0.0, 0.0] },
        Filter::Zoom { centre: [32.0, 64.0], amount: 0.3, toward: [0.0, -1.0] },
        Filter::Chromatic {
            centre: [0.0, 0.0],
            amount: 0.2,
            displace: false,
            offset: [0.0; 2],
            blur: 0.5,
            falloff: 0.2,
            transparency: 0.0,
        },
        Filter::Chromatic {
            centre: [0.0, 0.0],
            amount: 0.0,
            displace: true,
            offset: [6.0, 2.0],
            blur: 0.3,
            falloff: 0.0,
            transparency: 0.5,
        },
        Filter::Halftone { pitch: 8.0, kind: 0 },
        Filter::Halftone { pitch: 8.0, kind: 1 },
        Filter::Halftone { pitch: 8.0, kind: 2 },
        Filter::Bloom { sigma: 6.0, amount: 1.0, threshold: 0.0, transition: 1.0, burn: 0.15 },
        Filter::Glitch { kind: 0, amount: 1.0, values: [1.0, 0.3, 0.0], zoom: 1.0, seed: 0.5 },
        Filter::Glitch { kind: 1, amount: 1.0, values: [1.0, 0.5, 0.0], zoom: 0.2, seed: 0.5 },
        Filter::Glitch { kind: 2, amount: 1.0, values: [1.0, 0.3, 0.0], zoom: 1.0, seed: 0.5 },
        Filter::Glitch { kind: 3, amount: 1.0, values: [1.0, -0.5, 0.8], zoom: 0.5, seed: 0.5 },
    ];
    for f in filters {
        canvas.begin_transform(layer);
        canvas.set_filter(f);
        canvas.render(&gpu, &mut doc, None);
        let px = canvas.read_layer(&gpu, layer).unwrap();
        assert_ne!(px, original, "{f:?} changed nothing");
        canvas.end_transform(false);
        canvas.render(&gpu, &mut doc, None);
        assert_eq!(canvas.read_layer(&gpu, layer).unwrap(), original, "{f:?} was not put back");
    }
}

#[test]
fn a_zoom_blur_with_an_arrow_streaks_only_what_lies_ahead_of_its_centre() {
    use procraft_gpu::Filter;
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(128, 128);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    let mut ink = StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0);
    ink.hardness = 1.0;
    canvas.begin_stroke(layer, ink);
    // a dot ahead of the centre (64, 64) as the arrow points (up), one behind it, one to the side
    let (ahead, behind, side) = (Vec2::new(64.0, 24.0), Vec2::new(64.0, 104.0), Vec2::new(104.0, 62.0));
    canvas.add_stamps(&[dot(ahead, 5.0), dot(behind, 5.0), dot(side, 5.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let original = canvas.read_layer(&gpu, layer).unwrap();
    let alpha = |px: &[u8], x: u32, y: u32| pixel(px, 128, x, y)[3];
    let mut blurred = |toward: [f32; 2]| {
        canvas.begin_transform(layer);
        canvas.set_filter(Filter::Zoom { centre: [64.0, 64.0], amount: 0.5, toward });
        canvas.render(&gpu, &mut doc, None);
        let px = canvas.read_layer(&gpu, layer).unwrap();
        canvas.end_transform(false);
        canvas.render(&gpu, &mut doc, None);
        px
    };
    let one_way = blurred([0.0, -1.0]);
    // behind the centre and square to the arrow nothing moves
    let half = (64 * 128 * 4) as usize;
    assert_eq!(one_way[half..], original[half..], "behind the centre changed");
    assert!(alpha(&one_way, 104, 62).abs_diff(alpha(&original, 104, 62)) <= 16, "the side dot moved");
    // ahead, the dot is drawn out along its ray: thinner where it was, out beyond it
    assert_eq!(alpha(&original, 64, 10), 0);
    assert!(alpha(&one_way, 64, 10) > 40, "nothing streaked ahead: {}", alpha(&one_way, 64, 10));
    assert!(alpha(&one_way, 64, 24) < 200, "the dot ahead stayed: {}", alpha(&one_way, 64, 24));
    // all round, the dot behind streaks too
    let all_round = blurred([0.0, 0.0]);
    assert_eq!(alpha(&original, 64, 118), 0);
    assert!(alpha(&all_round, 64, 118) > 40, "nothing streaked all round: {}", alpha(&all_round, 64, 118));
}

#[test]
fn curves_gradient_maps_and_colour_balance_draw() {
    use procraft_gpu::Filter;
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(32, 32);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    let mut grey = StrokeStyle::new(StrokeTool::Paint, [0.5, 0.5, 0.5], 1.0);
    grey.hardness = 1.0;
    canvas.begin_stroke(layer, grey);
    canvas.add_stamps(&[dot(Vec2::new(16.0, 16.0), 12.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    // every channel to full: white
    let table: Vec<u8> = (0..256).flat_map(|_| [255u8, 255, 255, 255]).collect();
    canvas.set_table(&gpu, &table);
    canvas.begin_transform(layer);
    canvas.set_filter(Filter::Curves);
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_layer(&gpu, layer).unwrap(), 32, 16, 16), [255, 255, 255, 255]);
    // a red gradient, all the way
    let red: Vec<u8> = (0..256).flat_map(|_| [255u8, 0, 0, 255]).collect();
    canvas.set_table(&gpu, &red);
    canvas.set_filter(Filter::GradientMap { amount: 1.0 });
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_layer(&gpu, layer).unwrap(), 32, 16, 16), [255, 0, 0, 255]);
    // more red in the midtones: redder, as bright
    canvas.set_filter(Filter::ColorBalance { shadows: [0.0; 3], midtones: [1.0, 0.0, 0.0], highlights: [0.0; 3] });
    canvas.render(&gpu, &mut doc, None);
    let [r, g, _, _] = pixel(&canvas.read_layer(&gpu, layer).unwrap(), 32, 16, 16);
    assert!(r > g + 40, "{r} {g}");
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
}

/// A stamp of a liquify brush at (`x`, `y`), going right.
fn liquify_stamp(
    mode: procraft_gpu::LiquifyMode,
    [x, y]: [f32; 2],
    radius: f32,
    amount: f32,
) -> procraft_gpu::LiquifyStamp {
    procraft_gpu::LiquifyStamp { centre: [x, y], way: [1.0, 0.0], radius, amount, mode }
}

/// Where the paint in row `y` of `px` (`width` across) lies: the middle of each run of it.
fn runs_of_paint(px: &[u8], width: u32, y: u32) -> Vec<f32> {
    let row: Vec<f32> = (0..width).map(|x| f32::from(pixel(px, width, x, y)[3])).collect();
    let mut runs = Vec::new();
    let mut x = 0;
    while x < row.len() {
        if row[x] > 30.0 {
            let start = x;
            while x < row.len() && row[x] > 30.0 {
                x += 1;
            }
            let weight: f32 = row[start..x].iter().sum();
            runs.push(row[start..x].iter().enumerate().map(|(i, v)| (start + i) as f32 * v).sum::<f32>() / weight);
        }
        x += 1;
    }
    runs
}

#[test]
fn liquify_carries_the_picture_along_with_the_brush_as_measured() {
    use procraft_gpu::{Filter, LiquifyMode};
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(2048, 256);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    canvas.begin_stroke(layer, style(StrokeTool::Paint, 1.0));
    let lines = [1284.0, 1339.0, 1404.0, 1459.0, 1524.0, 1579.0, 1637.0, 1695.0, 1756.0];
    for x in lines {
        canvas.add_stamps(&(0..256).step_by(2).map(|y| dot(Vec2::new(x, y as f32), 3.0)).collect::<Vec<_>>());
    }
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    canvas.begin_transform(layer);
    canvas.begin_liquify();
    // a brush of 351 from 600 to 1400 at full pressure, along the row looked at
    for k in 0..200 {
        let x = 600.0 + 4.0 * (k + 1) as f32;
        canvas.liquify_stamps(&[liquify_stamp(LiquifyMode::Push, [x, 128.0], 351.0, 4.0)]);
    }
    canvas.set_filter(Filter::Liquify { amount: 1.0 });
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, layer).unwrap();
    let runs = runs_of_paint(&px, 2048, 128);
    let measured = [1465.0, 1482.0, 1506.0, 1529.0, 1562.0, 1597.0, 1642.0, 1696.0, 1756.0];
    assert_eq!(runs.len(), measured.len(), "{runs:?}");
    for (run, wanted) in runs.iter().zip(measured) {
        assert!((run - wanted).abs() < 4.0, "{run} for {wanted}: {runs:?}");
    }
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
}

#[test]
fn liquify_twirls_as_long_as_it_works_and_rebuilds_as_fast_as_measured() {
    use procraft_gpu::{Filter, LiquifyMode};
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(256, 256);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    canvas.begin_stroke(layer, style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(158.0, 128.0), 4.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    canvas.begin_transform(layer);
    canvas.begin_liquify();
    // 30 from the centre of a brush of 100: right turns clockwise on the screen, by 4 radians a
    // second at the centre and 0.82 of that there, so a quarter of a second takes it 0.82 round
    for _ in 0..25 {
        canvas.liquify_stamps(&[liquify_stamp(LiquifyMode::TwirlRight, [128.0, 128.0], 100.0, 0.01)]);
    }
    canvas.set_filter(Filter::Liquify { amount: 1.0 });
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, layer).unwrap();
    let (x, y) = (128.0 + 30.0 * 0.82f32.cos(), 128.0 + 30.0 * 0.82f32.sin());
    assert!(pixel(&px, 256, x as u32, y as u32)[3] > 100, "not at ({x}, {y})");
    assert!(pixel(&px, 256, 158, 128)[3] < 30, "still where it was");
    // half a second of rebuilding at the centre leaves 3% of it
    for _ in 0..50 {
        canvas.liquify_stamps(&[liquify_stamp(LiquifyMode::Reconstruct, [128.0, 128.0], 400.0, 0.01)]);
    }
    canvas.set_filter(Filter::Liquify { amount: 1.0 });
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, layer).unwrap();
    assert!(pixel(&px, 256, 158, 128)[3] > 100, "not back");
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
}

#[test]
fn liquify_held_long_goes_on_growing_and_doesnt_fold() {
    use procraft_gpu::{Filter, LiquifyMode};
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    canvas.begin_stroke(layer, style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(32.0, 32.0), 6.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    canvas.begin_transform(layer);
    canvas.begin_liquify();
    for _ in 0..60 {
        canvas.liquify_stamps(&[liquify_stamp(LiquifyMode::Expand, [32.0, 32.0], 24.0, 0.05)]);
    }
    canvas.set_filter(Filter::Liquify { amount: 1.0 });
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, layer).unwrap();
    // all of it from the dot's middle: none from past the dot, as steps overshooting it would be
    for x in [32, 36, 40, 44] {
        assert!(pixel(&px, 64, x, 32)[3] > 200, "{x}: {:?}", pixel(&px, 64, x, 32));
    }
    canvas.liquify_clear();
    canvas.end_transform(true);
    canvas.render(&gpu, &mut doc, None);
}

#[test]
fn liquify_kept_at_part_of_itself_looks_as_it_did_shown_at_that_part() {
    use procraft_gpu::{Filter, LiquifyMode};
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(128, 128);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    canvas.begin_stroke(layer, style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(50.0, 64.0), 8.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    canvas.begin_transform(layer);
    canvas.begin_liquify();
    for k in 0..30 {
        let x = 50.0 + k as f32;
        canvas.liquify_stamps(&[liquify_stamp(LiquifyMode::Push, [x, 64.0], 40.0, 1.0)]);
    }
    canvas.set_filter(Filter::Liquify { amount: 0.4 });
    canvas.render(&gpu, &mut doc, None);
    let shown = canvas.read_layer(&gpu, layer).unwrap();
    canvas.liquify_scale(0.4);
    canvas.set_filter(Filter::Liquify { amount: 1.0 });
    canvas.render(&gpu, &mut doc, None);
    let kept = canvas.read_layer(&gpu, layer).unwrap();
    let most = shown.iter().zip(&kept).map(|(a, b)| a.abs_diff(*b)).max();
    assert!(most.is_some_and(|d| d <= 2), "{most:?}");
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
}

#[test]
fn an_adjustment_painted_on_shows_only_where_painted_and_clone_copies() {
    use procraft_gpu::Filter;
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    let mut red = StrokeStyle::new(StrokeTool::Paint, [1.0, 0.0, 0.0], 1.0);
    red.hardness = 1.0;
    canvas.begin_stroke(layer, red);
    canvas.add_stamps(&[dot(Vec2::new(16.0, 32.0), 10.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);

    // brightness down everywhere, but painted on only at the left
    canvas.begin_transform(layer);
    canvas.begin_pencil();
    assert!(canvas.is_pencil());
    let mut white = StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0);
    white.hardness = 1.0;
    canvas.begin_pencil_stroke(layer, white);
    canvas.add_stamps(&[dot(Vec2::new(10.0, 32.0), 4.0)]);
    canvas.end_stroke();
    canvas.set_filter(Filter::Hsb { hue: 0.0, saturation: 1.0, brightness: -1.0 });
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, layer).unwrap();
    assert_eq!(pixel(&px, 64, 10, 32), [0, 0, 0, 255]);
    assert_eq!(pixel(&px, 64, 22, 32), [255, 0, 0, 255]);
    // clone: the red copied from 30 to the left onto where it is painted
    canvas.set_filter(Filter::Clone { offset: [6.0, 0.0] });
    canvas.begin_pencil_stroke(layer, white);
    canvas.add_stamps(&[dot(Vec2::new(10.0, 32.0), 4.0)]);
    canvas.end_stroke();
    canvas.set_filter(Filter::Clone { offset: [6.0, 0.0] });
    canvas.render(&gpu, &mut doc, None);
    canvas.end_transform(true);
    canvas.render(&gpu, &mut doc, None);
    assert!(!canvas.is_pencil());
    assert!(canvas.can_undo());
}

#[test]
fn a_stamps_colour_shift_turns_its_hue() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [1.0, 0.0, 0.0], 1.0));
    let mut turned = dot(Vec2::new(20.0, 32.0), 6.0);
    turned.tint = Tint { hue: 0.5, ..Tint::NONE };
    let mut drained = dot(Vec2::new(44.0, 32.0), 6.0);
    drained.tint = Tint { saturation: [0.0, 0.0], ..Tint::NONE };
    canvas.add_stamps(&[turned, drained]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_composite(&gpu).unwrap();
    // half a turn from red is cyan; drained of its colour, white
    assert_eq!(pixel(&px, 64, 20, 32), [0, 255, 255, 255]);
    assert_eq!(pixel(&px, 64, 44, 32), [255, 255, 255, 255]);
}

#[test]
fn wet_edges_pool_on_the_canvas_and_a_threshold_hardens() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let draw = |style: StrokeStyle| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        canvas.begin_stroke(doc.current(), style);
        let mut soft = dot(Vec2::new(32.0, 32.0), 20.0);
        soft.alpha = 1.0;
        canvas.add_stamps(&[soft]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        canvas.read_composite(&gpu).unwrap()
    };
    let plain = StrokeStyle { hardness: 0.3, ..style(StrokeTool::Paint, 1.0) };
    let base = draw(plain);
    // on the canvas the full middle settles back to about 0.78 and a full ring forms where the
    // dot fades through about 0.7
    let wet = draw(StrokeStyle { wet_edges: 1.0, ..plain });
    let middle = pixel(&wet, 64, 32, 32)[0];
    assert!((45..=65).contains(&middle), "{middle}");
    let ring = (12..32).map(|x| pixel(&wet, 64, x, 32)[0]).min().unwrap();
    assert!(ring <= 3, "{ring}");
    // a preview leaves the middle as it was and makes the edge at most a little stronger (darker)
    let preview = draw(StrokeStyle { wet_edges: 1.0, preview: true, ..plain });
    assert!(pixel(&preview, 64, 32, 32)[0].abs_diff(pixel(&base, 64, 32, 32)[0]) <= 2);
    for x in 12..32 {
        let (w, b) = (pixel(&preview, 64, x, 32)[0], pixel(&base, 64, x, 32)[0]);
        assert!(w <= b && b - w <= 25, "{x}: {w} {b}");
    }
    // a threshold turns the soft edge either fully on or off
    let hard = draw(StrokeStyle { alpha_threshold: 0.5, ..plain });
    for x in 12..52 {
        let v = pixel(&hard, 64, x, 32)[0];
        assert!(!(2..=253).contains(&v), "{x}: {v}");
    }
}

#[test]
fn a_capped_stroke_builds_up_to_its_transfer() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    canvas.begin_stroke(doc.current(), StrokeStyle { rendering: Rendering::Capped, ..style(StrokeTool::Paint, 1.0) });
    // forty full stamps letting half through: black over white stops at half
    let stamps: Vec<Stamp> = (0..40).map(|_| Stamp { transfer: 0.5, ..dot(Vec2::new(32.0, 32.0), 10.0) }).collect();
    canvas.add_stamps(&stamps);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let middle = pixel(&canvas.read_composite(&gpu).unwrap(), 64, 32, 32)[0];
    if gpu.caps_transfer() {
        assert!((120..=135).contains(&middle), "{middle}");
    } else {
        // without dual-source blending each stamp takes its transfer and they build up
        assert!(middle < 10, "{middle}");
    }
}

#[test]
fn burnt_edges_burn_only_the_paint_under_a_thin_stroke() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let orange = StrokeStyle::new(StrokeTool::Paint, [0.86, 0.6, 0.42], 1.0);
    // burnt edges as given, or (negative) as much in a preview
    let draw = |burnt_edges: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        // an orange band down the middle
        canvas.begin_stroke(doc.current(), orange);
        canvas.add_stamps(&(0..64).map(|y| dot(Vec2::new(32.0, y as f32), 8.0)).collect::<Vec<_>>());
        canvas.end_stroke();
        // a soft orange dot across it and off it
        let preview = burnt_edges < 0.0;
        canvas.begin_stroke(
            doc.current(),
            StrokeStyle { hardness: 0.3, burnt_edges: burnt_edges.abs(), preview, ..orange },
        );
        canvas.add_stamps(&[dot(Vec2::new(32.0, 32.0), 20.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        canvas.read_layer(&gpu, doc.current()).unwrap()
    };
    let (base, burnt) = (draw(0.0), draw(1.0));
    // over the band, where the dot is thin, its paint is burnt darker
    let (b, p) = (pixel(&burnt, 64, 32, 47), pixel(&base, 64, 32, 47));
    assert!(b[1] + 10 < p[1], "{b:?} {p:?}");
    // off the band nothing burns, and the band away from the dot is as it was
    for (x, y) in [(47, 32), (50, 40), (32, 4)] {
        let (b, p) = (pixel(&burnt, 64, x, y), pixel(&base, 64, x, y));
        assert!(b.iter().zip(p).all(|(b, p)| b.abs_diff(p) <= 1), "{x},{y}: {b:?} {p:?}");
    }
    // a preview burns nothing
    let preview = draw(-1.0);
    assert_eq!(pixel(&preview, 64, 32, 47), pixel(&base, 64, 32, 47));
}

#[test]
fn a_dual_brush_s_burnt_edges_burn_where_its_second_took_paint_away() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let orange = StrokeStyle::new(StrokeTool::Paint, [0.86, 0.6, 0.42], 1.0);
    // a soft dot over an orange band, burnt edges on, as a dual brush whose second (by Normal)
    // went elsewhere: the second took nothing away there, so its soft rim isn't burnt
    let draw = |burnt_edges: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        canvas.begin_stroke(doc.current(), orange);
        canvas.add_stamps(&(0..64).map(|y| dot(Vec2::new(32.0, y as f32), 8.0)).collect::<Vec<_>>());
        canvas.end_stroke();
        let second = procraft_gpu::SecondStroke { style: orange, mode: 0, threshold: -1.0 };
        canvas.begin_dual_stroke(doc.current(), StrokeStyle { hardness: 0.3, burnt_edges, ..orange }, second);
        canvas.add_stamps(&[dot(Vec2::new(32.0, 32.0), 20.0)]);
        canvas.add_second_stamps(&[dot(Vec2::new(4.0, 4.0), 2.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        canvas.read_layer(&gpu, doc.current()).unwrap()
    };
    let (base, burnt) = (draw(0.0), draw(1.0));
    let (b, p) = (pixel(&burnt, 64, 32, 47), pixel(&base, 64, 32, 47));
    assert!(b.iter().zip(p).all(|(b, p)| b.abs_diff(p) <= 1), "{b:?} {p:?}");
}

#[test]
fn wet_paint_mixes_with_what_is_under_it() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    // a grey band down the middle
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.5, 0.5, 0.5], 1.0));
    canvas.add_stamps(&(0..64).map(|y| dot(Vec2::new(32.0, y as f32), 8.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    // white across it, wet (half water, a little charge) with a strong pull, half a pixel a
    // millisecond (laid a display frame at a time): over the grey it mixes, and past it the grey
    // it picked up goes on with it
    let wet = StrokeStyle {
        wet_pull: Some(0.85),
        dilution: 0.5,
        load: 0.1,
        ..StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0)
    };
    canvas.begin_stroke(doc.current(), wet);
    canvas.add_stamps(
        &(0..120)
            .map(|i| Stamp {
                time_ms: f64::from(i),
                distance: i as f32 * 0.5,
                ..Stamp::round(Vec2::new(4.0 + i as f32 * 0.5, 32.0), 4.0, 0.5)
            })
            .collect::<Vec<_>>(),
    );
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    let over = pixel(&layer, 64, 32, 32);
    assert!(over[0] > 135 && over[0] < 250, "mixed over the band: {over:?}");
    let past = pixel(&layer, 64, 50, 32);
    assert!(past[3] > 128 && past[0] < 250 && past[0] >= over[0], "past the band: {past:?}");
    // as one undo step
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    let back = pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 64, 32, 32);
    assert!((126..=130).contains(&back[0]), "{back:?}");
}

#[test]
fn a_slower_wet_stroke_runs_out_sooner() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // the same oily stroke drawn quickly and four times as slowly: laid a display frame at a
    // time, the slower one lays over what was under each frame's first stamp more often, so
    // once its charge has gone it thins sooner
    let laid = |ms_a_stamp: f64| {
        let mut doc = Document::new(400, 32);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let oily = StrokeStyle {
            wet_pull: Some(0.96),
            dilution: 0.85,
            load: 0.46,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), oily);
        canvas.add_stamps(
            &(0..720)
                .map(|i| Stamp {
                    time_ms: f64::from(i) * ms_a_stamp,
                    distance: i as f32 * 0.5,
                    ..Stamp::round(Vec2::new(4.0 + i as f32 * 0.5, 16.0), 6.0, 1.0)
                })
                .collect::<Vec<_>>(),
        );
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (pixel(&layer, 400, 60, 16)[3], pixel(&layer, 400, 340, 16)[3])
    };
    let (quick, slow) = (laid(0.5), laid(2.0));
    // both loaded early on; long after the charge has gone, the slow one has less left
    assert!(quick.0 > 150 && slow.0 > 150, "{quick:?} {slow:?}");
    assert!(slow.1 + 20 < quick.1, "{quick:?} {slow:?}");
}

#[test]
fn a_stamps_bleed_lays_its_strong_parts_whole_and_its_faint_ones_not_at_all() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // one soft dry stamp, without and with the pressure's bleed at its most
    let laid = |bleed: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        canvas.begin_stroke(
            doc.current(),
            StrokeStyle { hardness: 0.0, ..StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0) },
        );
        canvas.add_stamps(&[Stamp { grain_bleed: bleed, ..dot(Vec2::new(32.0, 32.0), 24.0) }]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (pixel(&layer, 64, 33, 32)[3], pixel(&layer, 64, 50, 32)[3])
    };
    let (plain, bled) = (laid(0.0), laid(1.0));
    assert!(plain.0 > 240 && bled.0 > 240, "{plain:?} {bled:?}");
    assert!(plain.1 > 20 && bled.1 == 0, "{plain:?} {bled:?}");
}

#[test]
fn a_little_pressure_bleed_lays_even_a_stamp_s_whole_middle_short_of_full() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // the bleed's contrast is (1 + b) a - b / sqrt(b p), b eight times the bleed and p its
    // pressure: a little bleed cuts more than it lifts, so even a whole part lays 1 + b - sqrt(b)
    // at full pressure (0.815 for a bleed of 0.0075), less at a lighter one; a bleed of 1/8 or
    // more lays it whole
    let laid = |bleed: f32, press: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0));
        canvas.add_stamps(&[Stamp { grain_bleed: bleed, grain_press: press, ..dot(Vec2::new(32.0, 32.0), 24.0) }]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        i32::from(pixel(&layer, 64, 32, 32)[3])
    };
    let contrast = |bleed: f32, press: f32| {
        let b = 8.0 * bleed;
        (((1.0 + b) - b / (b * press).sqrt()).clamp(0.0, 1.0) * 255.0).round() as i32
    };
    assert!(laid(0.0, 1.0) > 250, "{}", laid(0.0, 1.0));
    for (bleed, press) in [(0.0075, 1.0), (0.0075, 0.5), (0.125, 1.0)] {
        let (got, formula) = (laid(bleed, press), contrast(bleed, press));
        assert!((got - formula).abs() <= 3, "bleed {bleed} at {press}: {got}, formula {formula}");
    }
    assert!(contrast(0.0075, 1.0) < 210 && contrast(0.125, 1.0) == 255);
}

#[test]
fn a_wet_stamps_bleed_lays_its_strong_parts_whole_and_its_faint_ones_not_at_all() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // one soft wet stamp, fully charged, without and with the pressure's bleed at its most
    let laid = |bleed: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let wet = StrokeStyle {
            wet_pull: Some(0.5),
            dilution: 0.5,
            load: 1.0,
            hardness: 0.0,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0)
        };
        canvas.begin_stroke(doc.current(), wet);
        canvas.add_stamps(&[Stamp { grain_bleed: bleed, ..Stamp::round(Vec2::new(32.0, 32.0), 24.0, 1.0) }]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (pixel(&layer, 64, 33, 32)[3], pixel(&layer, 64, 50, 32)[3])
    };
    let (plain, bled) = (laid(0.0), laid(1.0));
    // the middle laid whole either way; the faint rim only without the bleed
    assert!(plain.0 > 240 && bled.0 > 240, "{plain:?} {bled:?}");
    assert!(plain.1 > 20 && bled.1 == 0, "{plain:?} {bled:?}");
}

#[test]
fn dry_paint_laid_in_place_builds_up_by_what_is_left() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // twenty stamps a quarter strong on one spot, each in a frame of its own: laid in place, each
    // adds a quarter of what is left, squared, as the rule says (drawn over, they would reach
    // 1 - 0.75^20)
    let mut doc = Document::new(32, 32);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let style = StrokeStyle {
        rendering: Rendering::Gradual,
        laid_in_place: true,
        ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
    };
    canvas.begin_stroke(doc.current(), style);
    canvas.add_stamps(
        &(0..20)
            .map(|i| Stamp { time_ms: f64::from(i) * 20.0, ..Stamp::round(Vec2::new(16.0, 16.0), 8.0, 0.25) })
            .collect::<Vec<_>>(),
    );
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let laid = f32::from(pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 32, 16, 16)[3]) / 255.0;
    let mut built = 0.0f32;
    for _ in 0..20 {
        built += 0.25 * (1.0 - built) * (1.0 - built);
    }
    assert!((laid - built).abs() < 0.03, "{laid} for {built}");
    assert!(laid < 0.95);
}

#[test]
fn a_grain_s_levels_reach_paint_laid_in_place() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a dot laid in place through a moving grain: its brightness takes the grain to nothing or to
    // all, as a stamp drawn over would have it
    let cover = |grain_brightness: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let style = StrokeStyle {
            grain: Some(0),
            grain_mode: procraft_engine::brush::GrainMode::Moving,
            grain_depth: 1.0,
            grain_scale: 64.0,
            grain_blend: 1,
            grain_brightness,
            rendering: Rendering::Gradual,
            laid_in_place: true,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 20.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (24..40).map(|x| u32::from(pixel(&layer, 64, x, 32)[3])).sum::<u32>()
    };
    let (dark, plain, light) = (cover(-1.0), cover(0.0), cover(1.0));
    assert_eq!(dark, 0, "the grain taken to nothing lets nothing through");
    assert!(plain > 0 && light > plain + 16 * 20, "plain {plain}, light {light}");
}

#[test]
fn a_dual_brush_s_paint_laid_in_place_is_cut_by_its_second() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a dry brush laid in place on the right, its second on the left: by Multiply the paint shows
    // only where both went; by Normal the second shows on its own too, also when its stamps come
    // first; and undo takes it all back
    let laid = |mode: u32, second_first: bool| {
        let mut doc = Document::new(64, 32);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let first = StrokeStyle {
            rendering: Rendering::Gradual,
            laid_in_place: true,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
        };
        let second = procraft_gpu::SecondStroke {
            style: StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0),
            mode,
            threshold: -1.0,
        };
        canvas.begin_dual_stroke(doc.current(), first, second);
        if second_first {
            canvas.add_second_stamps(&[Stamp::round(Vec2::new(24.0, 16.0), 12.0, 1.0)]);
            canvas.render(&gpu, &mut doc, None);
        }
        canvas.add_stamps(&[Stamp::round(Vec2::new(40.0, 16.0), 12.0, 1.0)]);
        if !second_first {
            canvas.add_second_stamps(&[Stamp::round(Vec2::new(24.0, 16.0), 12.0, 1.0)]);
        }
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        let shown = [16, 32, 48].map(|x| pixel(&layer, 64, x, 16)[3]);
        canvas.undo();
        canvas.render(&gpu, &mut doc, None);
        let undone = canvas.read_layer(&gpu, doc.current()).unwrap();
        (shown, [16, 32, 48].map(|x| pixel(&undone, 64, x, 16)[3]))
    };
    let (multiply, undone) = laid(1, false);
    assert!(multiply[0] == 0 && multiply[1] > 240 && multiply[2] == 0, "{multiply:?}");
    assert_eq!(undone, [0, 0, 0]);
    for second_first in [false, true] {
        let (normal, _) = laid(0, second_first);
        assert!(normal.iter().all(|&a| a > 240), "{normal:?} {second_first}");
    }
}

/// A grain fixed to the canvas, `tile` wide, in `style`: 512 square, its left half white and the
/// rest black, so that multiplied paint shows only on the left of each tile.
fn half_grain(canvas: &mut Canvas, gpu: &Gpu, style: &mut StrokeStyle, tile: f32) {
    style.grain_mode = procraft_engine::brush::GrainMode::Texturized;
    style.grain_depth = 1.0;
    style.grain_scale = tile;
    style.grain_blend = 1;
    let pixels: Vec<u8> = (0..512 * 512).map(|i| if i % 512 < 256 { 255 } else { 0 }).collect();
    canvas.use_full_grain(gpu, style, 7, 512, &pixels);
}

#[test]
fn a_stroke_reads_a_brush_file_s_grain_at_its_own_size() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // wet, dry laid in place and stamped: each reads the image it was given, not the brush images
    for (wet_pull, laid_in_place) in [(Some(0.5), false), (None, true), (None, false)] {
        let mut doc = Document::new(128, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let mut style = StrokeStyle {
            wet_pull,
            laid_in_place,
            rendering: Rendering::Gradual,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
        };
        half_grain(&mut canvas, &gpu, &mut style, 128.0);
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(64.0, 32.0), 60.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        let (left, right) = (pixel(&layer, 128, 32, 32)[3], pixel(&layer, 128, 96, 32)[3]);
        assert!(left > 200 && right < 40, "{wet_pull:?} {laid_in_place}: {left} {right}");
    }
    // a moving grain too, read as far from the stamp's middle as the pixel is: the stamp's right
    // falls on the tile's white half, its left on the black
    for (wet_pull, laid_in_place) in [(Some(0.5), false), (None, true), (None, false)] {
        let mut doc = Document::new(128, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let mut style = StrokeStyle {
            wet_pull,
            laid_in_place,
            rendering: Rendering::Gradual,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
        };
        half_grain(&mut canvas, &gpu, &mut style, 128.0);
        style.grain_mode = procraft_engine::brush::GrainMode::Moving;
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(64.0, 32.0), 60.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        let (left, right) = (pixel(&layer, 128, 32, 32)[3], pixel(&layer, 128, 96, 32)[3]);
        assert!(right > 200 && left < 40, "moving {wet_pull:?} {laid_in_place}: {left} {right}");
    }
    // a smudge too: paint dragged down off a black band goes only where the grain lets it
    let mut doc = Document::new(128, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0));
    canvas.add_stamps(&(0..128).map(|x| dot(Vec2::new(x as f32, -8.0), 32.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    for x in [32.0, 96.0] {
        let mut style = StrokeStyle::new(StrokeTool::Smudge, [0.0; 3], 1.0);
        half_grain(&mut canvas, &gpu, &mut style, 128.0);
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&(0..30).map(|i| dot(Vec2::new(x, 12.0 + i as f32), 10.0)).collect::<Vec<_>>());
        canvas.end_stroke();
    }
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    let (left, right) = (pixel(&layer, 128, 32, 32)[3], pixel(&layer, 128, 96, 32)[3]);
    assert!(left > 60 && right < 10, "smudge: {left} {right}");
}

#[test]
fn a_moving_grain_at_its_own_size_is_read_at_the_level_its_tile_calls_for() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // stripes a texel wide, four texels to a canvas pixel: the right level averages them to grey,
    // where the finest would land on one stripe or the other. Sharp, the stripes of that level
    let pixels: Vec<u8> = (0..1024 * 1024).map(|i| if i % 2 == 0 { 255 } else { 0 }).collect();
    let middle = |grain_sharp: bool| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let mut style = StrokeStyle {
            grain_mode: procraft_engine::brush::GrainMode::Moving,
            grain_depth: 1.0,
            grain_scale: 256.0,
            grain_blend: 1,
            grain_sharp,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
        };
        canvas.use_full_grain(&gpu, &mut style, 8, 1024, &pixels);
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.125, 32.0), 24.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (24..40).map(|x| pixel(&layer, 64, x, 32)[3]).collect::<Vec<u8>>()
    };
    let (filtered, sharp) = (middle(false), middle(true));
    assert!(filtered.iter().all(|a| a.abs_diff(127) < 16), "{filtered:?}");
    assert!(sharp.iter().all(|a| a.abs_diff(127) < 16), "{sharp:?}");
}

#[test]
fn a_stroke_going_on_to_its_layer_keeps_its_grain_as_the_next_begins() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // both queued for one render: the first is put together with its own image, the next with none
    let mut doc = Document::new(128, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let mut first = StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0);
    half_grain(&mut canvas, &gpu, &mut first, 128.0);
    canvas.begin_stroke(doc.current(), first);
    canvas.add_stamps(&[Stamp::round(Vec2::new(64.0, 16.0), 12.0, 1.0)]);
    canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 16.0), 12.0, 1.0)]);
    canvas.add_stamps(&[Stamp::round(Vec2::new(96.0, 16.0), 12.0, 1.0)]);
    canvas.end_stroke();
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0));
    canvas.add_stamps(&[Stamp::round(Vec2::new(96.0, 48.0), 12.0, 1.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    let (left, right, next) =
        (pixel(&layer, 128, 32, 16)[3], pixel(&layer, 128, 96, 16)[3], pixel(&layer, 128, 96, 48)[3]);
    assert!(left > 200 && right < 40 && next > 200, "{left} {right} {next}");
}

#[test]
fn a_texturized_grain_on_paint_laid_in_place_goes_by_its_blend_mode() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a dot through a grain fixed to the canvas: multiplied it is thinned, and a grain taken as a
    // height, which the full dot stands well above, leaves it whole; wet or dry
    let cover = |grain_blend: u32, wet_pull: Option<f32>| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let style = StrokeStyle {
            wet_pull,
            laid_in_place: wet_pull.is_none(),
            rendering: Rendering::Gradual,
            grain: Some(0),
            grain_mode: procraft_engine::brush::GrainMode::Texturized,
            grain_depth: 1.0,
            grain_scale: 64.0,
            grain_blend,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 20.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        let shown = (24..40).map(|x| u32::from(pixel(&layer, 64, x, 32)[3])).sum::<u32>();
        canvas.undo();
        canvas.render(&gpu, &mut doc, None);
        let undone = canvas.read_layer(&gpu, doc.current()).unwrap();
        assert_eq!(pixel(&undone, 64, 32, 32)[3], 0);
        shown
    };
    for wet_pull in [Some(0.5), None] {
        let (multiply, height) = (cover(1, wet_pull), cover(27, wet_pull));
        assert!(height > 16 * 240 && multiply + 16 * 20 < height, "{wet_pull:?}: {multiply} {height}");
    }
}

#[test]
fn paint_laid_in_place_takes_each_stamp_s_own_colour() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // red paint: a stamp turned a third of the way round the hues is green, and one all of the
    // secondary colour is blue, wet or dry, and laid apart (for its wet edges) or not
    let shown = |wet_pull: Option<f32>, wet_edges: f32, stamp: Stamp| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let style = StrokeStyle {
            wet_pull,
            laid_in_place: wet_pull.is_none(),
            rendering: Rendering::Gradual,
            secondary: [0.0, 0.0, 1.0],
            wet_edges,
            ..StrokeStyle::new(StrokeTool::Paint, [1.0, 0.0, 0.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[stamp]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        pixel(&layer, 64, 32, 32)
    };
    // as strong as the paint shows there, and none of the other two
    let near = |a: [u8; 4], channel: usize| {
        a[3] > 190 && (0..3).all(|c| a[c].abs_diff(if c == channel { a[3] } else { 0 }) < 8)
    };
    let plain = Stamp::round(Vec2::new(32.0, 32.0), 20.0, 1.0);
    let turned = Stamp { tint: Tint { hue: 1.0 / 3.0, ..Tint::NONE }, ..plain };
    let second = Stamp { secondary: 1.0, ..plain };
    for wet_pull in [Some(0.5), None] {
        for wet_edges in [0.0, 0.5] {
            let [red, green, blue] = [plain, turned, second].map(|s| shown(wet_pull, wet_edges, s));
            assert!(
                near(red, 0) && near(green, 1) && near(blue, 2),
                "{wet_pull:?} {wet_edges}: {red:?} {green:?} {blue:?}"
            );
        }
    }
}

#[test]
fn wet_paint_with_luminance_blending_mixes_squared() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // white taking a quarter of the way over black: a quarter by plain mixing, a half squared
    let shown = |luminance: bool| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0));
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 24.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let style = StrokeStyle {
            wet_pull: Some(0.0),
            attack: 0.25,
            dilution: 1.0,
            rendering: Rendering::Gradual,
            luminance,
            ..StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 20.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        pixel(&layer, 64, 32, 32)
    };
    let (plain, squared) = (shown(false), shown(true));
    assert!(plain[0].abs_diff(64) < 6 && plain[3] > 250, "{plain:?}");
    assert!(squared[0].abs_diff(128) < 6 && squared[3] > 250, "{squared:?}");
}

#[test]
fn wet_paint_lays_as_far_as_its_wetness() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let shown = |wetness: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let style =
            StrokeStyle { wet_pull: Some(0.5), wetness, ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0) };
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 20.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        pixel(&layer, 64, 32, 32)[3]
    };
    let (full, half) = (shown(1.0), shown(0.5));
    assert!(full > 250 && half.abs_diff(128) < 6, "{full} {half}");
    assert_eq!(StrokeStyle::wetness_at(1.0), 1.0);
    assert_eq!(StrokeStyle::wetness_at(0.0), 0.0);
    assert!((StrokeStyle::wetness_at(0.5) - (1.0 - 0.5f32.powf(2.2))).abs() < 1e-6);
}

#[test]
fn wet_paint_goes_over_the_layer_squared_only_when_it_mixes_with_the_layer_as_it_was() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // white over black, shown 0.78 of the way by its wet edges: plainly, or as its colours mix
    let shown = |wet_from_before: bool| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0));
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 24.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let style = StrokeStyle {
            wet_pull: Some(0.5),
            wet_from_before,
            wet_edges: 1.0,
            rendering: Rendering::Gradual,
            luminance: true,
            ..StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 20.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        pixel(&layer, 64, 32, 32)[0]
    };
    let (recursive, from_before) = (shown(false), shown(true));
    assert!(recursive.abs_diff(200) < 8 && from_before.abs_diff(226) < 8, "{recursive} {from_before}");
}

#[test]
fn burnt_edges_keep_the_squared_mix_of_a_glaze_with_luminance_blending() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // white over black, 0.78 of the way by its wet edges, mixing squared: burnt by a plain blend
    // the paint is what it was, so nothing shows, and the mix stays squared
    let shown = |burnt_edges: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0));
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 24.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let style = StrokeStyle {
            wet_pull: Some(0.5),
            wet_from_before: true,
            wet_edges: 1.0,
            rendering: Rendering::Gradual,
            luminance: true,
            burnt_edges,
            burnt_blend: 0,
            ..StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 20.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        pixel(&layer, 64, 32, 32)[0]
    };
    let (plain, burnt) = (shown(0.0), shown(1.0));
    assert!(plain.abs_diff(226) < 8 && burnt.abs_diff(plain) < 4, "{plain} {burnt}");
}

#[test]
fn a_stamp_under_a_pixel_across_lays_as_much_on_the_pixels_it_reaches() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // its quad reaches half a pixel past it, so a tiny stamp thins by that, not by its area as well:
    // the middle of one under a pixel across is as strong as the middle of one over
    let middle = |radius: f32| {
        let mut doc = Document::new(16, 16);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let style = StrokeStyle { shape: Some(0), ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0) };
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(8.5, 8.5), radius, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        pixel(&layer, 16, 8, 8)[3]
    };
    let (under, over) = (middle(0.3), middle(0.6));
    assert!(over > 100 && u32::from(under) * 10 > u32::from(over) * 8, "{under} {over}");
}

#[test]
fn burnt_edges_burn_the_paint_wet_paint_goes_over_and_nothing_else() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // blue pooled to 0.78 over red, or over nothing: burnt by multiply, it goes darker where it is
    // thin over the red, and the same over nothing
    let shown = |burnt_edges: f32, over_red: bool| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        if over_red {
            canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [1.0, 0.0, 0.0], 1.0));
            canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 24.0, 1.0)]);
            canvas.end_stroke();
            canvas.render(&gpu, &mut doc, None);
        }
        let style = StrokeStyle {
            wet_pull: Some(0.5),
            rendering: Rendering::Gradual,
            wet_edges: 1.0,
            burnt_edges,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 1.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 20.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        pixel(&layer, 64, 32, 32)
    };
    let (plain, burnt) = (shown(0.0, true), shown(1.0, true));
    assert!(plain[2].abs_diff(200) < 8 && burnt[2].abs_diff(156) < 8, "{plain:?} {burnt:?}");
    assert!(plain[0].abs_diff(burnt[0]) < 4, "{plain:?} {burnt:?}");
    let (bare, bare_burnt) = (shown(0.0, false), shown(1.0, false));
    assert!(bare.iter().zip(bare_burnt).all(|(a, b)| a.abs_diff(b) < 3), "{bare:?} {bare_burnt:?}");
}

#[test]
fn paint_laid_in_place_turns_a_mirrored_stamp_s_shape() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a shape with its ink on the left half only: mirrored, the ink is on the right, wet or dry
    for wet_pull in [Some(0.5), None] {
        let shown = |flip: [bool; 2]| {
            let mut doc = Document::new(64, 64);
            let mut canvas = Canvas::new(&gpu, &doc).unwrap();
            let pixels: Vec<u8> = (0..TEXTURE_SIZE * TEXTURE_SIZE)
                .map(|i| if i % TEXTURE_SIZE < TEXTURE_SIZE / 2 { 255 } else { 0 })
                .collect();
            let brush = Brush {
                shape: procraft_engine::brush::Shape {
                    source: procraft_engine::brush::Source::new(pixels),
                    ..Brush::round_pen().shape
                },
                ..Brush::round_pen()
            };
            let mut style = StrokeStyle {
                wet_pull,
                laid_in_place: wet_pull.is_none(),
                rendering: Rendering::Gradual,
                ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
            };
            canvas.use_sources(&gpu, &mut style, &brush);
            canvas.begin_stroke(doc.current(), style);
            canvas.add_stamps(&[Stamp { flip, ..Stamp::round(Vec2::new(32.0, 32.0), 20.0, 1.0) }]);
            canvas.end_stroke();
            canvas.render(&gpu, &mut doc, None);
            let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
            (pixel(&layer, 64, 22, 32)[3], pixel(&layer, 64, 42, 32)[3])
        };
        let (plain, mirrored) = (shown([false, false]), shown([true, false]));
        assert!(plain.0 > 200 && plain.1 < 40, "{wet_pull:?}: {plain:?}");
        assert!(mirrored.1 > 200 && mirrored.0 < 40, "{wet_pull:?}: {mirrored:?}");
    }
}

#[test]
fn wet_edges_pool_wet_paint_by_how_much_of_it_shows() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // light stamps of watery paint (as a light press lays) show only part of their paint, though
    // after a few the paper is touched all over: the wet edges pool by the part that shows, as a
    // dry stroke's do by its coverage (so a stroke's thinning ends get the rim, near full where
    // about 0.7 shows), not by the touch
    let shown = |wet_edges: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let style = StrokeStyle {
            wet_pull: Some(0.5),
            dilution: 1.0,
            load: 1.0,
            attack: 1.0,
            rendering: Rendering::Gradual,
            wet_edges,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), style);
        let stamps: Vec<Stamp> = (0..40)
            .map(|i| Stamp {
                time_ms: f64::from(i),
                distance: i as f32 * 0.5,
                ..Stamp::round(Vec2::new(28.0 + i as f32 * 0.2, 32.0), 20.0, 0.1)
            })
            .collect();
        canvas.add_stamps(&stamps);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        f32::from(pixel(&layer, 64, 32, 32)[3]) / 255.0
    };
    let (plain, pooled) = (shown(0.0), shown(1.0));
    assert!((0.2..0.95).contains(&plain), "thinned paint shows part: {plain}");
    let x = 4.5 * plain;
    let expected = 1.0 - (x.sin() / x).abs();
    assert!((pooled - expected).abs() < 0.04, "pooled {pooled}, by what shows {expected} (plain {plain})");
}

#[test]
fn wet_edges_pool_paint_laid_in_place() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // full paint settles back to about 0.78 as it pools, wet or dry, and undo takes it all back
    let shown = |wet_edges: f32, wet_pull: Option<f32>| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let style = StrokeStyle {
            wet_pull,
            laid_in_place: wet_pull.is_none(),
            rendering: Rendering::Gradual,
            wet_edges,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 20.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        let middle = pixel(&layer, 64, 32, 32)[3];
        canvas.undo();
        canvas.render(&gpu, &mut doc, None);
        let undone = canvas.read_layer(&gpu, doc.current()).unwrap();
        assert_eq!(pixel(&undone, 64, 32, 32)[3], 0);
        middle
    };
    for wet_pull in [Some(0.5), None] {
        let (plain, pooled) = (shown(0.0, wet_pull), shown(1.0, wet_pull));
        assert!(plain > 250 && (190..=210).contains(&pooled), "{wet_pull:?}: {plain} {pooled}");
    }
}

#[test]
fn a_dual_brush_s_wet_paint_is_cut_by_its_second() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // wet paint on the right, its second on the left: by Multiply the paint shows only where both
    // went, and undo takes it all back
    let mut doc = Document::new(64, 32);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let first = StrokeStyle { wet_pull: Some(0.5), ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0) };
    assert!(first.takes_second());
    let second = procraft_gpu::SecondStroke {
        style: StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0),
        mode: 1,
        threshold: -1.0,
    };
    canvas.begin_dual_stroke(doc.current(), first, second);
    canvas.add_stamps(&[Stamp::round(Vec2::new(40.0, 16.0), 12.0, 1.0)]);
    canvas.add_second_stamps(&[Stamp::round(Vec2::new(24.0, 16.0), 12.0, 1.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    let shown = [16, 32, 48].map(|x| pixel(&layer, 64, x, 16)[3]);
    assert!(shown[0] == 0 && shown[1] > 240 && shown[2] == 0, "{shown:?}");
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    let undone = canvas.read_layer(&gpu, doc.current()).unwrap();
    assert_eq!([16, 32, 48].map(|x| pixel(&undone, 64, x, 16)[3]), [0, 0, 0]);
}

#[test]
fn a_dual_brush_in_a_height_mode_leaves_no_clear_pixel_where_both_painted() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // dots of 64 strengths, wet edged, through a deep grain taken as a height (which takes most of
    // them to full), all under a second that is full everywhere, by Height and by Linear Height:
    // each dot shows whole (a coverage a rounding past full once gave a clear pixel)
    for mode in [27, 28] {
        let mut doc = Document::new(128, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let first = StrokeStyle {
            wet_edges: 1.0,
            grain: Some(0),
            grain_mode: procraft_engine::brush::GrainMode::Texturized,
            grain_depth: 1.0,
            grain_scale: 64.0,
            grain_blend: 27,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
        };
        let second = procraft_gpu::SecondStroke {
            style: StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0),
            mode,
            threshold: -1.0,
        };
        let at = |i: u32| (4 + (i % 16) * 8, 4 + (i / 16) * 8 + 16);
        canvas.begin_dual_stroke(doc.current(), first, second);
        let dots: Vec<Stamp> = (0..64)
            .map(|i| {
                let (x, y) = at(i);
                Stamp::round(Vec2::new(x as f32 + 0.5, y as f32 + 0.5), 3.0, (i + 1) as f32 / 64.0)
            })
            .collect();
        canvas.add_stamps(&dots);
        canvas.add_second_stamps(&[Stamp::round(Vec2::new(64.0, 32.0), 100.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        let clear: Vec<(u32, u32)> = (12..64)
            .flat_map(|i| {
                let (x, y) = at(i);
                [(x, y), (x + 1, y), (x, y + 1), (x - 1, y), (x, y - 1)]
            })
            .filter(|&(x, y)| pixel(&layer, 128, x, y)[3] == 0)
            .collect();
        assert!(clear.is_empty(), "mode {mode}: clear at {clear:?}");
    }
}

#[test]
fn a_dual_brush_s_second_adds_nothing_where_the_first_is_not() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // the first on the right, its second on the left, by Hard Mix, which fills what the two add up
    // to one: where only the second went nothing shows, by Normal it does
    let shown = |mode: u32| {
        let mut doc = Document::new(64, 32);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let second = procraft_gpu::SecondStroke {
            style: StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0),
            mode,
            threshold: -1.0,
        };
        let first = StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0);
        canvas.begin_dual_stroke(doc.current(), first, second);
        canvas.add_stamps(&[Stamp::round(Vec2::new(40.0, 16.0), 12.0, 1.0)]);
        canvas.add_second_stamps(&[Stamp::round(Vec2::new(24.0, 16.0), 12.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        [16, 32, 48].map(|x| pixel(&layer, 64, x, 16)[3])
    };
    let hard_mix = shown(17);
    assert!(hard_mix[0] == 0 && hard_mix[1] > 240 && hard_mix[2] > 240, "{hard_mix:?}");
    let normal = shown(0);
    assert!(normal.iter().all(|&a| a > 240), "{normal:?}");
}

#[test]
fn modulated_wet_paint_lays_a_pixel_once() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a long wet stroke whose charge runs out: building up, its later stamps keep laying the
    // paint they drag along over pixels already painted; building up by what is left (the
    // modulated mode), a pixel takes paint only from the first stamps to reach it, so the stroke
    // falls away towards its fresh paint
    let late = |rendering: Rendering| {
        let mut doc = Document::new(400, 32);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let wet = StrokeStyle {
            wet_pull: Some(0.8),
            dilution: 0.8,
            load: 0.45,
            grade: 1.0,
            rendering,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), wet);
        canvas.add_stamps(
            &(0..760)
                .map(|i| Stamp {
                    time_ms: f64::from(i),
                    distance: i as f32 * 0.5,
                    ..Stamp::round(Vec2::new(10.0 + i as f32 * 0.5, 16.0), 10.0, 1.0)
                })
                .collect::<Vec<_>>(),
        );
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (pixel(&layer, 400, 20, 16)[3], pixel(&layer, 400, 350, 16)[3])
    };
    let (built, kept) = (late(Rendering::BuildUp), late(Rendering::Gradual));
    assert!(built.0 > 100 && kept.0 > 100, "{built:?} {kept:?}");
    assert!(u16::from(kept.1) + 20 < u16::from(built.1), "{built:?} {kept:?}");
}

#[test]
fn the_wet_mix_blur_softens_what_a_smudge_lays_down() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // black on the left of x = 32, clear on the right; one smudge stamp over the edge lays back
    // what it picked up: the edge as sharp as it was, or blurred as far as the blur says
    let smudged = |blur: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0));
        canvas.add_stamps(&(0..64).map(|y| dot(Vec2::new(-8.0, y as f32), 40.0)).collect::<Vec<_>>());
        canvas.end_stroke();
        let smudge = StrokeStyle { blur, ..StrokeStyle::new(StrokeTool::Smudge, [0.0; 3], 1.0) };
        canvas.begin_stroke(doc.current(), smudge);
        canvas.add_stamps(&[dot(Vec2::new(32.0, 32.0), 16.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (pixel(&layer, 64, 28, 32)[3], pixel(&layer, 64, 36, 32)[3])
    };
    let (inside, outside) = smudged(0.0);
    assert!(inside >= 250 && outside <= 5, "unblurred: {inside} {outside}");
    // a little blur is under a pixel's sigma: still sharp
    let (inside, outside) = smudged(0.1);
    assert!(inside >= 245 && outside <= 10, "a little: {inside} {outside}");
    // the whole blur spreads the edge across the stamp
    let (inside, outside) = smudged(1.0);
    assert!(inside < 230 && outside > 25, "blurred: {inside} {outside}");
}

#[test]
fn a_smudge_carries_fine_detail_without_blurring_it() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a thin line down the middle; what a smudge carries is a pixel a cell, moving a whole pixel
    // at a time, so nothing it picks up and lays back down is resampled on the way
    let mut doc = Document::new(128, 128);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0));
    canvas.add_stamps(&(0..128).map(|y| dot(Vec2::new(64.0, y as f32), 1.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let id = doc.current();
    let line = |canvas: &Canvas| {
        let layer = canvas.read_layer(&gpu, id).unwrap();
        (60..68).map(|x| pixel(&layer, 128, x, 64)[3]).collect::<Vec<_>>()
    };
    let lost = |a: &[u8], b: &[u8]| a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
    let before = line(&canvas);
    // a big stamp held over it, twice (the second lays back what the first picked up)
    canvas.begin_stroke(id, StrokeStyle::new(StrokeTool::Smudge, [0.0; 3], 1.0));
    canvas.add_stamps(&[dot(Vec2::new(64.0, 64.0), 48.0), dot(Vec2::new(64.0, 64.0), 48.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let held = line(&canvas);
    assert!(lost(&before, &held) <= 2, "{before:?} {held:?}");
    canvas.undo();
    // a stroke along the line, its stamps anywhere between pixels: the line stays as sharp
    canvas.begin_stroke(id, StrokeStyle::new(StrokeTool::Smudge, [0.0; 3], 1.0));
    canvas.add_stamps(&(0..120).map(|i| dot(Vec2::new(64.3, 20.0 + i as f32 * 0.73), 12.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let along = line(&canvas);
    assert!(lost(&before, &along) <= 4, "{before:?} {along:?}");
}

#[test]
fn a_smudge_wider_than_what_it_carries_still_drags_colour() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // red on the left, blue on the right; a stamp wider than the carried grid (more than a pixel
    // a cell) dragged from the red into the blue takes red along
    let mut doc = Document::new(512, 128);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    for (rgb, x) in [([1.0, 0.0, 0.0], 128.0), ([0.0, 0.0, 1.0], 384.0)] {
        canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, rgb, 1.0));
        canvas.add_stamps(&(0..128).map(|y| dot(Vec2::new(x, y as f32), 128.0)).collect::<Vec<_>>());
        canvas.end_stroke();
    }
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Smudge, [0.0; 3], 1.0));
    canvas.add_stamps(&(0..40).map(|i| dot(Vec2::new(200.0 + i as f32 * 2.0, 64.0), 150.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    let dragged = pixel(&layer, 512, 290, 64);
    assert!(dragged[0] > 60, "red carried into the blue: {dragged:?}");
}

#[test]
fn a_wet_stroke_redrawn_whole_is_laid_once() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // faint stamps along a line, drawn live and then redrawn whole (as a stroke's end does), or
    // only drawn whole: the same paint either way, the faint stamps adding up as they should
    let stamps: Vec<Stamp> = (0..200).map(|i| Stamp::round(Vec2::new(4.0 + i as f32 * 0.3, 32.0), 6.0, 0.01)).collect();
    let paint = |live: bool| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let wet = StrokeStyle { wet_pull: Some(0.0), ..StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0) };
        canvas.begin_stroke(doc.current(), wet);
        if live {
            canvas.add_stamps(&stamps);
        }
        canvas.replace_stamps(&stamps);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 64, 32, 32)[3]
    };
    let (once, redrawn) = (paint(false), paint(true));
    assert!(once.abs_diff(redrawn) <= 2, "{once} {redrawn}");
    // about 40 stamps over the middle at 1% each
    let expected = 255.0 * (1.0 - 0.99f32.powf(12.0 * 1.0 / 0.3));
    assert!((f32::from(once) - expected).abs() < 8.0, "{once} {expected}");
}

#[test]
fn wet_paint_thins_as_its_charge_runs_out() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // all water, loaded at a half: along an empty layer the paint goes on thinner as it goes
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let wet = StrokeStyle {
        wet_pull: Some(0.75),
        dilution: 1.0,
        load: 0.5,
        ..StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0)
    };
    canvas.begin_stroke(doc.current(), wet);
    canvas.add_stamps(
        &(0..140).map(|i| Stamp::round(Vec2::new(4.0 + i as f32 * 0.4, 32.0), 4.0, 0.1)).collect::<Vec<_>>(),
    );
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    let (start, end) = (pixel(&layer, 64, 10, 32), pixel(&layer, 64, 54, 32));
    assert!(end[3] > 0 && end[3] + 20 < start[3], "{start:?} {end:?}");
}

#[test]
fn wet_attack_puts_more_paint_over_what_is_there() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let glaze = |attack: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.5, 0.5, 0.5], 1.0));
        canvas.add_stamps(&(0..64).map(|y| dot(Vec2::new(32.0, y as f32), 8.0)).collect::<Vec<_>>());
        canvas.end_stroke();
        // all water, so the paint goes on only as far as the attack puts it
        let wet = StrokeStyle {
            wet_pull: Some(0.85),
            dilution: 1.0,
            attack,
            ..StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), wet);
        canvas.add_stamps(
            &(0..60).map(|i| Stamp::round(Vec2::new(10.0 + i as f32 * 0.7, 32.0), 4.0, 0.5)).collect::<Vec<_>>(),
        );
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 64, 32, 32)[0]
    };
    let soft = glaze(0.0);
    let hard = glaze(1.0);
    // nearly the paint, a little of the grey under it picked up on the way
    assert!(hard > soft, "attack {hard} vs none {soft}");
    assert!(hard >= 235, "attack {hard} vs none {soft}");
}

#[test]
fn wet_paint_runs_out_with_the_path_not_with_how_far_jitter_threw_its_stamps() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // the same path, its stamps kept on it or thrown from side to side
    let far_end = |thrown: f32| {
        let mut doc = Document::new(256, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let wet = StrokeStyle {
            wet_pull: Some(0.9),
            dilution: 0.8,
            load: 0.5,
            ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), wet);
        let stamps: Vec<Stamp> = (0..400)
            .map(|i| {
                let y = 32.0 + if i % 2 == 0 { thrown } else { -thrown };
                Stamp {
                    time_ms: f64::from(i),
                    distance: i as f32 * 0.5,
                    ..Stamp::round(Vec2::new(20.0 + i as f32 * 0.5, y), 16.0, 1.0)
                }
            })
            .collect();
        canvas.add_stamps(&stamps);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        pixel(&layer, 256, 210, 32)[3]
    };
    let (kept, thrown) = (far_end(0.0), far_end(8.0));
    assert!(kept.abs_diff(thrown) < 10, "{kept} {thrown}");
}

/// Stripes four texels wide over an image 1024 a side, and a brush with the same at the size of
/// the brush images, where four pixels are one texel.
fn striped_shape() -> (Vec<u8>, Brush) {
    let side = 1024usize;
    let stripes = (0..side * side).map(|i| if (i % side / 4).is_multiple_of(2) { 255 } else { 0 }).collect();
    let small = (0..TEXTURE_SIZE * TEXTURE_SIZE).map(|i| if i.is_multiple_of(2) { 255 } else { 0 }).collect();
    let shape = Shape { source: Source::new(small), ..Shape::default() };
    (stripes, Brush { shape, ..Brush::round_pen() })
}

/// How many of a row's pixels are neither paint nor bare: a magnified shape's blur.
fn in_between(values: impl Iterator<Item = u8>) -> usize {
    values.filter(|a| (40..215).contains(a)).count()
}

#[test]
fn a_shape_at_its_own_size_stays_sharp_where_the_brush_images_would_be_magnified() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (stripes, brush) = striped_shape();
    let row = |wet_pull: Option<f32>, own: bool| {
        let mut doc = Document::new(1100, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let mut style = StrokeStyle { wet_pull, ..style(StrokeTool::Paint, 1.0) };
        canvas.use_sources(&gpu, &mut style, &brush);
        if own {
            canvas.use_full_shape(&gpu, &mut style, 7, 1024, &stripes);
        }
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(550.0, 32.0), 512.0, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        in_between((200..900).map(|x| pixel(&layer, 1100, x, 32)[3]))
    };
    // laid by the stamp pipeline, and wet
    for wet_pull in [None, Some(0.5)] {
        let (sharp, magnified) = (row(wet_pull, true), row(wet_pull, false));
        assert!(magnified > 300 && sharp * 3 < magnified, "{wet_pull:?}: {sharp} {magnified}");
    }
}

#[test]
fn a_shape_at_its_own_size_lays_as_the_brush_images_do_where_they_hold_enough() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // the same picture at both sizes, each texel of the small one four of the big one's a side:
    // squares that a stamp of any size here reads through its mip levels and its coarsest
    let n = TEXTURE_SIZE;
    let small: Vec<u8> = (0..n * n).map(|i| if (i % n / 8 + i / n / 8).is_multiple_of(2) { 255 } else { 40 }).collect();
    let side = n * 4;
    let big: Vec<u8> = (0..side * side).map(|i| small[(i / side / 4) * n + i % side / 4]).collect();
    let brush = Brush { shape: Shape { source: Source::new(small), ..Shape::default() }, ..Brush::round_pen() };
    let row = |wet_pull: Option<f32>, radius: f32, cap: f32, own: bool| {
        let mut doc = Document::new(256, 256);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let mut style = StrokeStyle { wet_pull, shape_lod_cap: cap, ..style(StrokeTool::Paint, 1.0) };
        canvas.use_sources(&gpu, &mut style, &brush);
        if own {
            canvas.use_full_shape(&gpu, &mut style, 7, side as u32, &big);
        }
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(128.0, 128.0), radius, 1.0)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (0..256).map(|x| pixel(&layer, 256, x, 128)[3]).collect::<Vec<u8>>()
    };
    // (both coarsest levels, and from stamps the cap holds to ones read near the images' full size)
    for wet_pull in [None, Some(0.5)] {
        for cap in [4.0, 5.0] {
            for radius in [3.0, 10.0, 40.0, 100.0] {
                let (own, shipped) = (row(wet_pull, radius, cap, true), row(wet_pull, radius, cap, false));
                let most = own.iter().zip(&shipped).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
                assert!(shipped.iter().any(|&a| a > 0) && most <= 6, "{wet_pull:?} {cap} {radius}: {most}");
            }
        }
    }
    // and as a predicted tail, which reads it for itself
    let (w, h) = (256u32, 256u32);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let predicted = |radius: f32, own: bool| {
        let mut doc = Document::new(w, h);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let mut style = style(StrokeTool::Paint, 1.0);
        canvas.use_sources(&gpu, &mut style, &brush);
        if own {
            canvas.use_full_shape(&gpu, &mut style, 7, side as u32, &big);
        }
        canvas.begin_stroke(doc.current(), style);
        canvas.set_prediction(&[Stamp::round(Vec2::new(128.0, 128.0), radius, 1.0)]);
        canvas.render(
            &gpu,
            &mut doc,
            Some(Target {
                view: &view,
                format,
                size: (w, h),
                to_screen: View::fit((w, h), (w, h)).to_screen(),
                surround: [0.0; 3],
                grid: None,
                selection: None,
                backdrop: None,
                convert: None,
                lut: None,
                crisp_from: f32::INFINITY,
                grey: false,
            }),
        );
        let px = read(&gpu, &target, w, h);
        canvas.end_stroke();
        (0..w).map(|x| pixel(&px, w, x, 128)[0]).collect::<Vec<u8>>()
    };
    for radius in [10.0, 40.0, 100.0] {
        let (own, shipped) = (predicted(radius, true), predicted(radius, false));
        let most = own.iter().zip(&shipped).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(most <= 6, "predicted {radius}: {most}");
    }
}

#[test]
fn a_smudge_goes_by_a_shape_at_its_own_size() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (stripes, brush) = striped_shape();
    let row = |own: bool| {
        let mut doc = Document::new(1100, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        // paint to the left of the middle, none to the right, and the first stamp carries it over
        canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
        let along = (0..55).map(|i| Stamp::round(Vec2::new(i as f32 * 10.0, 32.0), 40.0, 1.0)).collect::<Vec<_>>();
        canvas.add_stamps(&along);
        canvas.end_stroke();
        let mut smudge = style(StrokeTool::Smudge, 1.0);
        smudge.pull = 1.0;
        canvas.use_sources(&gpu, &mut smudge, &brush);
        if own {
            canvas.use_full_shape(&gpu, &mut smudge, 7, 1024, &stripes);
        }
        canvas.begin_stroke(doc.current(), smudge);
        canvas.add_stamps(&[
            Stamp::round(Vec2::new(300.0, 32.0), 512.0, 1.0),
            Stamp::round(Vec2::new(800.0, 32.0), 512.0, 1.0),
        ]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (600..1000).map(|x| pixel(&layer, 1100, x, 32)[3]).collect::<Vec<u8>>()
    };
    let (sharp, magnified) = (row(true), row(false));
    // the paint carried over shows through the shape, stripes and all
    assert!(sharp.iter().any(|&a| a > 200) && sharp.iter().any(|&a| a < 40), "{sharp:?}");
    let (a, b) = (in_between(sharp.iter().copied()), in_between(magnified.iter().copied()));
    assert!(a * 2 < b, "{a} {b}");
}

#[test]
fn a_predicted_tail_shows_a_shape_at_its_own_size() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (stripes, brush) = striped_shape();
    let (w, h) = (1100u32, 64u32);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mut doc = Document::new(w, h);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let mut style = style(StrokeTool::Paint, 1.0);
    canvas.use_sources(&gpu, &mut style, &brush);
    canvas.use_full_shape(&gpu, &mut style, 7, 1024, &stripes);
    canvas.begin_stroke(doc.current(), style);
    canvas.set_prediction(&[Stamp::round(Vec2::new(550.0, 32.0), 512.0, 1.0)]);
    canvas.render(
        &gpu,
        &mut doc,
        Some(Target {
            view: &view,
            format,
            size: (w, h),
            to_screen: View::fit((w, h), (w, h)).to_screen(),
            surround: [0.0; 3],
            grid: None,
            selection: None,
            backdrop: None,
            convert: None,
            lut: None,
            crisp_from: f32::INFINITY,
            grey: false,
        }),
    );
    let px = read(&gpu, &target, w, h);
    let row: Vec<u8> = (200..900).map(|x| pixel(&px, w, x, 32)[0]).collect();
    // on white, black paint in stripes: both, as the shape has them, and not the first brush image
    let (paint, bare) = (row.iter().filter(|&&v| v < 40).count(), row.iter().filter(|&&v| v > 215).count());
    assert!(paint > 200 && bare > 200, "{paint} {bare}");
    canvas.end_stroke();
}

#[test]
fn wet_paint_keeps_a_stroke_of_stamps_a_display_frame_apart() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // far apart, each stamp is a pass of its own, and with wet edges each lot of them is put
    // together as it comes: more slots a stamp than a closely spaced stroke takes
    let mut doc = Document::new(640, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let wet = StrokeStyle {
        wet_pull: Some(0.9),
        wet_edges: 0.5,
        grade: 1.0,
        rendering: Rendering::Gradual,
        ..StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0)
    };
    canvas.begin_stroke(doc.current(), wet);
    let stamps: Vec<Stamp> = (0..60)
        .map(|i| Stamp {
            time_ms: f64::from(i) * 20.0,
            distance: i as f32 * 10.0,
            ..Stamp::round(Vec2::new(20.0 + i as f32 * 10.0, 32.0), 8.0, 1.0)
        })
        .collect();
    for stamp in &stamps {
        canvas.add_stamps(std::slice::from_ref(stamp));
    }
    // and its end redraws it whole, all in the one render when frames have fallen behind
    canvas.replace_stamps(&stamps);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    assert!(pixel(&layer, 640, 300, 32)[3] > 100, "{:?}", pixel(&layer, 640, 300, 32));
}

#[test]
fn wet_paint_thins_over_bare_paper_as_far_as_its_pull_lets_it() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a wet stroke across bare paper: how covered it is near its start and at its far end
    let stroke = |pull: f32| {
        let mut doc = Document::new(128, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let wet = StrokeStyle {
            wet_pull: Some(pull),
            dilution: 0.84,
            load: 0.46,
            ..StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), wet);
        canvas.add_stamps(
            &(0..140)
                .map(|i| Stamp {
                    distance: i as f32 * 0.7,
                    ..Stamp::round(Vec2::new(10.0 + i as f32 * 0.7, 32.0), 4.0, 0.5)
                })
                .collect::<Vec<_>>(),
        );
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (pixel(&layer, 128, 14, 32)[3], pixel(&layer, 128, 100, 32)[3])
    };
    let (dry_start, dry_end) = stroke(0.0);
    let (kept_start, kept_end) = stroke(0.96);
    // loaded at the start either way; without pull the brush gives its paint up to the paper
    assert!(dry_start > 150 && kept_start > 150, "starts: {dry_start} and {kept_start}");
    assert!(dry_end + 20 < kept_end, "ends: no pull {dry_end}, pull {kept_end}");
    assert!(dry_end < dry_start, "no pull thins: {dry_start} to {dry_end}");
}

#[test]
fn a_stroke_blends_into_its_layer_by_its_mode() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    // a blue band down the middle, a mid grey one across the top
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 1.0], 1.0));
    canvas.add_stamps(&(0..64).map(|y| dot(Vec2::new(32.0, y as f32), 8.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.5, 0.5, 0.5], 1.0));
    canvas.add_stamps(&(0..64).map(|x| dot(Vec2::new(x as f32, 6.0), 4.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    // red multiplied across the blue: black over it, red off it
    let multiply = StrokeStyle { blend: 1, ..StrokeStyle::new(StrokeTool::Paint, [1.0, 0.0, 0.0], 1.0) };
    canvas.begin_stroke(doc.current(), multiply);
    canvas.add_stamps(&(0..64).map(|x| dot(Vec2::new(x as f32, 40.0), 4.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    // grey added to the grey: white
    let add = StrokeStyle { blend: 9, ..StrokeStyle::new(StrokeTool::Paint, [0.5, 0.5, 0.5], 1.0) };
    canvas.begin_stroke(doc.current(), add);
    canvas.add_stamps(&(0..20).map(|x| dot(Vec2::new(x as f32, 6.0), 4.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    let over = pixel(&layer, 64, 32, 40);
    assert!(over[0] < 8 && over[2] < 8 && over[3] == 255, "multiplied: {over:?}");
    let off = pixel(&layer, 64, 10, 40);
    assert!(off[0] > 248 && off[2] < 8, "red where nothing was: {off:?}");
    let added = pixel(&layer, 64, 10, 6);
    assert!(added[0] > 248 && added[1] > 248, "added: {added:?}");
    let plain = pixel(&layer, 64, 40, 6);
    assert!((120..=136).contains(&plain[0]), "grey not reached: {plain:?}");
}

#[test]
fn a_stroke_behind_goes_under_the_paint() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    // a blue band down the middle, then red laid behind it across
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 1.0], 1.0));
    canvas.add_stamps(&(0..64).map(|y| dot(Vec2::new(32.0, y as f32), 8.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    let behind = StrokeStyle {
        blend: procraft_engine::brush::BRUSH_BEHIND,
        ..StrokeStyle::new(StrokeTool::Paint, [1.0, 0.0, 0.0], 1.0)
    };
    canvas.begin_stroke(doc.current(), behind);
    canvas.add_stamps(&(0..64).map(|x| dot(Vec2::new(x as f32, 40.0), 4.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    assert_eq!(pixel(&layer, 64, 32, 40), [0, 0, 255, 255], "the band stays over it");
    let off = pixel(&layer, 64, 10, 40);
    assert!(off[0] > 248 && off[2] < 8 && off[3] > 248, "red where nothing was: {off:?}");
}

#[test]
fn stamps_mix_in_the_secondary_colour() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let style = StrokeStyle { secondary: [0.0, 0.0, 1.0], ..StrokeStyle::new(StrokeTool::Paint, [1.0, 0.0, 0.0], 1.0) };
    canvas.begin_stroke(doc.current(), style);
    canvas.add_stamps(&[dot(Vec2::new(16.0, 32.0), 6.0), Stamp { secondary: 1.0, ..dot(Vec2::new(48.0, 32.0), 6.0) }]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    assert_eq!(pixel(&layer, 64, 16, 32), [255, 0, 0, 255]);
    assert_eq!(pixel(&layer, 64, 48, 32), [0, 0, 255, 255]);
}

#[test]
fn the_grain_combines_with_the_stroke_by_its_blend_mode() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a half-strong dot over the paper grain, the grain multiplied or screened
    let cover = |grain_blend: u32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let style = StrokeStyle {
            grain: Some(0),
            grain_depth: 1.0,
            grain_scale: 64.0,
            grain_blend,
            ..StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 20.0, 0.5)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (24..40).map(|x| u32::from(pixel(&layer, 64, x, 32)[3])).sum::<u32>()
    };
    let (multiplied, screened) = (cover(1), cover(7));
    assert!(screened > multiplied + 16 * 40, "multiply {multiplied}, screen {screened}");
}

#[test]
fn a_grain_raises_faint_paint_only_in_part() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a faint dot and a light one laid normally over the paper grain: the grain lifts the light
    // one to about its own level, the faint one (a quarter of the way to full rise) far less
    let cover = |alpha: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let style = StrokeStyle {
            grain: Some(0),
            grain_depth: 1.0,
            grain_scale: 64.0,
            grain_blend: 0,
            ..StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0)
        };
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 20.0, alpha)]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (24..40).map(|x| u32::from(pixel(&layer, 64, x, 32)[3])).sum::<u32>()
    };
    let (faint, light) = (cover(0.004), cover(0.06));
    assert!(faint > 0 && faint * 2 < light, "faint {faint}, light {light}");
}

#[test]
fn a_tapered_round_tip_thins_its_rim_too() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // the same small stamp, as drawn and as a taper thinned a bigger one to: the tapered one
    // drawn as the whole stamp scaled down, its rim thinner
    let width = |taper: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let mut s = style(StrokeTool::Paint, 1.0);
        s.hardness = 0.95;
        canvas.begin_stroke(doc.current(), s);
        canvas.add_stamps(&[Stamp { taper, ..dot(Vec2::new(32.0, 32.0), 2.0) }]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let px = canvas.read_composite(&gpu).unwrap();
        (0..64).filter(|&x| pixel(&px, 64, x, 32)[0] < 128).count()
    };
    let (whole, tapered) = (width(1.0), width(0.2));
    assert!(tapered < whole, "{tapered} {whole}");
}

#[test]
fn a_stamp_taller_than_wide_draws_in_full() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
    // 8 px wide, 24 px tall
    canvas.add_stamps(&[Stamp { roundness: 3.0, ..dot(Vec2::new(32.0, 32.0), 4.0) }]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_composite(&gpu).unwrap();
    let painted = |x: u32, y: u32| pixel(&px, 64, x, y)[0] < 128;
    assert!(painted(32, 32 + 9) && painted(32, 32 - 9), "its height");
    assert!(!painted(32 + 7, 32), "its width");
}

#[test]
fn canvases_take_turns_with_a_shared_stroke_and_make_brush_images_when_wanted() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let shared = procraft_gpu::SharedStroke::new(&gpu, 64, 64);
    let mut docs = [Document::new(64, 64), Document::new(64, 64)];
    let mut a = Canvas::for_drawn_strokes(&gpu, &docs[0], &shared).unwrap();
    let mut b = Canvas::for_drawn_strokes(&gpu, &docs[1], &shared).unwrap();
    assert!(!a.has_brush_images() && !b.has_brush_images());
    assert!(a.textures().contains(shared.texture()) && b.textures().contains(shared.texture()));
    // a stroke on each in turn, through the one texture: each keeps only its own
    let [da, db] = &mut docs;
    a.begin_stroke(da.current(), style(StrokeTool::Paint, 1.0));
    a.add_stamps(&[dot(Vec2::new(16.0, 16.0), 6.0)]);
    a.end_stroke();
    a.render(&gpu, da, None);
    // stamps want the brush images: made then
    assert!(a.has_brush_images());
    b.begin_stroke(db.current(), style(StrokeTool::Paint, 1.0));
    b.add_stamps(&[dot(Vec2::new(48.0, 48.0), 6.0)]);
    b.end_stroke();
    b.render(&gpu, db, None);
    let (la, lb) = (a.read_layer(&gpu, da.current()).unwrap(), b.read_layer(&gpu, db.current()).unwrap());
    assert_eq!(pixel(&la, 64, 16, 16)[3], 255);
    assert_eq!(pixel(&la, 64, 48, 48)[3], 0);
    assert_eq!(pixel(&lb, 64, 48, 48)[3], 255);
    assert_eq!(pixel(&lb, 64, 16, 16)[3], 0);
    // let go, and an ordinary canvas made to draw in the shared texture
    a.let_go_of_brush_images(&gpu);
    assert!(!a.has_brush_images());
    let mut c = Canvas::new(&gpu, da).unwrap();
    assert!(c.draw_strokes_in(&gpu, &shared));
    assert!(c.textures().contains(shared.texture()));
}

#[test]
fn a_turned_grain_rolling_along_a_stroke_smears_along_it() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // how much the coverage changes from pixel to pixel along the middle of a straight stroke
    let roughness = |rotation: f32| {
        let mut doc = Document::new(400, 96);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let style = StrokeStyle {
            grain: Some(3),
            grain_mode: procraft_engine::brush::GrainMode::Moving,
            grain_depth: 1.0,
            grain_scale: 128.0,
            ..style(StrokeTool::Paint, 1.0)
        };
        canvas.begin_stroke(doc.current(), style);
        let stamps: Vec<Stamp> = (0..360)
            .map(|x| {
                let center = Vec2::new(20.0 + x as f32, 48.0);
                Stamp { grain_offset: center, grain_turn: rotation, ..Stamp::round(center, 30.0, 0.02) }
            })
            .collect();
        canvas.add_stamps(&stamps);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (120..280)
            .map(|x| i32::from(pixel(&layer, 400, x, 48)[3]).abs_diff(i32::from(pixel(&layer, 400, x + 1, 48)[3])))
            .sum::<u32>()
    };
    // unturned the grain stays put on the canvas, crisp under every stamp; turned half way round
    // each stamp reads it running the other way, so it blurs out along the stroke
    let (still, turned) = (roughness(0.0), roughness(std::f32::consts::PI));
    assert!(turned * 3 < still, "{turned} against {still}");
}

#[test]
fn a_drawn_stroke_takes_its_grain_from_a_picture_of_its_own() {
    use wgpu::util::DeviceExt;
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(32, 32);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    // the grain: black on the left half, white on the right (in as many layers as an array
    // needs to be read on this backend, as the app's are)
    let layers = gpu.array_layers_least();
    let pixels: Vec<u8> = (0..32 * 32 * layers).map(|i| if i % 32 < 16 { 0 } else { 255 }).collect();
    let picture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("stroke grain"),
            size: wgpu::Extent3d { width: 32, height: 32, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &pixels,
    );
    let grain = picture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    // a stroke with a grain fixed to the canvas, all of it deep, drawn into from elsewhere: a
    // band across the middle
    let grained = StrokeStyle {
        grain: Some(3),
        grain_depth: 1.0,
        grain_scale: 7.0,
        grain_rotation: 0.4,
        ..StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0)
    };
    canvas.begin_drawn_stroke(doc.current(), grained);
    canvas.set_stroke_grain(&gpu.device, &grain);
    // stamped into the stroke here (as a projection would draw it)
    canvas.add_stamps(&(0..32).map(|x| dot(Vec2::new(x as f32, 16.0), 5.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    // painted where the picture is white, not where it is black, whatever the brush's own grain
    assert_eq!(pixel(&layer, 32, 24, 16)[3], 255);
    assert_eq!(pixel(&layer, 32, 8, 16)[3], 0);
    // the picture goes with the stroke: the next one reads the brush's grains again
    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(8.0, 8.0), 3.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 32, 8, 8)[3], 255);
}

#[test]
fn the_grey_view_shows_each_colour_as_the_grey_of_its_luminance() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    // a background of each sRGB primary, shown in colour and in greys
    for (colour, grey) in [([1.0, 0.0, 0.0], 0.2126f32), ([0.0, 1.0, 0.0], 0.7152), ([0.0, 0.0, 1.0], 0.0722)] {
        let mut doc = Document::new(10, 10);
        doc.background = colour;
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 10, height: 10, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let to_screen = View::fit((10, 10), (10, 10)).to_screen();
        let shown = |canvas: &mut Canvas, doc: &mut Document, grey: bool| {
            canvas.render(
                &gpu,
                doc,
                Some(Target {
                    view: &view,
                    format,
                    size: (10, 10),
                    to_screen,
                    surround: [0.0; 3],
                    grid: None,
                    selection: None,
                    backdrop: None,
                    convert: None,
                    lut: None,
                    crisp_from: f32::INFINITY,
                    grey,
                }),
            );
            pixel(&read(&gpu, &target, 10, 10), 10, 5, 5)
        };
        let byte = |c: f32| (c * 255.0).round() as u8;
        assert_eq!(shown(&mut canvas, &mut doc, false), [byte(colour[0]), byte(colour[1]), byte(colour[2]), 255]);
        // the grey of the same luminance, through sRGB's tone curve
        let encoded = if grey <= 0.003_130_8 { grey * 12.92 } else { 1.055 * grey.powf(1.0 / 2.4) - 0.055 };
        let [r, g, b, a] = shown(&mut canvas, &mut doc, true);
        assert!(r == g && g == b && a == 255, "{colour:?} shown {r} {g} {b}");
        assert!(i32::from(r).abs_diff(i32::from(byte(encoded))) <= 1, "{colour:?}: {r}, not {}", byte(encoded));
        // the composite itself (what's read and recorded) keeps its colour
        assert_eq!(
            pixel(&canvas.read_composite(&gpu).unwrap(), 10, 5, 5)[..3],
            [byte(colour[0]), byte(colour[1]), byte(colour[2])]
        );
    }
}

#[test]
fn a_small_stamp_s_thin_shape_keeps_its_strength() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a shape that is an upright line, a sixteenth of its width across
    let n = procraft_engine::textures::TEXTURE_SIZE;
    let pixels: Vec<u8> = (0..n * n).map(|i| if (i % n).abs_diff(n / 2) < n / 32 { 255 } else { 0 }).collect();
    let laid = |filtering: f32| {
        let mut doc = Document::new(64, 64);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let mut brush = Brush::round_pen();
        brush.shape.source = procraft_engine::brush::Source::new(pixels.clone());
        brush.set_setting("shape_filtering", filtering);
        let mut s = StrokeStyle {
            rendering: Rendering::Strongest,
            ..StrokeStyle::for_brush(&brush, StrokeTool::Paint, [0.0, 0.0, 0.0], 1.0, false)
        };
        canvas.use_sources(&gpu, &mut s, &brush);
        canvas.begin_stroke(doc.current(), s);
        // stamps 2 px across, a fifth of a pixel apart: each lays its line where it passes
        let stamps: Vec<Stamp> = (0..100).map(|k| dot(Vec2::new(22.0 + k as f32 * 0.2, 32.0), 1.0)).collect();
        canvas.add_stamps(&stamps);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (26..38).map(|x| pixel(&layer, 64, x, 32)[3]).min().unwrap()
    };
    // at the level the size alone would pick the line is averaged to almost nothing, which the
    // strongest stamp can't build past: no coarser than 8 texels a side a quarter of it stays, and
    // no coarser than 16 with classic filtering half
    let (improved, classic) = (laid(2.0), laid(1.0));
    assert!(improved.abs_diff(63) < 12 && classic.abs_diff(127) < 24, "{improved} {classic}");
}

#[test]
fn a_shape_fills_the_quad_that_reaches_past_its_stamp() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a full shape on a stamp 2 px across, in the middle of a pixel: the quad reaches 0.71 px past
    // it each way, so pixels 1.5 px off are laid and pixels 2 px off aren't; squashed to a sliver
    // the quad still reaches as far across it, and a shape's stamp too small to see still lays
    let n = procraft_engine::textures::TEXTURE_SIZE;
    let laid = |stamp: Stamp| {
        let mut doc = Document::new(16, 16);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let mut brush = Brush::round_pen();
        brush.shape.source = procraft_engine::brush::Source::new(vec![255; n * n]);
        let mut s = style(StrokeTool::Paint, 1.0);
        canvas.use_sources(&gpu, &mut s, &brush);
        canvas.begin_stroke(doc.current(), s);
        canvas.add_stamps(&[stamp]);
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
        (
            pixel(&layer, 16, 9, 8)[3],
            pixel(&layer, 16, 10, 8)[3],
            pixel(&layer, 16, 8, 9)[3],
            pixel(&layer, 16, 8, 8)[3],
        )
    };
    let whole = laid(dot(Vec2::new(8.5, 8.5), 1.0));
    assert!(whole.0 > 240 && whole.2 > 240, "{whole:?}");
    let far = laid(dot(Vec2::new(7.9, 8.5), 1.0));
    assert!(far.1 == 0 && far.0 > 240, "{far:?}");
    let sliver = laid(Stamp { roundness: 0.05, ..dot(Vec2::new(8.5, 8.5), 4.0) });
    assert!(sliver.2 < 5 && sliver.3 > 240, "{sliver:?}");
    let sliver = laid(Stamp { roundness: 0.05, ..dot(Vec2::new(8.5, 9.0), 4.0) });
    assert!(sliver.2 > 240, "{sliver:?}");
    let tiny = laid(dot(Vec2::new(8.6, 8.5), 0.02));
    assert!(tiny.3 > 240 && tiny.0 == 0, "{tiny:?}");
}

/// A driver stops work that runs too long, and the device with it. A long stroke goes to the GPU
/// in pieces, its redrawing at the end too, each of them laid as it would have been in one.
#[test]
fn a_stroke_sent_in_pieces_lays_what_it_would_in_one() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let stamps = |n: usize| -> Vec<Stamp> {
        (0..n)
            .map(|i| Stamp {
                time_ms: i as f64 * 3.0,
                ..Stamp::round(Vec2::new(20.0 + i as f32 * 0.6, 40.0 + (i as f32 * 0.2).sin() * 20.0), 9.0, 0.4)
            })
            .collect()
    };
    let wet = StrokeStyle {
        wet_pull: Some(0.85),
        dilution: 0.5,
        load: 0.1,
        ..StrokeStyle::new(StrokeTool::Paint, [0.9, 0.2, 0.1], 1.0)
    };
    let styles = [
        ("paint", StrokeStyle::new(StrokeTool::Paint, [0.1, 0.4, 0.9], 0.7)),
        ("erase", style(StrokeTool::Erase, 0.8)),
        ("smudge", StrokeStyle::new(StrokeTool::Smudge, [0.0; 3], 1.0)),
        ("wet paint", wet),
    ];
    for (name, stroke) in styles {
        let laid = |work: Option<u64>| {
            let mut doc = Document::new(128, 96);
            let mut canvas = Canvas::new(&gpu, &doc).unwrap();
            if let Some(work) = work {
                canvas.set_work_at_once(work);
            }
            // something to erase, smudge and mix with
            canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.5, 0.6, 0.2], 1.0));
            canvas.add_stamps(&(0..96).map(|y| dot(Vec2::new(64.0, y as f32), 18.0)).collect::<Vec<_>>());
            canvas.end_stroke();
            canvas.render(&gpu, &mut doc, None);
            let before = canvas.pieces_sent();
            // drawn as it comes, then again whole as a stroke's end does
            canvas.begin_stroke(doc.current(), stroke);
            canvas.add_stamps(&stamps(100));
            canvas.render(&gpu, &mut doc, None);
            canvas.replace_stamps(&stamps(140));
            canvas.end_stroke();
            canvas.render(&gpu, &mut doc, None);
            (canvas.read_layer(&gpu, doc.current()).unwrap(), canvas.pieces_sent() - before)
        };
        let (whole, sent) = laid(None);
        assert_eq!(sent, 0, "{name}: this little goes as one piece");
        let (cut, sent) = laid(Some(1));
        assert!(sent > 100, "{name}: {sent} pieces");
        assert!(whole == cut, "{name}: laid differently in pieces");
    }
}

/// A long stroke's end redraws all of it in one render: no more stamps go up at once than a
/// buffer holds (here 50), the rest in renders after it, and the stroke comes out the same, the
/// strokes after it too.
#[test]
fn a_stroke_of_more_stamps_than_one_upload_takes_is_laid_whole() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let stamps = |n: usize, y: f32| -> Vec<Stamp> {
        (0..n)
            .map(|i| Stamp {
                time_ms: i as f64 * 3.0,
                ..Stamp::round(Vec2::new(10.0 + i as f32 * 0.5, y + (i as f32 * 0.1).sin() * 12.0), 7.0, 0.35)
            })
            .collect()
    };
    let styles = [
        ("paint", StrokeStyle::new(StrokeTool::Paint, [0.1, 0.4, 0.9], 0.7)),
        ("smudge", StrokeStyle::new(StrokeTool::Smudge, [0.0; 3], 1.0)),
    ];
    for (name, stroke) in styles {
        let laid = |most: Option<u64>| {
            let mut doc = Document::new(128, 96);
            let mut canvas = Canvas::new(&gpu, &doc).unwrap();
            if let Some(most) = most {
                canvas.set_stamps_at_once(most);
            }
            canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.5, 0.6, 0.2], 1.0));
            canvas.add_stamps(&(0..96).map(|y| dot(Vec2::new(64.0, y as f32), 18.0)).collect::<Vec<_>>());
            canvas.end_stroke();
            canvas.render(&gpu, &mut doc, None);
            // drawn as it comes, then again whole as a stroke's end does: 220 stamps at once
            canvas.begin_stroke(doc.current(), stroke);
            canvas.add_stamps(&stamps(120, 30.0));
            canvas.render(&gpu, &mut doc, None);
            canvas.replace_stamps(&stamps(220, 30.0));
            canvas.end_stroke();
            canvas.render(&gpu, &mut doc, None);
            // and one after it
            canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.9, 0.1, 0.3], 1.0));
            canvas.add_stamps(&stamps(80, 70.0));
            canvas.end_stroke();
            canvas.render(&gpu, &mut doc, None);
            canvas.read_layer(&gpu, doc.current()).unwrap()
        };
        let whole = laid(None);
        assert!(whole.chunks(4).any(|p| p[3] > 0), "{name}: something laid");
        assert!(whole == laid(Some(50)), "{name}: laid differently 50 stamps at a time");
    }
}

/// Behind with a fast stroke, a frame finds a tip for each input event since the last: only the
/// last is ever seen, so the others are left out, and the stroke comes out the same.
#[test]
fn a_tip_taken_off_before_it_shows_is_not_drawn() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let laid = |frames: usize| {
        let mut doc = Document::new(128, 128);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        canvas.set_work_at_once(1);
        canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 0.6));
        for i in 0..60 {
            let at = Vec2::new(20.0 + i as f32, 40.0);
            canvas.add_stamps(&[dot(at, 6.0)]);
            canvas.set_tip(&[dot(at + Vec2::new(6.0, 12.0), 14.0), dot(at + Vec2::new(12.0, 16.0), 14.0)]);
            if i % frames == 0 {
                canvas.render(&gpu, &mut doc, None);
            }
        }
        canvas.render(&gpu, &mut doc, None);
        let sent = canvas.pieces_sent();
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        (canvas.read_layer(&gpu, doc.current()).unwrap(), sent)
    };
    // each event its own frame, then a frame every twenty
    let (each, sent_each) = laid(1);
    let (behind, sent_behind) = laid(20);
    assert!(each == behind, "the stroke came out differently");
    // in one frame, sixty stamps and at most the last tip's two: none of the others were sent
    assert!(sent_behind < sent_each / 2 + 8, "{sent_behind} pieces of {sent_each}");
}

/// A stroke redrawn at its end goes to the GPU as many pieces however it was drawn, and a stamp
/// reaching past the canvas counts only for what it covers on it.
#[test]
fn a_long_stroke_of_large_stamps_is_more_than_one_piece() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(1024, 1024);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let across =
        |n: usize| -> Vec<Stamp> { (0..n).map(|i| dot(Vec2::new(512.0, 300.0 + (i % 400) as f32), 300.0)).collect() };
    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 0.5));
    canvas.replace_stamps(&across(4000));
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    assert!(canvas.pieces_sent() >= 16, "{} pieces", canvas.pieces_sent());
    assert!(pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 1024, 512, 512)[3] > 100);
    // the same stamps all off the canvas are next to nothing
    let sent = canvas.pieces_sent();
    canvas.begin_stroke(doc.current(), style(StrokeTool::Paint, 0.5));
    canvas.replace_stamps(&(0..4000).map(|_| dot(Vec2::new(-400.0, -400.0), 300.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(canvas.pieces_sent(), sent);
}

#[test]
fn a_hard_round_tip_s_thinnest_line_keeps_close_to_its_size() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // the ink across a line of hard round stamps (pixels of full paint, wherever the line falls
    // on the pixels), laid as a stroke spaces them
    let ink = |radius: f32, gap: f32| {
        let offsets = [0.0, 0.25, 0.5, 0.75];
        let total: f32 = offsets
            .iter()
            .map(|&off| {
                let mut doc = Document::new(64, 64);
                let mut canvas = Canvas::new(&gpu, &doc).unwrap();
                let mut s = style(StrokeTool::Paint, 1.0);
                s.hardness = 0.98;
                canvas.begin_stroke(doc.current(), s);
                let stamps: Vec<Stamp> = (0..)
                    .map(|i| 8.0 + i as f32 * gap)
                    .take_while(|&x| x < 56.0)
                    .map(|x| dot(Vec2::new(x, 32.0 + off), radius))
                    .collect();
                canvas.add_stamps(&stamps);
                canvas.end_stroke();
                canvas.render(&gpu, &mut doc, None);
                let px = canvas.read_composite(&gpu).unwrap();
                (16..48).map(|y| f32::from(255 - pixel(&px, 64, 32, y)[0]) / 255.0).sum::<f32>()
            })
            .sum();
        total / offsets.len() as f32
    };
    // the least tip (half a pixel across) lays a line about one and a half pixels of paint, not
    // the two and a quarter a tip reaching as far as a big one's would
    let least = ink(0.25, 0.6);
    assert!(least > 1.2 && least < 1.8, "{least}");
    // from about 6 px across a line is as wide as before: d + 1.5 or so
    let wide = ink(4.0, 1.35);
    assert!((wide - 9.5).abs() < 0.3, "{wide}");
}
