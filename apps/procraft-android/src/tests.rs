//! The session on the host: input in window pixels → strokes → pixels, on a headless GPU.

use procraft_engine::view::View;
use procraft_gpu::Gpu;

use crate::{CanvasSession, PenSample, PointerKind, ToolKind};

fn sample(x: f32, y: f32, t: f64) -> PenSample {
    PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: t }
}

/// A session on a headless GPU with a `window` × `window` "screen", or `None` without a GPU.
fn session(canvas: u32, window: u32) -> Option<std::sync::Arc<CanvasSession>> {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipped: {e}");
            return None;
        }
    };
    let s = CanvasSession::new(canvas, canvas);
    {
        let mut st = s.shared.lock();
        st.window_size = (window, window);
        st.view = Some(View::fit((canvas, canvas), (window, window)));
        st.gpu = Some(gpu);
        st.ensure_canvas();
    }
    Some(s)
}

/// Renders what is queued and returns the composite's pixel at canvas (x, y).
fn pixel(s: &CanvasSession, x: u32, y: u32) -> [u8; 4] {
    let mut guard = s.shared.lock();
    let st = &mut *guard;
    let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
    canvas.render(gpu, &mut st.doc, None);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
    let px = canvas.read_composite(gpu).unwrap();
    let i = ((y * st.doc.width() + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

#[test]
fn a_pen_stroke_paints_and_undoes() {
    let Some(s) = session(100, 200) else { return };
    // the 100 px canvas is shown at 2×: window (100, 100) is canvas (50, 50)
    s.stroke_begin(PointerKind::Stylus, vec![sample(60.0, 100.0, 0.0)]);
    s.stroke_add((1..20).map(|i| sample(60.0 + i as f32 * 4.0, 100.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![sample(140.0, 100.0, 170.0)]);
    assert_eq!(pixel(&s, 50, 50), [0, 0, 0, 255]);
    assert_eq!(pixel(&s, 50, 90), [255, 255, 255, 255]);
    assert!(s.info().can_undo);
    s.undo();
    assert_eq!(pixel(&s, 50, 50), [255, 255, 255, 255]);
    s.redo();
    assert_eq!(pixel(&s, 50, 50), [0, 0, 0, 255]);
}

#[test]
fn eraser_end_and_erase_tool_erase() {
    let Some(s) = session(100, 100) else { return };
    // the eraser starts on a soft airbrush; give it the hard round pen, as large as it goes
    s.set_tool(ToolKind::Erase);
    assert_eq!(s.current_brush().name, "Soft Airbrush");
    assert!(s.select_brush(1, 0));
    assert!(!s.select_brush(99, 0));
    s.set_brush_size(1.0);
    s.set_tool(ToolKind::Paint);
    s.set_brush_size(1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 50, 50), [0, 0, 0, 255]);
    s.stroke_begin(PointerKind::StylusEraser, vec![sample(50.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 50, 50), [255, 255, 255, 255]);

    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 20.0, 0.0)]);
    s.stroke_end(vec![]);
    s.set_tool(ToolKind::Erase);
    assert_eq!(s.info().tool, ToolKind::Erase);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 20.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 20, 20), [255, 255, 255, 255]);
}

#[test]
fn cancel_colour_opacity_and_clear() {
    let Some(s) = session(100, 100) else { return };
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    s.stroke_cancel();
    assert_eq!(pixel(&s, 30, 30), [255, 255, 255, 255]);

    s.set_color(1.0, 0.0, 0.0);
    s.set_brush_opacity(0.5);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    let [r, g, b, _] = pixel(&s, 30, 30);
    assert!(r == 255 && (126..=129).contains(&g) && g == b, "{r} {g} {b}");

    s.clear_layer();
    assert_eq!(pixel(&s, 30, 30), [255, 255, 255, 255]);
    s.undo();
    assert_eq!(pixel(&s, 30, 30)[1], pixel(&s, 30, 30)[2]);
}

#[test]
fn a_blending_brush_builds_up_past_its_opacity_and_a_glaze_stops_at_it() {
    let Some(s) = session(100, 100) else { return };
    s.set_color(1.0, 0.0, 0.0);
    s.set_brush_opacity(0.5);
    let line = |y: f32| {
        s.stroke_begin(
            PointerKind::Finger,
            (0..40).map(|i| sample(10.0 + i as f32 * 2.0, y, f64::from(i) * 8.0)).collect(),
        );
        s.stroke_end(vec![]);
    };
    // a glaze (the round pen): the stroke at half
    line(30.0);
    let glaze = pixel(&s, 50, 30)[1];
    assert!((120..=136).contains(&glaze), "{glaze}");
    // blending, each stamp is lighter but they build up well past it
    s.studio_begin();
    s.set_studio_setting("rendering_mode".to_owned(), 5.0);
    s.studio_end(true);
    line(70.0);
    let blend = pixel(&s, 50, 70)[1];
    assert!(blend + 50 < glaze, "{blend} {glaze}");
}

#[test]
fn gestures_move_the_view_and_bad_values_are_ignored() {
    let Some(s) = session(100, 100) else { return };
    s.view_gesture(50.0, 50.0, 0.0, 0.0, 2.0, 0.0);
    assert!((s.info().zoom - 2.0).abs() < 1e-4);
    s.view_gesture(f32::NAN, 0.0, 0.0, 0.0, 1.0, 0.0);
    s.set_brush_size(f32::NAN);
    s.set_color(f32::NAN, 2.0, -1.0);
    assert!((s.info().zoom - 2.0).abs() < 1e-4);
    s.fit_view();
    assert!((s.info().zoom - 1.0).abs() < 1e-4);
    // settling to fit gets there over its time, frame by frame
    s.view_gesture(50.0, 50.0, 0.0, 0.0, 2.0, 0.0);
    s.settle_to_fit();
    std::thread::sleep(std::time::Duration::from_secs_f32(s.view_settle_seconds()));
    s.shared.lock().step_view_motion();
    assert!((s.info().zoom - 1.0).abs() < 1e-4);
    // no window on the host
    assert!(s.attach_window(1, 10, 10).is_err());
}

#[test]
fn layers_change_undoably() {
    let Some(s) = session(100, 100) else { return };
    let first = s.layers()[0].id;
    let second = s.add_layer();
    assert_eq!(s.layers().iter().map(|l| l.id).collect::<Vec<_>>(), vec![second, first], "top first");
    assert!(s.layers()[0].current);
    let r0 = s.info().revision;
    // an opacity drag is one undo step
    for v in [0.9, 0.6, 0.3] {
        s.set_layer_opacity(second, v, false);
    }
    s.set_layer_opacity(second, 0.2, true);
    assert!(s.info().revision > r0);
    assert!((s.layers()[0].opacity - 0.2).abs() < 1e-6);
    s.undo();
    assert!((s.layers()[0].opacity - 1.0).abs() < 1e-6, "back to before the drag");
    s.undo();
    assert_eq!(s.layers().len(), 1, "the add is undone");
    s.redo();
    assert_eq!(s.layers().len(), 2);
}

#[test]
fn merge_duplicate_blend_and_locks() {
    let Some(s) = session(100, 100) else { return };
    let bottom = s.layers()[0].id;
    s.set_color(1.0, 0.0, 0.0);
    s.set_brush_size(1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    let copy = s.duplicate_layer(bottom).unwrap();
    assert_eq!(s.layers().len(), 2);
    assert!(s.set_layer_blend(copy, procraft_engine::document::BlendMode::Multiply.id()));
    assert!(!s.set_layer_blend(copy, 999));
    assert_eq!(crate::layers::blend_modes().len(), 27);
    assert!(s.merge_down(copy));
    assert_eq!(s.layers().len(), 1);
    // red multiplied by red is still red
    assert_eq!(pixel(&s, 50, 50), [255, 0, 0, 255]);
    // a locked layer takes no paint and can't be cleared
    assert!(s.set_layer_locked(bottom, true));
    s.set_color(0.0, 0.0, 1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    s.clear_layer();
    assert_eq!(pixel(&s, 50, 50), [255, 0, 0, 255]);
    let thumb = s.layer_thumbnail(bottom, 10).unwrap();
    assert_eq!(thumb.width.max(thumb.height), 10);
    assert!(!s.delete_layer(bottom), "locked, and the last layer");
}

#[test]
fn background_settings() {
    let Some(s) = session(100, 100) else { return };
    assert!(s.set_background_color(0.0, 0.0, 0.0, true));
    assert_eq!(pixel(&s, 10, 10), [0, 0, 0, 255]);
    assert!(s.set_background_visible(false));
    assert!(!s.set_background_visible(false), "no change, no step");
    assert_eq!(pixel(&s, 10, 10), [0, 0, 0, 0]);
    s.undo();
    assert!(s.background().visible);
}

#[test]
fn every_brush_draws_a_preview() {
    let Some(s) = session(100, 100) else { return };
    let library = s.brush_library();
    assert!(library.len() >= 6);
    for set in &library {
        for b in &set.brushes {
            let p = s.brush_preview(b.set, b.index, 240, 60).unwrap();
            assert_eq!((p.width, p.height, p.rgba.len()), (240, 60, 240 * 60 * 4));
            let painted = p.rgba.chunks(4).filter(|px| px[3] > 0).count();
            assert!(painted > 100, "{} drew {painted} pixels", b.name);
            assert!(p.rgba.chunks(4).all(|px| px[3] == 0 || px[0] <= px[3]), "{} premultiplied", b.name);
        }
    }
    // the preview canvas leaves the artwork alone
    assert!(!s.info().can_undo);
}

#[test]
fn brushes_keep_their_own_sliders() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.9);
    assert!(s.select_brush(0, 0));
    // at the size its preview shows (0.09 of its range: the slider's place for it)
    let start = 0.09f32.powf(1.0 / procraft_engine::brush::SIZE_CURVE);
    assert!((s.info().size - start).abs() < 1e-6, "a fresh brush starts at its default");
    s.set_brush_size(0.1);
    assert!(s.select_brush(1, 0));
    assert!((s.info().size - 0.9).abs() < 1e-6, "back to the pen's own size");
    // a textured brush paints through the renderer without trouble
    assert!(s.select_brush(4, 0));
    s.stroke_begin(PointerKind::Stylus, (0..20).map(|i| sample(20.0 + i as f32 * 3.0, 50.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![]);
    let [_, _, _, a] = pixel(&s, 50, 50);
    assert_eq!(a, 255, "background stays opaque under the stroke");
}

/// Writes every brush preview as raw RGBA (`<set>-<index>.rgba`, 480×120) into the directory in
/// `PROCRAFT_DUMP`, for looking at the brushes: `cargo test -p procraft-android dump -- --ignored`.
#[test]
#[ignore]
fn dump_brush_previews() {
    let Ok(dir) = std::env::var("PROCRAFT_DUMP") else { return };
    let Some(s) = session(100, 100) else { return };
    let mut names = String::new();
    for set in s.brush_library() {
        for b in &set.brushes {
            let p = s.brush_preview(b.set, b.index, 480, 120).unwrap();
            std::fs::write(format!("{dir}/{}-{}.rgba", b.set, b.index), &p.rgba).unwrap();
            names += &format!(
                "{}-{}	{}: {}
",
                b.set, b.index, set.name, b.name
            );
        }
    }
    std::fs::write(format!("{dir}/names.txt"), names).unwrap();
}

#[test]
fn smudge_tool_moves_paint_and_is_one_undo_step() {
    let Some(s) = session(100, 100) else { return };
    s.set_color(1.0, 0.0, 0.0);
    s.set_brush_size(0.4);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 30, 50), [255, 0, 0, 255]);
    let outside = pixel(&s, 70, 50);
    assert_eq!(outside, [255, 255, 255, 255]);
    s.set_tool(ToolKind::Smudge);
    assert_eq!(s.current_brush().name, "Round Brush");
    s.set_brush_size(0.6);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 50.0, 0.0)]);
    s.stroke_add((1..30).map(|i| sample(30.0 + i as f32 * 1.5, 50.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![sample(75.0, 50.0, 250.0)]);
    let [r, g, _, _] = pixel(&s, 60, 50);
    assert!(r > g, "red dragged right: {r} {g}");
    s.undo();
    assert_eq!(pixel(&s, 60, 50), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 30, 50), [255, 0, 0, 255], "the paint itself stays");
}

