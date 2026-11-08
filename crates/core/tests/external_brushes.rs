//! Imports a brush file from outside and reports what came in and what it draws. Run with
//! `BRUSH_FILE=/path/to/file.abr cargo test -p procraft-core --test external_brushes -- --ignored --nocapture`.

use std::sync::Arc;

use procraft_core::{CanvasSession, PenSample, PointerKind};

fn session() -> Option<Arc<CanvasSession>> {
    let s = CanvasSession::new(512, 512);
    if !s.prepare_offscreen() {
        return None;
    }
    s.resize_window(512, 512);
    s.fit_view();
    Some(s)
}

#[test]
#[ignore]
fn an_external_file_imports_and_draws() {
    let path = std::env::var("BRUSH_FILE").expect("BRUSH_FILE");
    let bytes = std::fs::read(&path).expect("the brush file");
    if std::env::var("BRUSH_DUMP").is_ok() {
        if let Some(brushes) = procraft_engine::abr::read_brushes(&bytes) {
            for b in &brushes {
                let tip = b.tip.as_ref().map(|t| {
                    let n = t.pixels.len().max(1);
                    (
                        t.width,
                        t.height,
                        t.pixels.iter().map(|&v| u32::from(v)).sum::<u32>() / n as u32,
                        t.pixels.iter().copied().max(),
                    )
                });
                let tex = b.texture.as_ref().map(|t| {
                    let n = t.pixels.len().max(1);
                    (
                        t.width,
                        t.height,
                        t.invert,
                        t.depth,
                        t.blend.clone(),
                        t.pixels.iter().map(|&v| u32::from(v)).sum::<u32>() / n as u32,
                        t.pixels.iter().copied().min(),
                        t.pixels.iter().copied().max(),
                    )
                });
                if b.name.contains("Deharme") || b.name.contains("Starfield") || b.name.contains("Sol") {
                    println!(
                        "ABR {:?}: tip {tip:?} texture {tex:?} spacing {:?} diameter {:?} dynamics {:?}",
                        b.name, b.spacing, b.diameter, b.dynamics
                    );
                }
            }
        }
    }
    let Some(s) = session() else { return };
    let before = s.brush_library().len();
    let Some(set) = s.import_any_brushes(bytes, path.rsplit('/').next().map(str::to_owned), 0) else {
        panic!("{path} did not import");
    };
    let library = s.brush_library();
    assert!(library.len() > before, "{path} imported no set");
    let entry = &library[set as usize];
    println!("{path}: set {:?}, {} brushes", entry.name, entry.brushes.len());
    let mut blank = Vec::new();
    let mut painted = 0;
    for brush in &entry.brushes {
        let Some(p) = s.brush_preview(brush.set, brush.index, 240, 60) else {
            blank.push(format!("{}: no preview", brush.name));
            continue;
        };
        let pixels = p.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count();
        if pixels < 100 {
            blank.push(format!("{}: {pixels} px", brush.name));
            if std::env::var("BRUSH_PREVIEW_PROBE").is_ok() {
                s.select_brush(brush.set, brush.index);
                s.studio_begin();
                let keys = ["preview_size", "preview_stamp", "preview_pressure_min", "preview_pressure_scale", "max_size", "min_size", "flow", "pressure_opacity", "pressure_flow", "scatter", "count", "spacing"]
                    .iter().map(|k| (*k).to_owned()).collect();
                let vals = s.studio_settings(keys);
                s.set_studio_setting("preview_size".to_owned(), 0.6);
                let after = s.brush_preview(brush.set, brush.index, 240, 60).map(|p| p.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).count());
                s.studio_end(false);
                println!("  preview {} settings {vals:?}, at 0.6: {after:?}", brush.name);
            }
        } else {
            painted += 1;
        }
    }
    println!(
        "{painted} brushes draw a preview; {} weak/blank: {:?}",
        blank.len(),
        blank.iter().take(20).collect::<Vec<_>>()
    );
    if std::env::var("BRUSH_ALL").is_ok() {
        // each brush's own stroke on a clear layer, to see whether a blank preview is only a
        // preview or the brush itself
        let mut dead = Vec::new();
        for brush in &entry.brushes {
            s.select_brush(brush.set, brush.index);
            s.set_brush_size(0.5);
            s.set_color(0.0, 0.0, 0.0);
            let layer = s.layers().last().map(|l| l.id).expect("a layer");
            s.select_layer(layer);
            s.clear_layer();
            let sample = |x: f32| PenSample {
                x,
                y: 256.0,
                pressure: 0.8,
                tilt: 0.0,
                orientation: 0.0,
                time_ms: f64::from(x) * 2.0,
                roll: 0.0,
            };
            s.stroke_begin(PointerKind::Stylus, vec![sample(96.0)]);
            s.stroke_add((1..=24).map(|i| sample(96.0 + i as f32 * 8.0)).collect());
            s.stroke_end(vec![]);
            let image = s.layer_thumbnail(layer, 512).expect("a stroke");
            let touched = image.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 8).count();
            if touched < 50 {
                dead.push(format!("{}: {touched} px", brush.name));
                s.select_brush(brush.set, brush.index);
                s.studio_begin();
                let keys = [
                    "spacing",
                    "flow",
                    "max_size",
                    "min_size",
                    "count",
                    "pressure_size",
                    "pressure_opacity",
                    "grain_depth",
                    "preview_size",
                    "rendering_mode",
                    "scatter",
                    "jitter_size",
                ]
                .iter()
                .map(|k| (*k).to_owned())
                .collect();
                let vals = s.studio_settings(keys);
                s.studio_end(false);
                println!("  dead {} settings {vals:?}", brush.name);
                // try full pressure and size, then with the grain taken out
                let probe = |s: &CanvasSession, pressure: f32| {
                    s.set_brush_size(1.0);
                    let layer = s.layers().last().map(|l| l.id).expect("a layer");
                    s.select_layer(layer);
                    s.clear_layer();
                    let sample = |x: f32| PenSample {
                        x,
                        y: 256.0,
                        pressure,
                        tilt: 0.0,
                        orientation: 0.0,
                        time_ms: f64::from(x) * 2.0,
                        roll: 0.0,
                    };
                    s.stroke_begin(PointerKind::Stylus, vec![sample(96.0)]);
                    s.stroke_add((1..=24).map(|i| sample(96.0 + i as f32 * 8.0)).collect());
                    s.stroke_end(vec![]);
                    let image = s.layer_thumbnail(layer, 512).expect("a stroke");
                    image.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 8).count()
                };
                let full = probe(&s, 1.0);
                s.select_brush(brush.set, brush.index);
                s.studio_begin();
                s.set_studio_setting("grain_depth".to_owned(), 0.0);
                s.studio_end(true);
                let dry = probe(&s, 1.0);
                println!("  dead {}: full pressure {full} px, without grain {dry} px", brush.name);
            }
        }
        println!("{} brushes draw no stroke: {:?}", dead.len(), dead.iter().take(30).collect::<Vec<_>>());
    }
    // the first brush, a pressure stroke
    let first = &entry.brushes[0];
    s.select_brush(first.set, first.index);
    s.set_brush_size(0.6);
    s.set_color(0.0, 0.0, 0.0);
    let layer = s.layers().last().map(|l| l.id).expect("a layer");
    s.select_layer(layer);
    let sample = |x: f32, p: f32| PenSample {
        x,
        y: 256.0,
        pressure: p,
        tilt: 0.0,
        orientation: 0.0,
        time_ms: f64::from(x) * 2.0,
        roll: 0.0,
    };
    s.stroke_begin(PointerKind::Stylus, vec![sample(96.0, 0.2)]);
    s.stroke_add((1..=32).map(|i| sample(96.0 + i as f32 * 8.0, 0.2 + i as f32 / 32.0 * 0.75)).collect());
    s.stroke_end(vec![]);
    let image = s.layer_thumbnail(layer, 512).expect("a stroke");
    let touched = image.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 8).count();
    println!("{}: {touched} px from a pressure stroke", first.name);
    assert!(touched > 200, "{} drew nothing", first.name);
}
