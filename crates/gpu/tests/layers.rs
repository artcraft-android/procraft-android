//! Layers on a real GPU: blend modes, clipping, alpha lock, merge, duplicate, delete and their
//! undo (skipped where the machine has none).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use procraft_engine::document::{BlendMode, Document, LayerId};
use procraft_engine::geom::Vec2;
use procraft_engine::stroke::Stamp;
use procraft_gpu::{Canvas, Gpu, StrokeStyle, StrokeTool};

const W: u32 = 16;

fn setup() -> Option<(Gpu, Document, Canvas)> {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipped: {e}");
            return None;
        }
    };
    let doc = Document::new(W, W);
    let canvas = Canvas::new(&gpu, &doc).unwrap();
    Some((gpu, doc, canvas))
}

fn paint(canvas: &mut Canvas, doc: &Document, layer: LayerId, rgb: [u8; 3], at: Vec2, radius: f32, tool: StrokeTool) {
    let color = rgb.map(|c| f32::from(c) / 255.0);
    let preserve_alpha = doc.layer(layer).is_some_and(|l| l.alpha_lock);
    canvas.begin_stroke(layer, StrokeStyle { preserve_alpha, ..StrokeStyle::new(tool, color, 1.0) });
    canvas.add_stamps(&[Stamp::round(at, radius, 1.0)]);
    canvas.end_stroke();
}

/// Covers the whole canvas.
fn fill(canvas: &mut Canvas, doc: &Document, layer: LayerId, rgb: [u8; 3]) {
    paint(canvas, doc, layer, rgb, Vec2::new(8.0, 8.0), 100.0, StrokeTool::Paint);
}