#[test]
fn prediction_is_drawn_but_never_kept() {
    let Some(s) = session(100, 100) else { return };
    s.stroke_begin(PointerKind::Stylus, vec![sample(10.0, 50.0, 0.0)]);
    s.stroke_predict(vec![sample(90.0, 50.0, 16.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 90, 50), [255, 255, 255, 255]);
    // without an open stroke a prediction does nothing
    s.stroke_predict(vec![sample(50.0, 50.0, 0.0)]);
}

#[test]
fn artworks_save_and_open() {
    let Some(s) = session(100, 100) else { return };
    s.set_color(0.0, 1.0, 0.0);
    s.set_brush_size(0.3);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    let top = s.add_layer();
    s.set_color(0.0, 0.0, 1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(70.0, 70.0, 0.0)]);
    s.stroke_end(vec![]);
    assert!(s.set_layer_opacity(top, 0.5, true));
    assert!(s.set_name("Test Piece".to_owned()));
    s.set_author("  A. Painter ".to_owned());
    s.add_tracked_time(90_000);
    let info = s.artwork_info();
    assert_eq!((info.strokes, info.tracked_ms, info.author.as_str()), (2, 90_000, "A. Painter"));
    assert!(info.created_ms > 0);
    let stats = s.layer_stats();
    assert_eq!((stats.used, stats.masks, stats.groups), (2, 0, 0));
    assert!(stats.maximum >= stats.used);
    let dir = std::env::temp_dir().join(format!("procraft-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    assert!(dir.join("artwork.json").exists() && dir.join("thumbnail.png").exists());

    let opened = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
    {
        let gpu = Gpu::headless().unwrap();
        let mut st = opened.shared.lock();
        st.window_size = (100, 100);
        st.view = Some(View::fit((100, 100), (100, 100)));
        st.gpu = Some(gpu);
        st.ensure_canvas();
    }
    assert_eq!(opened.name(), "Test Piece");
    assert_eq!(opened.artwork_info().author, "A. Painter");
    assert_eq!(opened.artwork_info().created_ms, info.created_ms);
    assert_eq!(opened.layers(), s.layers());
    assert_eq!(pixel(&opened, 30, 30), pixel(&s, 30, 30));
    assert_eq!(pixel(&opened, 70, 70), pixel(&s, 70, 70));
    // saving again over the same folder works, and a deleted layer's file goes away
    assert!(opened.delete_layer(top));
    opened.save(dir.to_string_lossy().into_owned()).unwrap();
    assert_eq!(std::fs::read_dir(dir.join("layers")).unwrap().count(), 1);
    assert!(CanvasSession::open(dir.join("missing").to_string_lossy().into_owned()).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn straight_alpha_undoes_premultiplying() {
    let px = crate::project::straight_alpha(&[64, 32, 0, 128, 0, 0, 0, 0, 255, 255, 255, 255]);
    assert_eq!(px, vec![128, 64, 0, 128, 0, 0, 0, 0, 255, 255, 255, 255]);
}

#[test]
fn the_eyedropper_sees_the_window_and_the_colour_under_the_point() {
    let Some(s) = session(100, 200) else { return };
    // a black line across window y = 100 (canvas y = 50), shown at 2×
    s.stroke_begin(PointerKind::Stylus, vec![sample(20.0, 100.0, 0.0)]);
    s.stroke_add((1..40).map(|i| sample(20.0 + i as f32 * 4.0, 100.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![sample(180.0, 100.0, 330.0)]);
    let on_line = s.pick(100.0, 100.0, 81).unwrap();
    assert_eq!(on_line.rgb, Some(0x000000));
    assert_eq!(on_line.pixels.len(), 81 * 81 * 4);
    // the window's middle is the line, its top row (40 px above) the white canvas
    assert_eq!(&on_line.pixels[(40 * 81 + 40) * 4..(40 * 81 + 40) * 4 + 3], &[0, 0, 0]);
    assert_eq!(&on_line.pixels[40 * 4..40 * 4 + 3], &[255, 255, 255]);
    assert_eq!(s.pick(100.0, 40.0, 21).unwrap().rgb, Some(0xFFFFFF));
    // the window is 200 wide and the canvas fills it: nothing is off the canvas inside it, but
    // outside the window is
    assert_eq!(s.pick(250.0, 100.0, 21).unwrap().rgb, None);
}

#[test]
fn content_shape_holds_every_painted_pixel() {
    use crate::transform::content_shape;
    let mut px = vec![0u8; 4 * 5 * 4];
    assert_eq!(content_shape(&px, 5), None);
    px[(5 + 1) * 4 + 3] = 1; // (1, 1)
    px[(3 * 5 + 3) * 4 + 3] = 255; // (3, 3)
    let (bounds, hull) = content_shape(&px, 5).unwrap();
    assert_eq!(bounds, [1.0, 1.0, 3.0, 3.0]);
    // the two pixels' outer corners
    assert_eq!(hull.len(), 6, "{hull:?}");
    assert_eq!(content_shape(&px, 0), None);
}

#[test]
fn a_transform_moves_the_layer_as_one_undo_step() {
    use crate::transform::{Interpolation, TransformAction, TransformHandle, TransformKind};
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    assert_eq!(s.transform_begin(), None, "nothing to transform yet");
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    let b = s.transform_begin().unwrap();
    let [x0, y0, x1, _, _, y1, ..] = b.corners[..] else { panic!() };
    assert!(x1 - x0 < 30.0 && y1 - y0 < 30.0, "{b:?}");
    assert!(x0 < 30.0 && x1 > 30.0 && y0 < 30.0 && y1 > 30.0);
    assert!(!b.can_undo);
    // painting waits while the transform is open
    s.stroke_begin(PointerKind::Finger, vec![sample(80.0, 20.0, 0.0)]);
    s.stroke_end(vec![]);
    s.transform_grab(TransformHandle::Move, 30.0, 30.0);
    s.transform_drag(50.0, 50.0);
    let moved = s.transform_drag(70.0, 70.0).unwrap();
    assert!((moved.corners[0] - (x0 + 40.0)).abs() < 1e-3);
    let released = s.transform_release().unwrap();
    assert!(released.can_undo);
    assert_eq!(pixel(&s, 70, 70), [0, 0, 0, 255]);
    assert_eq!(pixel(&s, 30, 30), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 80, 20), [255, 255, 255, 255]);
    // undone inside the transform, then again
    s.transform_apply(TransformAction::Undo);
    assert_eq!(pixel(&s, 30, 30), [0, 0, 0, 255]);
    s.transform_grab(TransformHandle::Move, 30.0, 30.0);
    s.transform_drag(70.0, 70.0);
    // a second finger: put back
    s.transform_cancel_drag();
    assert_eq!(pixel(&s, 30, 30), [0, 0, 0, 255]);
    s.transform_grab(TransformHandle::Move, 30.0, 30.0);
    s.transform_drag(70.0, 70.0);
    s.transform_release();
    let nudged = s.transform_nudge(99.0, 70.0).unwrap();
    assert!((nudged.corners[0] - released.corners[0] - 1.0).abs() < 1e-3);
    s.transform_apply(TransformAction::Undo);
    s.transform_set_guides(crate::transform::GuideSettings {
        magnetics: true,
        snapping: true,
        distance: 3.0,
        velocity: 10.0,
    });
    let warped = s.transform_set_kind(TransformKind::Warp).unwrap();
    assert_eq!(warped.mesh.len(), 32);
    assert_eq!(warped.grid.len(), 8 * 25 * 2);
    s.transform_grab(TransformHandle::Mesh { index: 15 }, 0.0, 0.0);
    s.transform_drag(5.0, 5.0);
    s.transform_release();
    s.transform_apply(TransformAction::Undo);
    s.transform_set_kind(TransformKind::Distort);
    s.transform_set_interpolation(Interpolation::Nearest);
    s.transform_end(true);
    assert_eq!(pixel(&s, 70, 70), [0, 0, 0, 255]);
    s.undo();
    assert_eq!(pixel(&s, 30, 30), [0, 0, 0, 255]);
    assert_eq!(pixel(&s, 70, 70), [255, 255, 255, 255]);
    // cancelled, and kept by another change
    s.transform_begin().unwrap();
    s.transform_apply(TransformAction::FlipVertical);
    s.transform_apply(TransformAction::FitToCanvas);
    s.transform_end(false);
    assert_eq!(pixel(&s, 30, 30), [0, 0, 0, 255]);
    s.transform_begin().unwrap();
    s.transform_grab(TransformHandle::Move, 30.0, 30.0);
    s.transform_drag(70.0, 70.0);
    s.add_layer();
    assert_eq!(pixel(&s, 70, 70), [0, 0, 0, 255]);
    assert_eq!(s.transform_drag(10.0, 10.0), None);
    assert_eq!(s.view_matrix(), vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
}

#[test]
fn a_selection_limits_painting_and_its_tools_work() {
    use crate::selection::SelectionJoin;
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    // the left half, drawn on the window (1:1)
    s.select_shape(false, 0.0, 0.0, 50.0, 100.0, SelectionJoin::Add);
    assert!(s.has_selection());
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 47, 50), [0, 0, 0, 255]);
    assert_eq!(pixel(&s, 53, 50), [255, 255, 255, 255]);
    // inverted: the right half; filled with red there
    s.select_invert();
    s.set_color(1.0, 0.0, 0.0);
    s.fill_selection();
    assert_eq!(pixel(&s, 90, 10), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 10, 10), [255, 255, 255, 255]);
    s.undo();
    assert_eq!(pixel(&s, 90, 10), [255, 255, 255, 255]);
    // a circle taken away, then saved, cleared and loaded
    s.select_shape(true, 60.0, 0.0, 100.0, 40.0, SelectionJoin::Remove);
    assert_eq!(s.save_selection(), 1);
    s.select_none();
    assert!(!s.has_selection());
    assert!(s.load_selection(0));
    assert!(!s.load_selection(5));
    // each change to the selection is an undo step
    s.select_none();
    s.undo();
    assert!(s.has_selection());
    s.redo();
    assert!(!s.has_selection());
    s.undo();
    // copied to a layer of its own: the left half's paint is not selected, so the copy is empty
    let id = s.copy_selection_to_layer().unwrap();
    assert!(s.info().revision > 0);
    s.select_none();
    s.select_polygon(vec![0.0, 0.0, 50.0, 0.0, 50.0, 100.0, 0.0, 100.0], SelectionJoin::Add);
    s.select_feather(0.02);
    s.select_feather(0.0);
    s.select_feather(0.05);
    s.select_feather_done();
    s.select_by_colour(10.0, 10.0, 0.1, SelectionJoin::Add, true);
    s.select_by_colour(10.0, 10.0, 0.3, SelectionJoin::Add, false);
    s.set_selection_display(crate::selection::SelectionDisplay::Tint);
    assert!(s.has_selection());
    let _ = id;
    // clearing with a selection clears only what is selected
    s.select_none();
    let paint = s.layers().iter().map(|l| l.id).rfind(|&l| l != id).unwrap();
    assert!(s.select_layer(paint));
    s.select_shape(false, 0.0, 0.0, 45.0, 100.0, SelectionJoin::Add);
    s.clear_layer();
    assert_eq!(pixel(&s, 44, 50), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 48, 50), [0, 0, 0, 255]);
}

#[test]
fn a_transform_with_a_selection_moves_only_what_it_takes() {
    use crate::selection::SelectionJoin;
    use crate::transform::{TransformAction, TransformHandle};
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 50.0, 0.0)]);
    s.stroke_add((1..30).map(|i| sample(20.0 + i as f32 * 2.0, 50.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![sample(80.0, 50.0, 250.0)]);
    // the left half selected: the box holds only its part of the stroke
    s.select_shape(false, 0.0, 0.0, 50.0, 100.0, SelectionJoin::Add);
    let b = s.transform_begin().unwrap();
    assert!(b.corners[2] <= 50.5, "{b:?}");
    s.transform_grab(TransformHandle::Move, 30.0, 50.0);
    s.transform_drag(30.0, 80.0);
    s.transform_release();
    s.transform_end(true);
    // the left part moved down, the right part stayed
    assert_eq!(pixel(&s, 30, 80), [0, 0, 0, 255]);
    assert_eq!(pixel(&s, 30, 50), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 70, 50), [0, 0, 0, 255]);
    // and the selection went with it
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 20.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 30, 20), [255, 255, 255, 255]);
    let _ = TransformAction::Reset;
}

#[test]
fn adjustments_change_the_layer_as_one_undo_step() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert!(s.adjust_begin());
    assert!(s.is_adjusting());
    // brightness to none: black; then back to as it was
    s.adjust_hsb(0.5, 0.5, 0.0);
    assert_eq!(pixel(&s, 50, 50), [0, 0, 0, 255]);
    s.adjust_hsb(0.5, 0.5, 0.5);
    assert_eq!(pixel(&s, 50, 50), [255, 0, 0, 255]);
    s.adjust_swipe(crate::adjust::SwipeAdjustment::GaussianBlur, 0.2);
    for kind in [
        crate::adjust::SwipeAdjustment::Sharpen,
        crate::adjust::SwipeAdjustment::Noise,
        crate::adjust::SwipeAdjustment::PerspectiveBlur,
        crate::adjust::SwipeAdjustment::ChromaticAberration,
        crate::adjust::SwipeAdjustment::Halftone,
        crate::adjust::SwipeAdjustment::Bloom,
    ] {
        s.adjust_swipe(kind, 0.5);
    }
    assert!((s.adjust_motion(100.0, 0.0) - 0.1).abs() < 1e-3);
    s.adjust_curves(vec![0.0, 0.0, 0.5, 0.8, 1.0, 1.0], vec![], vec![], vec![]);
    s.adjust_color_balance(vec![0.5, 0.0, 0.0], vec![], vec![0.0, 0.0, -0.5]);
    s.adjust_gradient_map(vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0], 1.0);
    s.adjust_noise(crate::adjust::NoiseKind::Ridges, 0.5, 0.3, 0.5, 0.7, true, false);
    s.adjust_bloom(0.5, 1.0, 0.3, 0.35);
    s.adjust_halftone(crate::adjust::HalftoneKind::Newspaper, 0.3);
    s.adjust_chromatic(crate::adjust::ChromaticKind::Displace, 0.0, 12.0, -4.0, 0.2, f32::NAN);
    for kind in [crate::adjust::GlitchKind::Artifact, crate::adjust::GlitchKind::Diverge] {
        s.adjust_glitch(kind, 0.5, vec![0.9, -0.5, f32::NAN], 0.5);
    }
    s.adjust_swipe(crate::adjust::SwipeAdjustment::GaussianBlur, 0.2);
    s.adjust_end(true);
    assert!(!s.is_adjusting());
    assert_ne!(pixel(&s, 50, 50), [255, 0, 0, 255]);
    s.undo();
    assert_eq!(pixel(&s, 50, 50), [255, 0, 0, 255]);
    // put back
    assert!(s.adjust_begin());
    s.adjust_hsb(0.5, 0.5, 0.0);
    s.adjust_end(false);
    assert_eq!(pixel(&s, 50, 50), [255, 0, 0, 255]);
}

#[test]
fn liquify_moves_paint_as_one_undo_step() {
    use crate::adjust::{LiquifyBrush, LiquifyKind};
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert!(s.liquify_begin());
    let brush = LiquifyBrush { kind: LiquifyKind::Push, size: 0.15, pressure: 1.0, distortion: 0.0 };
    let path: Vec<f32> = (0..=20).flat_map(|i| [30.0 + i as f32, 50.0, 1.0]).collect();
    s.liquify_stroke(path, true, brush);
    assert!(pixel(&s, 45, 50)[0] < 128, "{:?}", pixel(&s, 45, 50));
    s.liquify_amount(0.5);
    s.liquify_reset();
    assert_eq!(pixel(&s, 45, 50), [255, 255, 255, 255]);
    s.liquify_stroke((0..=20).flat_map(|i| [30.0 + i as f32, 50.0, 1.0]).collect(), true, brush);
    // lifted while moving, it carries on further right
    let row = |s: &CanvasSession| (40..90).map(|x| pixel(s, x, 50)).collect::<Vec<_>>();
    let before = row(&s);
    s.liquify_coast(0.1, 0.0, 1.0, brush);
    assert_ne!(row(&s), before);
    s.liquify_coast(f32::NAN, 0.0, 1.0, brush);
    s.adjust_end(true);
    s.undo();
    assert_eq!(pixel(&s, 30, 50), [0, 0, 0, 255]);
}

