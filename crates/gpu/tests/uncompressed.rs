//! The canvas with its textures kept uncompressed, as on Mali GPUs (a test binary of its own: the
//! switch is the whole process's).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use procraft_engine::document::Document;
use procraft_engine::geom::Vec2;
use procraft_engine::stroke::Stamp;
use procraft_gpu::{Canvas, Gpu, StrokeStyle, StrokeTool};

fn pixel(px: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

#[test]
fn a_canvas_kept_uncompressed_paints_layers_and_undoes_as_any() {
    procraft_gpu::keep_textures_uncompressed(true);
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) => return eprintln!("skipped: {e}"),
    };
    let mut doc = Document::new(64, 64);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    // a layer under the one painted on, so the live picture goes through the caches of what is
    // below and above
    let below = doc.current();
    doc.add_layer();
    canvas.render(&gpu, &mut doc, None);
    canvas.begin_stroke(below, StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0));
    canvas.add_stamps(&[Stamp::round(Vec2::new(20.0, 20.0), 6.0, 1.0)]);
    canvas.render(&gpu, &mut doc, None);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, [0.0; 3], 1.0));
    for i in 0..8 {
        canvas.add_stamps(&[Stamp::round(Vec2::new(36.0 + i as f32 * 2.0, 40.0), 5.0, 1.0)]);
        canvas.render(&gpu, &mut doc, None);
        // drawn live, a part at a time
        assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 64, 36, 40), [0, 0, 0, 255], "{i}");
    }
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let px = canvas.read_composite(&gpu).unwrap();
    assert_eq!(pixel(&px, 64, 20, 20), [0, 0, 0, 255]);
    assert_eq!(pixel(&px, 64, 44, 40), [0, 0, 0, 255]);
    assert_eq!(pixel(&px, 64, 2, 2), [255, 255, 255, 255]);
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 64, 44, 40), [255, 255, 255, 255]);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
}