fn composite_at(gpu: &Gpu, doc: &mut Document, canvas: &mut Canvas, x: u32, y: u32) -> [u8; 4] {
    canvas.render(gpu, doc, None);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
    let px = canvas.read_composite(gpu).unwrap();
    let i = ((y * W + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

fn layer_at(gpu: &Gpu, doc: &mut Document, canvas: &mut Canvas, id: LayerId, x: u32, y: u32) -> [u8; 4] {
    canvas.render(gpu, doc, None);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
    let px = canvas.read_layer(gpu, id).unwrap();
    let i = ((y * W + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

/// The usual compositing definitions on 0..1 values, for opaque colours.
fn reference(mode: BlendMode, b: f32, s: f32) -> f32 {
    match mode {
        BlendMode::Multiply => b * s,
        BlendMode::Screen => b + s - b * s,
        BlendMode::Darken => b.min(s),
        BlendMode::Lighten => b.max(s),
        BlendMode::Difference => (b - s).abs(),
        BlendMode::Exclusion => b + s - 2.0 * b * s,
        BlendMode::Add => (b + s).min(1.0),
        BlendMode::Subtract => (b - s).max(0.0),
        BlendMode::LinearBurn => (b + s - 1.0).max(0.0),
        BlendMode::Overlay => {
            if b <= 0.5 {
                s * 2.0 * b
            } else {
                let t = 2.0 * b - 1.0;
                s + t - s * t
            }
        }
        _ => s,
    }
}

#[test]
fn separable_blend_modes_match_the_definitions() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let bottom = doc.current();
    let top = doc.add_layer();
    let (b, s) = ([200u8, 90, 30], [60u8, 140, 250]);
    fill(&mut canvas, &doc, bottom, b);
    fill(&mut canvas, &doc, top, s);
    for mode in [
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Add,
        BlendMode::Subtract,
        BlendMode::LinearBurn,
        BlendMode::Overlay,
        BlendMode::Normal,
    ] {
        doc.set_blend(top, mode);
        canvas.invalidate();
        let got = composite_at(&gpu, &mut doc, &mut canvas, 8, 8);
        for c in 0..3 {
            let want = (reference(mode, f32::from(b[c]) / 255.0, f32::from(s[c]) / 255.0) * 255.0).round() as i32;
            assert!((i32::from(got[c]) - want).abs() <= 2, "{mode:?} channel {c}: got {} want {want}", got[c]);
        }
        assert_eq!(got[3], 255);
    }
}

#[test]
fn shade_darkens_and_shows_nothing_over_clear() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let bottom = doc.current();
    let top = doc.add_layer();
    let (b, s) = ([200u8, 90, 30], [60u8, 140, 250]);
    fill(&mut canvas, &doc, bottom, b);
    fill(&mut canvas, &doc, top, s);
    doc.set_blend(top, BlendMode::Shade);
    canvas.invalidate();
    let got = composite_at(&gpu, &mut doc, &mut canvas, 8, 8);
    for c in 0..3 {
        assert!((i32::from(got[c]) - i32::from(b[c].min(s[c]))).abs() <= 2, "channel {c}: {got:?}");
    }
    // with nothing below it, a shaded layer adds nothing
    doc.set_visible(bottom, false);
    doc.background_visible = false;
    canvas.invalidate();
    let got = composite_at(&gpu, &mut doc, &mut canvas, 8, 8);
    assert!(got[3] <= 1, "{got:?}");
}

#[test]
fn colour_modes_keep_the_backdrop_lightness() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let bottom = doc.current();
    let top = doc.add_layer();
    fill(&mut canvas, &doc, bottom, [128, 128, 128]);
    fill(&mut canvas, &doc, top, [255, 0, 0]);
    doc.set_blend(top, BlendMode::Color);
    let [r, g, b, _] = composite_at(&gpu, &mut doc, &mut canvas, 8, 8);
    // red hue at grey's lightness: reddish, and about as light (0.3 r + 0.59 g + 0.11 b ≈ 128)
    assert!(r > g && r > b, "{r} {g} {b}");
    let lum = 0.3 * f32::from(r) + 0.59 * f32::from(g) + 0.11 * f32::from(b);
    assert!((lum - 128.0).abs() < 4.0, "lum {lum}");
    doc.set_blend(top, BlendMode::Luminosity);
    canvas.invalidate();
    let [r, g, b, _] = composite_at(&gpu, &mut doc, &mut canvas, 8, 8);
    assert!(r == g && g == b, "grey stays grey: {r} {g} {b}");
}

#[test]
fn opacity_visibility_and_clipping() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let base = doc.current();
    let clipped = doc.add_layer();
    // the base covers the left part only; the clipped layer covers everything
    paint(&mut canvas, &doc, base, [0, 0, 255], Vec2::new(2.0, 8.0), 4.0, StrokeTool::Paint);
    fill(&mut canvas, &doc, clipped, [255, 0, 0]);
    doc.set_clipped(clipped, true);
    canvas.invalidate();
    assert_eq!(composite_at(&gpu, &mut doc, &mut canvas, 2, 8), [255, 0, 0, 255]);
    assert_eq!(composite_at(&gpu, &mut doc, &mut canvas, 14, 8), [255, 255, 255, 255], "clipped away");
    doc.set_opacity(clipped, 0.5);
    canvas.invalidate();
    let [r, _, b, _] = composite_at(&gpu, &mut doc, &mut canvas, 2, 8);
    assert!((126..=129).contains(&r) && (126..=129).contains(&b), "half red over blue: {r} {b}");
    // hiding the base hides what is clipped to it
    doc.set_visible(base, false);
    canvas.invalidate();
    assert_eq!(composite_at(&gpu, &mut doc, &mut canvas, 2, 8), [255, 255, 255, 255]);
}

#[test]
fn alpha_lock_recolours_only_painted_pixels() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let layer = doc.current();
    paint(&mut canvas, &doc, layer, [0, 0, 0], Vec2::new(4.0, 4.0), 3.0, StrokeTool::Paint);
    doc.set_alpha_lock(layer, true);
    fill(&mut canvas, &doc, layer, [255, 0, 0]);
    assert_eq!(layer_at(&gpu, &mut doc, &mut canvas, layer, 4, 4), [255, 0, 0, 255]);
    assert_eq!(layer_at(&gpu, &mut doc, &mut canvas, layer, 12, 12)[3], 0, "outside stays empty");
}

#[test]
fn merge_down_and_undo() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let lower = doc.current();
    let upper = doc.add_layer();
    fill(&mut canvas, &doc, lower, [200, 200, 200]);
    fill(&mut canvas, &doc, upper, [100, 50, 0]);
    doc.set_blend(upper, BlendMode::Multiply);
    let before = doc.clone();
    canvas.merge_down(upper, lower, before);
    assert!(doc.finish_merge_down(upper));
    assert_eq!(doc.layers().len(), 1);
    let merged = layer_at(&gpu, &mut doc, &mut canvas, lower, 8, 8);
    assert_eq!(merged[..3], [78, 39, 0], "200 × 100 / 255 and so on");
    // the composite is unchanged by merging
    assert_eq!(composite_at(&gpu, &mut doc, &mut canvas, 8, 8)[..3], [78, 39, 0]);

    canvas.undo();
    assert_eq!(layer_at(&gpu, &mut doc, &mut canvas, lower, 8, 8), [200, 200, 200, 255]);
    assert_eq!(doc.layers().len(), 2);
    assert_eq!(layer_at(&gpu, &mut doc, &mut canvas, upper, 8, 8), [100, 50, 0, 255]);
    canvas.redo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(doc.layers().len(), 1);
}