#[test]
fn an_adjustment_painted_on_and_clone() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert!(s.adjust_begin());
    s.adjust_hsb(0.5, 0.5, 0.0);
    s.adjust_pencil();
    // nothing painted on yet: as it was
    assert_eq!(pixel(&s, 30, 50), [255, 0, 0, 255]);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 30, 50), [0, 0, 0, 255]);
    s.adjust_end(true);
    assert_eq!(pixel(&s, 30, 50), [0, 0, 0, 255]);
    s.undo();
    assert_eq!(pixel(&s, 30, 50), [255, 0, 0, 255]);
    // clone the red 40 to the right
    assert!(s.clone_begin());
    s.clone_source(30.0, 50.0);
    assert_eq!(pixel(&s, 70, 50), [255, 255, 255, 255]);
    // the clone brush is the soft airbrush: a dab copies some of it
    s.stroke_begin(PointerKind::Finger, vec![sample(70.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    let cloned = pixel(&s, 70, 50);
    assert!(cloned[0] == 255 && cloned[1] < 250 && cloned[1] == cloned[2], "{cloned:?}");
    // each stroke is its own step, undone and redone while cloning goes on
    s.stroke_begin(PointerKind::Finger, vec![sample(70.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    let twice = pixel(&s, 70, 50);
    assert!(twice[1] < cloned[1], "{twice:?}");
    s.undo();
    assert_eq!(pixel(&s, 70, 50), cloned);
    assert!(s.is_adjusting());
    s.redo();
    assert_eq!(pixel(&s, 70, 50), twice);
    s.adjust_end(true);
    assert_eq!(pixel(&s, 70, 50), twice);
    // left, its strokes are one step (simplified undos)
    s.undo();
    assert_eq!(pixel(&s, 70, 50), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 30, 50), [255, 0, 0, 255]);
    s.redo();
    assert_eq!(pixel(&s, 70, 50), twice);
    // without simplified undos they stay apart
    s.set_simplified_undos(false);
    assert!(s.clone_begin());
    s.clone_source(30.0, 50.0);
    for _ in 0..2 {
        s.stroke_begin(PointerKind::Finger, vec![sample(70.0, 60.0, 0.0)]);
        s.stroke_end(vec![]);
    }
    let both = pixel(&s, 70, 60);
    s.adjust_end(true);
    s.undo();
    assert!(pixel(&s, 70, 60)[1] > both[1] && pixel(&s, 70, 60)[1] < 255, "{:?}", pixel(&s, 70, 60));
}

#[test]
fn newspaper_halftone_is_grey() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.6);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert!(s.adjust_begin());
    s.adjust_halftone(crate::adjust::HalftoneKind::Newspaper, 0.1);
    for (x, y) in [(50, 50), (48, 52), (53, 47)] {
        let [r, g, b, _] = pixel(&s, x, y);
        assert!(r == g && g == b, "{:?}", pixel(&s, x, y));
    }
}

#[test]
fn an_imported_brush_shows_its_own_preview_until_changed() {
    use procraft_engine::brush::Brush;
    use procraft_engine::brushfile::{self, Pictures};
    let Some(s) = session(100, 100) else { return };
    // a file whose preview is a solid white band across its middle
    let (w, h) = (327u32, 100u32);
    let band: Vec<u8> =
        (0..w * h).flat_map(|i| if (40..60).contains(&(i / w)) { [255u8; 4] } else { [0u8; 4] }).collect();
    let thumbnail = crate::project::rgba_png(w, h, &band);
    let shape = crate::project::rgba_png(1, 1, &[255; 4]);
    let file = brushfile::write(
        &Brush { name: "Band".to_owned(), ..Brush::round_pen() },
        "",
        &Pictures {
            shape_png: &shape,
            grain_png: None,
            metallic_png: None,
            roughness_png: None,
            thumbnail_png: &thumbnail,
            author_png: None,
            signature_png: None,
        },
    );
    let set = s.import_brushes(file.clone()).unwrap();
    let alpha = |s: &CanvasSession| {
        let p = s.brush_preview(set, 0, w, h).unwrap();
        (p.rgba[((50 * w + 5) * 4 + 3) as usize], p.rgba[((10 * w + 160) * 4 + 3) as usize])
    };
    assert_eq!(alpha(&s), (255, 0));
    // kept with the library, and written back out as it came
    let Some(other) = session(100, 100) else { return };
    assert!(other.load_brush_library(s.saved_brush_library()));
    assert_eq!(alpha(&other), (255, 0));
    let out = brushfile::read(&s.brush_as_file(set, 0).unwrap()).unwrap();
    assert_eq!(out.thumbnail.as_deref(), Some(thumbnail.as_slice()));
    // changed, it is drawn: nothing at the band's far left
    assert!(s.select_brush(set, 0));
    s.studio_begin();
    s.set_studio_setting("spacing".to_owned(), 0.3);
    assert_eq!(alpha(&s).0, 0);
}

#[test]
fn spacing_set_in_the_studio_is_spaced_the_current_way() {
    let Some(s) = session(100, 100) else { return };
    s.studio_begin();
    s.set_studio_setting("spacing_version".to_owned(), 0.0);
    s.set_studio_setting("max_size".to_owned(), 0.5);
    let version = |s: &CanvasSession| s.studio_settings(vec!["spacing_version".to_owned()])[0];
    assert_eq!(version(&s), 0.0);
    s.set_studio_setting("spacing".to_owned(), 0.2);
    assert_eq!(version(&s), 1.0);
}

#[test]
fn a_closely_spaced_brush_keeps_its_size_controls() {
    let Some(s) = session(100, 100) else { return };
    s.studio_begin();
    s.set_studio_setting("spacing".to_owned(), 0.05);
    s.set_studio_setting("max_size".to_owned(), 0.8);
    s.set_studio_setting("min_size".to_owned(), 0.3);
    s.studio_end(true);
    let Some(other) = session(100, 100) else { return };
    assert!(other.load_brush_library(s.saved_brush_library()));
    other.studio_begin();
    let sizes = other.studio_settings(vec!["max_size".to_owned(), "min_size".to_owned()]);
    assert!((sizes[0] - 0.8).abs() < 1e-3 && (sizes[1] - 0.3).abs() < 1e-3, "{sizes:?}");
}

#[test]
fn a_brush_keeps_its_images_and_their_sizes() {
    let Some(s) = session(100, 100) else { return };
    let size = crate::studio::source_size() as usize;
    let sizes = vec!["max_size".to_owned()];
    // its own shape (sizes kept in that shape's unit), a shipped grain
    s.studio_begin();
    assert!(s.set_studio_source(crate::studio::SourceKind::Shape, vec![200; size * size]));
    assert!(s.set_studio_shipped_source(crate::studio::SourceKind::Grain, 2));
    s.set_studio_setting("max_size".to_owned(), 0.6);
    let grain = s.studio_source(crate::studio::SourceKind::Grain);
    s.studio_end(true);
    let Some(other) = session(100, 100) else { return };
    assert!(other.load_brush_library(s.saved_brush_library()));
    other.studio_begin();
    assert!((other.studio_settings(sizes.clone())[0] - 0.6).abs() < 1e-3);
    assert_eq!(other.studio_source(crate::studio::SourceKind::Shape), vec![200; size * size]);
    assert_eq!(other.studio_source(crate::studio::SourceKind::Grain), grain);
    other.studio_end(false);
    // a shipped shape in place of the round tip
    s.studio_begin();
    s.studio_reset();
    assert!(s.set_studio_shipped_source(crate::studio::SourceKind::Shape, 1));
    let shape = s.studio_source(crate::studio::SourceKind::Shape);
    s.studio_end(true);
    let Some(third) = session(100, 100) else { return };
    assert!(third.load_brush_library(s.saved_brush_library()));
    third.studio_begin();
    assert_eq!(third.studio_source(crate::studio::SourceKind::Shape), shape);
}

#[test]
fn a_brush_keeps_what_it_paints_materials_through() {
    use crate::studio::SourceKind::{Metallic, Roughness};
    let Some(s) = session(100, 100) else { return };
    let size = crate::studio::source_size() as usize;
    let amounts = || vec!["metallic_amount".to_owned(), "roughness_amount".to_owned(), "metallic_scale".to_owned()];
    s.studio_begin();
    // not metallic, half rough, each source tiled at a tenth, and neither an image (the amount all over)
    assert_eq!(s.studio_settings(amounts()), vec![0.0, 0.5, 0.1]);
    assert!(s.studio_source(Metallic).is_empty() && s.studio_source(Roughness).is_empty());
    assert!(s.set_studio_source(Metallic, vec![90; size * size]));
    assert!(s.set_studio_shipped_source(Roughness, 3));
    s.set_studio_setting("metallic_amount".to_owned(), 0.7);
    let rough = s.studio_source(Roughness);
    assert_eq!(rough.len(), size * size);
    s.studio_end(true);
    // kept with the library
    let Some(other) = session(100, 100) else { return };
    assert!(other.load_brush_library(s.saved_brush_library()));
    other.studio_begin();
    assert_eq!(other.studio_source(Metallic), vec![90; size * size]);
    assert_eq!(other.studio_source(Roughness), rough);
    assert!((other.studio_settings(amounts())[0] - 0.7).abs() < 1e-3);
    other.studio_end(false);
    // and with a brush file: both images its own, the amount as it was
    let b = s.current_brush();
    let set = other.import_brushes(s.brush_as_file(b.set, b.index).unwrap()).unwrap();
    assert!(other.select_brush(set, 0));
    other.studio_begin();
    assert_eq!(other.studio_source(Metallic), vec![90; size * size]);
    assert_eq!(other.studio_source(Roughness), rough);
    assert!((other.studio_settings(amounts())[0] - 0.7).abs() < 1e-3);
}

#[test]
fn a_studio_slider_has_its_own_pressure_curve_and_tilt() {
    let s = CanvasSession::new(64, 64);
    s.studio_begin();
    // which sliders offer what
    let scatter = crate::studio::slider_extras("scatter".into());
    assert!(scatter.pressure && !scatter.tilt);
    let count = crate::studio::slider_extras("count".into());
    assert!(count.pressure && count.tilt && count.tilt_switch == "count_tilt");
    let gradation = crate::studio::slider_extras("tilt_gradation".into());
    assert!(!gradation.pressure && gradation.tilt && gradation.tilt_switch.is_empty());
    // a curve given, read back, and taken away
    assert!(s.studio_slider_curve("scatter".into()).is_empty());
    s.set_studio_slider_curve("scatter".into(), vec![0.0, 0.0, 0.5, 0.8, 1.0, 1.0]);
    assert_eq!(s.studio_slider_curve("scatter".into()), vec![0.0, 0.0, 0.5, 0.8, 1.0, 1.0]);
    s.set_studio_slider_curve("scatter".into(), Vec::new());
    assert!(s.studio_slider_curve("scatter".into()).is_empty());
    // the size's own response: straight reads as off
    s.set_studio_slider_curve("pressure_size".into(), vec![0.0, 0.0, 0.5, 0.2, 1.0, 1.0]);
    assert_eq!(s.studio_slider_curve("pressure_size".into()).len(), 6);
    s.set_studio_slider_curve("pressure_size".into(), Vec::new());
    assert!(s.studio_slider_curve("pressure_size".into()).is_empty());
    // a slider without a curve keeps none
    s.set_studio_slider_curve("spacing".into(), vec![0.0, 0.0, 1.0, 1.0]);
    assert!(s.studio_slider_curve("spacing".into()).is_empty());
}

#[test]
fn the_brush_studio_edits_keeps_and_puts_back() {
    let Some(s) = session(100, 100) else { return };
    let name = s.studio_begin();
    assert!(!name.is_empty());
    let keys = vec!["spacing".to_owned(), "wet_pull".to_owned()];
    let start = s.studio_settings(keys.clone());
    s.set_studio_setting("spacing".to_owned(), 0.5);
    s.set_studio_setting("wet_pull".to_owned(), 0.2);
    assert_eq!(s.studio_settings(keys.clone()), vec![0.5, 0.2]);
    s.set_studio_curve(vec![0.0, 0.0, 0.5, 0.2, 1.0, 1.0]);
    assert_eq!(s.studio_curve().len(), 6);
    // cancelled, it opens as it was
    s.studio_end(false);
    s.studio_begin();
    assert_eq!(s.studio_settings(keys.clone()), start);
    assert_eq!(s.studio_curve().len(), 4);
    // kept, it stays; reset, it is as it ships
    s.set_studio_setting("spacing".to_owned(), 0.5);
    s.studio_end(true);
    s.studio_begin();
    assert_eq!(s.studio_settings(keys.clone())[0], 0.5);
    s.studio_reset();
    assert_eq!(s.studio_settings(keys.clone()), start);
    s.studio_end(true);
    // a stroke still draws with the edited brush
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    // changes carry to another session
    s.studio_begin();
    s.set_studio_setting("spacing".to_owned(), 0.33);
    s.set_studio_setting("wet_attack".to_owned(), 0.25);
    s.studio_end(true);
    let kept = s.saved_brush_library();
    let Some(other) = session(100, 100) else { return };
    assert!(other.load_brush_library(kept));
    other.studio_begin();
    assert_eq!(other.studio_settings(vec!["spacing".to_owned(), "wet_attack".to_owned()]), vec![0.33, 0.25]);
    other.studio_end(false);
    other.apply_brush_changes("not json".to_owned());
    assert!(!other.load_brush_library("not json".to_owned()));
    // a pad paints like it, in its own colour, at the preview size
    let Some(pad) = session(100, 100) else { return };
    s.studio_begin();
    s.set_studio_setting("preview_size".to_owned(), 0.5);
    pad.paint_like(s.clone(), 1.0, 0.0, 0.0);
    pad.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    pad.stroke_end(vec![]);
    assert_eq!(pixel(&pad, 50, 50), [255, 0, 0, 255]);
    s.studio_end(false);
}

#[test]
fn a_held_stroke_snaps_to_a_shape_and_is_edited() {
    use crate::quickshape::QuickShapeKind;
    let Some(s) = session(200, 200) else { return };
    s.set_brush_size(0.2);
    // a rough circle, held
    let pts: Vec<PenSample> = (0..=60)
        .map(|i| {
            let t = i as f32 / 60.0 * std::f32::consts::TAU;
            sample(100.0 + 60.0 * t.cos() + 2.0 * (7.0 * t).sin(), 100.0 + 40.0 * t.sin(), f64::from(i) * 8.0)
        })
        .collect();
    s.stroke_begin(PointerKind::Finger, pts[..1].to_vec());
    s.stroke_add(pts[1..].to_vec());
    let info = s.quickshape_hold().expect("a shape");
    assert_eq!(info.kind, QuickShapeKind::Ellipse);
    assert_eq!(info.handles.len(), 8);
    s.stroke_end(vec![]);
    // still open, and undo is there for it
    assert!(s.info().can_undo);
    // shown as a circle, then a handle dragged
    let circle = s.quickshape_choose(QuickShapeKind::Circle).unwrap();
    assert_eq!(circle.kind, QuickShapeKind::Circle);
    let moved = s.quickshape_move_handle(0, 190.0, 100.0).unwrap();
    assert!((moved.handles[0] - 190.0).abs() < 1.0, "{:?}", moved.handles);
    s.quickshape_done();
    assert!(s.quickshape_info().is_none());
    // the circle's edge is painted, its middle is not
    assert_eq!(pixel(&s, 10, 100)[0], 0);
    assert_eq!(pixel(&s, 100, 100), [255, 255, 255, 255]);
    s.undo();
    assert_eq!(pixel(&s, 10, 100), [255, 255, 255, 255]);
    // a scribble too short to be anything does not snap
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 10.0, 0.0)]);
    assert!(s.quickshape_hold().is_none());
    s.stroke_end(vec![]);
}

#[test]
fn copy_cut_paste_and_flip() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    let copied = s.copy_layer().unwrap();
    let layers = s.layers().len();
    let pasted = s.paste(copied.clone()).unwrap();
    assert_eq!(s.layers().len(), layers + 1);
    assert_eq!(s.layers().iter().find(|l| l.current).map(|l| l.id), Some(pasted));
    // flipped left to right, both layers' red is now on the right, as one undo step
    s.flip_canvas(true);
    assert_eq!(pixel(&s, 79, 50), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 20, 50), [255, 255, 255, 255]);
    s.undo();
    assert_eq!(pixel(&s, 20, 50), [255, 0, 0, 255]);
    // cut takes it off the layer
    let cut = s.cut_layer().unwrap();
    assert_eq!(cut.rgba, copied.rgba);
    s.delete_layer(pasted);
    s.cut_layer().unwrap();
    assert_eq!(pixel(&s, 20, 50), [255, 255, 255, 255]);
    // a photo comes in as straight colour, premultiplied
    let photo = crate::layers::ThumbnailImage { width: 2, height: 2, rgba: [255, 0, 0, 128].repeat(4) };
    assert!(s.insert_image(photo, false).is_some());
    let canvas = s.copy_canvas().unwrap();
    assert_eq!(canvas.rgba.len(), 100 * 100 * 4);
}

#[test]
fn a_private_picture_shows_on_the_canvas_but_not_in_the_thumbnail() {
    let Some(s) = session(100, 100) else { return };
    let photo = crate::layers::ThumbnailImage { width: 100, height: 100, rgba: [255, 0, 0, 255].repeat(100 * 100) };
    let id = s.insert_image(photo, true).unwrap();
    assert!(s.layers().iter().any(|l| l.id == id && l.private));
    assert_eq!(pixel(&s, 50, 50), [255, 0, 0, 255]);
    let dir = std::env::temp_dir().join(format!("procraft-private-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    let png = std::fs::read(dir.join("thumbnail.png")).unwrap();
    let (w, h, rgba) = crate::brush_files::thumbnail_pixels(&png).unwrap();
    let middle = ((h / 2 * w + w / 2) * 4) as usize;
    assert_eq!(&rgba[middle..middle + 3], &[255, 255, 255], "the thumbnail shows the white background");
    // the canvas itself still shows it, and a paste can be private too
    assert_eq!(pixel(&s, 50, 50), [255, 0, 0, 255]);
    let copied = s.copy_layer().unwrap();
    let pasted = s.paste_private(copied).unwrap();
    assert!(s.layers().iter().any(|l| l.id == pasted && l.private));
    s.undo();
    assert!(s.layers().iter().all(|l| l.id != pasted));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_drawing_guide_mirrors_and_assists_strokes() {
    use crate::guide::{GuideType, SymmetryType};
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.3);
    // symmetry: a dot on the left is drawn on the right too
    s.set_guide_on(true);
    s.set_guide_kind(GuideType::Symmetry);
    s.set_guide_symmetry(SymmetryType::Vertical, false);
    s.set_guide_assisted(true);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 80, 30)[0], 0);
    // the grid keeps a slanting stroke level
    s.set_guide_kind(GuideType::Grid);
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 70.0, 0.0)]);
    s.stroke_add((1..=20).map(|i| sample(10.0 + i as f32 * 3.0, 70.0 + i as f32 * 0.6, f64::from(i) * 8.0)).collect());
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 60, 70)[0], 0);
    assert_eq!(pixel(&s, 60, 80), [255, 255, 255, 255]);
    let info = s.guide();
    assert!(info.on && info.assisted && !info.lines.is_empty());
    // the editor fits the canvas inside a rectangle, and cancelling puts the guide back
    let before = s.view_matrix();
    s.guide_edit_begin(10.0, 10.0, 60.0, 60.0);
    s.set_guide_look(0.5, 0.5, 30.0, 0.7);
    assert_eq!(s.guide().size, 30.0);
    s.guide_edit_end(false);
    assert_eq!(s.guide().size, 71.0);
    assert_eq!(s.view_matrix(), before);
    // perspective points are added by tapping, three at most
    s.set_guide_kind(GuideType::Perspective);
    assert_eq!(s.touch_vanishing_point(5.0, 50.0), Some(0));
    assert_eq!(s.touch_vanishing_point(6.0, 50.0), Some(0));
    assert_eq!(s.touch_vanishing_point(95.0, 50.0), Some(1));
    s.remove_vanishing_point(1);
    assert_eq!(s.guide().vanishing.len(), 2);
}

