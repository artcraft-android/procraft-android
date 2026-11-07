#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Brush parity with the Procreate kit: the sets and key brushes are there, each draws a stroke of
//! its own, and the pressure, grain and wet controls do what their names say.

use std::sync::Arc;

use procraft_core::{CanvasSession, PenSample, PointerKind};

fn session() -> Option<Arc<CanvasSession>> {
    let s = CanvasSession::new(256, 256);
    if !s.prepare_offscreen() {
        return None;
    }
    s.resize_window(256, 256);
    s.fit_view();
    Some(s)
}

fn find(s: &CanvasSession, set: &str, name: &str) -> Option<(u32, u32)> {
    let lib = s.brush_library();
    let set = lib.iter().find(|s| s.name == set)?;
    let brush = set.brushes.iter().find(|b| b.name == name)?;
    Some((brush.set, brush.index))
}

fn setting(s: &CanvasSession, set: &str, name: &str, key: &str) -> Option<f32> {
    let (set, index) = find(s, set, name)?;
    assert!(s.select_brush(set, index));
    s.studio_begin();
    let value = s.studio_settings(vec![key.to_owned()]).first().copied();
    s.studio_end(false);
    value
}

/// A horizontal pen stroke across the canvas at `pressure`, and what it laid: pixels touched and
/// the darkest alpha along its middle.
fn stroke(s: &CanvasSession, set: &str, name: &str, pressure: f32) -> (usize, u128) {
    let (set, index) = find(s, set, name).expect("the brush");
    assert!(s.select_brush(set, index));
    s.studio_begin();
    s.set_studio_setting("streamline".to_owned(), 0.0);
    s.studio_end(true);
    s.set_brush_size(0.5);
    s.set_color(0.0, 0.0, 0.0);
    let layer = s.layers().last().map(|l| l.id).expect("a layer");
    s.select_layer(layer);
    let sample = |x: f32, p: f32| PenSample {
        x,
        y: 128.0,
        pressure: p,
        tilt: 0.0,
        orientation: 0.0,
        time_ms: f64::from(x) * 2.0,
        roll: 0.0,
    };
    s.stroke_begin(PointerKind::Stylus, vec![sample(32.0, pressure)]);
    s.stroke_add((1..=24).map(|i| sample(32.0 + i as f32 * 8.0, pressure)).collect());
    s.stroke_end(vec![]);
    let image = s.layer_thumbnail(layer, 256).expect("the stroke's picture");
    let touched = image.rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 8).count();
    let alpha: u128 = image.rgba.as_chunks::<4>().0.iter().map(|p| u128::from(p[3])).sum();
    (touched, alpha)
}

#[test]
fn procreate_sets_and_key_brushes_are_there() {
    let Some(s) = session() else { return };
    let lib = s.brush_library();
    for (set, least) in [
        ("Sketching", 6),
        ("Inking", 6),
        ("Drawing", 5),
        ("Painting", 6),
        ("Watercolour", 5),
        ("Calligraphy", 5),
        ("Airbrushing", 5),
        ("Spray Paint", 4),
        ("Charcoals", 5),
        ("Textures", 6),
        ("Abstract", 5),
    ] {
        let found = lib.iter().find(|s| s.name == set).unwrap_or_else(|| panic!("set {set}"));
        assert!(found.brushes.len() >= least, "{set}: {} brushes", found.brushes.len());
    }
    let total: usize = lib.iter().map(|s| s.brushes.len()).sum();
    assert!(total >= 150, "{total} brushes");
    // the best known brushes of the kit, each in the set Procreate keeps it in
    for (set, name) in [
        ("Sketching", "6B Pencil"),
        ("Inking", "Round Pen"),
        ("Painting", "Round Brush"),
        ("Airbrushing", "Soft Airbrush"),
        ("Charcoals", "Charcoal Stick"),
        ("Watercolour", "Wet Wash"),
        ("Calligraphy", "Script Brush"),
        ("Textures", "Canvas Texture"),
        ("Spray Paint", "Spray Can"),
    ] {
        assert!(find(&s, set, name).is_some(), "{name} in {set}");
    }
    // no two shipped brushes look the same in their own preview: the brush engine's whole range
    let mut seen = std::collections::HashMap::new();
    let mut same = Vec::new();
    for set in &lib {
        for brush in &set.brushes {
            let preview = s.brush_preview(brush.set, brush.index, 240, 60).expect("a preview");
            let hash = preview
                .rgba
                .iter()
                .fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
            if let Some(other) = seen.insert(hash, format!("{}/{}", set.name, brush.name)) {
                same.push((other, format!("{}/{}", set.name, brush.name)));
            }
        }
    }
    assert!(same.is_empty(), "brushes that draw the same preview: {same:?}");
}

#[test]
fn the_key_brushes_keep_their_procreate_character() {
    let Some(s) = session() else { return };
    // a pencil answers pressure with opacity and grain, not with a hard, clean edge
    assert!(setting(&s, "Sketching", "6B Pencil", "pressure_opacity").unwrap_or(0.0) >= 0.5);
    assert!(setting(&s, "Sketching", "6B Pencil", "grain_depth").unwrap_or(0.0) >= 0.5);
    // an airbrush is soft and answers pressure with opacity, not size
    assert!(setting(&s, "Airbrushing", "Soft Airbrush", "pressure_opacity").unwrap_or(0.0) >= 0.5);
    assert_eq!(setting(&s, "Airbrushing", "Soft Airbrush", "pressure_size"), Some(0.0));
    // an inking pen answers pressure with size and tapers
    assert!(setting(&s, "Inking", "Studio Ink", "pressure_size").unwrap_or(0.0) >= 0.5);
    // oil paint mixes wet; charcoal is grain
    assert!(setting(&s, "Painting", "Oil Paint", "wet_pull").unwrap_or(0.0) >= 0.3);
    assert!(setting(&s, "Charcoals", "Charcoal Stick", "grain_depth").unwrap_or(0.0) >= 0.5);
}

#[test]
fn pressure_lays_more_paint_for_pencils_and_airbrushes() {
    let Some(s) = session() else { return };
    for (set, name) in [("Sketching", "6B Pencil"), ("Airbrushing", "Soft Airbrush")] {
        let (light, light_ink) = stroke(&s, set, name, 0.15);
        s.clear_layer();
        let (heavy, heavy_ink) = stroke(&s, set, name, 0.95);
        println!("{name}: light {light} px / {light_ink} alpha, heavy {heavy} px / {heavy_ink} alpha");
        assert!(heavy > light, "{name}: heavy pressure must cover more");
        assert!(heavy_ink > light_ink, "{name}: heavy pressure must lay more");
    }
}