#[test]
fn duplicate_delete_and_undo_keep_pixels() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let a = doc.current();
    paint(&mut canvas, &doc, a, [0, 255, 0], Vec2::new(8.0, 8.0), 3.0, StrokeTool::Paint);
    let before = doc.clone();
    let b = doc.duplicate_layer(a).unwrap();
    canvas.duplicate_layer(a, b, before);
    assert_eq!(layer_at(&gpu, &mut doc, &mut canvas, b, 8, 8), [0, 255, 0, 255]);

    let before = doc.clone();
    assert!(doc.delete_layer(a));
    canvas.record_document(before);
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(doc.layers().len(), 1);
    // the deleted layer's pixels are kept while undo can bring it back
    assert_eq!(canvas.layer_texture_count(), 2);
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(doc.layers().len(), 2);
    assert_eq!(layer_at(&gpu, &mut doc, &mut canvas, a, 8, 8), [0, 255, 0, 255]);
}

#[test]
fn thumbnails_are_scaled_layers() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let a = doc.current();
    fill(&mut canvas, &doc, a, [10, 20, 30]);
    let t = canvas.layer_thumbnail(&gpu, &mut doc, a, 8).unwrap();
    assert_eq!((t.width, t.height), (8, 8));
    assert_eq!(t.rgba.len(), 8 * 8 * 4);
    assert_eq!(t.rgba[..4], [10, 20, 30, 255]);
}

#[test]
fn a_thumbnail_shows_what_is_drawn_never_enlarged() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let a = doc.current();
    // a 2 x 3 block near a corner of the 16 x 16 layer
    let mut pixels = vec![0u8; 16 * 16 * 4];
    for (x, y) in [(12, 1), (13, 1), (12, 2), (13, 2), (12, 3), (13, 3)] {
        pixels[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4].copy_from_slice(&[0, 0, 255, 255]);
    }
    canvas.write_layer(a, pixels);
    let t = canvas.layer_thumbnail(&gpu, &mut doc, a, 8).unwrap();
    assert_eq!((t.width, t.height), (2, 3));
    assert!(t.rgba.chunks(4).all(|p| p[3] > 0));
    // an empty layer: the whole canvas, fitted
    let b = doc.add_layer();
    let t = canvas.layer_thumbnail(&gpu, &mut doc, b, 8).unwrap();
    assert_eq!((t.width, t.height), (8, 8));
}

#[test]
fn smudge_drags_colour_and_undoes() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let layer = doc.current();
    // red on the left half, blue on the right
    paint(&mut canvas, &doc, layer, [255, 0, 0], Vec2::new(-92.0, 8.0), 100.0, StrokeTool::Paint);
    paint(&mut canvas, &doc, layer, [0, 0, 255], Vec2::new(108.0, 8.0), 100.0, StrokeTool::Paint);
    let before = layer_at(&gpu, &mut doc, &mut canvas, layer, 11, 8);
    assert_eq!(before[..3], [0, 0, 255]);
    // drag from the red into the blue
    let stamps: Vec<Stamp> = (0..30).map(|i| Stamp::round(Vec2::new(2.0 + i as f32 * 0.4, 8.0), 4.0, 1.0)).collect();
    canvas.begin_stroke(layer, StrokeStyle::new(StrokeTool::Smudge, [0.0; 3], 1.0));
    canvas.add_stamps(&stamps);
    let during = layer_at(&gpu, &mut doc, &mut canvas, layer, 11, 8);
    assert!(during[0] > 60, "red carried into the blue: {during:?}");
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    assert!(canvas.can_undo());
    canvas.undo();
    assert_eq!(layer_at(&gpu, &mut doc, &mut canvas, layer, 11, 8), before);

    // a cancelled smudge leaves the layer as it was
    canvas.begin_stroke(layer, StrokeStyle::new(StrokeTool::Smudge, [0.0; 3], 1.0));
    canvas.add_stamps(&stamps);
    canvas.render(&gpu, &mut doc, None);
    canvas.cancel_stroke();
    assert_eq!(layer_at(&gpu, &mut doc, &mut canvas, layer, 11, 8), before);
}

#[test]
fn a_held_edit_is_one_undo_step_or_put_back() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let a = doc.current();
    fill(&mut canvas, &doc, a, [10, 20, 30]);
    canvas.render(&gpu, &mut doc, None);
    let steps = |c: &Canvas| c.can_undo();
    assert!(steps(&canvas));
    // many puts, kept: one step back to the fill
    let before = doc.clone();
    canvas.hold_layer(a);
    for v in [100u8, 150, 200] {
        canvas.put_layer(a, [v, v, v, 255].repeat((W * W) as usize));
    }
    canvas.release_layer(true, Some(before));
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(layer_at(&gpu, &mut doc, &mut canvas, a, 3, 3), [200, 200, 200, 255]);
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(layer_at(&gpu, &mut doc, &mut canvas, a, 3, 3), [10, 20, 30, 255]);
    // put back: as if nothing happened
    canvas.hold_layer(a);
    canvas.put_layer(a, [9, 9, 9, 255].repeat((W * W) as usize));
    canvas.release_layer(false, None);
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(layer_at(&gpu, &mut doc, &mut canvas, a, 3, 3), [10, 20, 30, 255]);
}