#[test]
fn a_layers_own_actions() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    let id = s.layers()[0].id;
    // invert: red to cyan, the coverage kept
    assert!(s.invert_layer(id));
    assert_eq!(pixel(&s, 30, 30), [0, 255, 255, 255]);
    s.undo();
    // select what is on it
    assert!(s.select_layer_contents(id));
    assert!(s.has_selection());
    // fill within that selection with blue
    s.set_color(0.0, 0.0, 1.0);
    assert!(s.fill_layer(id));
    assert_eq!(pixel(&s, 30, 30), [0, 0, 255, 255]);
    assert_eq!(pixel(&s, 80, 80), [255, 255, 255, 255]);
    s.select_none();
    assert!(s.fill_layer(id));
    assert_eq!(pixel(&s, 80, 80), [0, 0, 255, 255]);
    assert_eq!(s.copy_layer_contents(id).unwrap().rgba.len(), 100 * 100 * 4);
    // one reference layer at a time
    let other = s.add_layer();
    assert!(s.set_layer_reference(id, true));
    assert!(s.set_layer_reference(other, true));
    let refs: Vec<bool> = s.layers().iter().map(|l| l.reference).collect();
    assert_eq!(refs.iter().filter(|&&r| r).count(), 1);
    assert!(s.layers().iter().any(|l| l.id == other && l.reference));
}

#[test]
fn masks_hide_and_groups_composite_together() {
    use crate::layers::LayerType;
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.6);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    let red = s.layers()[0].id;
    // a white mask shows everything; black painted on it hides the layer there
    let mask = s.add_mask(red).unwrap();
    assert_eq!(pixel(&s, 50, 50), [255, 0, 0, 255]);
    s.set_color(0.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 50, 50), [255, 255, 255, 255]);
    let list = s.layers();
    assert_eq!(list.iter().find(|l| l.id == mask).map(|l| (l.kind, l.parent)), Some((LayerType::Mask, Some(red))));
    s.undo();
    assert_eq!(pixel(&s, 50, 50), [255, 0, 0, 255]);
    // a blue layer above, combined down with the red one: a group at half opacity shows both
    // composited first, then halved
    let blue = s.add_layer();
    s.set_color(0.0, 0.0, 1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 20.0, 0.0)]);
    s.stroke_end(vec![]);
    let group = s.combine_down(blue).unwrap();
    assert!(s.layers().iter().any(|l| l.id == group && l.kind == LayerType::Group));
    assert!(s.layers().iter().filter(|l| l.parent == Some(group)).count() >= 2);
    s.set_layer_opacity(group, 0.5, true);
    let [r, g, b, _] = pixel(&s, 50, 50);
    assert!(r == 255 && (125..=130).contains(&g) && (125..=130).contains(&b), "{:?}", [r, g, b]);
    let [r, g, b, _] = pixel(&s, 20, 20);
    assert!((125..=130).contains(&r) && b == 255 && (125..=130).contains(&g), "{:?}", [r, g, b]);
    // hidden, the group hides all of it
    s.set_layer_visible(group, false);
    assert_eq!(pixel(&s, 50, 50), [255, 255, 255, 255]);
    s.set_layer_visible(group, true);
    // a group takes no paint
    s.select_layer(group);
    s.stroke_begin(PointerKind::Finger, vec![sample(80.0, 80.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 80, 80), [255, 255, 255, 255]);
    assert!(s.set_group_collapsed(group, true));
    assert!(s.layers().iter().any(|l| l.id == group && l.collapsed));
}

#[test]
fn groups_nest_flatten_and_merge_down() {
    let Some(s) = session(100, 100) else { return };
    let dot = |x: f32, y: f32, rgb: [f32; 3]| {
        s.set_color(rgb[0], rgb[1], rgb[2]);
        s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
        s.stroke_end(vec![]);
    };
    s.set_brush_size(0.6);
    dot(30.0, 30.0, [1.0, 0.0, 0.0]);
    let red = s.layers()[0].id;
    let blue = s.add_layer();
    dot(70.0, 70.0, [0.0, 0.0, 1.0]);
    let green = s.add_layer();
    dot(50.0, 50.0, [0.0, 1.0, 0.0]);
    // green and blue in a group above red, then a group in the group
    let outer = s.combine_down(green).unwrap();
    let inner = s.combine_down(green).unwrap();
    let info = |id: u64| s.layers().into_iter().find(|l| l.id == id).unwrap();
    assert_eq!((info(green).depth, info(inner).depth, info(outer).depth), (2, 1, 0));
    assert!(info(outer).can_merge && info(outer).can_combine && !info(outer).can_clip);
    assert!(!info(blue).can_merge && !info(blue).can_combine && info(green).can_clip);
    // the inner group at half opacity: its layers composited first, then halved
    s.set_layer_opacity(inner, 0.5, true);
    let near = |p: [u8; 4], q: [u8; 3]| (0..3).all(|i| p[i].abs_diff(q[i]) <= 2);
    assert!(near(pixel(&s, 50, 50), [128, 255, 128]), "{:?}", pixel(&s, 50, 50));
    assert!(near(pixel(&s, 70, 70), [128, 128, 255]), "{:?}", pixel(&s, 70, 70));
    assert_eq!(pixel(&s, 30, 30), [255, 0, 0, 255]);
    // flattened, it looks the same: one layer named after its bottom one, at the group's opacity
    let flat = s.flatten_group(inner).unwrap();
    assert!(near(pixel(&s, 50, 50), [128, 255, 128]), "{:?}", pixel(&s, 50, 50));
    assert!(near(pixel(&s, 70, 70), [128, 128, 255]));
    assert_eq!((info(flat).name.as_str(), info(flat).opacity, info(flat).parent), ("Layer 2", 0.5, Some(outer)));
    s.undo();
    assert!(s.layers().iter().any(|l| l.id == inner));
    s.redo();
    assert!(s.layers().iter().all(|l| l.id != inner));
    // the outer group merged into red: one layer, still looking the same
    assert!(s.merge_down(outer));
    assert_eq!(s.layers().len(), 1);
    assert_eq!(s.layers()[0].id, red);
    assert!(near(pixel(&s, 50, 50), [128, 255, 128]), "{:?}", pixel(&s, 50, 50));
    assert_eq!(pixel(&s, 30, 30), [255, 0, 0, 255]);
    s.undo();
    assert!(s.layers().iter().any(|l| l.id == outer));
    // ungrouped, its layers come out
    assert!(s.ungroup(outer));
    assert!(s.layers().iter().all(|l| l.depth == 0));
    // a duplicate keeps the layer's mask
    let mask = s.add_mask(red).unwrap();
    let copy = s.duplicate_layer(red).unwrap();
    assert!(s.layers().iter().any(|l| l.parent == Some(copy) && l.id != mask));
}

#[test]
fn brushes_and_sets_are_made_copied_and_deleted() {
    let Some(s) = session(100, 100) else { return };
    let names = |set: usize| s.brush_library()[set].brushes.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    let start = s.current_brush();
    let set_name = s.brush_library()[start.set as usize].name.clone();
    let first = names(start.set as usize);
    // a copy goes just above; the tool keeps its brush, now one down
    assert_eq!(s.duplicate_brush(start.set, start.index), Some(start.index));
    assert_eq!(names(start.set as usize).len(), first.len() + 1);
    assert_eq!(s.current_brush().index, start.index + 1);
    // the copy is edited, and the original deleted: the copy stays current
    assert!(s.select_brush(start.set, start.index));
    s.studio_begin();
    s.set_studio_setting("spacing".to_owned(), 0.4);
    s.studio_end(true);
    assert!(s.delete_brush(start.set, start.index + 1));
    assert_eq!((s.current_brush().index, names(start.set as usize).len()), (start.index, first.len()));
    // a new brush at the top of the set is chosen; deleting the current brush moves the tool to
    // the one in its place
    assert_eq!(s.new_brush(start.set), Some(0));
    assert_eq!((s.current_brush().index, s.current_brush().name.as_str()), (0, "Untitled Brush"));
    assert!(s.delete_brush(start.set, 0));
    assert_eq!(s.current_brush().name, first[start.index as usize]);
    assert_eq!(s.new_brush(start.set), Some(0));
    // a new set at the top of the library moves the others down
    let sets = s.brush_library().len();
    assert_eq!(s.new_brush_set(), 0);
    assert_eq!(s.brush_library().len(), sets + 1);
    assert_eq!(s.current_brush().set, start.set + 1);
    assert!(s.rename_brush_set(0, " Mine ".to_owned()) && !s.rename_brush_set(0, "  ".to_owned()));
    assert_eq!(s.brush_library()[0].name, "Mine");
    // everything carries to another session
    let kept = s.saved_brush_library();
    let Some(other) = session(100, 100) else { return };
    assert!(other.load_brush_library(kept));
    let lib = other.brush_library();
    assert_eq!(lib.len(), sets + 1);
    assert_eq!(lib[0].name, "Mine");
    assert_eq!(lib[start.set as usize + 1].brushes[0].name, "Untitled Brush");
    assert!(other.select_brush(start.set + 1, start.index + 1));
    other.studio_begin();
    assert_eq!(other.studio_settings(vec!["spacing".to_owned()]), vec![0.4]);
    other.studio_end(false);
    // a brush shared as a file comes in at the top of a set, edits and all
    let file = s.brush_file(start.set + 1, start.index + 1).unwrap();
    assert_eq!(other.import_brush_file(0, file), Some(0));
    assert!(other.select_brush(0, 0));
    other.studio_begin();
    assert_eq!(other.studio_settings(vec!["spacing".to_owned()]), vec![0.4]);
    other.studio_end(false);
    assert_eq!(other.import_brush_file(0, "{}".to_owned()), None);
    // a set shared as a file comes in at the top of the library; a copy goes just above
    let file = s.brush_set_file(start.set + 1).unwrap();
    let count = other.brush_library().len();
    assert_eq!(other.import_brush_file(3, file), Some(0));
    assert_eq!(other.brush_library().len(), count + 1);
    assert_eq!(other.brush_library()[0].brushes.len(), s.brush_library()[start.set as usize + 1].brushes.len());
    assert_eq!(other.duplicate_brush_set(0), Some(0));
    assert_eq!(other.brush_library()[1].name, other.brush_library()[0].name);
    // deleting a set takes its brushes; the last set stays
    assert!(s.delete_brush_set(start.set + 1));
    assert!(s.brush_library().iter().all(|set| set.name != set_name));
    while s.brush_library().len() > 1 {
        assert!(s.delete_brush_set(0));
    }
    assert!(!s.delete_brush_set(0));
    // a tool still draws with something
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
}

