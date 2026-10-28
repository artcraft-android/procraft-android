//! The canvas on a real GPU (skipped where the machine has none).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use procraft_engine::brush::{Brush, Rendering, Taper};
use procraft_engine::document::Document;
use procraft_engine::geom::{Homography, Vec2};
use procraft_engine::input::{Pointer, Sample};
use procraft_engine::profile::Profile;
use procraft_engine::stroke::{Stamp, StrokeBuilder, Tint};
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
        }),
    );
    let px = read(&gpu, &target, 200, 100);
    assert_eq!(pixel(&px, 200, 10, 50), [51, 51, 51, 255], "surround");
    assert_eq!(pixel(&px, 200, 75, 25), [0, 0, 0, 255], "the dot");
    assert_eq!(pixel(&px, 200, 125, 75), [255, 255, 255, 255], "canvas");
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
            backdrop: Some(Backdrop { regions: &regions, sigma: 24.0 }),
            convert: None,
            lut: None,
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
        Filter::Zoom { centre: [0.0, 0.0], amount: 0.3 },
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

#[test]
fn liquify_pushes_paint_along_and_reconstructs() {
    use procraft_gpu::{Filter, LiquifyMode, LiquifyStamp};
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    canvas.begin_stroke(layer, style(StrokeTool::Paint, 1.0));
    canvas.add_stamps(&[dot(Vec2::new(20.0, 32.0), 6.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    canvas.begin_transform(layer);
    canvas.begin_liquify();
    // pushed 10 to the right, in small steps
    let push = |x: f32| LiquifyStamp {
        centre: [x, 32.0],
        moved: [2.0, 0.0],
        radius: 20.0,
        pressure: 1.0,
        distortion: 0.0,
        seed: 0.0,
        mode: LiquifyMode::Push,
    };
    canvas.liquify_stamps(&[push(20.0), push(22.0), push(24.0), push(26.0), push(28.0)]);
    canvas.set_filter(Filter::Liquify { amount: 1.0 });
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, layer).unwrap();
    assert!(pixel(&px, 64, 28, 32)[3] > 200, "{:?}", pixel(&px, 64, 28, 32));
    assert!(pixel(&px, 64, 15, 32)[3] < 50, "{:?}", pixel(&px, 64, 15, 32));
    // reconstructed many times over: back where it was
    let back = LiquifyStamp { mode: LiquifyMode::Reconstruct, ..push(26.0) };
    canvas.liquify_stamps(&vec![back; 60]);
    canvas.set_filter(Filter::Liquify { amount: 1.0 });
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_layer(&gpu, layer).unwrap();
    assert!(pixel(&px, 64, 15, 32)[3] > 200, "{:?}", pixel(&px, 64, 15, 32));
    canvas.liquify_clear();
    canvas.end_transform(true);
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
fn wet_paint_mixes_with_what_is_under_it() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    // a grey band down the middle
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.5, 0.5, 0.5], 1.0));
    canvas.add_stamps(&(0..64).map(|y| dot(Vec2::new(32.0, y as f32), 8.0)).collect::<Vec<_>>());
    canvas.end_stroke();
    // white across it, wet (half water) with a strong pull: over the grey it mixes, and off it
    // the watery paint it carries goes on thin
    let wet = StrokeStyle {
        wet_pull: Some(0.85),
        dilution: 0.5,
        ..StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0)
    };
    canvas.begin_stroke(doc.current(), wet);
    canvas.add_stamps(
        &(0..120).map(|i| Stamp::round(Vec2::new(4.0 + i as f32 * 0.5, 32.0), 4.0, 0.5)).collect::<Vec<_>>(),
    );
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer(&gpu, doc.current()).unwrap();
    let over = pixel(&layer, 64, 32, 32);
    assert!(over[0] > 135 && over[0] < 250, "mixed over the band: {over:?}");
    let past = pixel(&layer, 64, 50, 32);
    assert!(past[3] > 10 && past[3] < 220, "thin past the band: {past:?}");
    // as one undo step
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    let back = pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 64, 32, 32);
    assert!((126..=130).contains(&back[0]), "{back:?}");
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
    assert!(hard > soft, "attack {hard} vs none {soft}");
    assert!(hard >= 250);
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