#[test]
fn exports_psd_tiff_pdf_and_layer_pngs() {
    let Some(s) = session(40, 30) else { return };
    s.set_brush_size(0.6);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 15.0, 0.0)]);
    s.stroke_end(vec![]);
    let top = s.add_layer();
    s.set_color(0.0, 0.0, 1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 10.0, 0.0)]);
    s.stroke_end(vec![]);
    s.combine_down(top).unwrap();
    s.add_mask(top).unwrap();
    let dir = std::env::temp_dir().join(format!("procraft-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
    s.export_psd(path("a.psd")).unwrap();
    let psd = std::fs::read(dir.join("a.psd")).unwrap();
    assert_eq!(&psd[..4], b"8BPS");
    // a group's end marker, the background, and the mask (channel -2) are there
    assert!(psd.windows(14).any(|w| w == b"</Layer group>") && psd.windows(10).any(|w| w == b"Background"));
    assert!(psd.windows(2).any(|w| w == [0xFF, 0xFE]));
    s.export_tiff(path("a.tif")).unwrap();
    assert_eq!(&std::fs::read(dir.join("a.tif")).unwrap()[..4], b"II*\0");
    s.export_pdf(path("a.pdf"), true, 100).unwrap();
    let pdf = String::from_utf8_lossy(&std::fs::read(dir.join("a.pdf")).unwrap()).into_owned();
    assert!(pdf.contains("/Count 2"));
    s.export_pdf(path("b.pdf"), false, 60).unwrap();
    let pngs = s.export_layer_pngs(path("pngs")).unwrap();
    assert_eq!(pngs.len(), 2);
    assert!(pngs.iter().all(|p| std::path::Path::new(p).exists()));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn text_is_edited_as_one_step_and_rasterized() {
    use crate::text::{TextAlignment, TextInfo};
    let Some(s) = session(40, 40) else { return };
    let info = TextInfo {
        text: "Hi".to_owned(),
        font: "sans-serif".to_owned(),
        style: "Regular".to_owned(),
        size: 20.0,
        kerning: 0.0,
        tracking: 0.0,
        leading: 0.0,
        baseline: 0.0,
        opacity: 1.0,
        align: TextAlignment::Left,
        underline: false,
        outline: false,
        vertical: false,
        all_caps: false,
        r: 1.0,
        g: 0.0,
        b: 0.0,
        x: 2.0,
        y: 2.0,
        width: 0.0,
    };
    let red = [255u8, 0, 0, 255].repeat(40 * 40);
    let id = s.text_begin(None, info.clone()).unwrap();
    assert_eq!(s.editing_text(), Some(id));
    s.put_text_pixels(red.clone());
    assert!(s.set_text(TextInfo { text: "Hello".to_owned(), ..info.clone() }));
    s.put_text_pixels(red.clone());
    assert_eq!(pixel(&s, 5, 5), [255, 0, 0, 255]);
    s.text_end(true);
    let layer = s.layers().into_iter().find(|l| l.id == id).unwrap();
    assert!(layer.text && layer.name == "Hello");
    assert_eq!(s.layer_text(id).unwrap().text, "Hello");
    // a text layer takes no paint
    s.set_color(0.0, 0.0, 1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(5.0, 5.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 5, 5), [255, 0, 0, 255]);
    // the step undo would undo next is that text's, so the app edits it again first
    assert_eq!(s.text_undo_layer(), Some(id));
    // edited again without a change, ending it makes no step: the text step is still next
    assert_eq!(s.text_begin(Some(id), s.layer_text(id).unwrap()), Some(id));
    assert_eq!(s.text_undo_layer(), None);
    s.text_end(true);
    assert_eq!(s.text_undo_layer(), Some(id));
    // one undo takes the new text layer away; redo brings it back, to edit again
    s.undo();
    assert_eq!(s.text_redo_layer(), Some(id));
    assert_eq!((s.layers().len(), pixel(&s, 5, 5)), (1, [255, 255, 255, 255]));
    s.redo();
    assert_eq!(s.layer_text(id).unwrap().text, "Hello");
    assert_eq!(pixel(&s, 5, 5), [255, 0, 0, 255]);
    // edited again and cancelled, it is as it was
    assert_eq!(s.text_begin(Some(id), info.clone()), Some(id));
    s.set_text(TextInfo { text: "Changed".to_owned(), ..info.clone() });
    s.put_text_pixels([0u8, 255, 0, 255].repeat(40 * 40));
    s.text_end(false);
    assert_eq!(s.layer_text(id).unwrap().text, "Hello");
    assert_eq!(pixel(&s, 5, 5), [255, 0, 0, 255]);
    // new text left empty goes away
    let count = s.layers().len();
    s.text_begin(None, TextInfo { text: String::new(), ..info.clone() }).unwrap();
    s.text_end(true);
    assert_eq!(s.layers().len(), count);
    // rasterized, it takes paint
    assert!(s.rasterize_layer(id) && s.layer_text(id).is_none());
    assert!(s.select_layer(id));
    s.stroke_begin(PointerKind::Finger, vec![sample(5.0, 5.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_ne!(pixel(&s, 5, 5), [255, 0, 0, 255]);
}

#[test]
fn the_canvas_thumbnail_is_the_picture() {
    let Some(s) = session(64, 64) else { return };
    s.set_background_color(0.0, 1.0, 0.0, true);
    let t = s.canvas_thumbnail(16).unwrap();
    assert_eq!((t.width, t.height), (16, 16));
    assert_eq!(&t.rgba[..4], &[0, 255, 0, 255]);
}

#[test]
fn a_crop_resizes_and_undoes_and_redoes() {
    let Some(s) = session(64, 64) else { return };
    s.set_brush_size(0.3);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(40.0, 40.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 40, 40), [255, 0, 0, 255]);
    // keep the 32 x 32 square from (24, 24): the dot lands at (16, 16)
    assert!(s.crop_canvas(24.0, 24.0, 32.0, 32.0, 0.0, false, 0, 0));
    assert_eq!((s.info().width, s.info().height), (32, 32));
    assert_eq!(pixel(&s, 16, 16), [255, 0, 0, 255]);
    assert!(s.info().can_undo);
    // painting after, then undoing that and the crop
    s.set_color(0.0, 0.0, 1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(5.0, 5.0, 0.0)]);
    s.stroke_end(vec![]);
    s.undo();
    s.undo();
    assert_eq!((s.info().width, s.info().height), (64, 64));
    assert_eq!(pixel(&s, 40, 40), [255, 0, 0, 255]);
    assert!(s.info().can_redo);
    s.redo();
    assert_eq!((s.info().width, s.info().height), (32, 32));
    assert_eq!(pixel(&s, 16, 16), [255, 0, 0, 255]);
    // resampled to twice the size
    assert!(s.crop_canvas(0.0, 0.0, 32.0, 32.0, 0.0, true, 64, 48));
    assert_eq!((s.info().width, s.info().height), (64, 48));
    assert_eq!(pixel(&s, 32, 24), [255, 0, 0, 255]);
    // undone, then something new: the crop can't be redone
    s.undo();
    s.set_color(0.0, 1.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(2.0, 2.0, 0.0)]);
    s.stroke_end(vec![]);
    s.redo();
    assert_eq!((s.info().width, s.info().height), (32, 32));
}

#[test]
fn an_artwork_from_a_picture() {
    use crate::layers::ThumbnailImage;
    let rgba = [255u8, 0, 0, 128].repeat(6 * 4);
    let s = CanvasSession::from_image(ThumbnailImage { width: 6, height: 4, rgba }, " Photo ".to_owned());
    assert_eq!((s.info().width, s.info().height), (6, 4));
    assert_eq!(s.name(), "Photo");
    let st = s.shared.lock();
    let (_, pixels) = st.pending_pixels.first().unwrap();
    assert_eq!(&pixels[..4], &[128, 0, 0, 128]);
}

#[test]
fn curve_samples_follow_the_curve() {
    let straight = crate::studio::curve_samples(vec![0.0, 0.0, 1.0, 1.0], 5);
    assert_eq!(straight.len(), 5);
    for (i, v) in straight.iter().enumerate() {
        assert!((v - i as f32 / 4.0).abs() < 1e-5);
    }
    // through a raised middle point (as its table reads it), never above the top
    let bent = crate::studio::curve_samples(vec![0.0, 0.0, 0.5, 0.8, 1.0, 1.0], 101);
    assert!((bent[50] - 0.8).abs() < 1e-3);
    assert!(bent.iter().all(|v| (0.0..=1.0).contains(v)));
}

#[test]
fn layers_export_as_animations() {
    let Some(s) = session(40, 40) else { return };
    s.set_brush_size(0.6);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 15.0, 0.0)]);
    s.stroke_end(vec![]);
    s.add_layer();
    s.set_color(0.0, 0.0, 1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 10.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(s.animation_frame_count(), 2);
    // each frame shows its own layer alone: red over white, then blue with nothing behind
    let first = s.animation_frame(0, false, 0).unwrap();
    assert_eq!((first.width, first.height), (40, 40));
    let px = |f: &crate::layers::ThumbnailImage| f.rgba.chunks(4).map(|p| p.to_vec()).collect::<Vec<_>>();
    assert!(px(&first).iter().all(|p| p[3] == 255 && p[0] >= p[2]));
    assert!(px(&first).iter().any(|p| p[0] > 200 && p[2] < 50));
    let second = s.animation_frame(1, true, 20).unwrap();
    assert_eq!((second.width, second.height), (20, 20));
    assert!(px(&second).iter().all(|p| p[3] == 0 || p[2] >= p[0]));
    assert!(px(&second).iter().any(|p| p[3] == 0), "no background when transparent");
    assert!(s.animation_frame(2, false, 0).is_none());
    let dir = std::env::temp_dir().join(format!("procraft-animation-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
    s.export_gif(path("a.gif"), 0, 15.0, true, false).unwrap();
    let gif = std::fs::read(dir.join("a.gif")).unwrap();
    assert_eq!(&gif[..6], b"GIF89a");
    assert_eq!(gif.windows(3).filter(|w| w == &[0x21, 0xF9, 4]).count(), 2);
    s.export_apng(path("a.png"), 0, 15.0, false).unwrap();
    let png = std::fs::read(dir.join("a.png")).unwrap();
    assert!(png.windows(4).any(|w| w == b"acTL") && png.windows(4).any(|w| w == b"fdAT"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn animation_assist_shows_onion_skins_and_plays() {
    let Some(s) = session(40, 40) else { return };
    s.set_brush_size(0.6);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 20.0, 0.0)]);
    s.stroke_end(vec![]);
    let second = s.add_layer();
    s.set_color(0.0, 0.0, 1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 20.0, 0.0)]);
    s.stroke_end(vec![]);
    assert!(s.animation_settings().is_none());
    assert!(s.set_animation_assist(true));
    assert_eq!(s.animation_settings().unwrap().fps, 15);
    assert_eq!(s.animation_frames().len(), 2);
    assert_eq!(s.current_frame(), Some(1));
    // drawing on the second frame, the first shows under it as an onion skin; playing the first,
    // only it shows
    s.set_play_frame(Some(0));
    {
        let mut st = s.shared.lock();
        let st = &mut *st;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        let rgba = canvas.read_composite(gpu).unwrap();
        let i = (20 * 40 + 20) * 4;
        assert!(rgba[i] > 200 && rgba[i + 2] < 60, "the first frame alone: red");
    }
    s.set_play_frame(None);
    let mut settings = s.animation_settings().unwrap();
    settings.fps = 99;
    s.set_animation_settings(settings);
    assert_eq!(s.animation_settings().unwrap().fps, 60);
    s.set_frame_hold(second, 2);
    assert_eq!(s.frame_hold(second), 2);
    assert!(s.set_animation_assist(false));
    assert!(s.animation_settings().is_none());
}

#[test]
fn layers_at_a_point_are_listed_top_first() {
    let Some(s) = session(40, 40) else { return };
    let bottom = s.layers().last().unwrap().id;
    s.set_brush_size(0.3);
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 10.0, 0.0)]);
    s.stroke_end(vec![]);
    let top = s.add_layer();
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 10.0, 0.0)]);
    s.stroke_end(vec![]);
    s.stroke_begin(PointerKind::Finger, vec![sample(32.0, 32.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(s.layers_at(10.0, 10.0), vec![top, bottom]);
    assert_eq!(s.layers_at(32.0, 32.0), vec![top]);
    assert!(s.layers_at(36.0, 4.0).is_empty());
    // hidden layers aren't picked
    s.set_layer_visible(top, false);
    assert_eq!(s.layers_at(10.0, 10.0), vec![bottom]);
}

#[test]
fn brushes_can_keep_their_size_on_the_screen() {
    let Some(s) = session(100, 100) else { return };
    let on_canvas = s.info().brush_diameter;
    // zoomed in twice: relative to the canvas the brush stays; relative to the screen it halves
    s.view_gesture(50.0, 50.0, 0.0, 0.0, 2.0, 0.0);
    assert!((s.info().brush_diameter - on_canvas).abs() < 1e-3);
    s.set_brush_scaling(true);
    assert!((s.info().brush_diameter - on_canvas / 2.0).abs() < 1e-2, "{} vs {}", s.info().brush_diameter, on_canvas);
    s.set_brush_scaling(false);
    assert!((s.info().brush_diameter - on_canvas).abs() < 1e-3);
}

#[test]
fn a_custom_canvas_keeps_its_size_dpi_background_and_name() {
    let s = CanvasSession::new_custom(1200, 800, 300.0, vec![0.25, 0.5, 0.75], false, "  Sketch  ".into());
    let info = s.info();
    assert_eq!((info.width, info.height, info.dpi), (1200, 800, 300.0));
    let bg = s.background();
    assert_eq!((bg.r, bg.g, bg.b, bg.visible), (0.25, 0.5, 0.75, false));
    assert_eq!(s.name(), "Sketch");
    // bad values are repaired, not kept
    let s = CanvasSession::new_custom(0, 5, f32::NAN, vec![2.0], true, " ".into());
    let info = s.info();
    assert_eq!((info.width, info.height), (1, 5));
    assert!(info.dpi.is_finite() && info.dpi >= 1.0);
    assert!(s.background().visible);
    assert_eq!(s.name(), "Untitled Artwork");
}

#[test]
fn the_last_stroke_snaps_to_a_shape_until_something_else_is_done() {
    use crate::quickshape::QuickShapeKind;
    let Some(s) = session(200, 200) else { return };
    s.set_brush_size(0.02);
    // a wavy line, drawn and lifted: it dips to y 64 at x 40 and rises to y 54 at x 76
    let line: Vec<PenSample> = (0..=40)
        .map(|i| sample(20.0 + 4.0 * i as f32, 60.0 + 5.0 * (i as f32 / 3.0).sin(), f64::from(i) * 8.0))
        .collect();
    s.stroke_begin(PointerKind::Finger, line[..1].to_vec());
    s.stroke_add(line[1..].to_vec());
    s.stroke_end(vec![]);
    // painted at (or a pixel beside) the dip and the rise
    let near = |x: u32, y: u32| (y - 1..=y + 1).any(|y| (x - 1..=x + 1).any(|x| pixel(&s, x, y)[0] < 128));
    assert!(near(40, 64) && near(76, 54));
    let info = s.quickshape_last().expect("a shape");
    assert_eq!(info.kind, QuickShapeKind::Line);
    s.quickshape_done();
    // straight now: neither the dip nor the rise is painted
    assert_eq!(pixel(&s, 40, 64), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 76, 54), [255, 255, 255, 255]);
    assert!((50..70).any(|y| pixel(&s, 100, y)[0] < 128));
    // one step: undone, nothing is left
    s.undo();
    assert!((50..70).all(|y| pixel(&s, 100, y) == [255, 255, 255, 255]));
    // once something else is done (an undo here), there is no last stroke to snap
    s.stroke_begin(PointerKind::Finger, line[..1].to_vec());
    s.stroke_add(line[1..].to_vec());
    s.stroke_end(vec![]);
    s.undo();
    assert!(s.quickshape_last().is_none());
}

#[test]
fn a_brush_moves_to_another_set_and_the_tool_keeps_it() {
    let s = CanvasSession::new(64, 64);
    let before = s.brush_library();
    let moved = before[0].brushes[1].name.clone();
    assert!(s.select_brush(0, 1));
    assert!(s.move_brush(0, 1, 1, 0));
    let after = s.brush_library();
    assert_eq!(after[1].brushes[0].name, moved);
    assert_eq!(after[0].brushes.len(), before[0].brushes.len() - 1);
    assert_eq!(after[1].brushes.len(), before[1].brushes.len() + 1);
    let c = s.current_brush();
    assert_eq!((c.set, c.index), (1, 0));
    // along its own set, the last to the top: the chosen brush moves down one
    let n = after[1].brushes.len();
    let last = after[1].brushes[n - 1].name.clone();
    assert!(s.move_brush(1, n as u32 - 1, 1, 0));
    assert_eq!(s.brush_library()[1].brushes[0].name, last);
    assert_eq!(s.current_brush().index, 1);
    // nowhere to go, or nothing there: refused
    assert!(!s.move_brush(99, 0, 0, 0));
    assert!(!s.move_brush(0, 0, 99, 0));
}

#[test]
fn a_moved_layer_snaps_onto_another_layers_bounds() {
    use crate::transform::{GuideSettings, TransformHandle};
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    // a dot on the first layer: where its box's middle is
    s.stroke_begin(PointerKind::Finger, vec![sample(70.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    let first = s.transform_begin().unwrap();
    let middle = (first.corners[0] + first.corners[2]) / 2.0;
    s.transform_end(false);
    // a dot on a new layer, moved to 2.5 from the first dot's middle: it lands on it
    s.add_layer();
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 80.0, 0.0)]);
    s.stroke_end(vec![]);
    s.transform_begin().unwrap();
    s.transform_set_guides(GuideSettings { magnetics: false, snapping: true, distance: 3.0, velocity: 10.0 });
    s.transform_grab(TransformHandle::Move, 30.0, 80.0);
    let moved = s.transform_drag(30.0 + (middle - 30.0) + 2.5, 80.0).unwrap();
    let now = (moved.corners[0] + moved.corners[2]) / 2.0;
    assert!((now - middle).abs() < 0.01, "{now} vs {middle}");
    s.transform_end(false);
}

#[test]
fn a_held_lasso_snaps_to_the_shape_it_looks_like() {
    // a rough circle, as x and y in turn
    let pts: Vec<f32> = (0..=60)
        .flat_map(|i| {
            let t = i as f32 / 60.0 * std::f32::consts::TAU;
            [100.0 + 60.0 * t.cos() + 2.0 * (7.0 * t).sin(), 100.0 + 60.0 * t.sin()]
        })
        .collect();
    let outline = crate::quickshape::quickshape_outline(pts).expect("a shape");
    assert!(outline.len() >= 6 && outline.len().is_multiple_of(2));
    // every point of it near the circle
    for p in outline.chunks(2) {
        let r = ((p[0] - 100.0).powi(2) + (p[1] - 100.0).powi(2)).sqrt();
        assert!((r - 60.0).abs() < 8.0, "{r}");
    }
    assert!(crate::quickshape::quickshape_outline(vec![1.0, 1.0]).is_none());
}

#[test]
fn a_brush_paints_with_a_shape_of_the_users_own_and_keeps_it() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.3);
    let n = crate::studio::source_size() as usize;
    // a filled square: square stamps where the round pen's are round
    s.studio_begin();
    assert!(!s.set_studio_source(crate::studio::SourceKind::Shape, vec![255; 10]), "the wrong size is refused");
    assert!(s.set_studio_source(crate::studio::SourceKind::Shape, vec![255; n * n]));
    assert_eq!(s.studio_source(crate::studio::SourceKind::Shape).len(), n * n);
    s.studio_end(true);
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    let corner = (40..50).find(|&d| pixel(&s, d, d)[0] < 128);
    let edge = (40..50).find(|&d| pixel(&s, d, 50)[0] < 128);
    // the square reaches as far along the diagonal as across
    let (Some(c), Some(e)) = (corner, edge) else { panic!("{corner:?} {edge:?}") };
    assert!(c <= e + 1, "{c} vs {e}");
    // kept with the library and back
    let saved = s.saved_brush_library();
    let t = CanvasSession::new(64, 64);
    assert!(t.load_brush_library(saved));
    t.studio_begin();
    assert_eq!(t.studio_source(crate::studio::SourceKind::Shape), vec![255; n * n]);
    // a shipped shape again
    assert!(t.set_studio_shipped_source(crate::studio::SourceKind::Shape, 0));
    assert_ne!(t.studio_source(crate::studio::SourceKind::Shape), vec![255; n * n]);
    assert_eq!(crate::studio::source_library(true).len(), procraft_engine::textures::grains().len());
}

#[test]
fn an_artwork_opens_as_it_was_shown() {
    let Some(s) = session(100, 100) else { return };
    s.view_gesture(50.0, 50.0, 5.0, -3.0, 2.0, 0.4);
    let shown = s.shared.lock().view.unwrap();
    let dir = std::env::temp_dir().join(format!("procraft-view-{}", std::process::id()));
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    let t = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
    let back = t.shared.lock().saved_view.and_then(|v| v.view((100, 100))).unwrap();
    let (a, b) = (shown.to_screen(), back.to_screen());
    for (x, y) in [(a.a, b.a), (a.b, b.b), (a.c, b.c), (a.d, b.d), (a.e, b.e), (a.f, b.f)] {
        assert!((x - y).abs() < 1e-2, "{a:?} vs {b:?}");
    }
    // without the memory it opens fitted
    t.forget_saved_view();
    assert!(t.shared.lock().saved_view.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn saves_at_the_same_time_keep_the_layers_whole() {
    let Some(s) = session(300, 300) else { return };
    s.set_brush_size(0.5);
    for i in 0..10 {
        let y = 20.0 + 25.0 * i as f32;
        s.stroke_begin(PointerKind::Finger, vec![sample(10.0, y, 0.0)]);
        s.stroke_add((1..30).map(|k| sample(10.0 + 9.0 * k as f32, y + 3.0, f64::from(k) * 8.0)).collect());
        s.stroke_end(vec![]);
    }
    let dir = std::env::temp_dir().join(format!("procraft-twice-{}", std::process::id()));
    let path = dir.to_string_lossy().into_owned();
    // the open's save and the close's save, together, a few times over
    for _ in 0..4 {
        std::thread::scope(|scope| {
            let a = scope.spawn(|| s.save(path.clone()));
            let b = scope.spawn(|| s.save(path.clone()));
            assert!(a.join().unwrap().is_ok() && b.join().unwrap().is_ok());
        });
    }
    let back = CanvasSession::open(path).unwrap();
    let pending = back.shared.lock().pending_pixels.clone();
    let painted = pending.iter().map(|(_, px)| px.chunks(4).filter(|p| p[3] > 0).count()).sum::<usize>();
    assert!(painted > 5000, "{painted}");
    // nothing left over from the writes
    let left: Vec<_> = std::fs::read_dir(dir.join("layers"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "tmp"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn frosted_panels_come_six_numbers_each() {
    let Some(s) = session(100, 100) else { return };
    s.set_backdrops(vec![10.0, 20.0, 60.0, 80.0, 12.0, 0.5, 1.0, 2.0]);
    assert_eq!(s.shared.lock().backdrops, vec![[10.0, 20.0, 60.0, 80.0, 12.0, 0.5]]);
    s.set_backdrops(Vec::new());
    assert!(s.shared.lock().backdrops.is_empty());
}

#[test]
fn sets_kept_under_the_old_icon_numbering_get_their_icons_back() {
    let Some(s) = session(100, 100) else { return };
    let old = r#"[{"name":"Inking","brushes":[],"icon":1},{"name":"Mine","brushes":[],"icon":7}]"#;
    assert!(s.load_brush_library(old.to_owned()));
    let icons: Vec<u32> = s.shared.lock().library.iter().map(|set| set.icon).collect();
    // the shipped set its own icon, the other the brush
    assert_eq!(icons, vec![43, 5]);
    // kept again, they stay
    let kept = s.saved_brush_library();
    assert!(s.load_brush_library(kept));
    assert_eq!(s.shared.lock().library.iter().map(|set| set.icon).collect::<Vec<_>>(), vec![43, 5]);
}

#[test]
fn a_canvas_keeps_its_colour_profile() {
    let Some(s) = session(40, 30) else { return };
    assert_eq!(s.profile(), "sRGB IEC61966-2.1");
    assert!(!s.set_new_profile("Adobe RGB".to_owned()));
    assert!(s.set_new_profile("Display P3".to_owned()));
    let dir = std::env::temp_dir().join(format!("procraft-profile-{}", std::process::id()));
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    let t = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
    assert_eq!(t.profile(), "Display P3");
    assert_eq!(crate::colour::color_profiles().first().map(String::as_str), Some("Display P3"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_p3_canvas_exports_with_its_profile() {
    let Some(s) = session(20, 10) else { return };
    assert!(s.set_new_profile("Display P3".to_owned()));
    let path = std::env::temp_dir().join(format!("procraft-p3-{}.png", std::process::id()));
    s.export_png(path.to_string_lossy().into_owned()).unwrap();
    let reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap())).read_info().unwrap();
    let icc = reader.info().icc_profile.as_ref().expect("an embedded profile").to_vec();
    assert_eq!(icc, procraft_engine::icc::icc(procraft_engine::profile::Profile::DisplayP3));
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_cmyk_canvas_keeps_rgb_pixels_and_prints_its_inks() {
    let Some(s) = session(20, 10) else { return };
    let cmyk = procraft_engine::cmyk::NAME.to_owned();
    assert!(s.set_new_profile(cmyk.clone()));
    assert!(s.is_cmyk());
    assert_eq!(s.profile(), cmyk);
    // the colour panel's numbers: green takes cyan and yellow; white is the paper
    let inks = s.cmyk_of(0x00FF00);
    assert!(inks[0] > 0.3 && inks[1] < 0.05 && inks[2] > 0.5 && inks[3] < 0.05, "{inks:?}");
    assert_eq!(s.cmyk_of(0xFFFFFF), vec![0.0; 4]);
    let proofed = s.proof_color(0x00FF00);
    assert_ne!(proofed, 0x00FF00, "pure green can't be printed");
    assert_eq!(s.rgb_of_cmyk(vec![0.0; 4]) & 0xF0F0F0, 0xF0F0F0, "no ink shows the paper");
    // a green layer: the layer keeps green; the PNG shows the press's green, the TIFF its inks
    s.set_brush_size(1.0);
    s.set_color(0.0, 1.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(2.0, 5.0, 0.0), sample(18.0, 5.0, 10.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 10, 5), [0, 255, 0, 255]);
    let dir = std::env::temp_dir().join(format!("procraft-cmyk-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let png_path = dir.join("a.png");
    s.export_png(png_path.to_string_lossy().into_owned()).unwrap();
    let mut reader =
        png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&png_path).unwrap())).read_info().unwrap();
    let icc = reader.info().icc_profile.as_ref().unwrap().to_vec();
    assert_eq!(icc, procraft_engine::icc::icc(procraft_engine::profile::Profile::DisplayP3));
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    let at = (5 * 20 + 10) * 4;
    let shown = (u32::from(buf[at]) << 16) | (u32::from(buf[at + 1]) << 8) | u32::from(buf[at + 2]);
    assert!(shown != 0x00FF00 && buf[at] > 40, "{shown:06x}");
    s.export_tiff(dir.join("a.tif").to_string_lossy().into_owned()).unwrap();
    let tiff = std::fs::read(dir.join("a.tif")).unwrap();
    // photometric 5: separated inks
    assert!(tiff.windows(4).any(|w| w == [6, 1, 3, 0]) && tiff.windows(8).any(|w| w == [6, 1, 3, 0, 1, 0, 0, 0]));
    // saved and opened again, it is still CMYK
    s.save(dir.join("art").to_string_lossy().into_owned()).unwrap();
    let t = CanvasSession::open(dir.join("art").to_string_lossy().into_owned()).unwrap();
    assert!(t.is_cmyk());
    assert_eq!(t.profile(), cmyk);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn changing_the_profile_turns_the_colours_and_undoes() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert!(!s.change_profile("sRGB IEC61966-2.1".to_owned()));
    assert!(!s.change_profile("Adobe RGB".to_owned()));
    assert!(s.change_profile("Display P3".to_owned()));
    assert_eq!(s.profile(), "Display P3");
    // the red in P3's numbers; white stays white
    assert_eq!(pixel(&s, 20, 50), [0xEA, 0x33, 0x23, 255]);
    assert_eq!(pixel(&s, 80, 80), [255, 255, 255, 255]);
    // one undo step puts the numbers and the profile back
    s.undo();
    assert_eq!(s.profile(), "sRGB IEC61966-2.1");
    assert_eq!(pixel(&s, 20, 50), [255, 0, 0, 255]);
    s.redo();
    assert_eq!(s.profile(), "Display P3");
    assert_eq!(pixel(&s, 20, 50), [0xEA, 0x33, 0x23, 255]);
}

#[test]
fn an_artwork_never_shown_exports_offscreen() {
    let Some(s) = session(40, 40) else { return };
    s.set_brush_size(0.4);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 20.0, 0.0)]);
    s.stroke_end(vec![]);
    let dir = std::env::temp_dir().join(format!("procraft-offscreen-{}", std::process::id()));
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    let t = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
    let png = dir.join("out.png");
    // not shown: no picture until it has a device of its own
    assert!(t.export_png(png.to_string_lossy().into_owned()).is_err());
    assert!(t.prepare_offscreen());
    t.export_png(png.to_string_lossy().into_owned()).unwrap();
    let mut reader =
        png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&png).unwrap())).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    let at = (20 * info.width as usize + 20) * 4;
    assert_eq!(&buf[at..at + 4], &[255, 0, 0, 255]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn saving_while_idle_writes_only_what_changed() {
    let Some(s) = session(40, 40) else { return };
    let dir = std::env::temp_dir().join(format!("procraft-autosave-{}", std::process::id()));
    let path = dir.to_string_lossy().into_owned();
    s.save(path.clone()).unwrap();
    let edits = s.edits();
    // a stroke under way isn't cut short
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 20.0, 0.0)]);
    assert!(!s.save_if_idle(path.clone()).unwrap());
    s.stroke_end(vec![]);
    assert_ne!(s.edits(), edits);
    assert!(s.save_if_idle(path.clone()).unwrap());
    // a new layer: the first one's file isn't written again
    let first = s.layers()[0].id;
    let file = dir.join("layers").read_dir().unwrap().flatten().next().unwrap().path();
    let written = std::fs::metadata(&file).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    s.add_layer();
    assert!(s.save_if_idle(path.clone()).unwrap());
    assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), written);
    assert_eq!(dir.join("layers").read_dir().unwrap().count(), 2);
    let t = CanvasSession::open(path).unwrap();
    assert_eq!(t.layers().len(), 2);
    assert!(t.layers().iter().any(|l| l.id == first));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn what_shipped_later_joins_a_kept_library_once() {
    let s = CanvasSession::new(64, 64);
    let names = |s: &CanvasSession| s.brush_library().into_iter().map(|set| set.name).collect::<Vec<_>>();
    // a new library: every set, in shelf order, with the tools on their brushes
    let all = names(&s);
    assert_eq!(&all[..3], ["Sketching", "Inking", "Drawing"]);
    assert_eq!(s.current_brush().name, "Round Pen");
    let counts = s.shipped_brush_counts();
    // a library kept when the first six sets shipped with three, three, three, two, two and two
    let old = CanvasSession::new(64, 64);
    let first = ["Sketching", "Inking", "Painting", "Airbrushing", "Charcoals", "Textures"];
    while let Some(i) = names(&old).iter().position(|n| !first.contains(&n.as_str())) {
        assert!(old.delete_brush_set(i as u32));
    }
    let inking = names(&old).iter().position(|n| n == "Inking").unwrap() as u32;
    while old.brush_library()[inking as usize].brushes.len() > 3 {
        assert!(old.delete_brush(inking, 3));
    }
    let kept = old.saved_brush_library();
    let t = CanvasSession::new(64, 64);
    assert!(t.load_brush_library(kept));
    assert_eq!(t.brush_library().len(), 6);
    assert_eq!(t.add_shipped_brushes(vec![3, 3, 3, 2, 2, 2]), counts);
    // the new sets at the end, the new inking brushes after the old ones
    assert_eq!(t.brush_library().len(), all.len());
    let ink = &t.brush_library()[inking as usize];
    assert_eq!(ink.brushes.len() as u32, counts[1]);
    assert_eq!(ink.brushes[3].name, "Dip Pen");
    // and only once, whatever it is told
    assert_eq!(t.add_shipped_brushes(vec![0; 6]), counts);
    assert_eq!(t.brush_library().len(), all.len());
    assert_eq!(t.brush_library()[inking as usize].brushes.len() as u32, counts[1]);
}

#[test]
fn a_canvas_tells_the_view_it_will_open_with() {
    let s = CanvasSession::new(200, 100);
    // fitted: 400 × 400 shows it 400 wide, 200 tall, in the middle
    assert_eq!(s.opening_view_matrix(400, 400), vec![2.0, 0.0, 0.0, 2.0, 0.0, 100.0]);
}

#[test]
fn undo_and_redo_tell_what_they_undid() {
    let Some(s) = session(100, 100) else { return };
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    s.flip_canvas(true);
    assert_eq!(s.undo().as_deref(), Some("flip canvas horizontally"));
    assert_eq!(s.undo().as_deref(), Some("Paint stroke"));
    assert_eq!(s.redo().as_deref(), Some("Paint stroke"));
    s.set_tool(ToolKind::Erase);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(s.undo().as_deref(), Some("Erase stroke"));
    // layer changes, made at once or by a slider
    let id = s.add_layer();
    assert_eq!(s.undo().as_deref(), Some("new layer"));
    s.redo();
    s.set_layer_opacity(id, 0.5, false);
    s.set_layer_opacity(id, 0.4, true);
    assert_eq!(s.undo().as_deref(), Some("layer opacity"));
    s.set_layer_visible(id, false);
    assert_eq!(s.undo().as_deref(), Some("hide layer"));
    // selections
    use crate::selection::SelectionJoin;
    s.select_shape(false, 10.0, 10.0, 40.0, 40.0, SelectionJoin::Add);
    s.select_shape(false, 50.0, 50.0, 60.0, 60.0, SelectionJoin::Add);
    s.select_invert();
    s.select_none();
    assert_eq!(s.undo().as_deref(), Some("clear selection"));
    assert_eq!(s.undo().as_deref(), Some("invert"));
    assert_eq!(s.undo().as_deref(), Some("add to selection"));
    assert_eq!(s.undo().as_deref(), Some("draw selection"));
}

#[test]
fn a_stamp_preview_shows_the_stamp_alone() {
    let Some(s) = session(100, 100) else { return };
    let b = s.current_brush();
    // painted columns: a stroke reaches across the preview, a stamp keeps to its middle
    let reach = |s: &CanvasSession| {
        let p = s.brush_preview(b.set, b.index, 327, 100).unwrap();
        let cols: Vec<usize> = (0..327).filter(|&x| (0..100).any(|y| p.rgba[(y * 327 + x) * 4 + 3] > 0)).collect();
        (cols.first().copied().unwrap_or(0), cols.last().copied().unwrap_or(0))
    };
    let (left, right) = reach(&s);
    assert!(left < 50 && right > 270, "stroke {left}..{right}");
    s.studio_begin();
    s.set_studio_setting("preview_stamp".into(), 1.0);
    s.studio_end(true);
    let (left, right) = reach(&s);
    assert!(left > 110 && right < 220, "stamp {left}..{right}");
}

#[test]
fn slider_notches_are_kept_with_the_library() {
    let s = CanvasSession::new(64, 64);
    assert!(s.add_slider_notch(true, 0.3));
    assert!(s.add_slider_notch(false, 0.5));
    assert!(!s.add_slider_notch(true, 0.31));
    assert_eq!(s.slider_notches(true), vec![0.3]);
    let t = CanvasSession::new(64, 64);
    assert!(t.load_brush_library(s.saved_brush_library()));
    assert_eq!(t.slider_notches(true), vec![0.3]);
    assert_eq!(t.slider_notches(false), vec![0.5]);
    assert!(t.remove_slider_notch(true, 0.32));
    assert!(t.slider_notches(true).is_empty());
    assert!(!t.remove_slider_notch(true, 0.3));
}

#[test]
fn a_brushs_own_pressure_curves_are_kept_with_the_library() {
    use procraft_engine::brush::PressureCurve;
    use procraft_engine::curve::Curve;
    use procraft_engine::geom::Vec2;
    let bend = Curve::new(&[Vec2::new(0.0, 0.2), Vec2::new(1.0, 0.6)]);
    let s = CanvasSession::new(64, 64);
    {
        let mut st = s.shared.lock();
        let (set, index) = st.brush_ref(st.tool);
        let brush = st.library.get_mut(set).and_then(|l| l.brushes.get_mut(index)).unwrap();
        brush.pressure_curves.set(PressureCurve::GrainDepth, Some(bend.clone()));
    }
    let t = CanvasSession::new(64, 64);
    assert!(t.load_brush_library(s.saved_brush_library()));
    let st = t.shared.lock();
    assert_eq!(st.brush(st.tool).pressure_curves.get(PressureCurve::GrainDepth), Some(&bend));
}

#[test]
fn a_pad_keeps_its_strokes_and_its_sample_gives_way() {
    let pad = CanvasSession::new(200, 120);
    let kept = |p: &CanvasSession| {
        p.shared.lock().pad_strokes.as_ref().map(|k| k.iter().map(|(_, s)| s.len()).collect::<Vec<_>>())
    };
    // a canvas keeps nothing unless asked
    assert_eq!(kept(&pad), None);
    pad.keep_strokes();
    pad.draw_sample_scribble();
    assert_eq!(kept(&pad).map(|k| k.len()), Some(1));
    // one's own stroke takes the sample's place
    pad.begin_own_stroke();
    assert_eq!(kept(&pad), Some(vec![]));
    pad.stroke_begin(PointerKind::Finger, vec![sample(10.0, 10.0, 0.0)]);
    pad.stroke_add(vec![sample(50.0, 40.0, 16.0)]);
    pad.stroke_end(vec![sample(90.0, 60.0, 32.0)]);
    assert_eq!(kept(&pad), Some(vec![3]));
    // a second one isn't the sample: nothing goes
    pad.begin_own_stroke();
    pad.stroke_begin(PointerKind::Finger, vec![sample(10.0, 90.0, 0.0)]);
    pad.stroke_cancel();
    assert_eq!(kept(&pad), Some(vec![3]));
    // drawn again, it is kept as it was
    pad.replay_strokes();
    assert_eq!(kept(&pad), Some(vec![3]));
    pad.forget_strokes();
    assert_eq!(kept(&pad), Some(vec![]));
}

#[test]
fn the_pad_turns_into_a_sphere_of_its_own_and_back() {
    let Some(pad) = session(300, 300) else { return };
    // where each kept stroke's samples are
    let kept = |p: &CanvasSession| {
        let k = p.shared.lock().pad_strokes.clone().unwrap_or_default();
        k.iter().map(|(_, s)| s.iter().map(|p| (p.x, p.y)).collect::<Vec<_>>()).collect::<Vec<_>>()
    };
    pad.keep_strokes();
    pad.set_background_color(0.1, 0.1, 0.1, true);
    // half a pixel a point: the sphere 120 pixels round
    pad.set_display_scale(0.5);
    pad.draw_sample_scribble();
    pad.begin_own_stroke();
    pad.stroke_begin(PointerKind::Finger, vec![sample(10.0, 10.0, 0.0)]);
    pad.stroke_end(vec![sample(290.0, 20.0, 32.0)]);
    let flat = kept(&pad);
    pad.show_sphere(true);
    // its own sample scribble, on the sphere's face (in the pad's middle), over nothing
    {
        let st = pad.shared.lock();
        assert!(st.pad_sample && !st.doc.background_visible);
    }
    let k = kept(&pad);
    assert_eq!(k.len(), 1);
    assert!(k[0].iter().all(|(x, y)| (x - 150.0).hypot(y - 150.0) < 120.0));
    // drawn: the pad's colour round the sphere, the sphere lit inside
    let px = {
        let mut guard = pad.shared.lock();
        let st = &mut *guard;
        let format = procraft_gpu::wgpu::TextureFormat::Rgba8Unorm;
        let gpu = st.gpu.clone().unwrap();
        let target = gpu.device.create_texture(&procraft_gpu::wgpu::TextureDescriptor {
            label: Some("test frame"),
            size: procraft_gpu::wgpu::Extent3d { width: 300, height: 300, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: procraft_gpu::wgpu::TextureDimension::D2,
            format,
            usage: procraft_gpu::wgpu::TextureUsages::RENDER_ATTACHMENT | procraft_gpu::wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&procraft_gpu::wgpu::TextureViewDescriptor::default());
        st.render_sphere(&view, format, (300, 300), None);
        assert_eq!(gpu.errors(), 0, "GPU validation errors");
        procraft_gpu::read_texture(&gpu, &target, 300, 300).unwrap()
    };
    let at = |x: usize, y: usize| px[(y * 300 + x) * 4];
    assert!(at(2, 2).abs_diff(26) <= 1, "{}", at(2, 2));
    assert!(at(150, 40) > at(2, 2) + 30, "{} {}", at(150, 40), at(2, 2));
    // flat again: its strokes as they were, over the pad's colour
    pad.show_sphere(false);
    assert_eq!(kept(&pad), flat);
    let st = pad.shared.lock();
    assert!(!st.pad_sample && st.doc.background_visible && st.sphere.is_none());
}

#[test]
fn choosing_a_layer_is_an_undo_step() {
    let Some(s) = session(32, 32) else { return };
    let current = |s: &CanvasSession| s.layers().iter().find(|l| l.current).map(|l| l.id);
    let first = current(&s).unwrap();
    let second = s.add_layer();
    // choosing the one already current makes no step
    assert!(s.select_layer(second));
    assert!(s.select_layer(first));
    assert_eq!(s.undo().as_deref(), Some("select layer"));
    assert_eq!(current(&s), Some(second));
    assert!(s.redo().is_some());
    assert_eq!(current(&s), Some(first));
}

#[test]
fn picked_layers_merge_duplicate_and_group_as_one_step_each() {
    let Some(s) = session(100, 100) else { return };
    let dot = |x: f32, y: f32, rgb: [f32; 3]| {
        s.set_color(rgb[0], rgb[1], rgb[2]);
        s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
        s.stroke_end(vec![]);
    };
    s.set_brush_size(0.6);
    dot(30.0, 30.0, [1.0, 0.0, 0.0]);
    let red = s.layers()[0].id;
    let middle = s.add_layer();
    let blue = s.add_layer();
    dot(70.0, 70.0, [0.0, 0.0, 1.0]);
    // blue current, red picked: the middle layer is left alone
    assert!(s.pick_layer(red));
    assert_eq!(s.chosen_layers(), [red, blue]);
    assert!(s.layers().iter().any(|l| l.id == red && l.picked));
    assert!(s.merge_picked());
    let ids: Vec<u64> = s.layers().iter().map(|l| l.id).collect();
    // top first
    assert_eq!(ids, [middle, red]);
    assert_eq!(pixel(&s, 70, 70), [0, 0, 255, 255]);
    assert_eq!(pixel(&s, 30, 30), [255, 0, 0, 255]);
    // one step undoes it all, the picks too
    assert_eq!(s.undo().as_deref(), Some("merge layers"));
    assert_eq!(s.layers().len(), 3);
    assert_eq!(s.chosen_layers(), [red, blue]);
    // the copies come out together above blue and are what is chosen
    assert!(s.duplicate_picked());
    assert_eq!(s.layers().len(), 5);
    let chosen = s.chosen_layers();
    assert!(chosen.iter().all(|c| ![red, middle, blue].contains(c)));
    assert_eq!(pixel(&s, 30, 30), [255, 0, 0, 255]);
    s.undo();
    let group = s.group_picked();
    assert_ne!(group, 0);
    let inside: Vec<u64> = s.layers().iter().filter(|l| l.parent == Some(group)).map(|l| l.id).collect();
    assert_eq!(inside, [blue, red]);
    assert!(s.layers().iter().all(|l| !l.picked));
    // choosing a layer clears the picks, as an undo step
    s.undo();
    assert!(s.select_layer(blue));
    assert!(s.chosen_layers() == [blue]);
    s.undo();
    assert_eq!(s.chosen_layers(), [red, blue]);
}

#[test]
fn picked_layers_transform_together() {
    use crate::transform::TransformHandle;
    let Some(s) = session(100, 100) else { return };
    let dot = |x: f32, y: f32| {
        s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
        s.stroke_end(vec![]);
    };
    dot(20.0, 20.0);
    let below = s.layers()[0].id;
    s.add_layer();
    dot(40.0, 40.0);
    // the box holds both dots
    assert!(s.pick_layer(below));
    let b = s.transform_begin().unwrap();
    let [x0, y0, x1, _, _, y1, ..] = b.corners[..] else { panic!() };
    assert!(x0 < 20.0 && y0 < 20.0 && x1 > 40.0 && y1 > 40.0, "{b:?}");
    s.transform_grab(TransformHandle::Move, 30.0, 30.0);
    s.transform_drag(60.0, 30.0);
    s.transform_release();
    s.transform_end(true);
    assert_eq!(pixel(&s, 50, 20), [0, 0, 0, 255]);
    assert_eq!(pixel(&s, 70, 40), [0, 0, 0, 255]);
    assert_eq!(pixel(&s, 20, 20), [255, 255, 255, 255]);
    // one step puts both back
    s.undo();
    assert_eq!(pixel(&s, 20, 20), [0, 0, 0, 255]);
    assert_eq!(pixel(&s, 40, 40), [0, 0, 0, 255]);
}

/// On request (`BRUSH_SHEET=path.png`): every library brush's sample stroke, three to a row on
/// dark grey, with their names listed beside it (`path.txt`), to look the library over.
#[test]
fn brush_sheet() {
    let Ok(out) = std::env::var("BRUSH_SHEET") else { return };
    let Some(s) = session(64, 64) else { return };
    let (cw, ch, cols) = (347u32, 110u32, 4u32);
    let brushes: Vec<(u32, u32, String)> = s
        .brush_library()
        .iter()
        .enumerate()
        .flat_map(|(i, set)| {
            set.brushes.iter().enumerate().map(move |(j, b)| (i as u32, j as u32, format!("{} / {}", set.name, b.name)))
        })
        .collect();
    let rows = (brushes.len() as u32).div_ceil(cols);
    let (w, h) = (cw * cols, ch * rows);
    let mut sheet: Vec<u8> = (0..w * h).flat_map(|_| [30u8, 30, 30, 255]).collect();
    let mut names = String::new();
    for (k, (set, index, name)) in brushes.iter().enumerate() {
        let (cx, cy) = ((k as u32 % cols) * cw, (k as u32 / cols) * ch);
        names.push_str(&format!("{} {}: {name}\n", k as u32 / cols, k as u32 % cols));
        let Some(p) = s.brush_preview(*set, *index, cw - 20, ch - 10) else { continue };
        for y in 0..p.height {
            for x in 0..p.width {
                let a = u32::from(p.rgba[((y * p.width + x) * 4 + 3) as usize]);
                let at = (((cy + 5 + y) * w + cx + 10 + x) * 4) as usize;
                for c in 0..3 {
                    sheet[at + c] = ((30 * (255 - a) + 255 * a) / 255) as u8;
                }
            }
        }
    }
    let file = std::fs::File::create(&out).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.write_header().unwrap().write_image_data(&sheet).unwrap();
    std::fs::write(format!("{out}.txt"), names).unwrap();
}

/// On request (`SET_SHEET=path.png`, `SET_FILE=set file`): the set file's brushes imported and
/// their previews drawn at the size brush files carry them, one to a row on dark grey.
#[test]
fn set_sheet() {
    let (Ok(out), Ok(file)) = (std::env::var("SET_SHEET"), std::env::var("SET_FILE")) else { return };
    let Some(s) = session(64, 64) else { return };
    let set = s.import_brushes(std::fs::read(file).unwrap()).unwrap();
    let count = s.brush_library().get(set as usize).unwrap().brushes.len() as u32;
    let (w, ch) = (327u32, 100u32);
    let h = ch * count;
    let mut sheet: Vec<u8> = (0..w * h).flat_map(|_| [30u8, 30, 30, 255]).collect();
    for k in 0..count {
        let Some(p) = s.brush_preview(set, k, w, ch) else { continue };
        for y in 0..p.height {
            for x in 0..p.width {
                let a = u32::from(p.rgba[((y * p.width + x) * 4 + 3) as usize]);
                let at = (((k * ch + y) * w + x) * 4) as usize;
                for c in 0..3 {
                    sheet[at + c] = ((30 * (255 - a) + 255 * a) / 255) as u8;
                }
            }
        }
    }
    let file = std::fs::File::create(&out).unwrap();
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.write_header().unwrap().write_image_data(&sheet).unwrap();
}

/// The rendering modes build up within a stroke as measured on the reference's drawing pad: a hard
/// round brush 22 px across, spaced as its file keeps 0.042, at 49% flow, one pass along a line.
#[test]
fn rendering_modes_build_up_as_measured() {
    use procraft_engine::brush::{Brush, Taper};
    use procraft_engine::document::Document;
    use procraft_engine::geom::Vec2;
    use procraft_engine::input::{Pointer, Sample};
    use procraft_engine::stroke::StrokeBuilder;
    use procraft_gpu::{Canvas, StrokeStyle, StrokeTool};
    let Ok(gpu) = Gpu::headless() else { return };
    let pass = |mode: f32| -> f32 {
        let mut b = Brush {
            streamline: 0.0,
            pressure_size: 0.0,
            pressure_opacity: 0.0,
            pen_taper: Taper::NONE,
            touch_taper: Taper::NONE,
            hardness: 1.0,
            ..Brush::round_pen()
        };
        b.set_setting("spacing", (0.042f32 / 2.0).powf(1.0 / 2.2));
        b.set_setting("flow", 0.49);
        b.set_setting("rendering_mode", mode);
        let mut doc = Document::new(200, 60);
        doc.background_visible = false;
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let layer = doc.current();
        let mut style = StrokeStyle::for_brush(&b, StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0, false);
        canvas.use_sources(&gpu, &mut style, &b);
        canvas.begin_stroke(layer, style);
        let mut stroke = StrokeBuilder::new(b, 22.0, Pointer::Stylus);
        let samples: Vec<Sample> =
            (0..=40).map(|i| Sample::at(Vec2::new(20.0 + i as f32 * 4.0, 30.0), 1.0, f64::from(i) * 8.0)).collect();
        canvas.add_stamps(&stroke.add(&samples));
        canvas.replace_stamps(&stroke.finish());
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        let px = canvas.read_composite(&gpu).unwrap();
        f32::from(px[((30 * 200 + 100) * 4 + 3) as usize]) / 255.0
    };
    // Light Glaze keeps the strongest stamp: 0.49 to the power 2.5
    assert!((pass(0.0) - 0.170).abs() < 0.02, "light glaze {}", pass(0.0));
    // Intense Glaze (as Uniformed Glaze and Intense Blending) builds up over
    assert!((pass(2.0) - 0.826).abs() < 0.04, "intense glaze {}", pass(2.0));
    // Heavy Glaze (as Uniform Blending) builds up gradually
    assert!((pass(3.0) - 0.638).abs() < 0.04, "heavy glaze {}", pass(3.0));
}

#[test]
fn the_brushes_in_use_are_kept_between_sessions() {
    let s = CanvasSession::new(64, 64);
    let chosen = |s: &CanvasSession, set: usize| s.brush_library()[set].chosen;
    // a brush chosen in set 1 at a size of its own, then one in set 0: set 1 keeps showing its own
    assert!(s.select_brush(1, 2));
    s.set_brush_size(0.7);
    assert!(s.select_brush(0, 3));
    assert_eq!((chosen(&s, 0), chosen(&s, 1)), (Some(3), Some(2)));
    // set 1's choice moved into set 0: set 1 has none, set 0 keeps its later choice
    assert!(s.move_brush(1, 2, 0, 0));
    assert_eq!((chosen(&s, 0), chosen(&s, 1)), (Some(4), None));
    // kept with the library, they come back in a new session, sliders and all
    let (library, choices) = (s.saved_brush_library(), s.saved_brush_choices());
    let t = CanvasSession::new(64, 64);
    assert!(t.load_brush_library(library));
    assert!(t.load_brush_choices(choices));
    assert_eq!((t.current_brush().set, t.current_brush().index), (0, 4));
    assert_eq!((chosen(&t, 0), chosen(&t, 1)), (Some(4), None));
    assert!(t.select_brush(0, 0));
    assert!((t.info().size - 0.7).abs() < 1e-6);
    // what can't be read changes nothing; places no longer there are passed over
    assert!(!t.load_brush_choices("{".to_owned()));
    assert!(t.load_brush_choices(r#"{"tools":[[99,0],[0,99],[0,0]],"sets":[[7,900]]}"#.to_owned()));
    assert_eq!((t.current_brush().set, t.current_brush().index), (0, 0));
    assert_eq!(chosen(&t, 0), None);
}

#[test]
fn a_brush_keeps_its_maker_and_a_reset_point() {
    use procraft_engine::brush::Brush;
    use procraft_engine::brushfile::{self, Pictures};
    let s = CanvasSession::new(64, 64);
    // a file by someone, with their picture (and a signature that isn't a picture)
    let face = crate::project::rgba_png(2, 2, &[200; 16]);
    let shape = crate::project::rgba_png(1, 1, &[255; 4]);
    let file = brushfile::write(
        &Brush { name: "Theirs".to_owned(), spacing: 0.2, ..Brush::round_pen() },
        "Someone",
        &Pictures {
            shape_png: &shape,
            grain_png: None,
            metallic_png: None,
            roughness_png: None,
            thumbnail_png: &shape,
            author_png: Some(&face),
            signature_png: Some(b"not a png"),
        },
    );
    let set = s.import_brushes(file).unwrap();
    assert!(s.select_brush(set, 0));
    // opened in the studio the first time, it is set aside to go back to
    s.studio_begin();
    let about = s.studio_about();
    assert_eq!(
        (about.author.as_str(), about.picture.as_deref(), about.signature),
        ("Someone", Some(face.as_slice()), None)
    );
    let first = about.reset_made.unwrap();
    assert!(first > 0);
    let spacing = s.studio_settings(vec!["spacing".to_owned()])[0];
    s.set_studio_setting("spacing".to_owned(), 0.9);
    s.studio_end(true);
    // opened again, the reset point stays as it was; resetting goes back to it
    s.studio_begin();
    assert_eq!(s.studio_about().reset_made, Some(first));
    s.studio_reset();
    assert_eq!(s.studio_settings(vec!["spacing".to_owned()])[0], spacing);
    // a new one, kept even when the edits are cancelled; and kept with the library
    s.set_studio_setting("spacing".to_owned(), 0.6);
    s.studio_new_reset_point();
    s.studio_end(false);
    let t = CanvasSession::new(64, 64);
    assert!(t.load_brush_library(s.saved_brush_library()));
    assert!(t.select_brush(0, 0));
    t.studio_begin();
    assert_eq!(t.studio_about().author, "Someone");
    t.studio_reset();
    assert!((t.studio_settings(vec!["spacing".to_owned()])[0] - 0.6).abs() < 1e-6);
    // and the maker goes back out with the brush
    let out = brushfile::read(&s.brush_as_file(set, 0).unwrap()).unwrap();
    assert_eq!((out.author.as_str(), out.author_picture), ("Someone", Some(face)));
}

#[test]
fn a_rolling_grain_stays_put_when_painting_wet() {
    use procraft_engine::brush::{Brush, Grain, GrainMode};
    let Some(s) = session(200, 200) else { return };
    // a wet brush whose fine grain rolls along with the stroke, light enough for the grain to
    // show (built up in full, it fills in)
    let mut b = Brush {
        grain: Grain {
            image: Some(procraft_engine::textures::grain::NOISE),
            mode: GrainMode::Moving,
            scale: 256.0,
            depth: 1.0,
            source: None,
        },
        ..Brush::round_pen()
    };
    b.set_setting("rendering_mode", 5.0);
    b.set_setting("wet_pull", 0.5);
    b.set_setting("grain_movement", 1.0);
    b.set_setting("grain_offset_jitter", 0.0);
    b.set_setting("flow", 0.35);
    {
        let mut st = s.shared.lock();
        let (set, index) = st.paint_brush;
        st.library[set].brushes[index] = b;
    }
    s.set_brush_size(0.6);
    s.set_color(0.0, 0.0, 0.0);
    s.stroke_begin(
        PointerKind::Stylus,
        (0..60).map(|i| sample(20.0 + i as f32 * 3.0, 100.0, i as f64 * 8.0)).collect(),
    );
    s.stroke_end(vec![]);
    // along the stroke the grain shows, as it does across it, rather than smeared into streaks
    let row: Vec<f32> = (60..140).map(|x| f32::from(pixel(&s, x, 100)[0])).collect();
    let mean = row.iter().sum::<f32>() / row.len() as f32;
    let spread = (row.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / row.len() as f32).sqrt();
    assert!(spread > 6.0, "{spread}");
}

#[test]
fn a_moving_grain_streaks_along_the_stroke_however_the_stamps_turn() {
    use procraft_engine::brush::{Brush, Grain, GrainMode};
    let Some(s) = session(200, 200) else { return };
    // a wet brush turning each stamp at random, its fine grain moving a tenth of the way along
    let mut b = Brush {
        grain: Grain {
            image: Some(procraft_engine::textures::grain::SPECKLED),
            mode: GrainMode::Moving,
            scale: 256.0,
            depth: 1.0,
            source: None,
        },
        ..Brush::round_pen()
    };
    b.shape.randomise = true;
    b.set_setting("rendering_mode", 5.0);
    b.set_setting("grain_movement", 0.9);
    b.set_setting("grain_offset_jitter", 0.0);
    b.set_setting("flow", 0.35);
    {
        let mut st = s.shared.lock();
        let (set, index) = st.paint_brush;
        st.library[set].brushes[index] = b;
    }
    s.set_brush_size(1.0);
    s.set_color(0.0, 0.0, 0.0);
    s.stroke_begin(
        PointerKind::Stylus,
        (0..60).map(|i| sample(20.0 + i as f32 * 3.0, 100.0, i as f64 * 8.0)).collect(),
    );
    s.stroke_end(vec![]);
    // the grain keeps its way up, so each stamp lays it a little further on: streaks along the
    // stroke, neighbours alike along it and not across it
    let at = |x: u32, y: u32| f32::from(pixel(&s, x, y)[0]);
    let (mut along, mut across) = (0.0, 0.0);
    for y in 96..104 {
        for x in 80..120 {
            along += (at(x + 1, y) - at(x, y)).abs();
            across += (at(x, y + 1) - at(x, y)).abs();
        }
    }
    assert!(across > 1.0 && along < across * 0.6, "along {along}, across {across}");
}

#[test]
fn stock_images_a_file_names_are_kept_and_named_again() {
    use procraft_engine::brush::{Brush, StockImage, StockImages};
    use procraft_engine::brushfile::{self, Pictures, Texture};
    use procraft_engine::textures::grain;
    let s = CanvasSession::new(64, 64);
    let png = crate::project::rgba_png(1, 1, &[255; 4]);
    let named = |name: &str| Some(StockImage { name: name.to_owned(), inverted: true });
    let file = brushfile::write(
        &Brush {
            name: "Stocked".to_owned(),
            stock: StockImages {
                shape: named("Hard-Round.png"),
                grain: named("Old-Recycled-Paper.png"),
                metallic: named("Brushed-Metal.png"),
                roughness: None,
            },
            ..Brush::round_pen()
        },
        "",
        &Pictures {
            shape_png: &png,
            grain_png: None,
            metallic_png: None,
            roughness_png: None,
            thumbnail_png: &png,
            author_png: None,
            signature_png: None,
        },
    );
    let set = s.import_brushes(file).unwrap();
    {
        let st = s.shared.lock();
        let b = &st.library[set as usize].brushes[0];
        assert_eq!(b.stock.grain.as_ref().map(|g| g.name.as_str()), Some("Old-Recycled-Paper.png"));
        assert_eq!(b.grain.image, Some(grain::RECYCLED));
    }
    // kept with the library, and our stand-in found again by name
    let t = CanvasSession::new(64, 64);
    assert!(t.load_brush_library(s.saved_brush_library()));
    let set = {
        let st = t.shared.lock();
        let set = st.library.iter().position(|x| x.brushes.iter().any(|b| b.name == "Stocked")).unwrap();
        let b = &st.library[set].brushes[0];
        assert_eq!(b.stock.shape.as_ref().map(|g| (g.name.as_str(), g.inverted)), Some(("Hard-Round.png", true)));
        assert_eq!(b.grain.image, Some(grain::RECYCLED));
        // a stock metalness source we have nothing like: the amount all over, its name kept
        assert_eq!(b.metallic, procraft_engine::brush::MaterialSource::default());
        assert!(b.stock.metallic.is_some() && b.stock.roughness.is_none());
        set as u32
    };
    // written out, it names them again
    let out = brushfile::read(&t.brush_as_file(set, 0).unwrap()).unwrap();
    assert_eq!(out.grain, Texture::Stock { name: "Old-Recycled-Paper.png".to_owned(), inverted: true });
    assert_eq!(out.metallic, Texture::Stock { name: "Brushed-Metal.png".to_owned(), inverted: true });
    assert_eq!(out.roughness, Texture::None);
    assert_eq!(out.shape, Texture::Stock { name: "Hard-Round.png".to_owned(), inverted: true });
}

#[test]
fn a_dropped_colour_fills_up_to_lines_and_is_one_undo_step() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    // a line down the middle splits the canvas in two
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, -10.0, 0.0)]);
    s.stroke_add((1..30).map(|i| sample(50.0, -10.0 + i as f32 * 4.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![sample(50.0, 110.0, 250.0)]);
    let layer = s.layers().iter().find(|l| l.current).map(|l| l.id).unwrap();
    // off the canvas nothing is poured
    assert!(!s.pour_begin(-5.0, 50.0, 1.0, 0.0, 0.0, 0.5));
    // red on the left half only
    assert!(s.pour_begin(20.0, 50.0, 1.0, 0.0, 0.0, 0.5));
    assert_eq!(pixel(&s, 20, 50), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 80, 50), [255, 255, 255, 255]);
    // the threshold all the way: the right half too, the line kept
    s.pour_threshold(1.0);
    assert_eq!(pixel(&s, 80, 50), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 50, 50), [0, 0, 0, 255]);
    s.pour_threshold(0.5);
    assert_eq!(pixel(&s, 80, 50), [255, 255, 255, 255]);
    s.pour_end(true);
    assert_eq!(pixel(&s, 20, 50), [255, 0, 0, 255]);
    // one undo takes the pour back; the line stays
    s.undo();
    assert_eq!(pixel(&s, 20, 50), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 50, 50), [0, 0, 0, 255]);
    // a pour taken back leaves nothing to undo for it
    assert!(s.pour_begin(80.0, 50.0, 0.0, 0.0, 1.0, 0.5));
    s.pour_end(false);
    assert_eq!(pixel(&s, 80, 50), [255, 255, 255, 255]);
    // a locked layer takes none
    assert!(s.set_layer_locked(layer, true));
    assert!(!s.pour_begin(20.0, 50.0, 1.0, 0.0, 0.0, 0.5));
}

#[test]
fn a_layered_psd_comes_in_as_an_artwork() {
    use procraft_engine::document::BlendMode;
    use procraft_engine::formats::{PsdKind, PsdLayer, psd};
    let dir = std::env::temp_dir().join(format!("procraft-psd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (w, h) = (8, 6);
    let px = |c: [u8; 4]| -> Vec<u8> { std::iter::repeat_n(c, (w * h) as usize).flatten().collect() };
    let (green, flat) = (px([0, 200, 0, 255]), px([0, 200, 0, 255]));
    let layers = [
        PsdLayer {
            name: "Ink",
            kind: PsdKind::Pixels { rgba: &green, mask: None },
            opacity: 0.75,
            visible: true,
            blend: BlendMode::Overlay,
            clipped: false,
        },
        PsdLayer {
            name: "Background",
            kind: PsdKind::Pixels { rgba: &flat, mask: None },
            opacity: 1.0,
            visible: true,
            blend: BlendMode::Normal,
            clipped: false,
        },
    ];
    let file = dir.join("in.psd");
    std::fs::write(&file, psd(w, h, 72.0, &[], &layers, &flat, None)).unwrap();
    let art = dir.join("art");
    let outcome = crate::project::import_psd(
        file.to_string_lossy().into_owned(),
        art.to_string_lossy().into_owned(),
        "Imported".into(),
    );
    assert_eq!(outcome, crate::project::PsdOutcome::Imported);
    assert!(art.join("thumbnail.png").exists());
    let s = CanvasSession::open(art.to_string_lossy().into_owned()).unwrap();
    assert_eq!(s.name(), "Imported");
    let names: Vec<String> = s.layers().iter().map(|l| l.name.clone()).collect();
    assert!(names.contains(&"Ink".to_owned()) && names.contains(&"Background".to_owned()), "{names:?}");
    // not a PSD
    std::fs::write(&file, b"nothing").unwrap();
    assert_eq!(
        crate::project::import_psd(
            file.to_string_lossy().into_owned(),
            dir.join("bad").to_string_lossy().into_owned(),
            String::new()
        ),
        crate::project::PsdOutcome::Unreadable
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_layer_shown_alone_hides_the_others_until_solo_ends() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    let dot = |x: f32| {
        s.stroke_begin(PointerKind::Finger, vec![sample(x, 50.0, 0.0)]);
        s.stroke_end(vec![]);
    };
    dot(20.0);
    let top = s.add_layer();
    dot(80.0);
    assert_eq!(pixel(&s, 20, 50), [0, 0, 0, 255]);
    assert!(s.set_solo(Some(top)));
    assert_eq!(s.solo(), Some(top));
    assert_eq!(pixel(&s, 20, 50), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 80, 50), [0, 0, 0, 255]);
    // undo changes the artwork, not how it is looked at
    s.undo();
    assert_eq!(s.solo(), Some(top));
    assert_eq!(pixel(&s, 20, 50), [255, 255, 255, 255]);
    assert!(s.set_solo(None));
    assert_eq!(pixel(&s, 20, 50), [0, 0, 0, 255]);
    assert!(!s.set_solo(Some(999)));
    // the layer shown alone undone away: every layer shows again
    assert!(s.set_solo(Some(top)));
    s.undo();
    assert_eq!(s.solo(), None);
    assert_eq!(pixel(&s, 20, 50), [0, 0, 0, 255]);
}
