//! The session on the host: input in window pixels → strokes → pixels, on a headless GPU.

use procraft_engine::view::View;
use procraft_gpu::Gpu;

use crate::{CanvasSession, PenSample, PointerKind, ToolKind};

fn sample(x: f32, y: f32, t: f64) -> PenSample {
    PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: t, roll: 0.0 }
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

/// The current brush draws just where the input goes.
fn no_streamline(s: &CanvasSession) {
    s.studio_begin();
    s.set_studio_setting("streamline".to_owned(), 0.0);
    s.studio_end(true);
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

/// What a pen stroke costs with the GPU, for a few kinds of brush on a 2048 square canvas: each
/// event's work, each frame's (one every two events, as a 240 Hz pen at 120 frames a second), and
/// the whole stroke with the GPU done, an event's share. Run on a device with `--ignored
/// --nocapture`.
#[test]
#[ignore]
fn stroke_cost() {
    let Some(s) = session(2048, 1600) else { return };
    let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
    let at = |v: &mut Vec<f64>, q: f64| {
        v.sort_by(f64::total_cmp);
        v.get(((v.len().max(1) - 1) as f64 * q) as usize).copied().unwrap_or(0.0)
    };
    let point = |k: usize| {
        let t = k as f32 / 2000.0;
        PenSample {
            x: 100.0 + t * 1400.0,
            y: 800.0 + (t * 25.0).sin() * 500.0,
            pressure: 0.3 + 0.6 * (t * 40.0).sin().abs(),
            tilt: 0.0,
            orientation: 0.0,
            time_ms: k as f64 * 4.17,
            roll: 0.0,
        }
    };
    let frame = |s: &CanvasSession| {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        if let (Some(c), Some(g)) = (st.canvas.as_mut(), st.gpu.as_ref()) {
            c.render(g, &mut st.doc, None);
        }
    };
    let library = s.brush_library();
    for wanted in ["Technical Pencil", "Soft Airbrush", "Charcoal Stick", "Oil Paint", "Gouache", "Wet Wash"] {
        let found = library
            .iter()
            .enumerate()
            .find_map(|(i, set)| set.brushes.iter().position(|b| b.name == wanted).map(|j| (i as u32, j as u32)));
        let Some((set, index)) = found else {
            eprintln!("no {wanted}");
            continue;
        };
        s.select_brush(set, index);
        s.set_brush_size(0.4);
        let started = std::time::Instant::now();
        s.stroke_begin(PointerKind::Stylus, vec![point(0)]);
        let (mut adds, mut frames) = (Vec::new(), Vec::new());
        for e in 1..=1000 {
            let t = std::time::Instant::now();
            s.stroke_add(vec![point(2 * e - 1), point(2 * e)]);
            adds.push(ms(t));
            if e % 2 == 0 {
                let t = std::time::Instant::now();
                frame(&s);
                frames.push(ms(t));
            }
        }
        s.stroke_end(vec![]);
        frame(&s);
        if let Some(g) = s.shared.lock().gpu.as_ref() {
            g.flush();
        }
        let whole = ms(started) / 1000.0;
        println!(
            "{wanted}: event {:.3} ms (99th {:.3}), frame {:.3} ms (99th {:.3}), whole with the GPU {:.3} ms an event",
            at(&mut adds, 0.5),
            at(&mut adds, 0.99),
            at(&mut frames, 0.5),
            at(&mut frames, 0.99),
            whole
        );
        s.undo();
    }
}

/// Real files damaged (cut short, bytes changed) never bring a reader down: each brush file, PSD,
/// layered package, model, palette and colour profile in the folders `READER_FUZZ` lists (`;` between), each damaged in
/// `READER_FUZZ_ROUNDS` ways (12 unless said). Run with `--ignored --nocapture`.
#[test]
#[ignore]
fn readers_never_panic_on_damaged_files() {
    let Ok(roots) = std::env::var("READER_FUZZ") else { return };
    let mut files = Vec::new();
    let mut folders: Vec<std::path::PathBuf> = roots.split(';').map(std::path::PathBuf::from).collect();
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path.metadata().is_ok_and(|m| m.len() < 8 << 20) {
                files.push(path);
            }
        }
    }
    files.sort();
    let rounds: usize = std::env::var("READER_FUZZ_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(12);
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let (mut tried, mut panics) = (0, Vec::new());
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else { continue };
        let name = path.file_name().and_then(|n| n.to_str()).map(str::to_owned);
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
        for round in 0..rounds {
            let mut damaged = bytes.clone();
            if round % 3 == 0 && !damaged.is_empty() {
                damaged.truncate((next() % damaged.len() as u64) as usize);
            } else {
                for _ in 0..1 + round % 12 {
                    if damaged.is_empty() {
                        break;
                    }
                    let at = (next() % damaged.len() as u64) as usize;
                    if let Some(b) = damaged.get_mut(at) {
                        *b = (next() & 0xff) as u8;
                    }
                }
            }
            tried += 1;
            let read = std::panic::catch_unwind(|| match extension.as_str() {
                "psd" | "psb" => _ = procraft_engine::psd::read(&damaged),
                "procreate" | "procraft" => _ = procraft_engine::native::read(&damaged),
                "obj" => _ = procraft_engine::mesh::read_obj(&String::from_utf8_lossy(&damaged), None),
                "usdz" => _ = procraft_engine::usd::read_usdz(&damaged),
                "usd" | "usdc" | "usda" => _ = procraft_engine::usd::read_usd(&damaged),
                "swatches" => _ = procraft_engine::palette::read_swatches(&damaged),
                "ase" | "aco" => _ = procraft_engine::palette::read_other(&damaged, name.as_deref()),
                "icc" | "icm" => _ = procraft_engine::cms::IccProfile::parse(&damaged),
                _ => _ = crate::brush_files::read_any_brushes(&damaged, name.clone()),
            });
            if read.is_err() {
                panics.push(format!("{} (round {round})", path.display()));
            }
        }
    }
    eprintln!("{} files, {tried} damaged copies read, {} panics", files.len(), panics.len());
    assert!(
        panics.is_empty(),
        "{}",
        panics.join(
            "
"
        )
    );
}

/// What the heavier steps cost on a canvas of 4K's pixels (2896 square) with six painted layers,
/// each with the GPU done: a Gaussian blur's slider dragged (a step and the frame after it), HSB
/// likewise, the adjustments kept, an undo and a redo, a transform dragged and kept, text typed, a
/// save. Run
/// on a device with `--ignored --nocapture` (`OP_COST_DIR` a folder to save into).
#[test]
#[ignore]
fn operation_cost() {
    use crate::Adjustment;
    use crate::transform::TransformHandle;
    // as the tablet keeps them (OP_COST_CACHE): the layers not worked on composited once, kept
    if std::env::var("OP_COST_CACHE").is_ok() {
        crate::layers::set_resident_budget(4 << 30);
    }
    let Some(s) = session(2896, 1600) else { return };
    let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
    let done = |s: &CanvasSession| {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        if let (Some(c), Some(g)) = (st.canvas.as_mut(), st.gpu.as_ref()) {
            c.render(g, &mut st.doc, None);
            g.flush();
        }
    };
    let timed = |name: &str, f: &dyn Fn()| {
        let t = std::time::Instant::now();
        f();
        done(&s);
        println!("{name}: {:.1} ms", ms(t));
    };
    let steps = |name: &str, n: usize, f: &dyn Fn(usize)| {
        let mut each: Vec<f64> = (0..n)
            .map(|i| {
                let t = std::time::Instant::now();
                f(i);
                done(&s);
                ms(t)
            })
            .collect();
        if std::env::var("OP_COST_EACH").is_ok() {
            println!("{name}: {}", each.iter().map(|v| format!("{v:.1}")).collect::<Vec<_>>().join(" "));
        }
        each.sort_by(f64::total_cmp);
        println!("{name}: {:.1} ms a step (most {:.1})", each[n / 2], each[n - 1]);
    };
    s.set_brush_size(1.0);
    for layer in 0..6 {
        if layer > 0 {
            s.add_layer();
        }
        let y = 200.0 + layer as f32 * 220.0;
        s.stroke_begin(PointerKind::Finger, vec![sample(100.0, y, 0.0)]);
        s.stroke_add(
            (1..40)
                .map(|i| sample(100.0 + i as f32 * 35.0, y + (i as f32 * 0.4).sin() * 80.0, i as f64 * 8.0))
                .collect(),
        );
        s.stroke_end(vec![]);
    }
    done(&s);
    // a colour poured around the top stroke and its threshold dragged, then let go (the layers
    // left as they were for what follows)
    if s.pour_begin(1400.0, 1500.0, 0.2, 0.4, 0.8, 0.3) {
        done(&s);
        steps("colour fill threshold dragged", 20, &|i| s.pour_threshold(0.3 + i as f32 * 0.02));
        s.pour_end(false, None);
        done(&s);
    }
    // Recolor's reticle dragged across the strokes (each move onto another colour measured
    // again), then its Flood slid
    s.recolor_start(700.0, 1150.0, 0.8, 0.3, 0.1, 0.3);
    steps("recolor dragged", 20, &|i| s.recolor_move_to(700.0 + i as f32 * 3.0, 1150.0 + i as f32 * 20.0));
    steps("recolor flood slid", 20, &|i| s.recolor_threshold(0.3 + i as f32 * 0.02));
    s.recolor_end();
    s.undo();
    done(&s);
    timed("Gaussian blur opened", &|| _ = s.adjustment_start(Adjustment::GaussianBlur));
    timed("its first step", &|| s.adjustment_amount_by(0.02));
    steps("Gaussian blur slider", 20, &|_| s.adjustment_amount_by(0.02));
    timed("Gaussian blur kept", &|| _ = s.adjustment_end(true));
    // other blurs' first steps: a filter's first use, or only the first adjustment's
    for other in [Adjustment::MotionBlur, Adjustment::GaussianBlur, Adjustment::Noise] {
        timed(&format!("{other:?} opened"), &|| _ = s.adjustment_start(other));
        timed(&format!("{other:?} first step"), &|| s.adjustment_amount_by(0.1));
        timed(&format!("{other:?} second step"), &|| s.adjustment_amount_by(0.1));
        _ = s.adjustment_end(false);
    }
    s.adjustment_start(Adjustment::Hsb);
    steps("HSB slider", 20, &|i| s.adjustment_hsb(0.5 + i as f32 * 0.01, 0.5, 0.5));
    timed("HSB kept", &|| _ = s.adjustment_end(true));
    timed("undo", &|| _ = s.undo());
    timed("redo", &|| _ = s.redo());
    timed("transform begun", &|| _ = s.transform_begin());
    s.transform_grab(TransformHandle::Move, 800.0, 800.0);
    steps("transform drag", 20, &|i| _ = s.transform_drag(800.0 + i as f32 * 5.0, 800.0 + i as f32 * 3.0));
    s.transform_release();
    timed("transform kept", &|| s.transform_end(true));
    // one-off steps the apps call as a button is tapped
    let current = || s.layers().iter().find(|l| l.current).map_or(0, |l| l.id);
    // a stroke across most of the canvas let go: its undo copy kept
    steps("a long stroke ended", 6, &|i| {
        let y = 300.0 + i as f32 * 150.0;
        s.stroke_begin(PointerKind::Finger, vec![sample(60.0, y, 0.0)]);
        s.stroke_add(
            (1..40)
                .map(|k| sample(60.0 + k as f32 * 37.0, y + (k as f32 * 0.3).sin() * 600.0, k as f64 * 8.0))
                .collect(),
        );
        done(&s);
        let t = std::time::Instant::now();
        s.stroke_end(vec![]);
        done(&s);
        if std::env::var("OP_COST_EACH").is_ok() {
            println!("  its end alone: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
        }
    });
    // the layer list changed: the pictures of the layers around the one worked on made again
    let added = std::cell::Cell::new(0);
    timed("add layer", &|| added.set(s.add_layer()));
    timed("delete it", &|| _ = s.delete_layer(added.get()));
    let bottom = s.layers().last().map_or(0, |l| l.id);
    timed("bottom layer hidden", &|| _ = s.set_layer_visible(bottom, false));
    timed("shown again", &|| _ = s.set_layer_visible(bottom, true));
    timed("flip canvas", &|| s.flip_canvas(true));
    timed("duplicate layer", &|| _ = s.duplicate_layer(current()));
    timed("merge down", &|| _ = s.merge_down(current()));
    timed("fill layer", &|| _ = s.fill_layer(current()));
    // all of the layer put back and again
    timed("its undo", &|| _ = s.undo());
    timed("its redo", &|| _ = s.redo());
    // tried again: the step undone is let go for the new one
    timed("undone and filled again", &|| {
        s.undo();
        s.fill_layer(current());
    });
    timed("invert layer", &|| _ = s.invert_layer(current()));
    // the filled layer, all of it painted, copied and pasted as a layer of its own
    let copied = std::cell::RefCell::new(None);
    timed("copy layer", &|| *copied.borrow_mut() = s.copy_layer());
    assert!(copied.borrow().is_some());
    timed("paste", &|| _ = copied.borrow_mut().take().and_then(|c| s.paste(c)));
    timed("clear layer", &|| s.clear_layer());
    // the automatic selection's threshold dragged, then let go
    steps("select by colour dragged", 20, &|i| {
        s.select_by_colour(800.0, 800.0, 0.1 + i as f32 * 0.02, crate::SelectionJoin::Add, true)
    });
    s.select_by_colour(800.0, 800.0, 0.5, crate::SelectionJoin::Add, false);
    s.undo();
    timed("select by colour", &|| s.select_by_colour(800.0, 800.0, 0.3, crate::SelectionJoin::Add, false));
    timed("select invert", &|| s.select_invert());
    timed("canvas thumbnail", &|| _ = s.canvas_thumbnail(300));
    timed("six layer thumbnails", &|| {
        for l in s.layers() {
            _ = s.layer_thumbnail(l.id, 120);
        }
    });
    // the brush library opened on a set: each brush's preview drawn (as Android asks for them)
    let library = s.brush_library();
    if let Some((set, brushes)) = library.iter().enumerate().max_by_key(|(_, b)| b.brushes.len()) {
        let mut each: Vec<f64> = (0..brushes.brushes.len() as u32)
            .map(|i| {
                let t = std::time::Instant::now();
                _ = s.brush_preview(set as u32, i, 530, 162);
                ms(t)
            })
            .collect();
        let total: f64 = each.iter().sum();
        each.sort_by(f64::total_cmp);
        println!(
            "brush previews: {} in {total:.0} ms ({:.1} each, most {:.1})",
            each.len(),
            each[each.len() / 2],
            each[each.len() - 1]
        );
    }
    // text typed into a layer of its own, a letter at a time: each drawing into the layer as the
    // apps make it (only where the text reaches), then the whole canvas drawn as it was before
    crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
    let typed = |i: usize| crate::TextInfo {
        size: 60.0,
        x: 300.0,
        y: 300.0,
        ..text_info(&"Hello there, typed".chars().take(i + 1).collect::<String>())
    };
    if s.text_begin(None, typed(0)).is_some() {
        timed("text's first drawing", &|| _ = s.draw_text_into_layer());
        steps("a letter typed", 17, &|i| {
            s.set_text(typed(i + 1));
            s.draw_text_into_layer();
        });
        let c = s.info();
        steps("a letter typed, the whole canvas drawn", 5, &|_| {
            s.set_text(typed(17));
            s.put_text_pixels(crate::draw_text(typed(17), c.width, c.height));
        });
        s.text_end(true);
    }
    if let Ok(dir) = std::env::var("OP_COST_DIR") {
        timed("first save", &|| s.save(dir.clone()).unwrap());
        s.stroke_begin(PointerKind::Finger, vec![sample(300.0, 300.0, 0.0)]);
        s.stroke_end(vec![]);
        timed("save after a stroke", &|| s.save(dir.clone()).unwrap());
        // what drawing waits for while a save is made beside it: the longest the lock was held
        s.stroke_begin(PointerKind::Finger, vec![sample(900.0, 300.0, 0.0)]);
        s.stroke_end(vec![]);
        let longest = std::thread::scope(|scope| {
            let saving = scope.spawn(|| s.save(dir.clone()).unwrap());
            let mut longest = 0.0f64;
            while !saving.is_finished() {
                let t = std::time::Instant::now();
                drop(s.shared.lock());
                longest = longest.max(ms(t));
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            longest
        });
        println!("a save beside drawing: the lock waited at most {longest:.1} ms");
        // opened again: read, then its canvas made (as the window does on its thread)
        let t = std::time::Instant::now();
        let opened = CanvasSession::open(dir.clone()).unwrap();
        let read = ms(t);
        // the device the app already has (its pipelines made)
        let Some(gpu) = s.shared.lock().gpu.clone() else { return };
        let t = std::time::Instant::now();
        opened.use_gpu(gpu);
        println!("opened: read {read:.1} ms, canvas made {:.1} ms", ms(t));
    }
    crate::layers::set_resident_budget(0);
}

#[test]
fn undo_with_an_adjustment_open_takes_back_its_changes_then_steps_and_it_stays_open() {
    use crate::Adjustment;
    let Some(s) = session(64, 64) else { return };
    let open = |s: &CanvasSession| s.adjustment_shown().adjustment;
    s.set_color(1.0, 0.0, 0.0);
    s.set_brush_size(1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(5.0, 32.0, 0.0)]);
    s.stroke_add((1..12).map(|i| sample(5.0 + i as f32 * 5.0, 32.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![]);
    let red = pixel(&s, 32, 32);
    assert!(red[0] > 200 && red[1] < 40, "{red:?}");
    assert!(s.adjustment_start(Adjustment::Hsb));
    s.adjustment_hsb(0.8, 0.5, 0.5);
    assert_ne!(pixel(&s, 32, 32), red);
    assert!(s.info().can_undo);
    // the change taken back, HSB still open
    assert_eq!(s.undo().as_deref(), Some("Hue, Saturation, Brightness"));
    assert_eq!(open(&s), Some(Adjustment::Hsb));
    assert_eq!(pixel(&s, 32, 32), red);
    // nothing changed now: the stroke goes, and HSB opens again on what is left
    assert!(s.undo().is_some());
    assert_eq!(open(&s), Some(Adjustment::Hsb));
    assert_eq!(pixel(&s, 32, 32)[3], 255);
    assert_ne!(pixel(&s, 32, 32), red);
    assert!(s.redo().is_some());
    assert_eq!(open(&s), Some(Adjustment::Hsb));
    assert_eq!(pixel(&s, 32, 32), red);
    // redo leaves a change made since alone; kept, it is one step
    s.adjustment_hsb(0.8, 0.5, 0.5);
    let changed = pixel(&s, 32, 32);
    assert_eq!(s.redo(), None);
    assert_eq!(pixel(&s, 32, 32), changed);
    assert!(s.adjustment_end(true));
    assert_eq!(pixel(&s, 32, 32), changed);
    assert!(s.undo().is_some());
    assert_eq!(pixel(&s, 32, 32), red);
    // a gradient map shows as it opens: undo steps back rather than standing still
    assert!(s.adjustment_start(Adjustment::GradientMap));
    assert!(s.undo().is_some());
    assert_eq!(open(&s), Some(Adjustment::GradientMap));
    assert!(s.adjustment_end(false));
    assert_ne!(pixel(&s, 32, 32), red);
}

/// Much of the app's work in one session, each step with a frame drawn and the GPU taking every
/// command: strokes on a few layers, every adjustment opened, changed and kept, liquify and clone,
/// each kind of transform dragged and kept, layers merged and grouped, all undone and redone.
#[test]
fn much_of_the_work_in_one_session_gives_the_gpu_no_errors() {
    use crate::Adjustment;
    use crate::transform::{TransformHandle, TransformKind};
    let Some(s) = session(300, 300) else { return };
    let stroke = |y: f32| {
        s.stroke_begin(PointerKind::Finger, vec![sample(20.0, y, 0.0)]);
        s.stroke_add(
            (1..20).map(|i| sample(20.0 + i as f32 * 13.0, y + (i as f32).sin() * 20.0, i as f64 * 8.0)).collect(),
        );
        s.stroke_end(vec![]);
        pixel(&s, 150, 150);
    };
    s.set_brush_size(0.4);
    s.set_color(0.9, 0.3, 0.1);
    stroke(80.0);
    let second = s.add_layer();
    s.set_color(0.1, 0.4, 0.9);
    stroke(150.0);
    s.add_layer();
    stroke(220.0);
    for group in crate::adjust_menu::adjustment_groups() {
        for adjustment in group {
            assert!(s.adjustment_start(adjustment), "{adjustment:?}");
            match adjustment {
                Adjustment::Hsb => s.adjustment_hsb(0.7, 0.6, 0.4),
                Adjustment::Liquify => {
                    let points = (0..60).flat_map(|i| [40.0 + i as f32 * 3.0, 150.0, 1.0, 8.0]).collect();
                    s.adjustment_liquify_stroke(points, true, false);
                }
                Adjustment::Clone => {
                    s.stroke_begin(PointerKind::Finger, vec![sample(60.0, 60.0, 0.0)]);
                    s.stroke_add((1..10).map(|i| sample(60.0 + i as f32 * 8.0, 60.0, i as f64 * 8.0)).collect());
                    s.stroke_end(vec![]);
                }
                _ => s.adjustment_amount_by(0.4),
            }
            pixel(&s, 150, 150);
            assert!(s.adjustment_end(true), "{adjustment:?}");
            pixel(&s, 150, 150);
        }
    }
    for kind in [TransformKind::Freeform, TransformKind::Uniform, TransformKind::Distort, TransformKind::Warp] {
        assert!(s.transform_begin().is_some(), "{kind:?}");
        s.transform_set_kind(kind);
        s.transform_grab(TransformHandle::Corner { index: 2 }, 250.0, 250.0);
        for i in 1..8 {
            s.transform_drag(250.0 - i as f32 * 4.0, 250.0 - i as f32 * 3.0);
            pixel(&s, 150, 150);
        }
        s.transform_release();
        s.transform_end(true);
        pixel(&s, 150, 150);
    }
    assert!(s.pick_layer(second));
    assert!(s.merge_picked());
    pixel(&s, 150, 150);
    let mut undone = 0;
    while s.info().can_undo && undone < 200 {
        s.undo();
        undone += 1;
        pixel(&s, 150, 150);
    }
    for _ in 0..undone {
        s.redo();
        pixel(&s, 150, 150);
    }
    assert!(undone > 10, "{undone}");
}

#[test]
fn a_long_liquify_drag_between_frames_renders_without_errors() {
    use crate::Adjustment;
    let Some(s) = session(256, 256) else { return };
    s.set_brush_size(0.3);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 128.0, 0.0)]);
    s.stroke_add((1..20).map(|i| sample(20.0 + i as f32 * 11.0, 128.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![]);
    assert!(s.adjustment_start(Adjustment::Liquify));
    // hundreds of stamps queued before a frame draws (a slow frame, a fast drag)
    s.set_brush_size(0.02);
    let points: Vec<f32> = (0..20)
        .flat_map(|leg| {
            (0..=46).map(move |i| {
                let x = if leg % 2 == 0 { 10.0 + i as f32 * 5.0 } else { 240.0 - i as f32 * 5.0 };
                [x, 20.0 + leg as f32 * 10.0, 1.0, 8.0]
            })
        })
        .flatten()
        .collect();
    s.adjustment_liquify_stroke(points, true, false);
    // `pixel` renders and asserts the GPU took every command
    pixel(&s, 128, 128);
    assert!(s.adjustment_end(true));
    pixel(&s, 128, 128);
}

/// A save with liquify open (the app going into the background) leaves it working, and the next
/// save keeps what it ended with, kept or cancelled.
#[test]
fn a_save_leaves_liquify_working_and_the_next_keeps_how_it_ended() {
    use crate::Adjustment;
    let Some(s) = session(128, 128) else { return };
    let Some(gpu) = s.shared.lock().gpu.clone() else { return };
    s.set_color(1.0, 0.0, 0.0);
    s.set_brush_size(0.3);
    s.stroke_begin(PointerKind::Finger, vec![sample(4.0, 64.0, 0.0)]);
    s.stroke_add((1..13).map(|i| sample(4.0 + i as f32 * 10.0, 64.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![]);
    let red = pixel(&s, 96, 64);
    assert!(red[0] > 200 && red[1] < 40, "{red:?}");
    let dir = std::env::temp_dir().join(format!("procraft-save-liquify-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.to_string_lossy().into_owned();
    let contents = |s: &CanvasSession| -> Vec<Vec<u8>> {
        let _ = pixel(s, 0, 0);
        s.layers().iter().map(|l| s.copy_layer_contents(l.id).map(|c| c.rgba).unwrap_or_default()).collect()
    };
    let saved = || {
        let opened = CanvasSession::open(path.clone()).unwrap();
        opened.use_gpu(gpu.clone());
        {
            let mut st = opened.shared.lock();
            st.window_size = (128, 128);
            st.view = Some(View::fit((128, 128), (128, 128)));
        }
        contents(&opened)
    };
    // a small brush pushed down across the line
    let push = |x: f32| {
        s.adjustment_liquify_stroke((0..12).flat_map(|i| [x, 24.0 + i as f32 * 8.0, 1.0, 8.0]).collect(), true, false)
    };
    assert!(s.adjustment_start(Adjustment::Liquify));
    s.adjustment_liquify_value(0, 0.1);
    push(32.0);
    s.save(path.clone()).unwrap();
    assert_eq!(s.adjustment_shown().adjustment, Some(Adjustment::Liquify));
    push(96.0);
    assert_ne!(pixel(&s, 96, 64), red, "liquify still works after the save");
    assert!(s.adjustment_end(true));
    s.save(path.clone()).unwrap();
    assert!(contents(&s) == saved(), "kept: saved as it ended");
    // cancelled after a save: the next keeps the layer as it was before
    assert!(s.adjustment_start(Adjustment::Liquify));
    s.adjustment_liquify_value(0, 0.1);
    push(64.0);
    s.save(path.clone()).unwrap();
    assert!(s.adjustment_end(false));
    s.save(path.clone()).unwrap();
    assert!(contents(&s) == saved(), "cancelled: saved as it was");
    let _ = std::fs::remove_dir_all(&dir);
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
    assert_eq!(crate::layers::blend_modes().len(), 28);
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
fn a_preview_mixes_wet_only_where_the_brush_says_it_does() {
    let Some(s) = session(100, 100) else { return };
    let (set, index) = {
        let st = s.shared.lock();
        let found = st.library.iter().enumerate().find_map(|(set, l)| {
            l.brushes.iter().position(procraft_engine::brush::Brush::mixes_wet).map(|index| (set, index))
        });
        found.unwrap()
    };
    let change = |key: &str, value: f32| {
        let mut st = s.shared.lock();
        st.library.get_mut(set).and_then(|l| l.brushes.get_mut(index)).unwrap().set_setting(key, value);
    };
    let preview = || s.brush_preview(set as u32, index as u32, 240, 60).unwrap().rgba;
    // ours that mix wet say so, and show it
    let wet = preview();
    change("preview_wet_mix", 0.0);
    let dry = preview();
    assert_ne!(wet, dry);
    // switched off, it is drawn as the brush would be with no wet mix at all
    change("wet_dilution", 0.0);
    assert_eq!(dry, preview());
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
/// `PROCRAFT_DUMP`, for looking at the brushes: `cargo test -p procraft-core dump -- --ignored`.
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
fn a_finger_strokes_head_is_drawn_in_its_layer_under_those_above() {
    let Some(s) = session(100, 100) else { return };
    let below = s.layers()[0].id;
    // an opaque blue layer over the half y < 50
    let above = s.add_layer();
    s.set_color(0.0, 0.0, 1.0);
    s.select_shape(false, 0.0, 0.0, 100.0, 50.0, crate::SelectionJoin::Add);
    s.fill_layer(above);
    s.select_none();
    s.select_layer(below);
    s.set_color(1.0, 0.0, 0.0);
    s.set_brush_size(0.4);
    no_streamline(&s);
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 70.0, 0.0)]);
    // the head runs ahead, up under the blue layer and out past it
    s.stroke_predict(vec![sample(60.0, 20.0, 16.0), sample(90.0, 90.0, 32.0)]);
    let red = |p: [u8; 4]| p[0] > 200 && p[2] < 50;
    let ahead = (60..100).find(|x| red(pixel(&s, *x, 64))).expect("the head shows ahead");
    assert_eq!(pixel(&s, 60, 20), [0, 0, 255, 255], "under the layer above");
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, ahead, 64), [255, 255, 255, 255], "never kept");
}

#[test]
fn an_open_artwork_is_kept_saved_until_that_stops() {
    let Some(s) = session(64, 64) else { return };
    let dir = std::env::temp_dir().join(format!("procraft-autosave-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::sync::Arc::clone(&s).start_autosave(dir.to_string_lossy().into_owned());
    assert_eq!(s.edits_saved(), None);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    let _ = pixel(&s, 0, 0);
    let edits = s.edits();
    let saved = (0..300).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(10));
        s.edits_saved() == Some(edits)
    });
    assert!(saved && dir.join("artwork.json").exists(), "not saved");
    // stopped: what changes next isn't saved
    s.stop_autosave();
    s.stroke_begin(PointerKind::Finger, vec![sample(40.0, 40.0, 0.0)]);
    s.stroke_end(vec![]);
    let _ = pixel(&s, 0, 0);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_ne!(s.edits(), edits);
    assert_eq!(s.edits_saved(), Some(edits));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Fast painting leaves only short gaps between strokes: an autosave that finds one under way
/// saves in the next gap, not at its next turn.
#[test]
fn an_autosave_that_finds_a_stroke_saves_as_it_ends() {
    let Some(s) = session(64, 64) else { return };
    let dir = std::env::temp_dir().join(format!("procraft-gap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let every = std::time::Duration::from_millis(1000);
    let started = std::time::Instant::now();
    std::sync::Arc::clone(&s).keep_saved(
        dir.to_string_lossy().into_owned(),
        every,
        std::time::Duration::from_millis(5),
    );
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 20.0, 0.0)]);
    s.stroke_end(vec![]);
    let _ = pixel(&s, 0, 0);
    // a stroke under way when its turn comes
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    std::thread::sleep((every + std::time::Duration::from_millis(100)).saturating_sub(started.elapsed()));
    assert_eq!(s.edits_saved(), None, "nothing saved with a stroke under way");
    s.stroke_end(vec![]);
    let _ = pixel(&s, 0, 0);
    let edits = s.edits();
    let saved = (0..400).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(1));
        s.edits_saved() == Some(edits)
    });
    s.stop_autosave();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(saved, "saved in the gap, before its next turn");
}

/// A device lost holds nothing to save: the autosave stops and the last save stands, and what
/// tells of it is the device being lost alone, not a failed save as well on top of it.
#[test]
fn an_autosave_ends_with_the_graphics_device() {
    let Some(s) = session(64, 64) else { return };
    let dir = std::env::temp_dir().join(format!("procraft-lost-autosave-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 20.0, 0.0)]);
    s.stroke_end(vec![]);
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    let layer = |d: &std::path::Path| {
        let mut files: Vec<_> = std::fs::read_dir(d.join("layers")).unwrap().flatten().map(|f| f.path()).collect();
        files.sort();
        files.iter().map(|f| std::fs::read(f).unwrap()).collect::<Vec<_>>()
    };
    let before = layer(&dir);
    {
        let st = s.shared.lock();
        let gpu = st.gpu.as_ref().unwrap();
        gpu.device.destroy();
        let _ = gpu.device.poll(procraft_gpu::wgpu::PollType::wait_indefinitely());
        assert!(gpu.is_lost());
    }
    std::sync::Arc::clone(&s).start_autosave(dir.to_string_lossy().into_owned());
    // something to save
    let edits = s.edits();
    s.add_layer();
    assert_ne!(s.edits(), edits);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(s.take_save_failure(), None);
    assert!(s.take_failure().is_some_and(|said| said.contains("last saved")));
    assert!(layer(&dir) == before, "what was saved stays as it was");
    s.stop_autosave();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failing_autosave_is_told_once_until_a_save_works() {
    let Some(s) = session(64, 64) else { return };
    // a file where its folder would be: every save there fails
    let blocked = std::env::temp_dir().join(format!("procraft-blocked-{}", std::process::id()));
    let good = std::env::temp_dir().join(format!("procraft-unblocked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&good);
    std::fs::write(&blocked, b"not a folder").unwrap();
    let paint = |x: f32| {
        s.stroke_begin(PointerKind::Finger, vec![sample(x, 30.0, 0.0)]);
        s.stroke_end(vec![]);
        let _ = pixel(&s, 0, 0);
    };
    let told = || {
        (0..300).find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(10));
            s.take_save_failure()
        })
    };
    std::sync::Arc::clone(&s).start_autosave(blocked.to_string_lossy().into_owned());
    paint(30.0);
    let why = told();
    assert!(why.is_some(), "a failed autosave told");
    assert_eq!(crate::save_failure_said(why.unwrap_or_default()), "This artwork couldn't be saved.");
    // failing still: not told again
    paint(40.0);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(s.take_save_failure(), None);
    // a save that works, then the autosave failing again: told again
    s.stop_autosave();
    s.save(good.to_string_lossy().into_owned()).unwrap();
    std::sync::Arc::clone(&s).start_autosave(blocked.to_string_lossy().into_owned());
    paint(20.0);
    let again = told();
    s.stop_autosave();
    let _ = std::fs::remove_file(&blocked);
    let _ = std::fs::remove_dir_all(&good);
    assert!(again.is_some(), "told again after a save worked");
    assert!(crate::save_failure_said("No space left on device (os error 28)".into()).contains("no room"));
}

/// An export says how far it has come while it runs (read without waiting on it): all the way
/// once its file is written.
#[test]
fn an_export_tells_how_far_it_has_come() {
    let Some(s) = session(64, 64) else { return };
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 10.0, 0.0)]);
    s.stroke_end(vec![]);
    let dir = std::env::temp_dir().join(format!("procraft-progress-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = |n: &str| dir.join(n).to_string_lossy().into_owned();
    s.export_psd(path("a.psd")).unwrap();
    assert_eq!(s.export_progress(), 1.0);
    s.export_jpeg(path("a.jpg")).unwrap();
    assert_eq!(s.export_progress(), 1.0);
    assert!(!s.export_layer_pngs(path("layers")).unwrap().is_empty());
    assert_eq!(s.export_progress(), 1.0);
    // the animated ones too, from wherever the last left it
    s.reset_export_progress();
    s.export_gif(path("a.gif"), 0, 15.0, true, true, false).unwrap();
    assert_eq!(s.export_progress(), 1.0);
    s.reset_export_progress();
    s.export_gif(path("b.gif"), 0, 15.0, true, false, false).unwrap();
    assert_eq!(s.export_progress(), 1.0);
    s.reset_export_progress();
    s.export_apng(path("a.png"), 0, 15.0, false).unwrap();
    assert_eq!(s.export_progress(), 1.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nothing_is_saved_from_a_lost_graphics_device() {
    let Some(s) = session(64, 64) else { return };
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    let dir = std::env::temp_dir().join(format!("procraft-lost-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    let kept = std::fs::read(dir.join("thumbnail.png")).unwrap();
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    let gpu = s.shared.lock().gpu.clone().unwrap();
    gpu.device.destroy();
    let _ = gpu.device.poll(procraft_gpu::wgpu::PollType::wait_indefinitely());
    assert!(gpu.is_lost());
    assert!(s.save(dir.to_string_lossy().into_owned()).is_err());
    assert_eq!(std::fs::read(dir.join("thumbnail.png")).unwrap(), kept);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_stroke_on_a_canvas_whose_graphics_stopped_tells_so_again() {
    let Some(s) = session(64, 64) else { return };
    let gpu = s.shared.lock().gpu.clone().unwrap();
    gpu.device.destroy();
    let _ = gpu.device.poll(procraft_gpu::wgpu::PollType::wait_indefinitely());
    assert!(gpu.is_lost());
    let told = s.take_failure();
    assert!(told.is_some());
    assert_eq!(s.take_failure(), None, "told once");
    // the notice gone (the app away meanwhile), a stroke that can't show says why again
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(s.take_failure(), told);
    assert_eq!(s.take_failure(), None);
}

#[test]
fn an_artwork_opened_again_has_every_layer_as_it_was() {
    let Some(s) = session(160, 160) else { return };
    // a few strokes on one, nothing on another, all of a third filled
    s.set_brush_size(0.2);
    s.set_color(0.9, 0.2, 0.1);
    s.stroke_begin(PointerKind::Finger, vec![sample(40.0, 50.0, 0.0)]);
    s.stroke_add((1..8).map(|i| sample(40.0 + i as f32 * 6.0, 50.0 + i as f32 * 3.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![]);
    let empty = s.add_layer();
    let full = s.add_layer();
    s.set_color(0.1, 0.5, 0.2);
    assert!(s.fill_layer(full));
    let contents = |s: &CanvasSession| -> Vec<(u64, Vec<u8>)> {
        let _ = pixel(s, 0, 0);
        s.layers().iter().map(|l| (l.id, s.copy_layer_contents(l.id).map(|c| c.rgba).unwrap_or_default())).collect()
    };
    let before = contents(&s);
    assert!(before.iter().any(|(id, px)| *id == empty && px.iter().all(|&v| v == 0)));
    let dir = std::env::temp_dir().join(format!("procraft-reopen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    let opened = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
    let Some(gpu) = s.shared.lock().gpu.clone() else { return };
    opened.use_gpu(gpu);
    {
        let mut st = opened.shared.lock();
        st.window_size = (160, 160);
        st.view = Some(View::fit((160, 160), (160, 160)));
    }
    let after = contents(&opened);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(before.len(), after.len());
    for ((id, a), (_, b)) in before.iter().zip(&after) {
        assert!(a == b, "layer {id} differs when opened again");
    }
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
    // a layer with Drawing Assist on counts as assisted
    assert_eq!(stats.assisted, 0);
    s.set_guide_assisted(true);
    assert_eq!(s.layer_stats().assisted, 1);
    let dir = std::env::temp_dir().join(format!("procraft-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    assert!(dir.join("artwork.json").exists() && dir.join("thumbnail.png").exists());

    let opened = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
    // saved before it is shown, its layers (still their files) are written back as they were
    let again = dir.with_extension("again");
    opened.save(again.to_string_lossy().into_owned()).unwrap();
    let mut compared = 0;
    for layer in opened.layers() {
        let file = |d: &std::path::Path| std::fs::read(d.join("layers").join(format!("{}.png", layer.id))).ok();
        let (before, after) = (file(&dir), file(&again));
        compared += usize::from(before.is_some());
        let rgba = |png: Option<Vec<u8>>| png.map(|b| crate::project::Opened::Png(b).pixels(100, 100).unwrap());
        assert_eq!(rgba(after), rgba(before));
    }
    assert_eq!(compared, 2);
    let _ = std::fs::remove_dir_all(&again);
    // into its own folder none is written (nor decoded): its files are there as they are
    let written = |d: &std::path::Path| -> Vec<std::time::SystemTime> {
        opened
            .layers()
            .iter()
            .filter_map(|l| {
                std::fs::metadata(d.join("layers").join(format!("{}.png", l.id))).and_then(|m| m.modified()).ok()
            })
            .collect()
    };
    let before = written(&dir);
    opened.save(dir.to_string_lossy().into_owned()).unwrap();
    assert_eq!((written(&dir), before.len()), (before, 2));
    {
        let gpu = Gpu::headless().unwrap();
        let mut st = opened.shared.lock();
        st.window_size = (100, 100);
        st.view = Some(View::fit((100, 100), (100, 100)));
        st.gpu = Some(gpu);
    }
    // as the render thread makes it: the layers go up with the state let go between them
    opened.shared.make_canvas();
    assert!(opened.shared.lock().pending_pixels.is_empty());
    // shown, and nothing changed: its files are known as read, so a save rewrites none
    let before = written(&dir);
    opened.save(dir.to_string_lossy().into_owned()).unwrap();
    assert_eq!(written(&dir), before, "unchanged layers left as they were");
    // a stroke on the bottom layer: only its file is written again
    let bottom = opened.layers().first().map(|l| l.id).unwrap();
    assert!(opened.select_layer(bottom));
    opened.stroke_begin(PointerKind::Finger, vec![sample(50.0, 50.0, 0.0)]);
    opened.stroke_end(vec![]);
    opened.save(dir.to_string_lossy().into_owned()).unwrap();
    let after = written(&dir);
    assert_ne!(after.first(), before.first(), "the painted layer written");
    assert_eq!(after.get(1), before.get(1), "the other left as it was");
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
    // the background hidden: a clear spot holds nothing (black), though the surround shows there
    s.set_background_visible(false);
    let clear = s.pick(100.0, 40.0, 21).unwrap();
    assert_eq!(clear.rgb, Some(0x000000));
    assert_ne!(&clear.pixels[(10 * 21 + 10) * 4..(10 * 21 + 10) * 4 + 3], &[0, 0, 0]);
    assert_eq!(s.pick(100.0, 100.0, 21).unwrap().rgb, Some(0x000000));
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
fn a_layer_s_mask_moves_with_it_in_a_transform() {
    use crate::transform::TransformHandle;
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    let layer = s.layers().into_iter().find(|l| l.current).unwrap().id;
    // its mask hides the dot
    s.add_mask(layer).unwrap();
    s.set_color(0.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 30, 30), [255, 255, 255, 255]);
    // the layer moved: the dot still hidden where it went
    assert!(s.select_layer(layer));
    assert!(s.transform_begin().is_some());
    s.transform_grab(TransformHandle::Move, 30.0, 30.0);
    s.transform_drag(70.0, 70.0);
    s.transform_release();
    s.transform_end(true);
    assert_eq!(pixel(&s, 70, 70), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 30, 30), [255, 255, 255, 255]);
    // undone: both back, the dot hidden where it was
    s.undo();
    assert_eq!(pixel(&s, 30, 30), [255, 255, 255, 255]);
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
fn where_layers_are_painted_is_found_on_the_gpu_as_on_the_cpu() {
    // wider than tall, past a stretch of the GPU's lines each way
    let Some(s) = session(1100, 1100) else { return };
    s.set_brush_size(0.2);
    let mut ids = vec![s.layers()[0].id];
    for (k, (x, y)) in [(600.0, 700.0), (1.0, 520.0), (1099.0, 1099.0)].into_iter().enumerate() {
        if k > 0 {
            ids.push(s.add_layer());
        }
        s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
        s.stroke_end(vec![]);
    }
    ids.push(s.add_layer());
    let _ = pixel(&s, 0, 0);
    let mut guard = s.shared.lock();
    let st = &mut *guard;
    let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
    let ids: Vec<_> = ids.into_iter().map(procraft_engine::document::LayerId).collect();
    let found = canvas.painted_boxes(gpu, &ids);
    for (id, gpu_box) in ids.iter().zip(&found) {
        let cpu_box = crate::layer_ops::painted_rect(&canvas.read_layer(gpu, *id).unwrap(), 1100);
        assert_eq!(*gpu_box, cpu_box, "{id:?}");
    }
    // the last is clear, the others aren't
    assert_eq!(found.iter().map(Option::is_some).collect::<Vec<_>>(), [true, true, true, false]);
}

#[test]
fn a_selection_copied_to_a_layer_is_what_it_takes_of_the_layer() {
    use crate::selection::SelectionJoin;
    let Some(s) = session(300, 300) else { return };
    s.set_brush_size(0.3);
    s.stroke_begin(PointerKind::Finger, vec![sample(60.0, 150.0, 0.0)]);
    s.stroke_add(vec![sample(240.0, 150.0, 60.0)]);
    s.stroke_end(vec![]);
    let source = s.layers()[0].id;
    let _ = pixel(&s, 0, 0);
    let before = s.copy_layer_contents(source).unwrap().rgba;
    // an ellipse over part of the stroke, feathered
    s.select_shape(true, 100.0, 100.0, 200.0, 190.0, SelectionJoin::Add);
    s.select_feather(0.05);
    s.select_feather_done();
    let mask = s.shared.lock().selection.clone().unwrap();
    let id = s.copy_selection_to_layer().unwrap();
    let _ = pixel(&s, 0, 0);
    let copied = s.copy_layer_contents(id).unwrap().rgba;
    let expected: Vec<u8> = before
        .as_chunks::<4>()
        .0
        .iter()
        .zip(&mask.data)
        .flat_map(|(px, &m)| px.map(|c| ((u16::from(c) * u16::from(m) + 127) / 255) as u8))
        .collect();
    assert!(copied == expected);
    assert!(copied.iter().any(|&v| v > 0));
    // the source layer's contents selected: its coverage everywhere
    assert!(s.select_layer_contents(source));
    let taken = s.shared.lock().selection.clone().unwrap();
    assert!(taken.data.iter().zip(before.as_chunks::<4>().0).all(|(&m, px)| m == px[3]));
    assert!(taken.data.iter().any(|&m| m > 0));
}

#[test]
fn a_layer_inverts_on_the_gpu_as_on_the_cpu_and_undoes() {
    let Some(s) = session(200, 200) else { return };
    let id = s.layers()[0].id;
    // a soft stroke: edges partly covered
    let soft =
        s.brush_library().iter().enumerate().find_map(|(i, set)| {
            set.brushes.iter().position(|b| b.name == "Soft Airbrush").map(|j| (i as u32, j as u32))
        });
    if let Some((set, index)) = soft {
        s.select_brush(set, index);
    }
    s.set_brush_size(0.3);
    s.set_color(0.2, 0.6, 0.9);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 100.0, 0.0)]);
    s.stroke_add(
        (1..15).map(|i| sample(30.0 + i as f32 * 10.0, 100.0 + (i as f32).sin() * 30.0, i as f64 * 8.0)).collect(),
    );
    s.stroke_end(vec![]);
    let layer = |s: &CanvasSession| {
        let _ = pixel(s, 0, 0);
        s.copy_layer_contents(id).unwrap().rgba
    };
    let before = layer(&s);
    assert!(before.as_chunks::<4>().0.iter().any(|p| p[3] > 0 && p[3] < 255), "soft edges");
    assert!(s.invert_layer(id));
    let after = layer(&s);
    let want: Vec<u8> = before
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| [a.saturating_sub(r), a.saturating_sub(g), a.saturating_sub(b), a])
        .collect();
    assert!(after == want, "inverted as on the CPU");
    s.undo();
    assert!(layer(&s) == before);
    s.redo();
    assert!(layer(&s) == want);
}

#[test]
fn the_canvas_flips_on_the_gpu_as_on_the_cpu_and_undoes() {
    let Some(s) = session(300, 300) else { return };
    s.set_brush_size(0.2);
    let mut ids = vec![s.layers()[0].id];
    for (k, (x, y)) in [(40.0, 60.0), (250.0, 120.0)].into_iter().enumerate() {
        if k > 0 {
            ids.push(s.add_layer());
        }
        s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
        s.stroke_add(vec![sample(x + 30.0, y + 140.0, 40.0)]);
        s.stroke_end(vec![]);
    }
    let layers = |s: &CanvasSession| -> Vec<Vec<u8>> {
        let _ = pixel(s, 0, 0);
        ids.iter().map(|id| s.copy_layer_contents(*id).unwrap().rgba).collect()
    };
    let before = layers(&s);
    for horizontal in [true, false] {
        let start = layers(&s);
        s.flip_canvas(horizontal);
        let after = layers(&s);
        for (a, b) in start.iter().zip(&after) {
            assert!(crate::clipboard::flipped(a, 300, horizontal) == *b, "{horizontal}");
        }
    }
    let flipped = layers(&s);
    s.undo();
    s.undo();
    assert!(layers(&s) == before);
    // redone, each flips again
    s.redo();
    s.redo();
    assert!(layers(&s) == flipped);
}

#[test]
fn a_crop_by_whole_pixels_copies_the_layers_as_the_cpu_would() {
    let Some(s) = session(300, 300) else { return };
    s.set_brush_size(0.2);
    let mut ids = vec![s.layers()[0].id];
    for (k, (x, y)) in [(40.0, 60.0), (250.0, 220.0)].into_iter().enumerate() {
        if k > 0 {
            ids.push(s.add_layer());
        }
        s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
        s.stroke_add(vec![sample(x + 20.0, y + 60.0, 40.0)]);
        s.stroke_end(vec![]);
    }
    let _ = pixel(&s, 0, 0);
    let before: Vec<Vec<u8>> = ids.iter().map(|id| s.copy_layer_contents(*id).unwrap().rgba).collect();
    // reaching past the left and the bottom edges
    let rect = [-30.0f32, 50.0, 250.0, 280.0];
    assert!(s.crop_canvas(rect[0], rect[1], rect[2], rect[3], 0.0, false, 0, 0));
    let c = procraft_engine::crop::crop(rect, 0.0, None);
    assert!(c.whole_pixels().is_some());
    let _ = pixel(&s, 0, 0);
    for (id, old) in ids.iter().zip(&before) {
        let now = s.copy_layer_contents(*id).unwrap();
        assert_eq!((now.width, now.height), (c.width, c.height));
        assert!(now.rgba == procraft_engine::crop::resample(old, 300, 300, &c), "{id}");
    }
}

#[test]
fn a_crop_turned_or_resized_resamples_on_the_gpu_as_the_cpu_would() {
    for (rect, angle, size) in
        [([20.0f32, 30.0, 240.0, 200.0], 0.4f32, None), ([-10.0, 5.0, 260.0, 250.0], 0.0, Some((180u32, 150u32)))]
    {
        let Some(s) = session(300, 300) else { return };
        s.set_brush_size(0.3);
        s.set_color(0.9, 0.3, 0.1);
        s.stroke_begin(PointerKind::Finger, vec![sample(60.0, 80.0, 0.0)]);
        s.stroke_add(vec![sample(220.0, 210.0, 40.0)]);
        s.stroke_end(vec![]);
        let id = s.layers()[0].id;
        let _ = pixel(&s, 0, 0);
        let before = s.copy_layer_contents(id).unwrap().rgba;
        let (w, h) = size.unwrap_or((0, 0));
        assert!(s.crop_canvas(rect[0], rect[1], rect[2], rect[3], angle, size.is_some(), w, h));
        let c = procraft_engine::crop::crop(rect, angle, size);
        assert!(c.whole_pixels().is_none());
        let _ = pixel(&s, 0, 0);
        let now = s.copy_layer_contents(id).unwrap();
        assert_eq!((now.width, now.height), (c.width, c.height));
        let cpu = procraft_engine::crop::resample(&before, 300, 300, &c);
        let off = now.rgba.iter().zip(&cpu).filter(|(a, b)| a != b).count();
        let worst = now.rgba.iter().zip(&cpu).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
        eprintln!("crop {rect:?} {angle}: {off} of {} values differ, by {worst} at most", cpu.len());
        assert!(worst <= 1 && off * 1000 <= cpu.len(), "{off} differ, by {worst}");
    }
}

#[test]
fn a_cleared_layer_keeps_only_what_was_painted_to_undo() {
    let Some(s) = session(300, 300) else { return };
    s.set_brush_size(0.2);
    s.stroke_begin(PointerKind::Finger, vec![sample(100.0, 100.0, 0.0)]);
    s.stroke_end(vec![]);
    let id = s.layers()[0].id;
    let layer = |s: &CanvasSession| {
        let _ = pixel(s, 0, 0);
        s.copy_layer_contents(id).unwrap().rgba
    };
    let history = |s: &CanvasSession| {
        let _ = pixel(s, 0, 0);
        s.shared.lock().canvas.as_ref().map_or(0, |c| c.history_bytes())
    };
    let (before, kept) = (layer(&s), history(&s));
    s.clear_layer();
    assert!(layer(&s).iter().all(|&v| v == 0));
    let kept = history(&s) - kept;
    assert!(kept > 0 && kept < 300 * 300, "kept {kept}");
    s.undo();
    assert!(layer(&s) == before, "undone");
}

#[test]
fn a_transform_keeps_only_where_it_changed_to_undo() {
    use crate::transform::{TransformHandle, TransformKind};
    let Some(s) = session(300, 300) else { return };
    s.set_brush_size(0.2);
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, 60.0, 0.0)]);
    s.stroke_add(vec![sample(70.0, 50.0, 30.0)]);
    s.stroke_end(vec![]);
    let id = s.layers()[0].id;
    let layer = |s: &CanvasSession| {
        let _ = pixel(s, 0, 0);
        s.copy_layer_contents(id).unwrap().rgba
    };
    let history = |s: &CanvasSession| {
        let _ = pixel(s, 0, 0);
        s.shared.lock().canvas.as_ref().map_or(0, |c| c.history_bytes())
    };
    // moved; then a corner pulled out (scaled up, resampled); then warped
    for kind in 0..3 {
        let (before, kept) = (layer(&s), history(&s));
        let b = s.transform_begin().unwrap();
        match kind {
            0 => s.transform_grab(TransformHandle::Move, b.corners[0] + 2.0, b.corners[1] + 2.0),
            1 => {
                s.transform_set_kind(TransformKind::Distort);
                s.transform_grab(TransformHandle::Corner { index: 2 }, b.corners[4], b.corners[5]);
            }
            _ => {
                let mesh = s.transform_set_kind(TransformKind::Warp).unwrap().mesh;
                s.transform_grab(TransformHandle::Mesh { index: 5 }, mesh[10], mesh[11]);
            }
        }
        let start = if kind == 1 { (b.corners[4], b.corners[5]) } else { (b.corners[0] + 2.0, b.corners[1] + 2.0) };
        s.transform_drag(start.0 + 40.0, start.1 + 25.0);
        s.transform_release();
        s.transform_end(true);
        let after = layer(&s);
        assert_ne!(after, before, "{kind}");
        let kept = history(&s) - kept;
        assert!(kept > 0 && kept < 300 * 300 * 2, "{kind}: kept {kept}");
        // undone and redone to the pixel
        s.undo();
        assert!(layer(&s) == before, "{kind}: undone");
        s.redo();
        assert!(layer(&s) == after, "{kind}: redone");
    }
}

#[test]
fn an_adjustment_within_a_selection_keeps_only_that_to_undo() {
    use crate::adjust_menu::Adjustment;
    use crate::selection::SelectionJoin;
    let Some(s) = session(300, 300) else { return };
    s.set_brush_size(0.3);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 150.0, 0.0)]);
    s.stroke_add(vec![sample(280.0, 150.0, 60.0)]);
    s.stroke_end(vec![]);
    let id = s.layers()[0].id;
    let layer = |s: &CanvasSession| {
        let _ = pixel(s, 0, 0);
        s.copy_layer_contents(id).unwrap().rgba
    };
    let history = |s: &CanvasSession| {
        let _ = pixel(s, 0, 0);
        s.shared.lock().canvas.as_ref().map_or(0, |c| c.history_bytes())
    };
    s.select_shape(false, 100.0, 120.0, 160.0, 180.0, SelectionJoin::Add);
    let (before, kept) = (layer(&s), history(&s));
    s.adjustment_start(Adjustment::GaussianBlur);
    s.adjustment_amount_by(0.3);
    s.adjustment_end(true);
    let after = layer(&s);
    assert_ne!(after, before);
    let kept = history(&s) - kept;
    assert!(kept > 0 && kept <= 62 * 62 * 4, "kept {kept}");
    s.undo();
    assert!(layer(&s) == before, "undone");
    s.redo();
    assert!(layer(&s) == after, "redone");
    // without one: only around what is painted, as far as the blur reached
    s.select_none();
    let (before, kept) = (layer(&s), history(&s));
    s.adjustment_start(Adjustment::GaussianBlur);
    s.adjustment_amount_by(0.05);
    s.adjustment_end(true);
    let after = layer(&s);
    let kept = history(&s) - kept;
    assert!(kept > 0 && kept < 300 * 300 * 4, "kept {kept}");
    s.undo();
    assert!(layer(&s) == before, "undone");
    s.redo();
    assert!(layer(&s) == after, "redone");
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
fn perspective_blur_opens_positional_and_directional_blurs_only_ahead_of_its_arrow() {
    use crate::{Adjustment, PerspectiveKind};
    let Some(s) = session(128, 128) else { return };
    let composite = |s: &CanvasSession| {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        assert_eq!(gpu.errors(), 0, "GPU validation errors");
        canvas.read_composite(gpu).unwrap()
    };
    s.set_brush_size(0.2);
    s.set_color(0.0, 0.0, 0.0);
    // a dot above the middle and one below it
    for y in [28.0, 100.0] {
        s.stroke_begin(PointerKind::Finger, vec![sample(64.0, y, 0.0)]);
        s.stroke_end(vec![]);
    }
    let before = composite(&s);
    assert_ne!(pixel(&s, 64, 28), [255; 4]);
    assert!(s.adjustment_start(Adjustment::PerspectiveBlur));
    // it opens positional, its puck in the middle and its arrow up
    let shown = s.adjustment_shown();
    assert_eq!(shown.perspective_kind, PerspectiveKind::Positional);
    let &[x, y, tx, ty] = shown.perspective_puck.as_deref().unwrap() else { panic!("{shown:?}") };
    assert!((tx - x).abs() < 1e-3 && ty < y, "{shown:?}");
    // directional, all of it: below the middle nothing changes, above it the dot streaks
    s.adjustment_perspective_kind(PerspectiveKind::Directional);
    s.adjustment_amount_by(1.0);
    let half = (64 * 128 * 4) as usize;
    let one_way = composite(&s);
    assert_eq!(one_way[half..], before[half..]);
    assert_ne!(one_way[..half], before[..half]);
    // positional, the dot below streaks too
    s.adjustment_perspective_kind(PerspectiveKind::Positional);
    assert_ne!(composite(&s)[half..], before[half..]);
    // kept, its undo step has no name
    s.adjustment_end(true);
    assert_ne!(composite(&s), before);
    assert_eq!(s.undo(), None);
    assert_eq!(composite(&s), before);
    // a new one starts positional again
    assert!(s.adjustment_start(Adjustment::PerspectiveBlur));
    assert_eq!(s.adjustment_shown().perspective_kind, PerspectiveKind::Positional);
    s.adjustment_end(false);
    assert_eq!(composite(&s), before);
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
fn a_discarded_adjustment_is_no_step_and_keeps_redo() {
    let Some(s) = session(100, 200) else { return };
    // one stroke, shown at 2x: window (60..140, 100) is canvas (30..70, 50)
    s.stroke_begin(PointerKind::Stylus, vec![sample(60.0, 100.0, 0.0)]);
    s.stroke_add((1..20).map(|i| sample(60.0 + i as f32 * 4.0, 100.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![sample(140.0, 100.0, 170.0)]);
    assert_eq!(pixel(&s, 50, 50), [0, 0, 0, 255]);
    // undone, so redo holds the stroke
    s.undo();
    assert_eq!(pixel(&s, 50, 50), [255, 255, 255, 255]);
    assert!(s.info().can_redo);
    // an adjustment changed and thrown away is no step, and redo still holds the stroke
    assert!(s.adjust_begin());
    s.adjust_hsb(0.5, 0.5, 0.0);
    s.adjust_end(false);
    assert_eq!(pixel(&s, 50, 50), [255, 255, 255, 255]);
    assert!(!s.info().can_undo, "discarding the adjustment must leave no step");
    assert!(s.info().can_redo, "discarding the adjustment must not clear redo");
    s.redo();
    assert_eq!(pixel(&s, 50, 50), [0, 0, 0, 255]);
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
    let path: Vec<f32> = (0..=20).flat_map(|i| [30.0 + i as f32, 50.0, 1.0, 8.0]).collect();
    s.liquify_stroke(path, true, false, brush);
    assert!(pixel(&s, 45, 50)[0] < 128, "{:?}", pixel(&s, 45, 50));
    s.liquify_amount(0.5);
    s.liquify_reset();
    assert_eq!(pixel(&s, 45, 50), [255, 255, 255, 255]);
    s.liquify_stroke((0..=20).flat_map(|i| [30.0 + i as f32, 50.0, 1.0, 8.0]).collect(), true, false, brush);
    // lifted while moving, it carries on further right as time passes
    let row = |s: &CanvasSession| (40..90).map(|x| pixel(s, x, 50)).collect::<Vec<_>>();
    let before = row(&s);
    assert!(s.liquify_lift(1.0, brush));
    assert_eq!(row(&s), before, "it goes on over time, not at once");
    assert!(s.liquify_tick(0.1, brush));
    assert_ne!(row(&s), before);
    assert!(!s.liquify_lift(f32::NAN, brush) || s.liquify_tick(0.1, brush));
    s.adjust_end(true);
    s.undo();
    assert_eq!(pixel(&s, 30, 50), [0, 0, 0, 255]);
}

#[test]
fn liquify_pushes_a_layer_s_mask_along() {
    use crate::adjust::{LiquifyBrush, LiquifyKind};
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    let layer = s.layers().into_iter().find(|l| l.current).unwrap().id;
    // its mask hides the dot
    s.add_mask(layer).unwrap();
    s.set_color(0.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert!(s.select_layer(layer));
    assert!(s.liquify_begin());
    let brush = LiquifyBrush { kind: LiquifyKind::Push, size: 0.15, pressure: 1.0, distortion: 0.0 };
    s.liquify_stroke((0..=20).flat_map(|i| [30.0 + i as f32, 50.0, 1.0, 8.0]).collect(), true, false, brush);
    // pushed right, the dot still hidden where it went
    assert_eq!(pixel(&s, 45, 50), [255, 255, 255, 255]);
    s.adjust_end(true);
    assert_eq!(pixel(&s, 45, 50), [255, 255, 255, 255]);
    // only the two dots' soft edges, each moved as the other was, show at all
    for x in 15..60 {
        assert!(pixel(&s, x, 50)[0] > 180, "{x}: {:?}", pixel(&s, x, 50));
    }
}

#[test]
fn liquify_moves_a_small_canvas_shown_large() {
    use crate::adjust::{LiquifyBrush, LiquifyKind};
    // a 16 px canvas in an 800 px window: a canvas pixel 50 across, a drag of 300 just 6 of them
    let Some(s) = session(16, 800) else { return };
    s.set_brush_size(0.1);
    s.stroke_begin(PointerKind::Finger, vec![sample(300.0, 400.0, 0.0)]);
    s.stroke_end(vec![]);
    let row = |s: &CanvasSession| (0..16).map(|x| pixel(s, x, 8)).collect::<Vec<_>>();
    let before = row(&s);
    assert!(s.liquify_begin());
    let brush = LiquifyBrush { kind: LiquifyKind::Push, size: 0.5, pressure: 1.0, distortion: 0.0 };
    s.liquify_stroke((0..=30).flat_map(|i| [300.0 + i as f32 * 10.0, 400.0, 1.0, 8.0]).collect(), true, false, brush);
    assert_ne!(row(&s), before, "the push moved nothing");
    s.adjust_end(false);
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
            secondary: None,
        },
    );
    let set = s.import_brushes(file.clone()).unwrap();
    let alpha = |s: &CanvasSession| {
        let p = s.brush_preview(set, 0, w, h).unwrap();
        (p.rgba[((50 * w + 5) * 4 + 3) as usize], p.rgba[((10 * w + 160) * 4 + 3) as usize])
    };
    assert_eq!(alpha(&s), (255, 0));
    // kept with the library (in a file of its own), and written back out as it came
    let Some(other) = session(100, 100) else { return };
    let dir = std::env::temp_dir().join(format!("procraft-library-{}", std::process::id()));
    let kept = dir.join("brushes").join("library.json").to_string_lossy().into_owned();
    assert!(s.save_brush_library_file(kept.clone()));
    assert!(other.load_brush_library_file(kept.clone()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!other.load_brush_library_file(kept), "none kept");
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
fn an_edited_shape_is_turned_by_its_outline_and_moved_elsewhere() {
    use crate::quickshape::QuickShapeKind;
    let Some(s) = session(200, 200) else { return };
    s.set_brush_size(0.2);
    let pts: Vec<PenSample> = (0..=60)
        .map(|i| {
            let t = i as f32 / 60.0 * std::f32::consts::TAU;
            sample(100.0 + 60.0 * t.cos(), 100.0 + 40.0 * t.sin(), f64::from(i) * 8.0)
        })
        .collect();
    s.stroke_begin(PointerKind::Finger, pts[..1].to_vec());
    s.stroke_add(pts[1..].to_vec());
    assert_eq!(s.quickshape_hold().unwrap().kind, QuickShapeKind::Ellipse);
    s.stroke_end(vec![]);
    let before = s.quickshape_info().unwrap();
    // on its outline (the first axis end), not in its middle
    let (ex, ey) = (before.handles[0], before.handles[1]);
    assert!(s.quickshape_near_edge(ex, ey - 5.0, 22.0));
    assert!(!s.quickshape_near_edge(100.0, 100.0, 22.0));
    // the outline dragged half as far out again: half as big again about its centre
    s.quickshape_pin(ex, ey, true);
    let grown = s.quickshape_pin(100.0 + (ex - 100.0) * 1.5, 100.0 + (ey - 100.0) * 1.5, false).unwrap();
    assert!((grown.handles[0] - (100.0 + (ex - 100.0) * 1.5)).abs() < 1.0, "{:?}", grown.handles);
    s.quickshape_unpin();
    // dragged elsewhere: it moves along
    let moved = s.quickshape_move(100.0, 100.0, 110.0, 95.0).unwrap();
    for (i, (a, b)) in grown.handles.iter().zip(&moved.handles).enumerate() {
        let d = if i % 2 == 0 { 10.0 } else { -5.0 };
        assert!((b - a - d).abs() < 0.5, "{i}: {a} -> {b}");
    }
    s.quickshape_done();
}

#[test]
fn a_shape_still_held_follows_the_pen_and_a_finger_snaps_it() {
    use crate::quickshape::QuickShapeKind;
    let Some(s) = session(200, 200) else { return };
    s.set_brush_size(0.2);
    let angle = |i: &crate::quickshape::QuickShapeInfo| {
        (i.handles[3] - i.handles[1]).atan2(i.handles[2] - i.handles[0]).to_degrees()
    };
    // a line, held at its end
    let pts: Vec<PenSample> = (0..=40).map(|i| sample(20.0 + i as f32 * 2.0, 100.0, f64::from(i) * 8.0)).collect();
    s.stroke_begin(PointerKind::Finger, pts[..1].to_vec());
    s.stroke_add(pts[1..].to_vec());
    let info = s.quickshape_hold().expect("a shape");
    assert_eq!(info.kind, QuickShapeKind::Line);
    // the pen moves on: the line's end goes with it, turning about its start
    s.stroke_add(vec![sample(80.0, 160.0, 400.0)]);
    let moved = s.quickshape_info().unwrap();
    assert!((moved.handles[2] - 80.0).abs() < 1.0 && (moved.handles[3] - 160.0).abs() < 1.0, "{:?}", moved.handles);
    assert!((moved.handles[0] - 20.0).abs() < 1.0 && (moved.handles[1] - 100.0).abs() < 1.0);
    // a finger down: the angle in 15 degree steps (45 already is one)
    assert!((angle(&s.quickshape_finger(true).unwrap()) - 45.0).abs() < 0.5);
    s.stroke_add(vec![sample(150.0, 130.0, 450.0)]);
    assert!((angle(&s.quickshape_info().unwrap()) - 15.0).abs() < 0.5);
    // lifted: back to following the pen freely (about 13 degrees)
    let free = s.quickshape_finger(false).unwrap();
    assert!((free.handles[2] - 150.0).abs() < 1.0 && (free.handles[3] - 130.0).abs() < 1.0, "{:?}", free.handles);
    // down again, and the pen lifts first: the stepped line is kept
    let on = s.quickshape_finger(true).unwrap();
    assert!((angle(&on) - 15.0).abs() < 0.5);
    s.stroke_end(vec![]);
    // let go, it stays put, fingers or not
    s.stroke_add(vec![sample(30.0, 30.0, 500.0)]);
    assert_eq!(s.quickshape_info().unwrap().handles, on.handles);
    assert_eq!(s.quickshape_finger(false).unwrap().handles, on.handles);
    s.quickshape_done();
    // a held ellipse near round is a circle while a finger is down
    let pts: Vec<PenSample> = (0..=60)
        .map(|i| {
            let t = i as f32 / 60.0 * std::f32::consts::TAU;
            sample(100.0 + 60.0 * t.cos() + 2.0 * (7.0 * t).sin(), 100.0 + 40.0 * t.sin(), f64::from(i) * 8.0)
        })
        .collect();
    s.stroke_begin(PointerKind::Finger, pts[..1].to_vec());
    s.stroke_add(pts[1..].to_vec());
    assert_eq!(s.quickshape_hold().unwrap().kind, QuickShapeKind::Ellipse);
    assert_eq!(s.quickshape_finger(true).unwrap().kind, QuickShapeKind::Circle);
    assert_eq!(s.quickshape_finger(false).unwrap().kind, QuickShapeKind::Ellipse);
    assert_eq!(s.quickshape_finger(true).unwrap().kind, QuickShapeKind::Circle);
    s.stroke_end(vec![]);
    assert_eq!(s.quickshape_info().unwrap().kind, QuickShapeKind::Circle);
    s.quickshape_done();
}

#[test]
fn undo_goes_back_through_a_shapes_drags_to_the_stroke_it_was_made_of() {
    use crate::quickshape::QuickShapeKind;
    let Some(s) = session(200, 200) else { return };
    s.set_brush_size(0.02);
    // a wavering line, a line once it snaps
    let pts: Vec<PenSample> = (0..=40)
        .map(|i| {
            let t = i as f32 / 40.0;
            sample(20.0 + 160.0 * t, 100.0 + 5.0 * (t * 18.0).sin(), f64::from(i) * 8.0)
        })
        .collect();
    let draw = || {
        s.stroke_begin(PointerKind::Finger, pts[..1].to_vec());
        s.stroke_add(pts[1..].to_vec());
    };
    // (as drawn, an open shape staying open)
    let seen = || {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        canvas.read_composite(gpu).unwrap()
    };
    let blank = seen();
    draw();
    s.stroke_end(vec![]);
    let stroke = seen();
    s.undo();
    assert_eq!(seen(), blank);
    draw();
    let info = s.quickshape_hold().expect("a shape");
    assert_eq!(info.kind, QuickShapeKind::Line);
    s.stroke_end(vec![]);
    let snapped = seen();
    assert_ne!(snapped, stroke);
    // a drag is a step of its own, back and forward, the shape staying open
    s.quickshape_step();
    let moved = s.quickshape_move_handle(1, 180.0, 60.0).unwrap();
    assert!(!s.info().can_redo);
    s.undo();
    assert_eq!(s.quickshape_info().unwrap().handles, info.handles);
    assert_eq!(seen(), snapped);
    assert!(s.info().can_redo);
    s.redo();
    assert_eq!(s.quickshape_info().unwrap().handles, moved.handles);
    assert!(!s.info().can_redo);
    s.undo();
    // past the drags the shape goes, its stroke back as drawn
    s.undo();
    assert!(s.quickshape_info().is_none());
    assert_eq!(seen(), stroke);
    assert!(s.info().can_undo && s.info().can_redo);
    // redone, it is the shape as it snapped, kept, its drags gone
    s.redo();
    assert!(s.quickshape_info().is_none());
    assert_eq!(seen(), snapped);
    assert!(!s.info().can_redo);
    // and so on either way, a kept shape as one being edited
    s.undo();
    assert_eq!(seen(), stroke);
    s.undo();
    assert_eq!(seen(), blank);
    s.redo();
    assert_eq!(seen(), stroke);
    s.redo();
    assert_eq!(seen(), snapped);
    // with something done since, undo takes that, and the shape is a step like any other
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 900.0)]);
    s.stroke_end(vec![]);
    s.undo();
    assert_eq!(seen(), snapped);
    s.undo();
    assert_eq!(seen(), blank);
}

#[test]
fn a_nodes_drag_by_touch_is_one_step_of_undo_and_a_tap_on_it_none() {
    let Some(s) = session(200, 200) else { return };
    s.set_brush_size(0.02);
    let pts: Vec<PenSample> = (0..=40).map(|i| sample(20.0 + 4.0 * i as f32, 100.0, f64::from(i) * 8.0)).collect();
    s.stroke_begin(PointerKind::Finger, pts[..1].to_vec());
    s.stroke_add(pts[1..].to_vec());
    let info = s.quickshape_hold().expect("a shape");
    s.stroke_end(vec![]);
    s.set_shape_editing(true);
    let (x, y) = (info.handles[2], info.handles[3]);
    let drag = |by: f32, from: f64| {
        s.touch_down(7, PointerKind::Finger, vec![sample(x, y, from)]);
        for i in 1..=5 {
            let (i, t) = (i as f32, f64::from(i) * 16.0);
            s.touch_move(7, vec![sample(x, y + by * i / 5.0, from + t)]);
        }
        s.touch_up(7, vec![]);
    };
    s.touch_down(7, PointerKind::Finger, vec![sample(x, y, 1900.0)]);
    s.touch_up(7, vec![]);
    drag(40.0, 2000.0);
    let moved = s.quickshape_info().unwrap();
    // it stayed for the first ten points of the way (two pixels each here), then went as the
    // finger did, behind it
    let lag = 40.0 - (moved.handles[3] - info.handles[3]);
    assert!((20.0..=28.0).contains(&lag), "{lag}");
    assert!((moved.handles[2] - info.handles[2]).abs() < 1.0, "{:?}", moved.handles);
    // five moves, one step: one undo has it all back, the shape still open
    s.undo();
    assert_eq!(s.quickshape_info().unwrap().handles, info.handles);
    s.redo();
    assert_eq!(s.quickshape_info().unwrap().handles, moved.handles);
    s.undo();
    // past it the shape goes: the tap made no step of its own
    s.undo();
    assert!(s.quickshape_info().is_none());
}

#[test]
fn a_mask_takes_what_is_pasted_by_its_brightness_only_when_asked() {
    let Some(s) = session(64, 64) else { return };
    s.set_color(1.0, 0.0, 0.0);
    let layer = s.layers().iter().find(|l| l.current).unwrap().id;
    assert!(s.fill_layer(layer));
    let mask = s.add_mask(layer).unwrap();
    s.select_layer(mask);
    // white on the left, clear on the right: a sketch on a layer of its own
    let rgba: Vec<u8> = (0..64 * 64).flat_map(|i| if i % 64 < 32 { [255; 4] } else { [0; 4] }).collect();
    let picture = crate::layers::ThumbnailImage { width: 64, height: 64, rgba };
    // as anything pasted, a new layer
    let layers = s.layers().len();
    let pasted = s.paste(picture.clone()).unwrap();
    assert_eq!(s.layers().len(), layers + 1);
    assert_ne!(pasted, mask);
    s.undo();
    assert_eq!(s.layers().len(), layers);
    s.select_layer(mask);
    // asked to, the mask holds it: the layer shows where it was bright, nowhere else
    let (_plugins, plugin_files) = crate::hooks::tests::paste_into_masks(&s, "mask-takes-paste");
    assert_eq!(s.paste(picture.clone()), Some(mask));
    assert_eq!(s.layers().len(), layers);
    assert_eq!(pixel(&s, 10, 32), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 50, 32), [255, 255, 255, 255]);
    // one step back to the mask as it was
    s.undo();
    assert_eq!(pixel(&s, 50, 32), [255, 0, 0, 255]);
    // from another app too: smaller than the canvas, in the middle of it, the rest hidden
    let square = crate::layers::ThumbnailImage { width: 16, height: 16, rgba: vec![255; 16 * 16 * 4] };
    assert!(s.paste_picture_into_mask(square.clone()));
    assert_eq!(s.layers().len(), layers);
    assert_eq!(pixel(&s, 32, 32), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 4, 4), [255, 255, 255, 255]);
    s.undo();
    // on any other layer it is a new layer still
    s.select_layer(layer);
    assert!(!s.paste_picture_into_mask(square));
    assert_ne!(s.paste(picture), Some(layer));
    assert_eq!(s.layers().len(), layers + 1);
    let _ = std::fs::remove_dir_all(plugin_files);
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
fn a_copy_reads_only_what_is_selected_and_painted_as_the_whole_layer_would_give() {
    use crate::selection::SelectionJoin;
    let Some(s) = session(300, 300) else { return };
    s.set_brush_size(0.3);
    s.set_color(0.2, 0.6, 1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(60.0, 80.0, 0.0)]);
    s.stroke_add(vec![sample(220.0, 200.0, 40.0)]);
    s.stroke_end(vec![]);
    let current = s.layers().iter().find(|l| l.current).map(|l| l.id).unwrap();
    // the whole layer, as much of each pixel as the selection takes (as copying did)
    let whole = |s: &CanvasSession| {
        let mut pixels = s.copy_layer_contents(current).unwrap().rgba;
        if let Some(mask) = s.shared.lock().selection.clone() {
            for (px, &m) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(&mask.data) {
                for c in px.iter_mut() {
                    *c = ((u16::from(*c) * u16::from(m) + 127) / 255) as u8;
                }
            }
        }
        pixels
    };
    assert!(s.copy_layer().unwrap().rgba == whole(&s));
    // an ellipse over part of the stroke (its soft edge), then one away from any paint
    s.select_shape(true, 100.0, 90.0, 190.0, 170.0, SelectionJoin::Add);
    let copied = s.copy_layer().unwrap();
    assert!(copied.rgba == whole(&s) && copied.rgba.iter().any(|&v| v > 0));
    s.select_none();
    s.select_shape(false, 0.0, 250.0, 40.0, 290.0, SelectionJoin::Add);
    assert!(s.copy_layer().unwrap().rgba.iter().all(|&v| v == 0));
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
fn a_private_picture_stays_out_of_the_time_lapse() {
    let Some(s) = session(100, 100) else { return };
    let dir = std::env::temp_dir().join(format!("procraft-private-lapse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let artwork = dir.to_string_lossy().into_owned();
    let lapse = crate::TimeLapse::new(artwork.clone());
    // a frame as the apps record one after each change, as large as the video
    let record = |s: &CanvasSession| {
        let shot = s.time_lapse_frame(crate::time_lapse_side(lapse.settings())).unwrap();
        lapse.record(shot.rgba, shot.width, shot.height)
    };
    // what the replay (and the video made of the frames) shows at (x, y) of frame `i`
    let shown = |i: u32, x: u32, y: u32| {
        let f = lapse.frame(i).unwrap();
        let at = ((y * f.width + x) * 4) as usize;
        [f.rgba[at], f.rgba[at + 1], f.rgba[at + 2]]
    };
    let white = |c: [u8; 3]| c.iter().all(|&v| v > 235);
    let photo = crate::layers::ThumbnailImage { width: 100, height: 100, rgba: [255, 0, 0, 255].repeat(100 * 100) };
    let id = s.insert_image(photo, true).unwrap();
    assert!(record(&s));
    assert!(white(shown(0, 50, 50)), "the picture is recorded: {:?}", shown(0, 50, 50));
    // a stroke on a layer over it is recorded as ever, the picture under it still left out
    s.add_layer();
    s.set_color(0.0, 0.0, 1.0);
    s.set_brush_size(0.3);
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 20.0, 0.0)]);
    s.stroke_add(vec![sample(90.0, 20.0, 40.0)]);
    s.stroke_end(vec![]);
    assert_eq!((pixel(&s, 50, 20), pixel(&s, 50, 50)), ([0, 0, 255, 255], [255, 0, 0, 255]), "the canvas shows both");
    assert!(record(&s));
    let [r, g, b] = shown(1, 50, 20);
    assert!(b > 200 && r < 60 && g < 60, "the stroke: {:?}", [r, g, b]);
    assert!(white(shown(1, 50, 50)), "the picture is recorded: {:?}", shown(1, 50, 50));
    // kept private in the artwork's file: opened again, its frames still leave it out
    s.save(artwork.clone()).unwrap();
    let opened = CanvasSession::open(artwork).unwrap();
    let Some(gpu) = s.shared.lock().gpu.clone() else { return };
    opened.use_gpu(gpu);
    assert!(opened.layers().iter().any(|l| l.id == id && l.private));
    let frame = opened.time_lapse_frame(crate::time_lapse_side(lapse.settings())).unwrap();
    let middle = ((50 * frame.width + 50) * 4) as usize;
    assert_eq!(&frame.rgba[middle..middle + 4], &[255, 255, 255, 255]);
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
    // a wavering stroke near the diagonal keeps the direction it set out in, not zigzagging
    // between the grid's two
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 10.0, 0.0)]);
    s.stroke_add(
        (1..=20)
            .map(|i| sample(10.0 + i as f32 * 2.0, 10.0 + i as f32 * 2.0 + [1.5, -1.5][i % 2], i as f64 * 8.0))
            .collect(),
    );
    s.stroke_end(vec![]);
    let row = (10..55).filter(|&x| pixel(&s, x, 10)[0] == 0).count();
    let column = (10..55).filter(|&y| pixel(&s, 10, y)[0] == 0).count();
    // (the symmetry dot is at 20, 30)
    let off =
        [(20, 20), (30, 30), (40, 40), (30, 20), (40, 25)].iter().filter(|&&(x, y)| pixel(&s, x, y)[0] == 0).count();
    assert!(row.max(column) > 40 && row.min(column) < 8 && off == 0, "row {row} column {column} off the line {off}");
    // drawing assist belongs to the layer: a new one draws freely
    let assisted = s.layers().iter().find(|l| l.current).map(|l| l.id).unwrap();
    assert!(s.layers().iter().any(|l| l.id == assisted && l.assisted));
    s.add_layer();
    assert!(!s.guide().assisted);
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 90.0, 0.0)]);
    s.stroke_add((1..=20).map(|i| sample(10.0 + i as f32 * 3.0, 90.0 - i as f32 * 0.6, f64::from(i) * 8.0)).collect());
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 70, 78)[0], 0);
    assert_eq!(pixel(&s, 70, 90), [255, 255, 255, 255]);
    assert!(s.select_layer(assisted));
    assert!(s.guide().assisted);
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
fn a_group_s_lock_shows_on_its_layers_and_their_unlock_takes_it_off() {
    use crate::layers::{Blocked, LayerOption as O};
    let Some(s) = session(64, 64) else { return };
    let lower = s.layers()[0].id;
    let upper = s.add_layer();
    assert!(s.pick_layer(lower));
    let group = s.group_picked();
    assert_ne!(group, 0);
    let locked = |id| s.layers().iter().find(|l| l.id == id).unwrap().locked;
    // as measured: its layers show locked, offer the locked menu and take no paint
    assert!(s.set_layer_locked(upper, true));
    assert!(s.set_layer_locked(group, true));
    assert!(locked(lower) && locked(upper));
    assert!(!s.layer_options(lower).iter().any(|r| r.option == O::Rename));
    assert!(s.select_layer(lower));
    assert_eq!(s.why_blocked(), Some(Blocked::Locked));
    // unlocking the group leaves a layer's own lock
    assert!(s.set_layer_locked(group, false));
    assert!(!locked(lower) && locked(upper));
    assert_eq!(s.why_blocked(), None);
    // locked by its group alone, a layer's Unlock unlocks the group: one undo step, called so
    assert!(s.set_layer_locked(group, true));
    assert_eq!(s.lock_said(lower, false), "Unlock Layer Group");
    assert_eq!(s.lock_said(upper, false), "Unlock layer");
    assert!(s.set_layer_locked(lower, false));
    assert!(!locked(group) && !locked(lower) && locked(upper));
    assert_eq!(s.undo().as_deref(), Some("unlock Layer Group"));
    assert!(locked(group) && locked(lower));
    // a group's step is named as measured ("Undo unlock Layer Group")
    assert!(s.set_layer_locked(group, false));
    assert_eq!(s.undo().as_deref(), Some("unlock Layer Group"));
}

#[test]
fn drawing_assist_is_a_step_and_a_mask_follows_its_layer() {
    let Some(s) = session(64, 64) else { return };
    let layer = s.layers()[0].id;
    let mask = s.add_mask(layer).unwrap();
    let assisted = |id| s.layers().iter().find(|l| l.id == id).unwrap().assisted;
    // as measured: the mask shows its layer's, and the change is a named step
    assert!(s.select_layer(layer));
    s.set_guide_assisted(true);
    assert!(assisted(layer) && assisted(mask));
    assert_eq!(s.undo().as_deref(), Some("Drawing Assist on"));
    assert!(!assisted(layer) && !assisted(mask));
}

#[test]
fn a_layer_s_options_follow_what_it_is() {
    use crate::layers::{LayerOption as O, LayerOptionRow, LayerType};
    let s = CanvasSession::new(64, 64);
    let bottom = s.layers()[0].id;
    let top = s.add_layer();
    let options = |id| s.layer_options(id).into_iter().map(|r: LayerOptionRow| r.option).collect::<Vec<_>>();
    let row = |id, o| s.layer_options(id).into_iter().find(|r| r.option == o).unwrap();
    // a paint layer with one under it: everything but the text's and a group's, Drawing Assist
    // once the guide has been on (still offered with it off again)
    assert!(!options(top).contains(&O::DrawingAssist));
    s.set_guide_on(true);
    s.set_guide_on(false);
    assert_eq!(
        options(top),
        [
            O::Rename,
            O::Select,
            O::Copy,
            O::Fill,
            O::Clear,
            O::AlphaLock,
            O::Mask,
            O::ClippingMask,
            O::DrawingAssist,
            O::Invert,
            O::Reference,
            O::MergeDown,
            O::CombineDown
        ]
    );
    // the bottom one has nothing to merge into or clip to
    assert!(!options(bottom).contains(&O::MergeDown) && !options(bottom).contains(&O::ClippingMask));
    // nor is anything merged or combined onto a locked one, as measured
    assert!(s.set_layer_locked(bottom, true));
    assert!(!options(top).contains(&O::MergeDown) && !options(top).contains(&O::CombineDown));
    assert!(s.set_layer_locked(bottom, false));
    // switches say whether they are on
    assert!(s.set_layer_alpha_lock(top, true));
    assert_eq!(row(top, O::AlphaLock).switch_on, Some(true));
    assert_eq!(row(top, O::Fill).switch_on, None);
    // locked, as measured: only what leaves its pixels, name and place as they are
    assert!(s.set_layer_locked(top, true));
    assert_eq!(options(top), [O::Select, O::Copy, O::Mask, O::ClippingMask, O::DrawingAssist, O::Reference]);
    // with a mask, no second one: Merge Mask last instead; the mask's own menu is short
    let mask = s.add_mask(bottom).unwrap();
    let with_mask = options(bottom);
    assert!(!with_mask.contains(&O::Mask) && with_mask.last() == Some(&O::MergeMask));
    assert_eq!(options(mask), [O::Rename, O::Select, O::Fill, O::Clear, O::Invert]);
    // a locked mask's menu is empty, as measured
    assert!(s.set_layer_locked(mask, true));
    assert!(options(mask).is_empty());
    assert!(s.set_layer_locked(mask, false));
    // chosen, the mask goes into its layer; with none left, Mask is back
    assert!(s.merge_mask(bottom));
    assert!(s.layers().iter().all(|l| l.kind != LayerType::Mask));
    assert!(options(bottom).contains(&O::Mask) && !options(bottom).contains(&O::MergeMask));
    assert!(!s.merge_mask(bottom));
    // a text layer: its own two before Reference, nothing that would paint into it
    assert!(s.set_layer_locked(top, false) && s.select_layer(top));
    let text = s.text_begin(None, text_info("Hi")).unwrap();
    assert_eq!(
        options(text),
        [
            O::Rename,
            O::Select,
            O::Copy,
            O::Mask,
            O::ClippingMask,
            O::EditText,
            O::Rasterize,
            O::Reference,
            O::MergeDown,
            O::CombineDown
        ]
    );
    assert!(s.layer_options(9999).is_empty());
}

#[test]
fn an_edit_to_part_of_a_layer_keeps_only_that_part_to_undo() {
    use crate::selection::SelectionJoin;
    let Some(s) = session(400, 400) else { return };
    let history = |s: &CanvasSession| {
        let _ = pixel(s, 0, 0);
        s.shared.lock().canvas.as_ref().map_or(0, |c| c.history_bytes())
    };
    let part = 42 * 22 * 4;
    // a 40 x 20 selection filled with red: the step keeps that much, not the whole layer
    s.select_shape(false, 100.0, 100.0, 140.0, 120.0, SelectionJoin::Add);
    s.set_color(1.0, 0.0, 0.0);
    let before = history(&s);
    s.fill_selection();
    let kept = history(&s) - before;
    assert!(kept > 0 && kept <= part, "{kept}");
    assert_eq!(pixel(&s, 120, 110), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 150, 110), [255, 255, 255, 255]);
    // inverted: only what is painted changes
    let id = s.layers()[0].id;
    s.select_none();
    let before = history(&s);
    assert!(s.invert_layer(id));
    let kept = history(&s) - before;
    assert!(kept > 0 && kept <= part, "{kept}");
    assert_eq!(pixel(&s, 120, 110), [0, 255, 255, 255]);
    // undone and redone as before
    s.undo();
    assert_eq!(pixel(&s, 120, 110), [255, 0, 0, 255]);
    s.undo();
    s.undo();
    assert_eq!(pixel(&s, 120, 110), [255, 255, 255, 255]);
    s.redo();
    assert_eq!(pixel(&s, 120, 110), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 150, 110), [255, 255, 255, 255]);
    // a clear layer has nothing to invert
    let empty = s.add_layer();
    assert!(!s.invert_layer(empty));
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
    assert!(s.copy_layer_contents(id).unwrap().rgba.chunks(4).all(|p| p == [0, 0, 255, 255]));
    // undone, the layer as the fill within the selection left it; redone, all of it again
    s.undo();
    assert_eq!(pixel(&s, 80, 80), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 30, 30), [0, 0, 255, 255]);
    s.redo();
    assert_eq!(pixel(&s, 80, 80), [0, 0, 255, 255]);
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
    // the outer group's Merge Down takes its top layer into red, under it outside the group: the
    // group stays, empty, and it all looks the same
    assert!(s.merge_down(outer));
    assert_eq!(s.layers().iter().map(|l| l.id).collect::<Vec<_>>(), [outer, red]);
    assert!(near(pixel(&s, 50, 50), [128, 255, 128]), "{:?}", pixel(&s, 50, 50));
    assert_eq!(pixel(&s, 30, 30), [255, 0, 0, 255]);
    s.undo();
    assert!(s.layers().iter().any(|l| l.id == flat));
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
    let (_, layer) = st.pending_pixels.first().unwrap();
    assert_eq!(&layer.clone().pixels(6, 4).unwrap()[..4], &[128, 0, 0, 128]);
}

#[test]
fn a_picture_saved_before_it_is_shown_has_its_thumbnail() {
    use crate::layers::ThumbnailImage;
    let dir = std::env::temp_dir().join(format!("procraft-unshown-thumbnail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let rgba = [0u8, 0, 255, 255].repeat(6 * 4);
    let s = CanvasSession::from_image(ThumbnailImage { width: 6, height: 4, rgba }, "Blue".to_owned());
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    let thumb = crate::project::read_png(&dir.join("thumbnail.png"), 6, 4).unwrap();
    assert_eq!(&thumb[..4], &[0, 0, 255, 255]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_picture_larger_than_a_canvas_may_be_is_scaled_down_to_fit() {
    use crate::layers::ThumbnailImage;
    let most = procraft_engine::document::canvas_edge_most();
    let (w, h) = (most + most / 4, 8);
    // the left half red, the right half blue
    let rgba: Vec<u8> =
        (0..w * h).flat_map(|i| if i % w < w / 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
    let s = CanvasSession::from_image(ThumbnailImage { width: w, height: h, rgba }, "Wide".to_owned());
    let (cw, ch) = (s.info().width, s.info().height);
    assert_eq!((cw, ch), (most, (h as f32 * most as f32 / w as f32).round() as u32));
    let st = s.shared.lock();
    let (_, layer) = st.pending_pixels.first().unwrap();
    let px = layer.clone().pixels(cw, ch).unwrap();
    let at = |x: u32, y: u32| px[((y * cw + x) * 4) as usize..][..4].to_vec();
    assert_eq!(at(10, ch / 2), [255, 0, 0, 255]);
    assert_eq!(at(cw - 10, ch / 2), [0, 0, 255, 255]);
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
fn a_group_frame_shows_the_layers_showing_in_it() {
    let Some(s) = session(40, 40) else { return };
    s.set_brush_size(0.6);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 15.0, 0.0)]);
    s.stroke_end(vec![]);
    let first = s.chosen_layers()[0];
    let second = s.add_layer();
    s.set_color(0.0, 0.0, 1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    s.pick_layer(first);
    let group = s.group_picked();
    assert_ne!(group, 0);
    let ink =
        |t: &crate::layers::ThumbnailImage, c: usize| t.rgba.chunks(4).filter(|p| p[c] > 100 && p[3] > 100).count();
    let both = s.frame_thumbnail(group, 40).expect("a page's picture");
    assert_eq!((both.width, both.height), (40, 40));
    assert!(ink(&both, 0) > 0 && ink(&both, 2) > 0, "both layers' paint");
    // a layer turned off in the group is left out
    s.set_layer_visible(second, false);
    let one = s.frame_thumbnail(group, 40).unwrap();
    assert!(ink(&one, 0) > 0 && ink(&one, 2) == 0);
    // and nothing showing is no picture
    s.set_layer_visible(first, false);
    assert!(s.frame_thumbnail(group, 40).is_none());
}

#[test]
fn the_studio_edits_either_part_of_a_dual_brush_and_a_cancel_puts_back_both() {
    let Some(s) = session(64, 64) else { return };
    let at = s.current_brush();
    assert!(s.combine_brushes(at.set, at.index, if at.index == 0 { 1 } else { 0 }));
    s.studio_begin();
    assert!(s.studio_is_dual());
    let spacing = || s.studio_settings(vec!["spacing".to_owned()])[0];
    let (first, was) = (spacing(), s.studio_combine_mode());
    s.studio_show_second(true);
    assert!(s.studio_shows_second());
    let second = spacing();
    s.set_studio_setting("spacing".to_owned(), second + 0.1);
    assert!((spacing() - second - 0.1).abs() < 1e-3, "the second took it");
    s.studio_show_second(false);
    assert_eq!(spacing(), first, "the first didn't");
    // how they combine is the pair's, whichever part shows
    let multiply = crate::layers::combine_blend_modes()[0].id;
    s.studio_show_second(true);
    s.set_studio_combine_mode(multiply);
    assert_eq!(s.studio_combine_mode(), multiply);
    assert!(s.studio_part_preview(false, 64, 32).is_some() && s.studio_part_preview(true, 64, 32).is_some());
    s.studio_end(false);
    s.studio_begin();
    assert!(!s.studio_shows_second(), "it opens on the first");
    s.studio_show_second(true);
    assert_eq!(spacing(), second);
    assert_eq!(s.studio_combine_mode(), was);
    s.studio_end(false);
}

#[test]
fn a_held_frame_splits_where_it_is_chosen_and_loses_a_place_on_delete() {
    let Some(s) = session(40, 40) else { return };
    s.add_layer();
    assert!(s.set_animation_assist(true));
    let first = s.animation_frames()[0];
    assert!(s.change_frame_hold(first, 2, false));
    assert!(s.change_frame_hold(first, 3, true));
    assert_eq!(s.animation_sequence(), vec![0, 0, 0, 0, 1]);
    // one step for the whole drag
    s.undo();
    assert_eq!(s.frame_hold(first), 0);
    s.redo();
    assert_eq!(s.frame_hold(first), 3);
    // the second held place: the frame keeps one, the copy holds the one after, and is current
    assert!(s.split_held_frame(first, 4).is_none());
    let copy = s.split_held_frame(first, 2).unwrap();
    assert_eq!(s.animation_sequence(), vec![0, 0, 1, 1, 2]);
    assert_eq!((s.frame_hold(first), s.frame_hold(copy)), (1, 1));
    assert_eq!(s.animation_frames()[1], copy);
    assert_eq!(s.current_frame(), Some(1));
    s.undo();
    assert_eq!(s.animation_sequence(), vec![0, 0, 0, 0, 1]);
    assert!(s.drop_held_frame(first));
    assert_eq!(s.animation_sequence(), vec![0, 0, 0, 1]);
    s.undo();
    assert_eq!(s.frame_hold(first), 3);
}

#[test]
fn a_saved_animation_can_be_drawn_with_no_window() {
    let Some(s) = session(40, 40) else { return };
    s.set_brush_size(0.6);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 15.0, 0.0)]);
    s.stroke_end(vec![]);
    s.add_layer();
    let dir = std::env::temp_dir().join(format!("procraft-plays-{}", std::process::id()));
    let path = dir.to_string_lossy().into_owned();
    s.save(path.clone()).unwrap();
    assert!(!crate::artwork_plays(path.clone()), "not animated");
    assert!(s.set_animation_assist(true));
    s.save(path.clone()).unwrap();
    assert!(crate::artwork_plays(path.clone()));
    let opened = CanvasSession::open(path).unwrap();
    assert!(opened.draw_without_window());
    assert_eq!(opened.animation_frame_count(), 2);
    let first = opened.animation_frame(0, false, 40).unwrap();
    assert!(first.rgba.chunks(4).any(|p| p[0] > 200 && p[2] < 50), "the first frame's paint");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_colour_dropped_in_a_ring_leaves_no_pale_rim_inside_the_line() {
    let Some(s) = session(400, 400) else { return };
    s.set_brush_size(0.1);
    s.set_color(0.93, 0.52, 0.27);
    let ring: Vec<PenSample> = (0..=120)
        .map(|i| {
            let a = i as f32 / 120.0 * 6.5;
            sample(200.0 + 130.0 * a.cos(), 200.0 + 130.0 * a.sin(), f64::from(i) * 8.0)
        })
        .collect();
    s.stroke_begin(PointerKind::Finger, ring[..1].to_vec());
    s.stroke_add(ring[1..].to_vec());
    s.stroke_end(vec![]);
    assert!(s.pour_begin(200.0, 200.0, 0.93, 0.52, 0.27, 0.5));
    s.pour_end(true, None);
    // from the middle out to the line and into it, nothing paler than the paint
    for spoke in 0..24 {
        let t = (spoke as f32 * 15.0 + 7.0).to_radians();
        for d in 100..124 {
            let d = d as f32;
            let [_, _, b, _] = pixel(&s, (200.0 + d * t.cos()) as u32, (200.0 + d * t.sin()) as u32);
            assert!(b < 75, "a rim at {d} on spoke {spoke}: {b}");
        }
    }
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
    s.export_gif(path("a.gif"), 0, 15.0, true, true, false).unwrap();
    let gif = std::fs::read(dir.join("a.gif")).unwrap();
    assert_eq!(&gif[..6], b"GIF89a");
    assert_eq!(gif.windows(3).filter(|w| w == &[0x21, 0xF9, 4]).count(), 2);
    // the frames sharing one palette: a table for the whole file, each frame reading back in
    // its colours
    s.export_gif(path("b.gif"), 0, 15.0, false, false, false).unwrap();
    let shared = std::fs::read(dir.join("b.gif")).unwrap();
    assert_eq!(shared[10] & 0x80, 0x80);
    let mut frames: Vec<Vec<u8>> = Vec::new();
    procraft_engine::gif::frames(&shared, |canvas, _| {
        frames.push(canvas.to_vec());
        true
    });
    assert_eq!(frames.len(), 2);
    assert!(frames[0].chunks(4).any(|p| p[0] > 200 && p[2] < 50));
    assert!(frames[1].chunks(4).any(|p| p[2] > 200 && p[0] < 50));
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
    no_streamline(&s);
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
    // undone, the stroke is back as drawn; undone again, nothing is left
    s.undo();
    assert!(near(40, 64) && near(76, 54));
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
fn two_brushes_combine_into_a_dual_brush_and_come_apart_again() {
    let s = CanvasSession::new(64, 64);
    let before = s.brush_library();
    let names: Vec<String> = before[0].brushes.iter().map(|b| b.name.clone()).collect();
    // the tool on the brush that joins: it then uses the dual brush, in the first's place
    assert!(s.select_brush(0, 3));
    assert!(s.combine_brushes(0, 1, 3));
    let after = s.brush_library();
    assert_eq!(after[0].brushes.len(), names.len() - 1);
    assert_eq!(after[0].brushes[1].name, names[1]);
    assert_eq!(after[0].brushes[3].name, names[4]);
    assert!(s.brush_is_dual(0, 1));
    assert_eq!((s.current_brush().set, s.current_brush().index), (0, 1));
    // a dual brush takes no third, and a brush can't join itself
    assert!(!s.combine_brushes(0, 1, 2));
    assert!(!s.combine_brushes(0, 2, 1));
    assert!(!s.combine_brushes(0, 2, 2));
    // the two trade places: the second leads, and back
    assert!(s.swap_dual_brush(0, 1));
    assert_eq!(s.brush_library()[0].brushes[1].name, names[3]);
    assert!(s.brush_is_dual(0, 1));
    assert!(s.swap_dual_brush(0, 1));
    assert_eq!(s.brush_library()[0].brushes[1].name, names[1]);
    assert!(!s.swap_dual_brush(0, 2));
    // written out and read back, it stays dual
    let saved = s.saved_brush_library();
    let t = CanvasSession::new(64, 64);
    assert!(t.load_brush_library(saved));
    assert!(t.brush_is_dual(0, 1));
    // apart: the second just below the first, the rest as before
    assert!(s.uncombine_brush(0, 1));
    assert!(!s.brush_is_dual(0, 1));
    let apart: Vec<String> = s.brush_library()[0].brushes.iter().map(|b| b.name.clone()).collect();
    assert_eq!(apart.len(), names.len());
    assert_eq!(apart[2], names[3]);
    assert!(!s.uncombine_brush(0, 1));
}

#[test]
fn library_work_keeps_the_tool_on_a_brush_and_comes_back_saved() {
    for seed in [7u32, 33] {
        let s = CanvasSession::new(64, 64);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        // each set's brushes as they show: names, and which are dual
        let shown = |s: &CanvasSession| {
            s.brush_library()
                .into_iter()
                .enumerate()
                .map(|(i, set)| {
                    let brushes: Vec<(String, bool)> =
                        set.brushes.iter().map(|b| (b.name.clone(), s.brush_is_dual(i as u32, b.index))).collect();
                    (set.name, brushes)
                })
                .collect::<Vec<_>>()
        };
        let mut done = Vec::new();
        for _ in 0..200 {
            let library = s.brush_library();
            let sets = library.len() as u32;
            let set = next(sets);
            let n = library.get(set as usize).map_or(0, |x| x.brushes.len() as u32);
            let op = next(10);
            done.push(op);
            match op {
                0 => _ = s.duplicate_brush(set, next(n)),
                1 => _ = s.move_brush(set, next(n), next(sets), next(8)),
                2 => _ = s.delete_brush(set, next(n)),
                3 => {
                    // a tool on either of the two then uses the dual brush, in the first's place
                    let (a, b) = (next(n), next(n));
                    let c = s.current_brush();
                    if s.combine_brushes(set, a, b) && c.set == set && (c.index == a || c.index == b) {
                        let now = s.current_brush();
                        let at = if a > b { a - 1 } else { a };
                        assert_eq!((now.set, now.index), (set, at), "seed {seed}: after {done:?}");
                        assert!(s.brush_is_dual(set, at));
                    }
                }
                4 => _ = s.uncombine_brush(set, next(n)),
                5 => _ = s.swap_dual_brush(set, next(n)),
                6 => _ = s.select_brush(set, next(n)),
                7 => s.set_tool([ToolKind::Paint, ToolKind::Erase, ToolKind::Smudge][next(3) as usize]),
                8 => _ = s.move_brush_set(set, next(sets)),
                _ => _ = s.new_brush(set),
            }
            // the tool is on a brush there is
            let library = s.brush_library();
            let c = s.current_brush();
            if library.iter().any(|x| !x.brushes.is_empty()) {
                let there = library.get(c.set as usize).is_some_and(|x| (c.index as usize) < x.brushes.len());
                assert!(there, "seed {seed}: the tool on ({}, {}) after {done:?}", c.set, c.index);
            }
        }
        // written out and read back: the same sets, brushes and dual brushes
        let t = CanvasSession::new(64, 64);
        assert!(t.load_brush_library(s.saved_brush_library()));
        assert_eq!(shown(&t), shown(&s), "seed {seed}: after {done:?}");
    }
}

/// Changes one place in `v` at random (`next(n)` a number below n): a number made 0, -1, huge or
/// a half, a flag turned over, a text emptied, an entry dropped or repeated, a key taken out.
fn damage_json(v: &mut serde_json::Value, next: &mut dyn FnMut(usize) -> usize) {
    fn places(v: &serde_json::Value, at: &mut Vec<Vec<String>>, path: Vec<String>) {
        at.push(path.clone());
        match v {
            serde_json::Value::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    places(item, at, [path.clone(), vec![i.to_string()]].concat());
                }
            }
            serde_json::Value::Object(map) => {
                for (k, item) in map {
                    places(item, at, [path.clone(), vec![k.clone()]].concat());
                }
            }
            _ => {}
        }
    }
    let mut at = Vec::new();
    places(v, &mut at, Vec::new());
    let Some(path) = at.get(next(at.len())) else { return };
    let mut node = &mut *v;
    for key in path {
        node = match node {
            serde_json::Value::Array(items) => &mut items[key.parse::<usize>().unwrap()],
            serde_json::Value::Object(map) => map.get_mut(key).unwrap(),
            _ => return,
        };
    }
    let pick = next(4);
    match node {
        serde_json::Value::Number(_) => *node = [0.0, -1.0, 1e9, 0.5][pick].into(),
        serde_json::Value::Bool(b) => *b = !*b,
        serde_json::Value::String(t) => t.clear(),
        serde_json::Value::Array(items) if !items.is_empty() => {
            let i = next(items.len());
            if pick < 2 {
                items.remove(i);
            } else {
                let copy = items[i].clone();
                items.insert(i, copy);
            }
        }
        serde_json::Value::Object(map) if !map.is_empty() => {
            let key = map.keys().nth(next(map.len())).cloned().unwrap();
            map.remove(&key);
        }
        _ => {}
    }
}

#[test]
fn saves_before_and_after_a_crop_leave_nothing_beside_and_open_at_the_new_size() {
    let Some(s) = session(64, 160) else { return };
    s.add_layer();
    s.stroke_begin(PointerKind::Stylus, vec![sample(20.0, 80.0, 0.0)]);
    s.stroke_end(vec![sample(140.0, 80.0, 50.0)]);
    let dir = std::env::temp_dir().join(format!("procraft-staged-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let d = dir.to_string_lossy().into_owned();
    s.save(d.clone()).unwrap();
    assert!(s.crop_canvas(8.0, 8.0, 40.0, 30.0, 0.0, false, 0, 0));
    s.save(d.clone()).unwrap();
    // every file in place, none left beside it
    let mut files = Vec::new();
    for folder in [dir.clone(), dir.join("layers")] {
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            files.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    let opened = CanvasSession::open(d);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(files.iter().all(|f| !f.ends_with(".tmp")), "{files:?}");
    let opened = opened.unwrap();
    assert_eq!((opened.info().width, opened.info().height), (40, 30));
    // each layer's file read at that size
    let pending = std::mem::take(&mut opened.shared.lock().pending_pixels);
    assert_eq!(crate::project::opened_pixels(pending, 40, 30).len(), s.layers().len());
}

#[test]
fn an_artwork_whose_file_has_settings_it_cant_read_opens_with_its_layers() {
    let Some(s) = session(64, 160) else { return };
    s.add_layer();
    s.add_layer();
    let dir = std::env::temp_dir().join(format!("procraft-unreadable-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    // how it was last shown, what it keeps about itself and the guide no longer readable
    let file = dir.join("artwork.json");
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    v["view"] = "here".into();
    v["info"] = 5.into();
    v["document"]["guide"] = serde_json::json!([1, 2]);
    std::fs::write(&file, v.to_string()).unwrap();
    let opened = CanvasSession::open(dir.to_string_lossy().into_owned());
    let _ = std::fs::remove_dir_all(&dir);
    let opened = opened.unwrap();
    assert_eq!(opened.layers().len(), s.layers().len());
}

#[test]
fn a_kept_library_with_a_brush_it_cant_read_keeps_the_rest() {
    let s = CanvasSession::new(64, 64);
    let before = s.brush_library();
    let mut v: serde_json::Value = serde_json::from_str(&s.saved_brush_library()).unwrap();
    // the first set's second brush damaged in part, its third without a name, the second set's
    // name gone
    v[0]["brushes"][1]["from"] = 0.5.into();
    v[0]["brushes"][1]["settings"]["size"] = "big".into();
    v[0]["brushes"][2].as_object_mut().unwrap().remove("name");
    v[1].as_object_mut().unwrap().remove("name");
    let t = CanvasSession::new(64, 64);
    assert!(t.load_brush_library(v.to_string()));
    let after = t.brush_library();
    assert_eq!(after.len(), before.len() - 1, "the set without a name left out");
    assert_eq!(after[0].brushes.len(), before[0].brushes.len() - 1, "only the brush without a name left out");
    assert_eq!(after[0].brushes[1].name, before[0].brushes[1].name, "the damaged one kept, by name");
    assert_eq!(after[0].brushes[2].name, before[0].brushes[3].name);
    assert_eq!(after[1].name, before[2].name);
}

#[test]
fn a_kept_library_damaged_anyhow_opens_with_each_tool_on_a_brush_that_paints() {
    let dir = std::env::temp_dir().join(format!("procraft-library-damaged-{}", std::process::id()));
    for seed in 0u32..10 {
        let mut state = seed.wrapping_mul(2_654_435_761) | 1;
        let mut next = |n: usize| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as usize % n.max(1)
        };
        // a library worked on, each tool on a brush, kept
        let s = CanvasSession::new(64, 64);
        for _ in 0..10 {
            let library = s.brush_library();
            let set = next(library.len()) as u32;
            let n = library.get(set as usize).map_or(0, |x| x.brushes.len());
            match next(5) {
                0 => _ = s.duplicate_brush(set, next(n) as u32),
                1 => _ = s.delete_brush(set, next(n) as u32),
                2 => _ = s.combine_brushes(set, next(n) as u32, next(n) as u32),
                3 => _ = s.new_brush(set),
                _ => {
                    s.set_tool([ToolKind::Paint, ToolKind::Erase, ToolKind::Smudge][next(3)]);
                    _ = s.select_brush(set, next(n) as u32);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        let d = dir.to_string_lossy().into_owned();
        assert!(s.keep_brushes(d.clone()));
        // its files damaged in a few places each
        for file in ["library.json", "choices.json"] {
            let path = dir.join(file);
            let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&path).unwrap()) else {
                continue;
            };
            for _ in 0..1 + next(6) {
                damage_json(&mut v, &mut next);
            }
            std::fs::write(&path, v.to_string()).unwrap();
        }
        // opened: each tool on a brush there is, which paints without a GPU error
        let Some(t) = session(64, 64) else { return };
        let _ = t.open_brushes(d);
        for (k, tool) in [ToolKind::Paint, ToolKind::Erase, ToolKind::Smudge].into_iter().enumerate() {
            t.set_tool(tool);
            let library = t.brush_library();
            let c = t.current_brush();
            if library.iter().any(|x| !x.brushes.is_empty()) {
                let there = library.get(c.set as usize).is_some_and(|x| (c.index as usize) < x.brushes.len());
                assert!(there, "seed {seed}: {tool:?} on ({}, {})", c.set, c.index);
            }
            let y = 10.0 + k as f32 * 20.0;
            t.stroke_begin(PointerKind::Stylus, vec![sample(5.0, y, 0.0)]);
            t.stroke_add((1..12).map(|i| sample(5.0 + i as f32 * 5.0, y, f64::from(i) * 8.0)).collect());
            t.stroke_end(vec![sample(60.0, y, 100.0)]);
        }
        let mut guard = t.shared.lock();
        let st = &mut *guard;
        if let (Some(c), Some(g)) = (st.canvas.as_mut(), st.gpu.as_ref()) {
            c.render(g, &mut st.doc, None);
            g.flush();
            assert_eq!(g.errors(), 0, "seed {seed}: GPU errors");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
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
fn a_shaped_brush_outlines_its_shape_and_the_round_tip_none() {
    let s = CanvasSession::new(64, 64);
    // across the library: some brushes are the round tip (no outline, the cursor a circle), others
    // outline their shape within their size (and their roundness)
    let (mut round, mut shaped) = (0, 0);
    for (set, brushes) in s.brush_library().into_iter().enumerate() {
        for b in brushes.brushes {
            assert!(s.select_brush(set as u32, b.index));
            let loops = s.brush_outline();
            if loops.is_empty() {
                round += 1;
                continue;
            }
            shaped += 1;
            let tall = s.current_brush_setting("roundness".into()).max(1.0);
            for ring in &loops {
                assert!(ring.len() >= 6 && ring.len() % 2 == 0, "{}: {} numbers", b.name, ring.len());
                assert!(ring.iter().all(|v| v.is_finite() && v.abs() <= 0.75 * tall), "{}: {ring:?}", b.name);
            }
            // asked again for the same brush: the same (kept)
            assert_eq!(s.brush_outline(), loops);
        }
    }
    assert!(round > 0 && shaped > 0, "round {round}, shaped {shaped}");
}

#[test]
fn an_old_brush_file_without_grain_depth_has_its_grain_at_full_depth_moving() {
    use procraft_engine::plist::Value;
    // a file from before grain depth and behaviour were kept: a named grain, its movement only
    let file = |extra: Vec<(&str, Value)>| {
        let mut b = procraft_engine::keyed::Builder::new();
        let root = b.reserve();
        let name = b.string("Old Canvas");
        let grain = b.string("Canvas.jpg");
        let mut fields = vec![("name", name), ("bundledGrainPath", grain), ("textureMovement", Value::Real(0.4))];
        fields.extend(extra);
        b.fill(&root, &["SilicaBrush", "ValkyrieBrush", "NSObject"], fields);
        let mut zip = procraft_engine::zip::ZipWriter::new();
        zip.add("Brush.archive", &b.finish(root));
        zip.finish()
    };
    let s = CanvasSession::new(64, 64);
    let grain_of = |bytes: Vec<u8>| {
        let set = s.import_brushes(bytes).unwrap() as usize;
        let st = s.shared.lock();
        let g = &st.library[set].brushes[0].grain;
        (g.depth, g.mode)
    };
    use procraft_engine::brush::GrainMode;
    assert_eq!(grain_of(file(vec![])), (1.0, GrainMode::Moving));
    // a file that names them keeps its own
    let kept = grain_of(file(vec![("grainDepth", Value::Real(0.3)), ("textureApplication", Value::Int(1))]));
    assert!((kept.0 - 0.3).abs() < 1e-6 && kept.1 == GrainMode::Texturized, "{kept:?}");
}

#[test]
fn a_set_moves_with_its_brushes_and_the_tools_keep_theirs() {
    let s = CanvasSession::new(64, 64);
    let names = |s: &CanvasSession| s.brush_library().into_iter().map(|set| set.name).collect::<Vec<_>>();
    let before = names(&s);
    assert!(before.len() >= 4);
    // the paint brush in the second set, the eraser in the fourth
    assert!(s.select_brush(1, 2));
    let painting = s.current_brush().name;
    s.set_tool(ToolKind::Erase);
    assert!(s.select_brush(3, 0));
    let erasing = s.current_brush().name;
    // the first set down to the third place: the two under it move up one
    assert!(s.move_brush_set(0, 2));
    let after = names(&s);
    assert_eq!(after[..4], [before[1].clone(), before[2].clone(), before[0].clone(), before[3].clone()]);
    let c = s.current_brush();
    assert_eq!((c.set, c.index, c.name), (3, 0, erasing));
    s.set_tool(ToolKind::Paint);
    let c = s.current_brush();
    assert_eq!((c.set, c.index, c.name), (0, 2, painting.clone()));
    // the set a tool is on, up to the top, and past the end: to the bottom
    assert!(s.move_brush_set(0, 0));
    assert_eq!(names(&s), after);
    let last = after.len() as u32 - 1;
    let brushes: Vec<String> = s.brush_library()[0].brushes.iter().map(|b| b.name.clone()).collect();
    assert!(s.move_brush_set(0, 99));
    assert_eq!(names(&s)[last as usize], after[0]);
    let c = s.current_brush();
    assert_eq!((c.set, c.name), (last, painting));
    // its brushes went with it; nothing there, nothing moves
    let moved: Vec<String> = s.brush_library()[last as usize].brushes.iter().map(|b| b.name.clone()).collect();
    assert_eq!(moved, brushes);
    assert!(!s.move_brush_set(99, 0));
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
    // snapping on before the transform begins: the same
    s.transform_begin().unwrap();
    s.transform_grab(TransformHandle::Move, 30.0, 80.0);
    let moved = s.transform_drag(30.0 + (middle - 30.0) + 2.5, 80.0).unwrap();
    let now = (moved.corners[0] + moved.corners[2]) / 2.0;
    assert!((now - middle).abs() < 0.01, "{now} vs {middle}");
    s.transform_end(false);
    // and off: the box goes where it is dragged
    s.transform_set_guides(GuideSettings { magnetics: false, snapping: false, distance: 3.0, velocity: 10.0 });
    s.transform_begin().unwrap();
    s.transform_grab(TransformHandle::Move, 30.0, 80.0);
    let moved = s.transform_drag(30.0 + (middle - 30.0) + 2.5, 80.0).unwrap();
    let now = (moved.corners[0] + moved.corners[2]) / 2.0;
    assert!((now - middle - 2.5).abs() < 0.01, "{now} vs {middle} + 2.5");
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
    // the grains' library starts with the Blank, which isn't a shipped grain
    let library = crate::studio::source_library(true);
    assert_eq!(library.len(), procraft_engine::textures::grains().len() + 1);
    assert!(library.first().is_some_and(|b| b.name == "Blank" && b.pixels.iter().all(|&v| v == 255)));
}

#[test]
fn a_picture_becomes_a_source_fitted_on_black() {
    let n = crate::studio::source_size() as usize;
    // twice as wide as tall: white on the left, see-through white on the right
    let rgba: Vec<u8> =
        (0..8 * 4).flat_map(|i| if i % 8 < 4 { [255, 255, 255, 255] } else { [255, 255, 255, 0] }).collect();
    let source = crate::studio::source_from_picture(rgba, 8, 4);
    assert_eq!(source.len(), n * n);
    let at = |x: usize, y: usize| source.get(y * n + x).copied().unwrap();
    // black above and below it, white where it is opaque, black where it shows through
    assert_eq!(at(n / 4, 2), 0);
    assert_eq!(at(n / 4, n - 3), 0);
    assert_eq!(at(n / 4, n / 2), 255);
    assert_eq!(at(n * 3 / 4, n / 2), 0);
    // pixels of the wrong size: nothing
    assert!(crate::studio::source_from_picture(vec![0; 7], 2, 1).is_empty());
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
    let (pending, w, h) = {
        let st = back.shared.lock();
        (st.pending_pixels.clone(), st.doc.width(), st.doc.height())
    };
    let pending = crate::project::opened_pixels(pending, w, h);
    assert!(!pending.is_empty());
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
    // the thin glass goes with them, and with them being cleared
    s.set_glass(vec![1.0, 2.0, 3.0, 4.0, 5.0, 1.0], vec![6.0, 7.0, 8.0, 9.0, 2.0, 0.5, 1.0]);
    assert_eq!(s.shared.lock().backdrops, vec![[1.0, 2.0, 3.0, 4.0, 5.0, 1.0]]);
    assert_eq!(s.shared.lock().backdrop_windows, vec![[6.0, 7.0, 8.0, 9.0, 2.0, 0.5]]);
    s.set_backdrops(Vec::new());
    assert!(s.shared.lock().backdrop_windows.is_empty());
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
fn an_export_is_named_after_its_artwork() {
    use crate::project::export_file_name;
    assert_eq!(export_file_name(" Sky / Sea: 2 ".to_owned(), false), "Sky  Sea 2");
    assert_eq!(export_file_name("?!".to_owned(), false), "Untitled Artwork");
    assert_eq!(export_file_name(String::new(), true), "Untitled_Artwork");
    assert_eq!(export_file_name("Night walk".to_owned(), true), "Night_walk");
}

#[test]
fn an_artwork_saved_while_its_canvas_is_being_made_keeps_its_picture() {
    let Ok(gpu) = Gpu::headless() else { return };
    // a picture brought in, its GPU given but its canvas not made yet (as on another thread)
    let red: Vec<u8> = [255u8, 0, 0, 255].repeat(64 * 64);
    let s = CanvasSession::from_image(crate::layers::ThumbnailImage { width: 64, height: 64, rgba: red }, "Red".into());
    s.shared.lock().gpu = Some(gpu);
    let dir = std::env::temp_dir().join(format!("procraft-unmade-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    let reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(dir.join("thumbnail.png")).unwrap()));
    let mut reader = reader.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    assert_eq!(&buf[..4], &[255, 0, 0, 255]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_jpeg_coded_in_stretches_reads_back_as_its_picture() {
    // tall enough to be coded in several stretches between restart markers
    let (w, h) = (96u32, 640u32);
    let rgb: Vec<u8> = (0..w * h).flat_map(|i| [(i % w * 2) as u8, (i / w % 256) as u8, 128]).collect();
    let jpg = procraft_engine::jpeg::encode(&rgb, w, h, 95);
    assert!(jpg.windows(2).any(|m| m == [0xFF, 0xDD]), "no restart interval");
    let back = image::load_from_memory_with_format(&jpg, image::ImageFormat::Jpeg).unwrap().to_rgb8();
    assert_eq!(back.dimensions(), (w, h));
    // within what JPEG at 95 keeps, everywhere (a stretch decoded wrongly would be far off)
    let worst = back.as_raw().iter().zip(&rgb).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    assert!(worst < 24, "{worst}");
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
    // a JPEG carries it too, the see-through canvas laid onto white
    let path = std::env::temp_dir().join(format!("procraft-p3-{}.jpg", std::process::id()));
    s.export_jpeg(path.to_string_lossy().into_owned()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xD8]);
    let at = bytes.windows(12).position(|w| w == b"ICC_PROFILE ").expect("an embedded profile");
    assert_eq!(&bytes[at + 14..at + 14 + icc.len()], &icc[..]);
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
fn changing_the_profile_keeps_the_numbers_and_undoes() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert!(!s.change_profile("sRGB IEC61966-2.1".to_owned()));
    assert!(!s.change_profile("Adobe RGB".to_owned()));
    assert!(s.change_profile("Display P3".to_owned()));
    assert_eq!(s.profile(), "Display P3");
    // as measured, the numbers kept: the red is P3's red now
    assert_eq!(pixel(&s, 20, 50), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 80, 80), [255, 255, 255, 255]);
    // one undo step puts the profile back
    assert_eq!(s.undo().as_deref(), Some("Set color profile \"Display P3\""));
    assert_eq!(s.profile(), "sRGB IEC61966-2.1");
    assert_eq!(pixel(&s, 20, 50), [255, 0, 0, 255]);
    s.redo();
    assert_eq!(s.profile(), "Display P3");
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
    let dir = std::env::temp_dir().join(format!("procraft-idle-save-{}", std::process::id()));
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
    // a layer whose file has gone is written again, though it hasn't changed
    std::fs::remove_file(&file).unwrap();
    assert!(s.save_if_idle(path.clone()).unwrap());
    assert!(file.exists());
    // and one painted on is
    let written = std::fs::metadata(&file).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert!(s.select_layer(first));
    s.stroke_begin(PointerKind::Finger, vec![sample(5.0, 5.0, 0.0)]);
    s.stroke_end(vec![]);
    assert!(s.save_if_idle(path.clone()).unwrap());
    assert_ne!(std::fs::metadata(&file).unwrap().modified().unwrap(), written);
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
    assert_eq!(s.undo().as_deref(), Some("Toggle visibility"));
    assert!(s.set_layer_alpha_lock(id, true));
    assert_eq!(s.undo().as_deref(), Some("Alpha Lock on"));
    assert!(s.set_layer_reference(id, true));
    assert_eq!(s.undo().as_deref(), Some("reference layer on"));
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
fn a_brush_files_sliders_start_where_it_left_them_and_go_back_out() {
    use procraft_engine::brush::Brush;
    use procraft_engine::brushfile::{self, Pictures};
    let Some(s) = session(100, 100) else { return };
    let png = crate::project::rgba_png(1, 1, &[255; 4]);
    let pictures = Pictures {
        shape_png: &png,
        grain_png: None,
        metallic_png: None,
        roughness_png: None,
        thumbnail_png: &png,
        author_png: None,
        signature_png: None,
        secondary: None,
    };
    let sliders = [Some([0.2, 0.6]), None, Some([0.7, 0.4])];
    let file = brushfile::write(&Brush { name: "Sliders".to_owned(), sliders, ..Brush::round_pen() }, "", &pictures);
    let set = s.import_brushes(file).unwrap();
    // kept with the library until it is picked
    let t = CanvasSession::new(64, 64);
    assert!(t.load_brush_library(s.saved_brush_library()));
    let kept = t.shared.lock().library[set as usize].brushes[0].sliders;
    let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
    let same = |a: [Option<[f32; 2]>; 3], b: [Option<[f32; 2]>; 3]| {
        a.iter().zip(&b).all(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => near(a[0], b[0]) && near(a[1], b[1]),
            (a, b) => a.is_none() && b.is_none(),
        })
    };
    // (written out, smudging's where it starts: its preview size, fully opaque)
    assert!(same([kept[0], None, kept[2]], sliders) && kept[1].is_some_and(|s| s[1] == 1.0), "{kept:?}");
    // each tool's sliders start where the file left them
    for (tool, size, opacity) in [(ToolKind::Paint, 0.2, 0.6), (ToolKind::Erase, 0.7, 0.4)] {
        s.set_tool(tool);
        assert!(s.select_brush(set, 0));
        let info = s.info();
        assert!(near(info.size, size) && near(info.opacity, opacity), "{tool:?}: {} {}", info.size, info.opacity);
    }
    // moved, they are written out where they stand; smudging's where it starts (fully opaque)
    s.set_brush_size(0.5);
    let out = brushfile::read(&s.brush_as_file(set, 0).unwrap()).unwrap();
    let [paint, smudge, erase] = out.sliders.map(Option::unwrap);
    assert!(near(paint[0], 0.2) && near(paint[1], 0.6) && near(smudge[1], 1.0), "{:?}", out.sliders);
    assert!(near(erase[0], 0.5) && near(erase[1], 0.4), "{:?}", out.sliders);
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
fn rows_pinched_together_merge_with_all_between() {
    let Some(s) = session(100, 100) else { return };
    let dot = |x: f32, y: f32, rgb: [f32; 3]| {
        s.set_color(rgb[0], rgb[1], rgb[2]);
        s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
        s.stroke_end(vec![]);
    };
    s.set_brush_size(0.6);
    let bottom = s.layers()[0].id;
    let red = s.add_layer();
    dot(30.0, 30.0, [1.0, 0.0, 0.0]);
    let middle = s.add_layer();
    let blue = s.add_layer();
    dot(70.0, 70.0, [0.0, 0.0, 1.0]);
    let top = s.add_layer();
    // the top row's finger is on blue, the lower one's on red: those and the middle merge
    assert!(s.select_layer(top));
    assert!(s.merge_between(blue, red));
    let ids: Vec<u64> = s.layers().iter().map(|l| l.id).collect();
    assert_eq!(ids, [top, red, bottom]);
    assert_eq!(s.chosen_layers(), [red]);
    assert_eq!(pixel(&s, 70, 70), [0, 0, 255, 255]);
    assert_eq!(pixel(&s, 30, 30), [255, 0, 0, 255]);
    // one step, and undone the layers come back as they were chosen
    assert_eq!(s.undo().as_deref(), Some("merge layers"));
    let ids: Vec<u64> = s.layers().iter().map(|l| l.id).collect();
    assert_eq!(ids, [top, blue, middle, red, bottom]);
    assert_eq!(s.chosen_layers(), [top]);
    // a locked one among them: nothing merges, nothing chosen changes
    assert!(s.set_layer_locked(middle, true));
    assert!(!s.merge_between(red, blue));
    assert_eq!(s.layers().len(), 5);
    assert_eq!(s.chosen_layers(), [top]);
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

/// The rendering modes build up within a stroke as measured on a drawing pad: a hard
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
fn a_zip_of_brushes_is_told_from_other_zips() {
    let s = CanvasSession::new(64, 64);
    assert!(crate::brush_files::is_brush_zip(s.brush_as_file(0, 0).unwrap()));
    let mut other = procraft_engine::zip::ZipWriter::new();
    other.add("model.obj", b"v 0 0 0");
    assert!(!crate::brush_files::is_brush_zip(other.finish()));
    assert!(!crate::brush_files::is_brush_zip(b"not a zip".to_vec()));
}

#[test]
fn kept_brush_choices_with_entries_it_cant_read_keep_the_rest() {
    let s = CanvasSession::new(64, 64);
    // the eraser's place and a set's choice unreadable, a slider cut short
    let json = r#"{"tools":[[0,3],"bad",[0,1]],"sets":[[0,2],"x"],"sliders":[[0,0,3,0.25,0.5],[9]]}"#;
    assert!(s.load_brush_choices(json.to_owned()));
    let on = |tool: ToolKind| {
        s.set_tool(tool);
        (s.current_brush().set, s.current_brush().index)
    };
    assert_eq!(on(ToolKind::Paint), (0, 3));
    assert_eq!(on(ToolKind::Smudge), (0, 1));
}

#[test]
fn the_brushes_are_kept_in_their_folder_and_an_earlier_version_s_taken_in() {
    let dir = std::env::temp_dir().join(format!("procraft-brushes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let d = dir.to_string_lossy().into_owned();
    // nothing kept: the library as it ships, what shipped noted
    let s = CanvasSession::new(64, 64);
    assert!(!s.open_brushes(d.clone()));
    assert!(dir.join("seen.txt").exists());
    // a brush chosen and kept, alone, then with the library: a new session opens on it
    assert!(s.select_brush(0, 3));
    assert!(s.keep_brush_choices(d.clone()));
    assert!(!dir.join("library.json").exists());
    assert!(s.keep_brushes(d.clone()));
    let t = CanvasSession::new(64, 64);
    assert!(t.open_brushes(d.clone()));
    assert_eq!((t.current_brush().set, t.current_brush().index), (0, 3));
    // an earlier version's choices and seen counts go in only where the folder has none
    let other = r#"{"tools":[[0,1],[0,0],[0,0]],"sets":[],"sliders":[]}"#.to_owned();
    assert!(crate::brush_library::adopt_kept_brushes(d.clone(), Some("1,1".to_owned()), Some(other.clone())));
    assert_eq!(std::fs::read_to_string(dir.join("choices.json")).unwrap(), t.saved_brush_choices());
    std::fs::remove_file(dir.join("choices.json")).unwrap();
    assert!(crate::brush_library::adopt_kept_brushes(d.clone(), None, Some(other)));
    let u = CanvasSession::new(64, 64);
    assert!(u.open_brushes(d));
    assert_eq!((u.current_brush().set, u.current_brush().index), (0, 1));
    let _ = std::fs::remove_dir_all(&dir);
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
            secondary: None,
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
            secondary: None,
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
fn keys_a_brush_file_has_that_nothing_draws_yet_are_kept_and_written_again() {
    use procraft_engine::brushfile;
    use procraft_engine::document::BlendMode;
    use procraft_engine::keyed::Builder;
    use procraft_engine::plist::Value;
    use procraft_engine::zip::ZipWriter;
    let mut b = Builder::new();
    let root = b.reserve();
    let name = b.string("Carried");
    b.fill(
        &root,
        &["SilicaBrush", "ValkyrieBrush", "NSObject"],
        vec![
            ("name", name),
            ("dynamicsPressureShapeRoundness", Value::Real(0.4)),
            ("maxPressureSizeClamped", Value::Bool(true)),
            ("attackRoll", Value::Bool(true)),
            // Multiply, as brush files number it
            ("dualBlendModeExtended", Value::Int(1)),
        ],
    );
    let mut zip = ZipWriter::new();
    zip.add("Brush.archive", &b.finish(root));
    let s = CanvasSession::new(64, 64);
    let set = s.import_brushes(zip.finish()).unwrap();
    let expected = [
        ("pressure_roundness_max", 0.4),
        ("pressure_size_clamped", 1.0),
        ("attack_roll", 1.0),
        ("dual_blend", BlendMode::Multiply.id() as f32),
    ];
    {
        let st = s.shared.lock();
        let brush = &st.library[set as usize].brushes[0];
        for (key, v) in expected {
            assert_eq!(brush.setting(key), v, "{key}");
        }
    }
    // kept with the library, and written out again from it
    let t = CanvasSession::new(64, 64);
    assert!(t.load_brush_library(s.saved_brush_library()));
    let set = t.brush_library().iter().position(|x| x.brushes.iter().any(|b| b.name == "Carried")).unwrap() as u32;
    let out = brushfile::read(&t.brush_as_file(set, 0).unwrap()).unwrap();
    for (key, v) in expected {
        assert_eq!(out.settings.iter().rev().find(|(k, _)| k == key).map(|(_, v)| *v), Some(v), "{key}");
    }
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
    // off the canvas nothing is poured, and a dragged colour doesn't rest there
    assert!(!s.pour_lands(-5.0, 50.0));
    assert!(!s.pour_begin(-5.0, 50.0, 1.0, 0.0, 0.0, 0.5));
    assert!(s.pour_lands(20.0, 50.0));
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
    s.pour_end(true, None);
    assert_eq!(pixel(&s, 20, 50), [255, 0, 0, 255]);
    // one undo takes the pour back; the line stays
    s.undo();
    assert_eq!(pixel(&s, 20, 50), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 50, 50), [0, 0, 0, 255]);
    // a pour taken back leaves nothing to undo for it
    assert!(s.pour_begin(80.0, 50.0, 0.0, 0.0, 1.0, 0.5));
    s.pour_end(false, None);
    assert_eq!(pixel(&s, 80, 50), [255, 255, 255, 255]);
    // a locked layer takes none
    assert!(s.set_layer_locked(layer, true));
    assert!(!s.pour_begin(20.0, 50.0, 1.0, 0.0, 0.0, 0.5));
}

#[test]
fn a_dragged_threshold_leaves_the_layer_as_a_pour_made_at_it() {
    let Some(s) = session(120, 120) else { return };
    s.set_brush_size(0.6);
    // soft-edged lines across: their edges near the seed's colour by every amount
    for (x0, y0, dx, dy) in [(-10.0, 30.0, 5.0, 1.5), (20.0, -10.0, 1.0, 5.0), (-10.0, 100.0, 5.0, -2.5)] {
        s.stroke_begin(PointerKind::Finger, vec![sample(x0, y0, 0.0)]);
        s.stroke_add((1..30).map(|i| sample(x0 + i as f32 * dx, y0 + i as f32 * dy, i as f64 * 8.0)).collect());
        s.stroke_end(vec![]);
    }
    let id = s.layers().iter().find(|l| l.current).map(|l| l.id).unwrap();
    let layer_now = |s: &CanvasSession| {
        let _ = pixel(s, 0, 0);
        s.copy_layer_contents(id).map(|c| c.rgba).unwrap()
    };
    // the threshold dragged up and down over one pour, then each made afresh: the same layer
    let thresholds = [0.1, 0.35, 0.2, 0.6, 0.05, 0.9, 0.45, 0.45, 1.0, 0.0];
    assert!(s.pour_begin(70.0, 60.0, 0.9, 0.2, 0.1, 0.0));
    let dragged: Vec<Vec<u8>> = thresholds
        .iter()
        .map(|&t| {
            s.pour_threshold(t);
            layer_now(&s)
        })
        .collect();
    s.pour_end(false, None);
    assert!(dragged.windows(2).filter(|w| w[0] != w[1]).count() >= 6);
    for (t, layer) in thresholds.iter().zip(dragged) {
        assert!(s.pour_begin(70.0, 60.0, 0.9, 0.2, 0.1, *t));
        assert!(layer_now(&s) == layer, "{t}");
        s.pour_end(false, None);
    }
}

#[test]
fn a_fill_runs_through_a_gap_in_a_wall() {
    let Some(s) = session(120, 120) else { return };
    s.set_brush_size(0.1);
    // a wall down the middle, open for a short way half down
    for (y0, y1) in [(-10.0, 50.0), (70.0, 130.0)] {
        s.stroke_begin(PointerKind::Finger, vec![sample(60.0, y0, 0.0)]);
        s.stroke_add((1..20).map(|i| sample(60.0, y0 + (y1 - y0) * i as f32 / 20.0, i as f64 * 8.0)).collect());
        s.stroke_end(vec![]);
    }
    let red = |s: &CanvasSession, x, y| {
        let p = pixel(s, x, y);
        p[0] > 200 && p[1] < 100
    };
    assert!(s.pour_begin(30.0, 60.0, 0.9, 0.2, 0.1, 0.3));
    s.pour_end(true, None);
    assert!(red(&s, 30, 20) && red(&s, 100, 60) && red(&s, 100, 20));
    s.undo();
    assert!(!red(&s, 30, 20));
}

#[test]
fn crop_and_resize_takes_typed_sizes_and_crops_on_done() {
    let Some(s) = session(100, 100) else { return };
    s.crop_begin();
    let c = s.crop_shown().unwrap();
    assert_eq!((c.x, c.y, c.width, c.height, c.size_width, c.size_height), (0.0, 0.0, 100.0, 100.0, 100, 100));
    assert_eq!(c.corners.len(), 8);
    // a typed width sizes the rectangle; with the aspect locked the height follows
    s.crop_set_aspect_locked(true);
    s.crop_set_size(Some(50), None);
    let c = s.crop_shown().unwrap();
    assert_eq!((c.width, c.height, c.size_width, c.size_height), (50.0, 50.0, 50, 50));
    // resampling: a typed size is what the rectangle becomes, the rectangle stays
    s.crop_set_resample(true);
    s.crop_set_size(None, Some(200));
    let c = s.crop_shown().unwrap();
    assert_eq!((c.width, c.height, c.size_width, c.size_height), (50.0, 50.0, 200, 200));
    s.crop_set_dpi(300.0);
    assert!(s.crop_end(true));
    assert!(s.crop_shown().is_none());
    let info = s.info();
    assert_eq!((info.width, info.height), (200, 200));
    assert!((info.dpi - 300.0).abs() < 0.01);
    // left as it was, or cancelled: nothing changes (the aspect lock is kept from before)
    s.crop_begin();
    assert!(s.crop_shown().unwrap().aspect_locked);
    assert!(!s.crop_end(true));
    s.crop_begin();
    s.crop_set_size(Some(10), None);
    assert!(!s.crop_end(false));
    assert_eq!(s.info().width, 200);
}

#[test]
fn undo_takes_back_recolor_s_open_fill_first_and_redo_makes_it_again() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, -10.0, 0.0)]);
    s.stroke_add((1..30).map(|i| sample(50.0, -10.0 + i as f32 * 4.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![sample(50.0, 110.0, 250.0)]);
    // the left half filled and kept by a tap on the right, which fills there
    s.recolor_start(20.0, 50.0, 1.0, 0.0, 0.0, 0.0);
    s.recolor_tap(80.0, 50.0);
    assert_eq!((pixel(&s, 20, 50), pixel(&s, 80, 50)), ([255, 0, 0, 255], [255, 0, 0, 255]));
    // the open fill goes first, then the kept one
    assert_eq!(s.undo().as_deref(), Some("Recolor"));
    assert_eq!((pixel(&s, 20, 50), pixel(&s, 80, 50)), ([255, 0, 0, 255], [255, 255, 255, 255]));
    assert_eq!(s.undo().as_deref(), Some("Recolor"));
    assert_eq!(pixel(&s, 20, 50), [255, 255, 255, 255]);
    assert!(s.recolor_on());
    // with a fill open again, a redo brings the kept one back and fills again over it
    s.recolor_again();
    assert_eq!(pixel(&s, 80, 50), [255, 0, 0, 255]);
    assert_eq!(s.redo().as_deref(), Some("Recolor"));
    assert_eq!((pixel(&s, 20, 50), pixel(&s, 80, 50)), ([255, 0, 0, 255], [255, 0, 0, 255]));
    s.recolor_end();
    assert!(s.info().can_undo);
}

#[test]
fn recolor_follows_its_point_and_colour_and_is_a_named_step() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.stroke_begin(PointerKind::Finger, vec![sample(50.0, -10.0, 0.0)]);
    s.stroke_add((1..30).map(|i| sample(50.0, -10.0 + i as f32 * 4.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![sample(50.0, 110.0, 250.0)]);
    // nothing open, nothing to move
    assert!(!s.pour_move(80.0, 50.0));
    assert!(s.pour_begin(20.0, 50.0, 1.0, 0.0, 0.0, 0.0));
    assert_eq!(pixel(&s, 20, 50), [255, 0, 0, 255]);
    // moved across the line: the left half as it was, the right half filled
    assert!(s.pour_move(80.0, 50.0));
    assert_eq!(pixel(&s, 20, 50), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 80, 50), [255, 0, 0, 255]);
    // a new colour refills
    s.pour_colour(0.0, 0.0, 1.0);
    assert_eq!(pixel(&s, 80, 50), [0, 0, 255, 255]);
    // moved within what it fills it stays; onto the line it fills the line alone
    assert!(s.pour_move(90.0, 20.0));
    assert_eq!((pixel(&s, 20, 50), pixel(&s, 80, 50)), ([255, 255, 255, 255], [0, 0, 255, 255]));
    assert!(s.pour_move(50.0, 50.0));
    assert_eq!((pixel(&s, 50, 50), pixel(&s, 80, 50)), ([0, 0, 255, 255], [255, 255, 255, 255]));
    assert!(s.pour_move(80.0, 50.0));
    assert_eq!(pixel(&s, 80, 50), [0, 0, 255, 255]);
    // off the canvas nothing is filled, back on it is again
    assert!(s.pour_move(-5.0, 50.0));
    assert_eq!(pixel(&s, 80, 50), [255, 255, 255, 255]);
    assert!(s.pour_move(80.0, 50.0));
    s.pour_end(true, Some("Recolor".to_owned()));
    assert_eq!(pixel(&s, 80, 50), [0, 0, 255, 255]);
    assert_eq!(s.undo().as_deref(), Some("Recolor"));
    assert_eq!(pixel(&s, 80, 50), [255, 255, 255, 255]);
    s.redo();
    // ended off the canvas, it makes no step
    assert!(s.pour_begin(20.0, 50.0, 0.0, 1.0, 0.0, 0.0));
    assert!(s.pour_move(-5.0, 50.0));
    s.pour_end(true, Some("Recolor".to_owned()));
    assert_eq!(pixel(&s, 20, 50), [255, 255, 255, 255]);
    assert_eq!(s.undo().as_deref(), Some("Recolor"));
    assert_eq!(pixel(&s, 80, 50), [255, 255, 255, 255]);
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
    assert_eq!(names, ["Ink"]);
    // the bottom layer, all one colour, is the canvas's background colour
    let back = s.background();
    assert!(back.visible && back.r == 0.0 && (back.g - 200.0 / 255.0).abs() < 1e-6 && back.b == 0.0);
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

#[test]
fn a_group_shown_alone_shows_its_layers_and_opens_whole_once_hidden() {
    let Some(s) = session(100, 100) else { return };
    s.shared.lock().canvas.as_mut().unwrap().set_cache_composites(true);
    s.set_brush_size(0.4);
    let dot = |x: f32| {
        s.stroke_begin(PointerKind::Finger, vec![sample(x, 50.0, 0.0)]);
        s.stroke_end(vec![]);
    };
    let black = [0, 0, 0, 255];
    let white = [255, 255, 255, 255];
    dot(10.0);
    let a = s.add_layer();
    dot(40.0);
    s.add_layer();
    dot(70.0);
    // the top two grouped, then the group shown alone
    assert!(s.pick_layer(a));
    let group = s.group_picked();
    assert_ne!(group, 0);
    assert!(s.set_solo(Some(group)));
    assert_eq!([pixel(&s, 10, 50), pixel(&s, 40, 50), pixel(&s, 70, 50)], [white, black, black]);
    // hidden as well, it leaves only the background; saved so, it opens with every layer shown
    // but the group's
    assert!(s.set_layer_visible(group, false));
    assert_eq!([pixel(&s, 10, 50), pixel(&s, 40, 50), pixel(&s, 70, 50)], [white, white, white]);
    let gpu = s.shared.lock().gpu.clone().unwrap();
    let dir = std::env::temp_dir().join(format!("procraft-solo-group-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    let opened = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    opened.use_gpu(gpu);
    {
        let mut st = opened.shared.lock();
        st.window_size = (100, 100);
        st.view = Some(View::fit((100, 100), (100, 100)));
        st.ensure_canvas();
        st.canvas.as_mut().unwrap().set_cache_composites(true);
    }
    assert_eq!(opened.solo(), None);
    assert_eq!(opened.layers().len(), s.layers().len());
    assert_eq!([pixel(&opened, 10, 50), pixel(&opened, 40, 50), pixel(&opened, 70, 50)], [black, white, white]);
    // and a new layer shows
    let bottom = opened.layers().last().unwrap().id;
    assert!(opened.select_layer(bottom));
    opened.set_brush_size(0.4);
    assert_ne!(opened.add_layer(), 0);
    opened.stroke_begin(PointerKind::Finger, vec![sample(90.0, 50.0, 0.0)]);
    opened.stroke_end(vec![]);
    assert_eq!(pixel(&opened, 90, 50), black);
}

/// The artwork saved and opened again in a new session, as after a restart (same GPU and window).
fn reopened(s: &CanvasSession, name: &str) -> std::sync::Arc<CanvasSession> {
    let gpu = s.shared.lock().gpu.clone().unwrap();
    let side = s.shared.lock().window_size;
    let dir = std::env::temp_dir().join(format!("procraft-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    let opened = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    opened.use_gpu(gpu);
    {
        let mut st = opened.shared.lock();
        let canvas = (st.doc.width(), st.doc.height());
        st.window_size = side;
        st.view = Some(View::fit(canvas, side));
        st.ensure_canvas();
    }
    opened
}

#[test]
fn a_layer_that_doesnt_show_takes_no_paint_and_says_why_until_shown() {
    use crate::layers::Blocked;
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    let dot = |s: &CanvasSession, x: f32| {
        s.stroke_begin(PointerKind::Finger, vec![sample(x, 50.0, 0.0)]);
        s.stroke_end(vec![]);
    };
    let black = [0, 0, 0, 255];
    let white = [255, 255, 255, 255];
    dot(&s, 20.0);
    let first = s.layers()[0].id;
    // the painted layer shown alone, then a new layer: it is left out of the picture
    assert!(s.set_solo(Some(first)));
    let top = s.add_layer();
    assert_eq!(s.why_blocked(), Some(Blocked::Hidden));
    // still so once saved and opened again, as after a restart: strokes on it say why rather
    // than going in unseen
    let s = reopened(&s, "soloed-out");
    s.set_brush_size(0.4);
    assert_eq!(s.why_blocked(), Some(Blocked::Hidden));
    dot(&s, 80.0);
    // Show ticks its own box only (ticked already: nothing), so solo stays and so does the alert
    assert_eq!(s.show_layer(top), None);
    assert_eq!((s.solo(), s.why_blocked()), (Some(first), Some(Blocked::Hidden)));
    // its box tapped ends solo (as in the panel); the stroke left nothing behind, the next paints
    assert!(s.tap_visibility(top).is_some());
    assert_eq!((s.solo(), s.why_blocked()), (None, None));
    assert_eq!([pixel(&s, 20, 50), pixel(&s, 80, 50)], [black, white]);
    dot(&s, 80.0);
    assert_eq!(pixel(&s, 80, 50), black);
    // hidden by its own box, Show ticks it: one step, told as a tap is
    assert!(s.set_layer_visible(top, false));
    assert_eq!(s.why_blocked(), Some(Blocked::Hidden));
    assert_eq!(s.show_layer(top).as_deref(), Some("Show layer"));
    assert_eq!(s.why_blocked(), None);
    s.undo();
    assert_eq!(s.why_blocked(), Some(Blocked::Hidden));
    s.redo();

    // in a hidden group: visible itself, but not seen
    let group = s.group_picked();
    assert_ne!(group, 0);
    assert!(s.set_layer_visible(group, false));
    assert!(s.layers().iter().any(|l| l.id == top && l.current && l.visible));
    let s = reopened(&s, "hidden-group");
    s.set_brush_size(0.4);
    assert_eq!(s.why_blocked(), Some(Blocked::Hidden));
    dot(&s, 50.0);
    // Show leaves the group hidden: the alert stays until the group's box is ticked
    assert_eq!(s.show_layer(top), None);
    assert_eq!(s.why_blocked(), Some(Blocked::Hidden));
    assert!(s.set_layer_visible(group, true));
    assert_eq!(s.why_blocked(), None);
    assert_eq!(pixel(&s, 50, 50), white);
    dot(&s, 50.0);
    assert_eq!(pixel(&s, 50, 50), black);
    s.undo();
    s.undo();
    assert_eq!(s.why_blocked(), Some(Blocked::Hidden), "the stroke and the group's box undone");
    // the group itself current: its own alert, whatever hides it
    assert!(s.select_layer(group));
    assert_eq!(s.why_blocked(), Some(Blocked::GroupHidden));
    assert!(s.set_layer_visible(group, true));
    assert!(s.set_solo(Some(first)));
    assert_eq!(s.why_blocked(), Some(Blocked::GroupHidden));
    assert_eq!(s.show_layer(group), None, "its box was ticked: solo stays");
    assert!(s.set_solo(None));
    assert_eq!(s.why_blocked(), None);
    assert!(s.set_layer_visible(group, false));
    assert!(s.select_layer(top));

    // a mask shown alone shows its layer with it, which takes paint
    let s = reopened(&s, "mask-alone");
    s.set_brush_size(0.4);
    assert!(s.set_layer_visible(group, true));
    let mask = s.add_mask(top).unwrap();
    assert!(s.set_solo(Some(mask)));
    assert!(s.select_layer(top));
    assert_eq!(s.why_blocked(), None);
    assert_eq!([pixel(&s, 20, 50), pixel(&s, 80, 50)], [white, black]);
    dot(&s, 35.0);
    assert_eq!(pixel(&s, 35, 50), black);
}

#[test]
fn a_stroke_or_a_fill_with_a_group_current_says_so() {
    use crate::layers::Blocked;
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    let dot = |s: &CanvasSession, x: f32| {
        s.stroke_begin(PointerKind::Finger, vec![sample(x, 50.0, 0.0)]);
        s.stroke_end(vec![]);
    };
    let black = [0, 0, 0, 255];
    let white = [255, 255, 255, 255];
    dot(&s, 20.0);
    let layer = s.layers()[0].id;
    // another layer beside the group, so that the one in it can go
    assert_ne!(s.add_layer(), 0);
    assert!(s.select_layer(layer));
    let group = s.group_picked();
    assert_ne!(group, 0);
    assert!(s.select_layer(group));
    // still current once saved and opened again, as after a restart
    let s = reopened(&s, "group-current");
    s.set_brush_size(0.4);
    assert_eq!(s.why_blocked(), None, "a group still takes other changes (it transforms whole)");
    assert_eq!(s.why_unpainted(), Some(Blocked::Group));
    dot(&s, 80.0);
    assert_eq!(pixel(&s, 80, 50), white, "no paint went anywhere");
    assert!(s.layers().iter().any(|l| l.id == group && l.current));
    // a layer in it takes paint
    assert!(s.select_layer(layer));
    assert_eq!(s.why_unpainted(), None);
    dot(&s, 80.0);
    assert_eq!(pixel(&s, 80, 50), black);
    // locked or hidden, those say so first; empty, a stroke still asks for a layer
    assert!(s.select_layer(group));
    assert!(s.set_layer_visible(group, false));
    assert_eq!(s.why_unpainted(), Some(Blocked::GroupHidden));
    assert!(s.set_layer_visible(group, true));
    assert!(s.delete_layer(layer));
    assert!(s.select_layer(group));
    assert_eq!(s.why_blocked(), Some(Blocked::EmptyGroup));
    assert_eq!(s.why_unpainted(), Some(Blocked::Group));
}

#[test]
fn a_group_with_a_hidden_layer_in_it_takes_no_change_and_says_so() {
    use crate::layers::{Blocked, HiddenBy};
    let Some(s) = session(64, 64) else { return };
    let lower = s.layers()[0].id;
    let upper = s.add_layer();
    assert!(s.pick_layer(lower));
    let group = s.group_picked();
    assert_ne!(group, 0);
    assert!(s.set_layer_visible(lower, false));
    assert!(s.select_layer(group));
    // the group's own box ticked: still hidden for a change, as a group, a stroke too
    assert_eq!(s.why_blocked(), Some(Blocked::GroupHidden));
    assert_eq!(s.why_unpainted(), Some(Blocked::GroupHidden));
    assert_eq!(s.hidden_by(), Some(HiddenBy::Inside));
    // the visible layer in it takes them
    assert!(s.select_layer(upper));
    assert_eq!((s.why_blocked(), s.hidden_by()), (None, None));
    assert!(s.set_layer_visible(lower, true));
    assert!(s.select_layer(group));
    assert_eq!(s.why_blocked(), None);
    assert_eq!(s.why_unpainted(), Some(Blocked::Group));
}

#[test]
fn a_documents_pages_come_in_as_page_layers() {
    let dir = std::env::temp_dir().join(format!("procraft-pages-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (w, h) = (6, 4);
    for (n, v) in [(1, 200u8), (2, 40u8)] {
        let rgba: Vec<u8> = std::iter::repeat_n([v, v, v, 255], (w * h) as usize).flatten().collect();
        crate::project::write_png(&dir.join(format!("page-{n}.png")), w, h, &rgba, &[]).unwrap();
    }
    let art = dir.join("art");
    let outcome = crate::project::import_pages(
        dir.to_string_lossy().into_owned(),
        2,
        w,
        h,
        300.0,
        art.to_string_lossy().into_owned(),
        "Comic".into(),
    );
    assert_eq!(outcome, crate::project::PagesOutcome::Imported);
    assert!(art.join("thumbnail.png").exists());
    let s = CanvasSession::open(art.to_string_lossy().into_owned()).unwrap();
    assert_eq!(s.name(), "Comic");
    let names: Vec<String> = s.layers().iter().map(|l| l.name.clone()).collect();
    // listed top first: the first page at the bottom
    assert_eq!(names, ["Page 2", "Page 1"]);
    assert!(s.animation_settings().is_some_and(|a| a.pages));
    // a page missing
    assert_eq!(
        crate::project::import_pages(
            dir.to_string_lossy().into_owned(),
            3,
            w,
            h,
            300.0,
            dir.join("bad").to_string_lossy().into_owned(),
            String::new()
        ),
        crate::project::PagesOutcome::Unreadable
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_photoshop_brush_files_tips_become_a_set() {
    // a version 2 file with one sampled 2 x 2 tip, its rows packed
    let mut body = Vec::new();
    body.extend_from_slice(&0u32.to_be_bytes());
    body.extend_from_slice(&40u16.to_be_bytes());
    let name: Vec<u16> = "Tip".encode_utf16().chain([0]).collect();
    body.extend_from_slice(&(name.len() as u32).to_be_bytes());
    for u in name {
        body.extend_from_slice(&u.to_be_bytes());
    }
    body.push(1);
    body.extend_from_slice(&[0; 8]);
    for v in [0u32, 0, 2, 2] {
        body.extend_from_slice(&v.to_be_bytes());
    }
    body.extend_from_slice(&8u16.to_be_bytes());
    body.push(1);
    body.extend_from_slice(&2u16.to_be_bytes());
    body.extend_from_slice(&2u16.to_be_bytes());
    body.extend_from_slice(&[0xFF, 255, 0xFF, 255]);
    let mut file = Vec::new();
    for v in [2u16, 1, 2] {
        file.extend_from_slice(&v.to_be_bytes());
    }
    file.extend_from_slice(&(body.len() as u32).to_be_bytes());
    file.extend_from_slice(&body);
    let s = CanvasSession::new(64, 64);
    assert_eq!(s.import_other_brushes(file, "Chalk".into()), Some(0));
    let library = s.brush_library();
    let set = library.first().unwrap();
    assert_eq!(set.name, "Chalk");
    assert_eq!(set.brushes.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["Tip"]);
    assert_eq!(s.import_other_brushes(b"nothing".to_vec(), String::new()), None);
}

/// Real brush files, when a folder of them is given (`BRUSH_CORPUS=folder`, never committed):
/// every brush file in it and below (ours, and Photoshop's, GIMP's, Krita's and Clip Studio's)
/// imports, each brush draws in its
/// preview, and a brush written out, read back and written out again comes out the same.
#[test]
fn a_corpus_of_real_brush_files_imports_draws_and_round_trips() {
    use procraft_engine::brushfile::{self, BrushFile};
    let Ok(root) = std::env::var("BRUSH_CORPUS") else { return };
    let extension =
        |p: &std::path::Path| p.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let mut files = Vec::new();
    let mut folders = vec![std::path::PathBuf::from(&root)];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if matches!(
                extension(&path).as_str(),
                "brush" | "brushset" | "abr" | "gbr" | "gih" | "vbr" | "kpp" | "bundle" | "sut"
            ) {
                files.push(path);
            }
        }
    }
    files.sort();
    assert!(!files.is_empty(), "no brush files in {root}");
    // the preview left out: drawn again each time
    let read = |b: &[u8]| brushfile::read(b).map(|f| BrushFile { thumbnail: None, ..f });
    let mut failures = Vec::new();
    let (mut brushes, mut grained) = (0, 0);
    for path in &files {
        let Some(s) = session(64, 64) else { return };
        let bytes = std::fs::read(path).unwrap();
        let kind = extension(path);
        let set = if kind == "brush" || kind == "brushset" {
            s.import_brushes(bytes.clone())
        } else {
            s.import_other_brushes(bytes.clone(), "Corpus".to_owned())
        };
        let Some(set) = set else {
            failures.push(format!("{}: not imported", path.display()));
            continue;
        };
        // a brush goes in at the top of its set; a set's brushes, or the tips, fill one of their own
        let count = if kind == "brush" { 1 } else { s.brush_library()[set as usize].brushes.len() as u32 };
        // the apps' way, read by the file's name off the window's thread, then added: the same brushes
        let Some(u) = session(64, 64) else { return };
        let name = path.file_name().and_then(|n| n.to_str()).map(str::to_owned);
        match crate::brush_files::read_brushes(bytes, name).and_then(|r| u.add_brushes_read(r, 0)) {
            None => failures.push(format!("{}: not read the apps' way", path.display())),
            Some(v) => {
                let theirs = if kind == "brush" { 1 } else { u.brush_library()[v as usize].brushes.len() as u32 };
                if theirs != count {
                    failures.push(format!("{}: {theirs} brushes the apps' way, {count} imported", path.display()));
                }
                for i in 0..count.min(theirs) {
                    match (
                        s.brush_as_file(set, i).as_deref().and_then(read),
                        u.brush_as_file(v, i).as_deref().and_then(read),
                    ) {
                        (Some(a), Some(b)) if changes(&a, &b).is_empty() => {}
                        _ => failures.push(format!("{} #{}: not the same the apps' way", path.display(), i + 1)),
                    }
                }
            }
        }
        for i in 0..count {
            brushes += 1;
            if s.shared
                .lock()
                .library
                .get(set as usize)
                .and_then(|l| l.brushes.get(i as usize))
                .is_some_and(|b| b.grain.source.is_some())
            {
                grained += 1;
            }
            let at = format!("{} #{}", path.display(), i + 1);
            let drawn =
                s.brush_preview(set, i, 200, 60).is_some_and(|p| p.rgba.as_chunks::<4>().0.iter().any(|px| px[3] > 0));
            if !drawn {
                failures.push(format!("{at}: draws nothing"));
            }
            // to look at beside another app's: each preview as a picture, if asked for
            if let (Ok(out), Some(p)) = (std::env::var("BRUSH_CORPUS_PREVIEWS"), s.brush_preview(set, i, 600, 180)) {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
                let file = std::path::Path::new(&out).join(format!("{stem}-{:02}.png", i + 1));
                crate::project::write_png(&file, p.width, p.height, &p.rgba, &[]).unwrap();
            }
            let Some(once) = s.brush_as_file(set, i) else {
                failures.push(format!("{at}: not written"));
                continue;
            };
            let Some(t) = session(64, 64) else { return };
            let again = t.import_brushes(once.clone()).and_then(|back| t.brush_as_file(back, 0));
            match (read(&once), again.as_deref().and_then(read)) {
                (Some(a), Some(b)) => {
                    let changed = changes(&a, &b);
                    if !changed.is_empty() {
                        failures.push(format!("{at}: changed when written out again: {changed}"));
                    }
                }
                _ => failures.push(format!("{at}: written out unreadable")),
            }
        }
    }
    eprintln!("{} files, {brushes} brushes, {grained} with a grain of their own", files.len());
    assert!(failures.is_empty(), "{} failed:\n{}", failures.len(), failures.join("\n"));
}

/// Whether the sliders' places `a` came back as `b` (written out, those not given come back where
/// they start).
fn sliders_kept(a: &[Option<[f32; 2]>; 3], b: &[Option<[f32; 2]>; 3]) -> bool {
    a.iter().zip(b).all(|(a, b)| match (a, b) {
        (None, _) => true,
        (Some(a), Some(b)) => a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4),
        (Some(_), None) => false,
    })
}

/// What differs between two brush files (nothing: empty): the settings that changed, and the other
/// fields.
fn changes(a: &procraft_engine::brushfile::BrushFile, b: &procraft_engine::brushfile::BrushFile) -> String {
    let mut out = Vec::new();
    for (k, v) in &a.settings {
        match b.settings.iter().find(|(k2, _)| k2 == k) {
            // a float's last place may move: controls go through powers on their way
            Some((_, w)) if (v - w).abs() <= 1e-6 * v.abs().max(1.0) => {}
            Some((_, w)) => out.push(format!("{k} {v} to {w}")),
            None => out.push(format!("{k} gone")),
        }
    }
    for (k, _) in &b.settings {
        if !a.settings.iter().any(|(k2, _)| k2 == k) {
            out.push(format!("{k} added"));
        }
    }
    let fields: [(&str, bool); 15] = [
        ("name", a.name == b.name),
        ("author", a.author == b.author),
        ("max_size", a.max_size == b.max_size),
        ("min_size", a.min_size == b.min_size),
        ("shape_angle", a.shape_angle == b.shape_angle),
        ("pressure_curve", a.pressure_curve == b.pressure_curve),
        ("size_curve", a.size_curve == b.size_curve),
        ("opacity_curve", a.opacity_curve == b.opacity_curve),
        ("pressure_curves", a.pressure_curves == b.pressure_curves),
        ("shape", a.shape == b.shape),
        ("grain", a.grain == b.grain),
        ("metallic and roughness", a.metallic == b.metallic && a.roughness == b.roughness),
        ("sliders", sliders_kept(&a.sliders, &b.sliders)),
        ("notches", a.notches == b.notches),
        ("author pictures", a.author_picture == b.author_picture && a.signature == b.signature),
    ];
    out.extend(fields.iter().filter(|f| !f.1).map(|f| f.0.to_owned()));
    let order = |f: &procraft_engine::brushfile::BrushFile| f.settings.iter().map(|s| s.0.clone()).collect::<Vec<_>>();
    if order(a) != order(b) {
        out.push("settings' order".to_owned());
    }
    if out.iter().any(|o| o == "max_size" || o == "min_size") {
        out.push(format!("sizes {} {:?} to {} {:?}", a.max_size, a.min_size, b.max_size, b.min_size));
    }
    out.join(", ")
}

#[test]
fn a_gimp_brush_becomes_a_brush_with_its_spacing() {
    let s = CanvasSession::new(64, 64);
    // version 2: header, size, grey, the magic word, 40% spacing, the name, a 4 × 4 image
    let name = b"Charcoal\0";
    let mut file = Vec::new();
    for v in [28 + name.len() as u32, 2, 4, 4, 1] {
        file.extend_from_slice(&v.to_be_bytes());
    }
    file.extend_from_slice(b"GIMP");
    file.extend_from_slice(&40u32.to_be_bytes());
    file.extend_from_slice(name);
    file.extend((0..16).map(|i| (i * 16) as u8));
    let set = s.import_other_brushes(file, "Charcoal pack".to_owned()).unwrap();
    let library = s.brush_library();
    let set = &library[set as usize];
    assert_eq!(set.name, "Charcoal pack");
    assert_eq!(set.brushes.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), ["Charcoal"]);
}

#[test]
fn a_moving_gif_comes_in_as_held_frames() {
    let Some(s) = session(8, 8) else { return };
    // a 2 × 2 GIF of two frames, red for 0.2 s then blue for 0.1 s (no compression: each index a
    // code of 3 bits after a clear code)
    let frame = |index: u8, centiseconds: u8| {
        let mut f = vec![0x21, 0xF9, 4, 0, centiseconds, 0, 0, 0, 0x2C, 0, 0, 0, 0, 2, 0, 2, 0, 0, 2];
        // clear (4), the index twice, clear, twice more, end (5), 3 bits each, low bits first
        let codes = [4u32, index.into(), index.into(), 4, index.into(), index.into(), 5];
        let (mut data, mut acc, mut bits) = (Vec::new(), 0u32, 0);
        for c in codes {
            acc |= c << bits;
            bits += 3;
            while bits >= 8 {
                data.push(acc as u8);
                acc >>= 8;
                bits -= 8;
            }
        }
        data.push(acc as u8);
        f.push(data.len() as u8);
        f.extend(data);
        f.push(0);
        f
    };
    let mut gif = b"GIF89a".to_vec();
    gif.extend_from_slice(&[2, 0, 2, 0, 0x81, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0]);
    gif.extend(frame(0, 20));
    gif.extend(frame(1, 10));
    gif.push(0x3B);
    let before = s.shared.lock().doc.layers().len();
    let done = s.insert_gif_frames(gif, false).unwrap();
    assert_eq!(done, crate::clipboard::FramesInserted { added: 2, full: false });
    assert_eq!(s.shared.lock().doc.layers().len(), before + 2);
    // Animation Assist is on; at 15 frames a second, 0.2 s is three frames: held two more
    let animation = s.shared.lock().doc.animation.clone().unwrap();
    assert!(!animation.pages);
    let holds: Vec<u32> = animation.holds.iter().map(|h| h.1).collect();
    assert_eq!(holds, [2, 1]);
    // the top frame is blue, filling the canvas
    assert_eq!(pixel(&s, 4, 4), [0, 0, 255, 255]);
    // a still picture isn't taken
    assert_eq!(s.insert_gif_frames(b"GIF89a".to_vec(), false), None);
}

#[test]
fn a_krita_preset_becomes_a_round_brush_with_its_settings() {
    let s = CanvasSession::new(64, 64);
    // its texture: a 100 px picture, half white, kept as the base64 of its base64
    let px: Vec<u8> = (0..100 * 100).flat_map(|i| if i % 100 < 50 { [255; 4] } else { [0, 0, 0, 255] }).collect();
    let paper = procraft_engine::base64::encode(
        procraft_engine::base64::encode(&crate::project::rgba_png(100, 100, &px)).as_bytes(),
    );
    let xml = format!(
        r#"<Preset paintopid="paintbrush" name="Soft Round"><param name="PressureSize"><![CDATA[false]]></param><param name="brush_definition"><![CDATA[<Brush type="auto_brush" spacing="0.2" useAutoSpacing="0"><MaskGenerator type="circle" ratio="0.5" hfade="0.25" vfade="0.25"/></Brush>]]></param><param name="Texture/Pattern/Enabled"><![CDATA[true]]></param><param name="Texture/Pattern/Pattern"><![CDATA[{paper}]]></param><param name="Texture/Pattern/Scale"><![CDATA[0.5]]></param><param name="Texture/Pattern/Strength"><![CDATA[0.7]]></param></Preset>"#
    );
    let chunk = |kind: &[u8], data: &[u8]| {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&[0; 4]);
        out
    };
    let mut kpp = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    kpp.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]));
    let mut text = b"preset\0".to_vec();
    text.extend_from_slice(xml.as_bytes());
    kpp.extend(chunk(b"tEXt", &text));
    kpp.extend(chunk(b"IEND", &[]));
    let set = s.import_other_brushes(kpp, "Krita".to_owned()).unwrap() as usize;
    let st = s.shared.lock();
    let b = &st.library[set].brushes[0];
    assert_eq!(b.name, "Soft Round");
    assert!((b.spacing - 0.2).abs() < 1e-6 && (b.hardness - 0.25).abs() < 1e-6 && b.shape.roundness == 0.5);
    assert_eq!(b.pressure_size, 0.0);
    assert!(b.shape.source.is_none());
    // the texture as paper grain: 70 % strong, its 100 px tiles at half size
    assert!(b.grain.source.is_some());
    assert_eq!((b.grain.depth, b.grain.scale), (0.7, 50.0));
}

#[test]
fn a_kept_view_that_leaves_the_canvas_off_the_page_is_not_used() {
    use crate::project::shows_canvas;
    let (window, canvas) = ((2560, 1600), (2048, 2048));
    // fitted: shows
    let fitted = View::fit(canvas, window);
    assert!(shows_canvas(&fitted, window, canvas));
    // centred on a point far beyond the canvas: none of it shows
    let away = View::centred(procraft_engine::geom::Vec2::new(9000.0, -7000.0), 0.78, 0.0, window).unwrap();
    assert!(!shows_canvas(&away, window, canvas));
    // half off the right edge: still enough to work on
    let half = View::centred(procraft_engine::geom::Vec2::new(0.0, 1024.0), 0.78, 0.0, window).unwrap();
    assert!(shows_canvas(&half, window, canvas));
    // zoomed far out, small but whole on screen
    let tiny = View::centred(procraft_engine::geom::Vec2::new(1024.0, 1024.0), 0.01, 0.0, window).unwrap();
    assert!(shows_canvas(&tiny, window, canvas));
}

#[test]
fn the_clone_source_starts_in_the_middle_is_dragged_and_follows_the_brush() {
    use crate::adjust_menu::Adjustment;
    let Some(s) = session(100, 100) else { return };
    let near = |x: f32, y: f32| {
        let p = s.adjustment_shown().clone_puck.unwrap_or_default();
        assert!(p.len() == 2 && (p[0] - x).abs() < 0.01 && (p[1] - y).abs() < 0.01, "{p:?} not ({x}, {y})");
    };
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 50.0, 0.0)]);
    s.stroke_end(vec![]);
    assert!(s.adjustment_start(Adjustment::Clone));
    near(50.0, 50.0);
    // a touch on it drags it from where it was taken
    s.touch_down(1, PointerKind::Finger, vec![sample(60.0, 50.0, 0.0)]);
    s.touch_move(1, vec![sample(30.0, 40.0, 20.0)]);
    s.touch_up(1, vec![sample(30.0, 40.0, 30.0)]);
    near(20.0, 40.0);
    // a touch off it paints, the source keeping its distance from the brush
    s.touch_down(1, PointerKind::Finger, vec![sample(90.0, 90.0, 100.0)]);
    s.touch_move(1, vec![sample(95.0, 92.0, 120.0)]);
    s.touch_up(1, vec![sample(95.0, 92.0, 130.0)]);
    near(25.0, 42.0);
    // the next stroke keeps that distance, wherever it starts
    s.touch_down(1, PointerKind::Finger, vec![sample(95.0, 95.0, 200.0)]);
    s.touch_up(1, vec![sample(95.0, 95.0, 210.0)]);
    near(25.0, 45.0);
    // undo puts it back where it was before each stroke, redo where the stroke took it
    s.undo();
    near(25.0, 42.0);
    s.undo();
    near(20.0, 40.0);
    s.redo();
    near(25.0, 42.0);
    assert!(s.adjustment_end(true));
    assert_eq!(s.adjustment_shown().clone_puck, None);
}

/// Text as the tests draw it: Inter, black, at the canvas's top left.
fn text_info(text: &str) -> crate::TextInfo {
    crate::TextInfo {
        text: text.to_owned(),
        font: "inter".to_owned(),
        style: "Regular".to_owned(),
        size: 40.0,
        kerning: 0.0,
        tracking: 0.0,
        leading: 0.0,
        baseline: 0.0,
        opacity: 1.0,
        align: crate::TextAlignment::Left,
        underline: false,
        outline: false,
        vertical: false,
        all_caps: false,
        r: 0.0,
        g: 0.0,
        b: 0.0,
        x: 10.0,
        y: 10.0,
        width: 0.0,
    }
}

/// The inked pixels of premultiplied RGBA `px` (`w` across), and their bounds.
fn inked(px: &[u8], w: usize) -> (usize, [usize; 4]) {
    let mut n = 0;
    let mut b = [usize::MAX, usize::MAX, 0, 0];
    for (i, p) in px.as_chunks::<4>().0.iter().enumerate() {
        if p[3] > 0 {
            n += 1;
            let (x, y) = (i % w, i / w);
            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
        }
    }
    (n, b)
}

#[test]
fn text_drawn_into_its_layer_a_part_at_a_time_is_as_drawn_whole() {
    crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
    let Some(s) = session(300, 300) else { return };
    let layer_now = |s: &CanvasSession, id: u64| {
        let _ = pixel(s, 0, 0);
        s.copy_layer_contents(id).map(|c| c.rgba).unwrap_or_else(|| vec![0; 300 * 300 * 4])
    };
    let first = crate::TextInfo { size: 40.0, x: 20.0, y: 30.0, ..text_info("Hi") };
    let id = s.text_begin(None, first.clone()).unwrap();
    // typed on, moved, made larger and italic, then shorter and back up: each drawing only where
    // the text was and is, the layer each time as the whole canvas drawn
    let edits = [
        crate::TextInfo { text: "Hello there".to_owned(), ..first.clone() },
        crate::TextInfo { text: "Hello there".to_owned(), x: 60.0, y: 150.0, ..first.clone() },
        crate::TextInfo {
            text: "Hello there".to_owned(),
            size: 60.0,
            style: "Italic".to_owned(),
            x: 10.0,
            y: 140.0,
            ..first.clone()
        },
        crate::TextInfo { text: "Hey".to_owned(), x: 200.0, y: 10.0, ..first.clone() },
    ];
    assert!(s.draw_text_into_layer());
    assert!(layer_now(&s, id) == crate::draw_text(first.clone(), 300, 300));
    for info in edits {
        assert!(s.set_text(info.clone()));
        assert!(s.draw_text_into_layer());
        assert!(
            layer_now(&s, id) == crate::draw_text(info.clone(), 300, 300),
            "{:?} at {}, {}",
            info.text,
            info.x,
            info.y
        );
    }
    // a drawing begun before the text changed again is dropped; the next one draws it
    let job = s.text_draw_start().unwrap();
    let last = crate::TextInfo { text: "Bye".to_owned(), ..first.clone() };
    assert!(s.set_text(last.clone()));
    assert!(!s.text_draw_put(job.draw()));
    assert!(s.draw_text_into_layer());
    assert!(layer_now(&s, id) == crate::draw_text(last.clone(), 300, 300));
    s.text_end(true);
    assert!(s.text_draw_start().is_none());
    // edited again later, moved far off where it was: the first drawing takes the whole layer
    assert_eq!(s.text_begin(Some(id), last.clone()), Some(id));
    let moved = crate::TextInfo { text: "Again".to_owned(), x: 150.0, y: 200.0, ..first.clone() };
    assert!(s.set_text(moved.clone()));
    assert!(s.draw_text_into_layer());
    assert!(layer_now(&s, id) == crate::draw_text(moved, 300, 300));
    s.text_end(true);
}

#[test]
fn every_letter_drawn_falls_within_where_the_text_reaches() {
    crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
    crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Bold.ttf").to_vec());
    let (w, h) = (700u32, 700u32);
    let base = crate::TextInfo { size: 60.0, x: 200.0, y: 200.0, ..text_info("Wavy Æ gjpq ÅÉ ffi %@ ~") };
    // each setting that moves ink at its widest
    let mut cases = vec![base.clone()];
    for style in ["Italic", "Bold Italic", "Black"] {
        cases.push(crate::TextInfo { style: style.to_owned(), ..base.clone() });
    }
    for v in [-1.0f32, 1.0] {
        cases.push(crate::TextInfo { kerning: v, ..base.clone() });
        cases.push(crate::TextInfo { tracking: v, ..base.clone() });
        cases.push(crate::TextInfo {
            leading: v,
            text: "Two
lines gj"
                .to_owned(),
            ..base.clone()
        });
        cases.push(crate::TextInfo { baseline: v, ..base.clone() });
    }
    cases.push(crate::TextInfo { underline: true, outline: true, ..base.clone() });
    cases.push(crate::TextInfo { vertical: true, text: "Tall gj".to_owned(), ..base.clone() });
    cases.push(crate::TextInfo { width: 150.0, align: crate::TextAlignment::Right, ..base.clone() });
    cases.push(crate::TextInfo { all_caps: true, style: "Italic".to_owned(), size: 140.0, x: 40.0, ..base.clone() });
    for info in cases {
        let drawn = crate::draw_text(info.clone(), w, h);
        let Some(inked) = crate::layer_ops::painted_rect(&drawn, w) else { continue };
        let reach = crate::text_draw::text_reach(&info);
        assert!(inked.intersect(reach) == inked, "{info:?}: ink {inked:?} past {reach:?}");
    }
}

#[test]
fn memory_trimmed_mid_work_leaves_the_canvas_as_it_was_and_working() {
    use crate::Adjustment;
    let Some(s) = session(240, 240) else { return };
    s.set_brush_size(0.5);
    for i in 0..2 {
        if i > 0 {
            s.add_layer();
        }
        let y = 60.0 + i as f32 * 90.0;
        s.stroke_begin(PointerKind::Finger, vec![sample(20.0, y, 0.0)]);
        s.stroke_add((1..20).map(|k| sample(20.0 + k as f32 * 10.0, y + k as f32, k as f64 * 8.0)).collect());
        s.stroke_end(vec![]);
    }
    let composite = |s: &CanvasSession| {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render_finished(gpu, &mut st.doc);
        assert_eq!(gpu.errors(), 0, "GPU validation errors");
        canvas.read_composite(gpu).unwrap()
    };
    // a blur under way (its working textures kept), a layer read back and a picture pasted (the
    // buffers kept): let go between its steps, then the work goes on
    s.adjustment_start(Adjustment::GaussianBlur);
    s.adjustment_amount_by(0.05);
    let blurred = composite(&s);
    let copied = s.copy_layer().unwrap();
    s.trim_memory();
    assert!(composite(&s) == blurred);
    s.adjustment_amount_by(0.05);
    s.adjustment_amount_by(-0.05);
    assert!(composite(&s) == blurred);
    s.adjustment_end(true);
    s.trim_memory();
    let kept = composite(&s);
    assert!(s.paste(copied).is_some());
    s.undo();
    assert!(composite(&s) == kept);
}

#[test]
fn text_typed_keeps_the_pictures_of_the_layers_below_it() {
    crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
    let Some(s) = session(300, 300) else { return };
    s.set_brush_size(0.5);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 150.0, 0.0)]);
    s.stroke_add((1..30).map(|i| sample(20.0 + i as f32 * 9.0, 150.0 + i as f32, i as f64 * 8.0)).collect());
    s.stroke_end(vec![]);
    s.add_layer();
    s.stroke_begin(PointerKind::Finger, vec![sample(150.0, 20.0, 0.0)]);
    s.stroke_add((1..30).map(|i| sample(150.0 + i as f32, 20.0 + i as f32 * 9.0, i as f64 * 8.0)).collect());
    s.stroke_end(vec![]);
    let composite = |s: &CanvasSession, cached: bool| {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        if canvas.composites_cached() != cached {
            canvas.set_cache_composites(cached);
        }
        canvas.render_finished(gpu, &mut st.doc);
        assert_eq!(gpu.errors(), 0, "GPU validation errors");
        assert_eq!(canvas.composites_cached(), cached);
        canvas.read_composite(gpu).unwrap()
    };
    let first = crate::TextInfo { size: 50.0, x: 30.0, y: 100.0, ..text_info("Hi") };
    s.text_begin(None, first.clone()).unwrap();
    assert!(s.draw_text_into_layer());
    composite(&s, true);
    // each letter put into the text layer leaves the picture of those below it as it was, and
    // the canvas shows as it would composited whole
    for (i, text) in ["Hi t", "Hi the", "Hi there", "Hey"].into_iter().enumerate() {
        assert!(s.set_text(crate::TextInfo { text: text.to_owned(), y: 100.0 + i as f32 * 20.0, ..first.clone() }));
        assert!(s.draw_text_into_layer());
        assert!(s.shared.lock().canvas.as_ref().unwrap().composites_cached(), "{text}");
        let cached = composite(&s, true);
        assert!(cached == composite(&s, false), "{text}");
        composite(&s, true);
    }
    s.text_end(true);
}

#[test]
fn pictures_of_the_layers_made_again_in_kept_textures_show_what_changed() {
    let Some(s) = session(200, 200) else { return };
    s.set_brush_size(0.5);
    let bottom = s.layers().iter().find(|l| l.current).map_or(0, |l| l.id);
    for i in 0..3 {
        if i > 0 {
            s.add_layer();
        }
        let y = 40.0 + i as f32 * 60.0;
        s.stroke_begin(PointerKind::Finger, vec![sample(10.0, y, 0.0)]);
        s.stroke_add((1..20).map(|k| sample(10.0 + k as f32 * 10.0, y + k as f32, k as f64 * 8.0)).collect());
        s.stroke_end(vec![]);
    }
    let composite = |s: &CanvasSession, cached: bool| {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        if canvas.composites_cached() != cached {
            canvas.set_cache_composites(cached);
        }
        canvas.render_finished(gpu, &mut st.doc);
        assert_eq!(gpu.errors(), 0, "GPU validation errors");
        assert_eq!(canvas.composites_cached(), cached);
        canvas.read_composite(gpu).unwrap()
    };
    let first = composite(&s, true);
    assert!(first == composite(&s, false));
    composite(&s, true);
    // the bottom layer's pixels sent again (as an artwork's are), nothing about it in the layer
    // list changed: the caches made again over it show them
    {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        let red = [200u8, 0, 0, 255].repeat(200 * 200);
        canvas.upload_layer(gpu, procraft_engine::document::LayerId(bottom), &red).unwrap();
    }
    let cached = composite(&s, true);
    assert!(cached != first);
    assert!(cached == composite(&s, false));
    composite(&s, true);
    // and as the layer list changes, each time as composited whole
    let added = std::cell::Cell::new(0);
    let changes: [(&str, &dyn Fn()); 4] = [
        ("hidden", &|| _ = s.set_layer_visible(bottom, false)),
        ("shown", &|| _ = s.set_layer_visible(bottom, true)),
        ("added", &|| added.set(s.add_layer())),
        ("deleted", &|| _ = s.delete_layer(added.get())),
    ];
    for (what, change) in changes {
        change();
        let cached = composite(&s, true);
        assert!(cached == composite(&s, false), "{what}");
        composite(&s, true);
    }
}

#[test]
fn text_is_drawn_inside_its_box_and_as_it_is_set() {
    let regular = include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf");
    assert_eq!(crate::add_text_font(regular.to_vec()), vec!["Inter".to_owned()]);
    assert!(crate::text_families().contains(&"Inter".to_owned()));
    let (w, h) = (600u32, 400u32);
    let plain = text_info("Hello");
    let b = crate::text_box(plain.clone());
    let (n, ink) = inked(&crate::draw_text(plain.clone(), w, h), w as usize);
    assert!(n > 200, "{n}");
    // the letters inside the box (a pixel's rounding allowed)
    assert!(ink[0] as f32 >= b[0] - 1.0 && ink[1] as f32 >= b[1] - 1.0, "{ink:?} {b:?}");
    assert!(ink[2] as f32 <= b[0] + b[2] + 1.0 && ink[3] as f32 <= b[1] + b[3] + 1.0, "{ink:?} {b:?}");
    // centred in a wide box, the letters move to its middle
    let centred = crate::TextInfo { width: 500.0, align: crate::TextAlignment::Center, ..plain.clone() };
    let (_, c) = inked(&crate::draw_text(centred, w, h), w as usize);
    let middle = (c[0] + c[2]) as f32 / 2.0;
    assert!((middle - (10.0 + 250.0)).abs() < 8.0, "{middle}");
    // underlined, ink below the baseline across the word
    let under = crate::TextInfo { underline: true, ..plain.clone() };
    let (nu, u) = inked(&crate::draw_text(under, w, h), w as usize);
    assert!(nu > n && u[3] > ink[3], "{nu} {u:?}");
    // outlined, a stroke round each letter: not the letters filled
    let outlined = crate::TextInfo { outline: true, size: 120.0, text: "O".to_owned(), ..plain.clone() };
    let filled = crate::TextInfo { size: 120.0, text: "O".to_owned(), ..plain.clone() };
    let (no, _) = inked(&crate::draw_text(outlined, w, h), w as usize);
    let (nf, _) = inked(&crate::draw_text(filled, w, h), w as usize);
    assert!(no > 0 && no < nf * 2, "{no} {nf}");
    // wrapped in a narrow box, taller; vertical, one letter a line; the leading spreads lines
    let wrapped =
        crate::text_box(crate::TextInfo { text: "Hello there world".to_owned(), width: 80.0, ..plain.clone() });
    assert!(wrapped[3] > b[3] * 2.0, "{wrapped:?} {b:?}");
    let vertical = crate::text_box(crate::TextInfo { vertical: true, ..plain.clone() });
    assert!(vertical[3] > b[3] * 4.0 && vertical[2] < b[2], "{vertical:?} {b:?}");
    let spread = crate::text_box(crate::TextInfo {
        text: "a
b"
        .to_owned(),
        leading: 1.0,
        ..plain.clone()
    });
    let tight = crate::text_box(crate::TextInfo {
        text: "a
b"
        .to_owned(),
        ..plain.clone()
    });
    assert!((spread[3] - tight[3] - 40.0).abs() < 1.0, "{spread:?} {tight:?}");
    // tracking widens the word; capitals are what all caps draws
    let tracked = crate::text_box(crate::TextInfo { tracking: 0.5, ..plain.clone() });
    assert!(tracked[2] > b[2] + 60.0, "{tracked:?} {b:?}");
    let caps = crate::draw_text(crate::TextInfo { all_caps: true, ..plain.clone() }, w, h);
    let upper = crate::draw_text(crate::TextInfo { text: "HELLO".to_owned(), ..plain.clone() }, w, h);
    assert_eq!(caps, upper);
    // the baseline lifts the box: its top is the origin less the baseline
    let lifted = crate::text_box(crate::TextInfo { baseline: 1.0, ..plain });
    assert!((lifted[1] - (10.0 - 40.0)).abs() < 0.01, "{lifted:?}");
}

#[test]
fn the_caret_and_chosen_letters_stand_where_the_text_is_drawn() {
    let regular = include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf");
    crate::add_text_font(regular.to_vec());
    let plain = crate::TextInfo {
        text: "Hello
there"
            .to_owned(),
        ..text_info("")
    };
    let b = crate::text_box(plain.clone());
    // before the first letter, at the box's left on the first line; after the last, at the
    // second line's end
    let first = crate::text_caret(plain.clone(), 0);
    assert!((first[0] - b[0]).abs() < 2.0 && (first[1] - b[1]).abs() < 0.01, "{first:?} {b:?}");
    let last = crate::text_caret(plain.clone(), 11);
    assert!(last[1] > first[2] - 1.0 && last[0] > b[0] + 20.0, "{last:?} {first:?}");
    // further along a line, further right
    let third = crate::text_caret(plain.clone(), 3);
    assert!(third[0] > first[0] + 10.0 && third[1] == first[1], "{third:?}");
    // an empty line centred: its caret in the middle
    let centred = crate::TextInfo {
        text: "ab
"
        .to_owned(),
        width: 300.0,
        align: crate::TextAlignment::Center,
        ..plain.clone()
    };
    let empty = crate::text_caret(centred, 3);
    assert!((empty[0] - (10.0 + 150.0)).abs() < 1.0, "{empty:?}");
    assert!(crate::text_caret(crate::TextInfo { vertical: true, ..plain.clone() }, 1).is_empty());
    // "ell" chosen: a picture from the caret before it to the one after it, ringed in blue
    let chosen = crate::draw_chosen_text(plain.clone(), 1, 4, 0xFF0076FD).expect("chosen letters");
    let (from, to) = (crate::text_caret(plain.clone(), 1), crate::text_caret(plain.clone(), 4));
    assert!((chosen.x as f32 - from[0]).abs() < 2.0, "{} {from:?}", chosen.x);
    assert!(((chosen.x + chosen.width as i32) as f32 - to[0]).abs() < 2.0, "{} {to:?}", chosen.width);
    assert_eq!(chosen.rgba.len(), chosen.width as usize * chosen.height as usize * 4);
    let blue = chosen.rgba.chunks(4).filter(|p| p[3] > 200 && p[2] > 200 && p[0] < 60).count();
    assert!(blue > 20, "{blue}");
    assert!(crate::draw_chosen_text(plain.clone(), 2, 2, 0xFF0076FD).is_none());
    // across both lines: as tall as two
    let both = crate::draw_chosen_text(plain, 0, 11, 0xFF0076FD).expect("both lines");
    assert!(both.height as f32 > (first[2] - first[1]) * 1.5, "{} {first:?}", both.height);
}

#[test]
fn the_text_tools_typefaces_and_styles() {
    let faces = crate::text_typefaces();
    assert_eq!(faces.first().map(|f| f.family.as_str()), Some("sans-serif"));
    // the eight generic ones, then Inter (then any imported)
    assert_eq!(faces.get(8).map(|f| (f.name.as_str(), f.family.as_str())), Some(("Inter", "inter")));
    let styles = crate::text_styles();
    assert_eq!(styles.len(), 12);
    assert_eq!(&styles[..6], ["Thin", "Thin Italic", "Light", "Light Italic", "Regular", "Italic"]);
    assert_eq!(styles.last().map(String::as_str), Some("Black Italic"));
}

#[test]
fn imported_fonts_are_kept_in_their_folder_and_offered() {
    let dir = std::env::temp_dir().join(format!("procraft-fonts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // nowhere to keep them yet
    assert!(crate::import_text_font(vec![1, 2, 3], "Nothing.ttf".into()).is_none());
    crate::set_text_font_folder(dir.to_string_lossy().into_owned());
    let inter = include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec();
    let kept = crate::import_text_font(inter, "My Font!.ttf".into()).unwrap();
    assert_eq!(kept.name, "My Font");
    assert!(kept.family.starts_with("file:") && dir.join("My Font.ttf").is_file());
    assert!(crate::text_typefaces().contains(&kept));
    // a file that isn't a font isn't kept
    assert!(crate::import_text_font(b"not a font".to_vec(), "Bad".into()).is_none());
    assert!(!dir.join("Bad.ttf").exists());
    // a font imported under a name kept already never overwrites it
    let regular = include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec();
    let bold = include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Bold.ttf").to_vec();
    let first = crate::import_text_font(regular.clone(), "Same.ttf".into()).unwrap();
    // the same file again: the same typeface, one file
    assert_eq!(crate::import_text_font(regular.clone(), "Same.ttf".into()), Some(first.clone()));
    // another under that name: a name of its own, the first kept as it was
    let second = crate::import_text_font(bold, "Same.ttf".into()).unwrap();
    assert_eq!(second.name, "Same 2");
    assert_eq!(std::fs::read(dir.join("Same.ttf")).unwrap(), regular);
    // a file that isn't a font, under that name: dropped, the first kept
    assert!(crate::import_text_font(b"no font".to_vec(), "Same.ttf".into()).is_none());
    assert_eq!(std::fs::read(dir.join("Same.ttf")).unwrap(), regular);
    assert!(!dir.join("Same 3.ttf").exists());
    // a name of nothing but signs still makes a file of its own
    assert!(crate::import_text_font(regular, "!!!".into()).is_some_and(|t| t.name == "Font"));
    // a text layer naming a font file outside the folder never opens it: the sans serif instead
    let outside = crate::TextInfo { font: r"file:\\server\share\x.ttf".to_owned(), ..text_info("Hi") };
    assert!(!crate::draw_text(outside, 200, 80).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn brushes_read_apart_are_added_once_where_they_belong() {
    let Some(s) = session(100, 100) else { return };
    let sets = s.brush_library().len();
    let file = s.brush_as_file(0, 0).unwrap();
    // read with nothing changed: a brush file's brush, for the "Imported" set made at the top
    let read = crate::read_brushes(file, Some("Brush.brush".into())).unwrap();
    assert!(crate::read_brushes(b"neither".to_vec(), Some("x.brush".into())).is_none());
    // meanwhile a set is made at the top: the brushes still go where they belong
    s.new_brush_set();
    let set = s.add_brushes_read(std::sync::Arc::clone(&read), 0).unwrap();
    let library = s.brush_library();
    assert_eq!(library.len(), sets + 2);
    assert_eq!(library.get(set as usize).map(|l| (l.name.as_str(), l.brushes.len())), Some(("Imported", 1)));
    // added once
    assert!(s.add_brushes_read(read, 0).is_none());
    assert_eq!(s.brush_library().len(), sets + 2);
}

#[test]
fn text_with_negative_leading_keeps_its_letters_in_its_box() {
    crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
    // as Android lays it out: the leading only between lines
    let plain = crate::text_box(text_info("Hello"));
    let tight = crate::text_box(crate::TextInfo { leading: -0.5, ..text_info("Hello") });
    assert!((plain[3] - tight[3]).abs() < 0.5, "{plain:?} {tight:?}");
    let two = crate::text_box(crate::TextInfo { leading: -0.5, ..text_info("Hello\nthere") });
    assert!((two[3] - (2.0 * plain[3] - 0.5 * 40.0)).abs() < 1.0, "{two:?} {plain:?}");
}

#[test]
fn the_caret_in_capitals_follows_letters_that_become_two() {
    crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
    // "ß" shows as "SS": the caret after it stands after both
    let caps = crate::TextInfo { all_caps: true, ..text_info("straße") };
    let shown = text_info("STRASSE");
    assert_eq!(crate::text_caret(caps.clone(), 6), crate::text_caret(shown.clone(), 7));
    assert_eq!(crate::text_caret(caps, 5), crate::text_caret(shown, 6));
}

#[test]
fn a_group_has_nothing_to_copy() {
    let Some(s) = session(64, 64) else { return };
    let group = s.group_picked();
    assert!(group != 0);
    assert!(s.select_layer(group));
    assert!(s.copy_layer().is_none());
}

#[test]
fn text_places_count_between_utf16_and_characters() {
    // "a😀b": the emoji is two UTF-16 units and one character
    let text = "a\u{1F600}b".to_owned();
    assert_eq!(crate::text_index_from_utf16(text.clone(), 0), 0);
    assert_eq!(crate::text_index_from_utf16(text.clone(), 1), 1);
    assert_eq!(crate::text_index_from_utf16(text.clone(), 3), 2);
    assert_eq!(crate::text_index_from_utf16(text.clone(), 4), 3);
    // inside the pair: after it
    assert_eq!(crate::text_index_from_utf16(text.clone(), 2), 2);
    assert_eq!(crate::text_index_to_utf16(text.clone(), 2), 3);
    assert_eq!(crate::text_index_to_utf16(text, 9), 4);
}

#[test]
fn a_long_run_of_mixed_work_undoes_to_nothing_and_redoes_to_the_end() {
    use crate::Adjustment;
    use crate::transform::TransformHandle;
    crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
    for seed in [3u32, 11, 29] {
        let Some(s) = session(160, 160) else { return };
        s.shared.lock().canvas.as_mut().unwrap().set_cache_composites(true);
        s.set_brush_size(0.3);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        let blank = composite(&s);
        let current = |s: &CanvasSession| s.layers().iter().find(|l| l.current).map_or(0, |l| l.id);
        let mut done = Vec::new();
        for _ in 0..50 {
            let op = next(16);
            done.push(op);
            let (x, y) = (10.0 + next(140) as f32, 10.0 + next(140) as f32);
            match op {
                0 | 1 => {
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    let (dx, dy) = (next(60) as f32 - 30.0, next(60) as f32 - 30.0);
                    s.stroke_add(
                        (1..12)
                            .map(|k| sample(x + dx * k as f32 / 11.0, y + dy * k as f32 / 11.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                2 => _ = s.add_layer(),
                3 => {
                    let layers = s.layers();
                    if let Some(l) = layers.get(next(layers.len() as u32) as usize) {
                        s.select_layer(l.id);
                    }
                }
                4 => {
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    _ = s.fill_layer(current(&s));
                }
                5 => s.clear_layer(),
                6 => _ = s.invert_layer(current(&s)),
                7 => s.flip_canvas(next(2) == 0),
                8 => _ = s.duplicate_layer(current(&s)),
                9 => _ = s.merge_down(current(&s)),
                10 => {
                    if s.transform_begin().is_some() {
                        s.transform_grab(TransformHandle::Move, x, y);
                        s.transform_drag(x + next(40) as f32 - 20.0, y + next(40) as f32 - 20.0);
                        s.transform_release();
                        s.transform_end(true);
                    }
                }
                11 => {
                    if s.adjustment_start(Adjustment::GaussianBlur) {
                        s.adjustment_amount_by(0.02 + next(10) as f32 / 100.0);
                        s.adjustment_end(true);
                    }
                }
                12 => {
                    if s.pour_begin(x, y, 0.3, 0.6, 0.2, 0.2) {
                        s.pour_threshold(next(100) as f32 / 100.0);
                        s.pour_end(true, None);
                    }
                }
                13 => {
                    if let Some(c) = s.copy_layer() {
                        _ = s.paste(c);
                    }
                }
                14 => _ = s.undo(),
                _ => {
                    let info = crate::TextInfo { size: 20.0, x, y, ..text_info("Ab") };
                    if s.text_begin(None, info.clone()).is_some() {
                        s.set_text(crate::TextInfo { text: "Abc d".to_owned(), ..info });
                        s.draw_text_into_layer();
                        s.text_end(true);
                    }
                }
            }
        }
        let can = |s: &CanvasSession, undo: bool| {
            let st = s.shared.lock();
            st.canvas.as_ref().is_some_and(|c| if undo { c.can_undo() } else { c.can_redo() })
        };
        // what is left undone redone first (work refused since, on a text layer, didn't drop it)
        while can(&s, false) {
            s.redo();
        }
        let end = composite(&s);
        // all of it taken back: the blank canvas; all of it done again: as it ended
        let mut steps = 0;
        while can(&s, true) && steps < 400 {
            s.undo();
            steps += 1;
        }
        assert!(composite(&s) == blank, "seed {seed}: undone {steps} steps of {done:?}");
        let mut again = 0;
        while can(&s, false) && again < 400 {
            s.redo();
            again += 1;
        }
        assert_eq!(steps, again, "seed {seed}");
        assert!(composite(&s) == end, "seed {seed}: redone {again} steps of {done:?}");
    }
}

#[test]
fn layers_settings_and_selections_paged_out_undo_and_redo_whole() {
    use crate::selection::SelectionJoin;
    use crate::transform::TransformHandle;
    for seed in [5u32, 23] {
        let Some(s) = session(120, 120) else { return };
        // room for three layers on the GPU: the rest parked and brought back as they are wanted
        s.shared.lock().canvas.as_mut().unwrap().set_resident_budget(3 * 120 * 120 * 4);
        s.set_brush_size(0.3);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        let can = |s: &CanvasSession, undo: bool| {
            let st = s.shared.lock();
            st.canvas.as_ref().is_some_and(|c| if undo { c.can_undo() } else { c.can_redo() })
        };
        let blank = composite(&s);
        let current = |s: &CanvasSession| s.layers().iter().find(|l| l.current).map_or(0, |l| l.id);
        let any = |s: &CanvasSession, k: u32| {
            let layers = s.layers();
            layers.get(k as usize % layers.len().max(1)).map_or(0, |l| l.id)
        };
        let mut done = Vec::new();
        for _ in 0..60 {
            let op = next(21);
            done.push(op);
            let (x, y) = (5.0 + next(110) as f32, 5.0 + next(110) as f32);
            match op {
                0 | 1 => {
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    let (dx, dy) = (next(50) as f32 - 25.0, next(50) as f32 - 25.0);
                    s.stroke_add(
                        (1..10)
                            .map(|k| sample(x + dx * k as f32 / 9.0, y + dy * k as f32 / 9.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                2 => _ = s.add_layer(),
                3 => _ = s.select_layer(any(&s, next(16))),
                4 => _ = s.fill_layer(current(&s)),
                5 => {
                    let (w, h) = (10.0 + next(60) as f32, 10.0 + next(60) as f32);
                    let join = if next(3) == 0 { SelectionJoin::Remove } else { SelectionJoin::Add };
                    s.select_shape(next(2) == 0, x, y, x + w, y + h, join);
                }
                6 => s.select_invert(),
                7 => {
                    s.select_feather(next(30) as f32 / 100.0);
                    s.select_feather_done();
                }
                8 => s.select_none(),
                9 => {
                    let id = any(&s, next(16));
                    let shown = s.layers().iter().find(|l| l.id == id).is_some_and(|l| l.visible);
                    _ = s.set_layer_visible(id, !shown);
                }
                10 => _ = s.set_layer_opacity(any(&s, next(16)), 0.2 + next(80) as f32 / 100.0, true),
                11 => _ = s.set_layer_blend(any(&s, next(16)), next(34)),
                12 => _ = s.set_layer_alpha_lock(current(&s), next(2) == 0),
                13 => _ = s.set_layer_clipped(current(&s), next(2) == 0),
                14 => _ = s.add_mask(current(&s)),
                15 => _ = s.move_layer(current(&s), next(8)),
                16 => {
                    if s.transform_begin().is_some() {
                        s.transform_grab(TransformHandle::Move, x, y);
                        s.transform_drag(x + next(30) as f32 - 15.0, y + next(30) as f32 - 15.0);
                        s.transform_release();
                        s.transform_end(true);
                    }
                }
                17 => _ = s.undo(),
                18 => _ = s.invert_layer(current(&s)),
                19 => _ = s.duplicate_layer(current(&s)),
                _ => {
                    // two rows pinched together: the current layer's mask into it, if it has one
                    let a = current(&s);
                    let mask = s.layers().iter().find(|l| l.parent == Some(a)).map(|l| l.id);
                    _ = s.merge_between(a, mask.unwrap_or_else(|| any(&s, next(16))));
                }
            }
        }
        while can(&s, false) {
            s.redo();
        }
        let end = composite(&s);
        let mut steps = 0;
        while can(&s, true) && steps < 400 {
            s.undo();
            steps += 1;
        }
        assert!(composite(&s) == blank, "seed {seed}: undone {steps} steps of {done:?}");
        let mut again = 0;
        while can(&s, false) && again < 400 {
            s.redo();
            again += 1;
        }
        assert_eq!(steps, again, "seed {seed}");
        assert!(composite(&s) == end, "seed {seed}: redone {again} steps of {done:?}");
    }
}

#[test]
fn a_mixed_artwork_saved_and_opened_again_shows_the_same() {
    crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
    for seed in [7u32, 31] {
        let Some(s) = session(120, 120) else { return };
        s.set_brush_size(0.3);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        let current = |s: &CanvasSession| s.layers().iter().find(|l| l.current).map_or(0, |l| l.id);
        let any = |s: &CanvasSession, k: u32| {
            let layers = s.layers();
            layers.get(k as usize % layers.len().max(1)).map_or(0, |l| l.id)
        };
        let mut done = Vec::new();
        for _ in 0..40 {
            let op = next(13);
            done.push(op);
            let (x, y) = (5.0 + next(110) as f32, 5.0 + next(110) as f32);
            match op {
                0..=2 => {
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    let (dx, dy) = (next(50) as f32 - 25.0, next(50) as f32 - 25.0);
                    s.stroke_add(
                        (1..10)
                            .map(|k| sample(x + dx * k as f32 / 9.0, y + dy * k as f32 / 9.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                3 => _ = s.add_layer(),
                4 => _ = s.select_layer(any(&s, next(16))),
                5 => {
                    let id = any(&s, next(16));
                    let shown = s.layers().iter().find(|l| l.id == id).is_some_and(|l| l.visible);
                    _ = s.set_layer_visible(id, !shown);
                }
                6 => _ = s.set_layer_opacity(any(&s, next(16)), 0.2 + next(80) as f32 / 100.0, true),
                7 => _ = s.set_layer_blend(any(&s, next(16)), next(34)),
                8 => _ = s.set_layer_clipped(current(&s), next(2) == 0),
                9 => _ = s.add_mask(current(&s)),
                10 => _ = s.duplicate_layer(current(&s)),
                12 => {
                    s.pick_layer(any(&s, next(16)));
                    s.group_picked();
                }
                _ => {
                    let info = crate::TextInfo { size: 18.0, x, y, ..text_info("Hi") };
                    if s.text_begin(None, info.clone()).is_some() {
                        s.set_text(crate::TextInfo { text: "Hi there".to_owned(), ..info });
                        s.draw_text_into_layer();
                        s.text_end(true);
                    }
                }
            }
        }
        let shown = composite(&s);
        let dir = std::env::temp_dir().join(format!("procraft-mixed-{}-{seed}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        s.save(dir.to_string_lossy().into_owned()).unwrap();
        let opened = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
        let gpu = s.shared.lock().gpu.clone().unwrap();
        opened.use_gpu(gpu);
        {
            let mut st = opened.shared.lock();
            st.window_size = (120, 120);
            st.view = Some(View::fit((120, 120), (120, 120)));
            st.ensure_canvas();
        }
        let again = composite(&opened);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(again == shown, "seed {seed}: {done:?}");
    }
}

#[test]
fn a_mixed_artwork_through_a_layered_package_comes_back_the_same() {
    crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
    for seed in [13u32, 58] {
        let Some(s) = session(120, 120) else { return };
        s.set_brush_size(0.3);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        let current = |s: &CanvasSession| s.layers().iter().find(|l| l.current).map_or(0, |l| l.id);
        let any = |s: &CanvasSession, k: u32| {
            let layers = s.layers();
            layers.get(k as usize % layers.len().max(1)).map_or(0, |l| l.id)
        };
        // a mask has no blend mode or opacity of its own in the package, a group no blend mode (nor
        // a blend letter in the layers panel)
        let pick = |s: &CanvasSession, k: u32, groups: bool| {
            let layers: Vec<_> = s
                .layers()
                .into_iter()
                .filter(|l| {
                    l.kind == crate::layers::LayerType::Paint || (groups && l.kind == crate::layers::LayerType::Group)
                })
                .collect();
            layers.get(k as usize % layers.len().max(1)).map_or(0, |l| l.id)
        };
        let mut done = Vec::new();
        for _ in 0..40 {
            let op = next(13);
            done.push(op);
            let (x, y) = (5.0 + next(110) as f32, 5.0 + next(110) as f32);
            match op {
                0..=2 => {
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    let (dx, dy) = (next(50) as f32 - 25.0, next(50) as f32 - 25.0);
                    s.stroke_add(
                        (1..10)
                            .map(|k| sample(x + dx * k as f32 / 9.0, y + dy * k as f32 / 9.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                3 => _ = s.add_layer(),
                4 => _ = s.select_layer(any(&s, next(16))),
                5 => {
                    let id = any(&s, next(16));
                    let shown = s.layers().iter().find(|l| l.id == id).is_some_and(|l| l.visible);
                    _ = s.set_layer_visible(id, !shown);
                }
                6 => _ = s.set_layer_opacity(pick(&s, next(16), true), 0.2 + next(80) as f32 / 100.0, true),
                7 => _ = s.set_layer_blend(pick(&s, next(16), false), next(34)),
                8 => _ = s.set_layer_clipped(current(&s), next(2) == 0),
                9 => _ = s.add_mask(current(&s)),
                10 => _ = s.duplicate_layer(current(&s)),
                12 => {
                    s.pick_layer(any(&s, next(16)));
                    s.group_picked();
                }
                _ => {
                    let info = crate::TextInfo { size: 18.0, x, y, ..text_info("Hi") };
                    if s.text_begin(None, info.clone()).is_some() {
                        s.set_text(crate::TextInfo { text: "Hi there".to_owned(), ..info });
                        s.draw_text_into_layer();
                        s.text_end(true);
                    }
                }
            }
        }
        let shown = composite(&s);
        let dir = std::env::temp_dir().join(format!("procraft-package-{}-{seed}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("out.package");
        s.export_package(file.to_string_lossy().into_owned(), u64::from(seed), None).unwrap();
        let opened = CanvasSession::open_package(file.to_string_lossy().into_owned()).unwrap();
        let gpu = s.shared.lock().gpu.clone().unwrap();
        opened.use_gpu(gpu);
        {
            let mut st = opened.shared.lock();
            st.window_size = (120, 120);
            st.view = Some(View::fit((120, 120), (120, 120)));
            st.ensure_canvas();
        }
        let again = composite(&opened);
        let _ = std::fs::remove_dir_all(&dir);
        // a mask's brightness kept as a grey byte: within a level
        let most = shown.iter().zip(&again).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
        assert!(most <= 1, "seed {seed}: off by {most} after {done:?}");
    }
}

#[test]
fn a_mixed_artwork_through_a_psd_comes_back_the_same() {
    use procraft_engine::document::BlendMode;
    // every mode the format names as it is (shade is written as darken)
    let modes: Vec<u32> = BlendMode::ALL.iter().filter(|m| **m != BlendMode::Shade).map(|m| m.id()).collect();
    for seed in [9u32, 44] {
        let Some(s) = session(96, 96) else { return };
        s.set_brush_size(0.3);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        let current = |s: &CanvasSession| s.layers().iter().find(|l| l.current).map_or(0, |l| l.id);
        let any = |s: &CanvasSession, k: u32| {
            let layers = s.layers();
            layers.get(k as usize % layers.len().max(1)).map_or(0, |l| l.id)
        };
        let mut done = Vec::new();
        for _ in 0..30 {
            let op = next(10);
            done.push(op);
            let (x, y) = (5.0 + next(86) as f32, 5.0 + next(86) as f32);
            match op {
                0..=2 => {
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    let (dx, dy) = (next(40) as f32 - 20.0, next(40) as f32 - 20.0);
                    s.stroke_add(
                        (1..10)
                            .map(|k| sample(x + dx * k as f32 / 9.0, y + dy * k as f32 / 9.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                3 => _ = s.add_layer(),
                4 => _ = s.select_layer(any(&s, next(16))),
                5 => {
                    let id = any(&s, next(16));
                    let shown = s.layers().iter().find(|l| l.id == id).is_some_and(|l| l.visible);
                    _ = s.set_layer_visible(id, !shown);
                }
                6 => _ = s.set_layer_opacity(any(&s, next(16)), (50 + next(205)) as f32 / 255.0, true),
                7 => _ = s.set_layer_blend(any(&s, next(16)), modes[next(modes.len() as u32) as usize]),
                8 => _ = s.set_layer_clipped(current(&s), next(2) == 0),
                _ => _ = s.add_mask(current(&s)),
            }
        }
        // a dot on every layer: an empty one isn't read back from a PSD (our reader leaves empty
        // layers out), and what was clipped to it would be clipped to the one below
        for l in s.layers() {
            s.select_layer(l.id);
            // a hidden layer takes no paint: shown for it
            s.set_layer_visible(l.id, true);
            s.stroke_begin(PointerKind::Finger, vec![sample(5.0 + next(80) as f32, 5.0 + next(80) as f32, 0.0)]);
            s.stroke_end(vec![]);
            s.set_layer_visible(l.id, l.visible);
        }
        let shown = composite(&s);
        let dir = std::env::temp_dir().join(format!("procraft-psd-mixed-{}-{seed}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("out.psd");
        s.export_psd(file.to_string_lossy().into_owned()).unwrap();
        let art = dir.join("art");
        let outcome = crate::project::import_psd(
            file.to_string_lossy().into_owned(),
            art.to_string_lossy().into_owned(),
            "Back".into(),
        );
        assert_eq!(outcome, crate::project::PsdOutcome::Imported, "seed {seed}");
        let opened = CanvasSession::open(art.to_string_lossy().into_owned()).unwrap();
        opened.use_gpu(s.shared.lock().gpu.clone().unwrap());
        {
            let mut st = opened.shared.lock();
            st.window_size = (96, 96);
            st.view = Some(View::fit((96, 96), (96, 96)));
            st.ensure_canvas();
        }
        let again = composite(&opened);
        let _ = std::fs::remove_dir_all(&dir);
        // as the format keeps colours (8 bits, not premultiplied) and opacity (a byte)
        let most = shown.iter().zip(&again).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
        assert!(most <= 3, "seed {seed}: off by {most} after {done:?}");
    }
}

#[test]
fn saved_again_and_again_into_one_folder_an_artwork_opens_as_it_was_last() {
    for seed in [17u32, 83] {
        let Some(first) = session(100, 100) else { return };
        let gpu = first.shared.lock().gpu.clone().unwrap();
        let mut s = first;
        s.set_brush_size(0.3);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        let current = |s: &CanvasSession| s.layers().iter().find(|l| l.current).map_or(0, |l| l.id);
        let any = |s: &CanvasSession, k: u32| {
            let layers = s.layers();
            layers.get(k as usize % layers.len().max(1)).map_or(0, |l| l.id)
        };
        let dir = std::env::temp_dir().join(format!("procraft-resaved-{}-{seed}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.to_string_lossy().into_owned();
        let mut done = Vec::new();
        for _ in 0..60 {
            let op = next(13);
            done.push(op);
            let (x, y) = (5.0 + next(90) as f32, 5.0 + next(90) as f32);
            match op {
                0..=2 => {
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    let (dx, dy) = (next(40) as f32 - 20.0, next(40) as f32 - 20.0);
                    s.stroke_add(
                        (1..10)
                            .map(|k| sample(x + dx * k as f32 / 9.0, y + dy * k as f32 / 9.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                3 => _ = s.add_layer(),
                4 => _ = s.select_layer(any(&s, next(16))),
                5 => _ = s.delete_layer(any(&s, next(16))),
                6 => _ = s.merge_down(current(&s)),
                7 => _ = s.duplicate_layer(current(&s)),
                8 => _ = s.undo(),
                9 => _ = s.redo(),
                10 => _ = s.fill_layer(current(&s)),
                11 => s.clear_layer(),
                _ => {
                    s.save(path.clone()).unwrap();
                    // now and then closed and opened again, the work going on in what opened
                    if next(2) == 0 {
                        let opened = CanvasSession::open(path.clone()).unwrap();
                        opened.use_gpu(gpu.clone());
                        {
                            let mut st = opened.shared.lock();
                            st.window_size = (100, 100);
                            st.view = Some(View::fit((100, 100), (100, 100)));
                            st.ensure_canvas();
                        }
                        s = opened;
                        s.set_brush_size(0.3);
                    }
                }
            }
        }
        s.save(path.clone()).unwrap();
        let shown = composite(&s);
        let opened = CanvasSession::open(path).unwrap();
        opened.use_gpu(gpu.clone());
        {
            let mut st = opened.shared.lock();
            st.window_size = (100, 100);
            st.view = Some(View::fit((100, 100), (100, 100)));
            st.ensure_canvas();
        }
        let again = composite(&opened);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(again == shown, "seed {seed}: {done:?}");
    }
}

#[test]
fn crops_among_other_work_undo_and_redo_whole() {
    use crate::transform::TransformHandle;
    for seed in [2u32, 19] {
        let Some(s) = session(100, 100) else { return };
        s.shared.lock().canvas.as_mut().unwrap().set_cache_composites(true);
        s.set_brush_size(0.3);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            (st.doc.width(), st.doc.height(), canvas.read_composite(gpu).unwrap())
        };
        // as the apps ask: a crop's own undo (the canvas from before it) counted
        let can = |s: &CanvasSession, undo: bool| {
            let info = s.info();
            if undo { info.can_undo } else { info.can_redo }
        };
        let blank = composite(&s);
        let current = |s: &CanvasSession| s.layers().iter().find(|l| l.current).map_or(0, |l| l.id);
        let mut done = Vec::new();
        // one crop at most: only the canvas from before the last one is kept to undo it
        let mut cropped = false;
        for _ in 0..30 {
            let op = next(9);
            done.push(op);
            let (x, y) = (5.0 + next(90) as f32, 5.0 + next(90) as f32);
            match op {
                0..=2 => {
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    let (dx, dy) = (next(40) as f32 - 20.0, next(40) as f32 - 20.0);
                    s.stroke_add(
                        (1..10)
                            .map(|k| sample(x + dx * k as f32 / 9.0, y + dy * k as f32 / 9.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                3 => _ = s.add_layer(),
                4 => _ = s.fill_layer(current(&s)),
                5 if !cropped => {
                    cropped = true;
                    s.crop_begin();
                    s.crop_set_aspect_locked(false);
                    s.crop_set_resample(next(2) == 0);
                    s.crop_set_size(Some(40 + next(120)), Some(40 + next(120)));
                    s.crop_end(true);
                }
                6 => {
                    if s.transform_begin().is_some() {
                        s.transform_grab(TransformHandle::Move, x, y);
                        s.transform_drag(x + next(30) as f32 - 15.0, y + next(30) as f32 - 15.0);
                        s.transform_release();
                        s.transform_end(true);
                    }
                }
                7 => _ = s.undo(),
                8 => _ = s.redo(),
                _ => _ = s.add_layer(),
            }
        }
        while can(&s, false) {
            s.redo();
        }
        let end = composite(&s);
        let mut steps = 0;
        while can(&s, true) && steps < 200 {
            s.undo();
            steps += 1;
        }
        assert!(composite(&s) == blank, "seed {seed}: undone {steps} steps of {done:?}");
        let mut again = 0;
        while can(&s, false) && again < 200 {
            s.redo();
            again += 1;
        }
        assert_eq!(steps, again, "seed {seed}");
        assert!(composite(&s) == end, "seed {seed}: redone {again} steps of {done:?}");
    }
}

#[test]
fn a_crop_drops_what_was_undone_before_it() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.4);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    s.undo();
    assert!(s.info().can_redo);
    s.crop_begin();
    s.crop_set_aspect_locked(false);
    s.crop_set_size(Some(60), Some(40));
    assert!(s.crop_end(true));
    // a new step: the stroke undone before it can't come back
    assert!(!s.info().can_redo);
    s.undo();
    assert_eq!((s.info().width, s.info().height), (100, 100));
    // redo is the crop, and only it
    assert!(s.info().can_redo);
    s.redo();
    assert_eq!((s.info().width, s.info().height), (60, 40));
    assert!(!s.info().can_redo);
    assert_eq!(pixel(&s, 30, 30), [255, 255, 255, 255]);
}

#[test]
fn text_work_among_other_work_undoes_and_redoes_whole() {
    crate::add_text_font(include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf").to_vec());
    for seed in [4u32, 61] {
        let Some(s) = session(120, 120) else { return };
        s.set_brush_size(0.3);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        let can = |s: &CanvasSession, undo: bool| {
            let info = s.info();
            if undo { info.can_undo } else { info.can_redo }
        };
        // text left open is ended (kept) before looking
        let settle = |s: &CanvasSession| s.text_end(true);
        let blank = composite(&s);
        let words = ["A", "Bc", "Hi there", "Wd ef", "Xyz 12"];
        let mut done = Vec::new();
        for _ in 0..30 {
            let op = next(8);
            done.push(op);
            let (x, y) = (5.0 + next(100) as f32, 5.0 + next(100) as f32);
            match op {
                0 | 1 => {
                    settle(&s);
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    let (dx, dy) = (next(40) as f32 - 20.0, next(40) as f32 - 20.0);
                    s.stroke_add(
                        (1..10)
                            .map(|k| sample(x + dx * k as f32 / 9.0, y + dy * k as f32 / 9.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                2 => {
                    settle(&s);
                    _ = s.add_layer();
                }
                3 => {
                    // new text, typed and kept or let go
                    let info = crate::TextInfo { size: 14.0 + next(20) as f32, x, y, ..text_info("T") };
                    if s.text_begin(None, info.clone()).is_some() {
                        s.set_text(crate::TextInfo { text: words[next(5) as usize].to_owned(), ..info });
                        s.draw_text_into_layer();
                        s.text_end(next(4) != 0);
                    }
                }
                4 => {
                    // a text layer edited again: moved and retyped
                    let texts: Vec<u64> =
                        s.layers().iter().map(|l| l.id).filter(|id| s.layer_text(*id).is_some()).collect();
                    if let Some(&id) = texts.get(next(texts.len().max(1) as u32) as usize)
                        && let Some(info) = s.layer_text(id)
                        && s.text_begin(Some(id), info.clone()).is_some()
                    {
                        let moved = crate::TextInfo { text: words[next(5) as usize].to_owned(), x, y, ..info };
                        s.set_text(moved);
                        s.draw_text_into_layer();
                        s.text_end(next(4) != 0);
                    }
                }
                5 => {
                    settle(&s);
                    _ = s.undo();
                }
                6 => {
                    settle(&s);
                    _ = s.redo();
                }
                _ => {
                    settle(&s);
                    let layers = s.layers();
                    if let Some(l) = layers.get(next(layers.len() as u32) as usize) {
                        s.select_layer(l.id);
                    }
                }
            }
        }
        settle(&s);
        while can(&s, false) {
            s.redo();
            settle(&s);
        }
        let end = composite(&s);
        let mut steps = 0;
        while can(&s, true) && steps < 300 {
            s.undo();
            settle(&s);
            steps += 1;
        }
        assert!(composite(&s) == blank, "seed {seed}: undone {steps} steps of {done:?}");
        let mut again = 0;
        while can(&s, false) && again < 300 {
            s.redo();
            settle(&s);
            again += 1;
        }
        assert!(composite(&s) == end, "seed {seed}: redone {again} of {steps} steps of {done:?}");
    }
}

#[test]
fn adjustments_among_other_work_undo_and_redo_whole() {
    use crate::Adjustment;
    use crate::selection::SelectionJoin;
    let kinds = [
        Adjustment::Hsb,
        Adjustment::GaussianBlur,
        Adjustment::MotionBlur,
        Adjustment::PerspectiveBlur,
        Adjustment::Noise,
        Adjustment::Sharpen,
        Adjustment::Bloom,
        Adjustment::Glitch,
        Adjustment::Halftone,
        Adjustment::ChromaticAberration,
        Adjustment::GradientMap,
        Adjustment::Liquify,
        Adjustment::Clone,
    ];
    for seed in [8u32, 37] {
        let Some(s) = session(100, 100) else { return };
        s.shared.lock().canvas.as_mut().unwrap().set_cache_composites(true);
        s.set_brush_size(0.4);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        let can = |s: &CanvasSession, undo: bool| {
            let info = s.info();
            if undo { info.can_undo } else { info.can_redo }
        };
        let blank = composite(&s);
        let mut done = Vec::new();
        for _ in 0..30 {
            let op = next(8);
            done.push(op);
            let (x, y) = (5.0 + next(90) as f32, 5.0 + next(90) as f32);
            match op {
                0..=2 => {
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    let (dx, dy) = (next(50) as f32 - 25.0, next(50) as f32 - 25.0);
                    s.stroke_add(
                        (1..10)
                            .map(|k| sample(x + dx * k as f32 / 9.0, y + dy * k as f32 / 9.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                3 => {
                    let kind = kinds[next(kinds.len() as u32) as usize];
                    if s.adjustment_start(kind) {
                        match kind {
                            Adjustment::Hsb => s.adjustment_hsb(
                                next(100) as f32 / 100.0,
                                next(100) as f32 / 100.0,
                                next(100) as f32 / 100.0,
                            ),
                            Adjustment::Liquify => {
                                let points = (0..20).flat_map(|i| [x + i as f32 * 2.0, y, 1.0, 8.0]).collect();
                                s.adjustment_liquify_stroke(points, true, false);
                            }
                            Adjustment::Clone => {
                                s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                                s.stroke_add((1..8).map(|i| sample(x + i as f32 * 4.0, y, i as f64 * 8.0)).collect());
                                s.stroke_end(vec![]);
                            }
                            _ => s.adjustment_amount_by(0.05 + next(40) as f32 / 100.0),
                        }
                        s.adjustment_end(next(4) != 0);
                    }
                }
                4 => {
                    let (w, h) = (10.0 + next(50) as f32, 10.0 + next(50) as f32);
                    s.select_shape(next(2) == 0, x, y, x + w, y + h, SelectionJoin::Add);
                }
                5 => s.select_none(),
                6 => _ = s.undo(),
                _ => _ = s.redo(),
            }
        }
        while can(&s, false) {
            s.redo();
        }
        let end = composite(&s);
        let mut steps = 0;
        while can(&s, true) && steps < 300 {
            s.undo();
            steps += 1;
        }
        assert!(composite(&s) == blank, "seed {seed}: undone {steps} steps of {done:?}");
        let mut again = 0;
        while can(&s, false) && again < 300 {
            s.redo();
            again += 1;
        }
        assert_eq!(steps, again, "seed {seed}");
        assert!(composite(&s) == end, "seed {seed}: redone {again} steps of {done:?}");
    }
}

#[test]
fn an_animation_saved_and_through_a_package_plays_the_same() {
    use crate::animation::AnimationPlayback;
    for seed in [12u32, 47] {
        let Some(s) = session(80, 80) else { return };
        s.set_brush_size(0.3);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        assert!(s.set_animation_assist(true));
        let mut done = Vec::new();
        for _ in 0..30 {
            let op = next(6);
            done.push(op);
            let (x, y) = (5.0 + next(70) as f32, 5.0 + next(70) as f32);
            match op {
                0 | 1 => {
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    let (dx, dy) = (next(40) as f32 - 20.0, next(40) as f32 - 20.0);
                    s.stroke_add(
                        (1..10)
                            .map(|k| sample(x + dx * k as f32 / 9.0, y + dy * k as f32 / 9.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                2 => _ = s.add_layer(),
                3 => {
                    let frames = s.animation_frames();
                    if let Some(&f) = frames.get(next(frames.len().max(1) as u32) as usize) {
                        s.set_frame_hold(f, next(4));
                    }
                }
                4 => {
                    if let Some(mut a) = s.animation_settings() {
                        a.fps = 1 + next(30);
                        a.onion_frames = next(5);
                        a.onion_opacity = next(100) as f32 / 100.0;
                        a.playback = [AnimationPlayback::Loop, AnimationPlayback::PingPong, AnimationPlayback::OneShot]
                            [next(3) as usize];
                        a.background = next(2) == 0;
                        a.foreground = next(2) == 0;
                        s.set_animation_settings(a);
                    }
                }
                _ => {
                    let frames = s.animation_frames();
                    if let Some(&f) = frames.get(next(frames.len().max(1) as u32) as usize) {
                        s.select_layer(f);
                    }
                }
            }
        }
        let look = |s: &CanvasSession| {
            let frames: Vec<Vec<u8>> = (0..s.animation_frame_count())
                .map(|i| s.animation_frame(i, false, 0).map(|f| f.rgba).unwrap_or_default())
                .collect();
            let settings =
                s.animation_settings().map(|a| (a.fps, a.onion_frames, a.playback, a.background, a.foreground));
            (s.animation_sequence(), settings, frames)
        };
        let before = look(&s);
        let gpu = s.shared.lock().gpu.clone().unwrap();
        let opened_from = |opened: std::sync::Arc<CanvasSession>| {
            opened.use_gpu(gpu.clone());
            {
                let mut st = opened.shared.lock();
                st.window_size = (80, 80);
                st.view = Some(View::fit((80, 80), (80, 80)));
                st.ensure_canvas();
            }
            opened
        };
        let dir = std::env::temp_dir().join(format!("procraft-animation-{}-{seed}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let saved = dir.join("saved");
        s.save(saved.to_string_lossy().into_owned()).unwrap();
        let reopened = opened_from(CanvasSession::open(saved.to_string_lossy().into_owned()).unwrap());
        assert!(look(&reopened) == before, "seed {seed}, saved: {done:?}");
        let file = dir.join("out.package");
        s.export_package(file.to_string_lossy().into_owned(), u64::from(seed), None).unwrap();
        let imported = opened_from(CanvasSession::open_package(file.to_string_lossy().into_owned()).unwrap());
        let (seq, settings, frames) = look(&imported);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(seq, before.0, "seed {seed}, package order: {done:?}");
        assert_eq!(settings, before.1, "seed {seed}, package settings: {done:?}");
        assert!(frames == before.2, "seed {seed}, package frames: {done:?}");
    }
}

#[test]
fn layer_and_canvas_settings_come_back_saved_and_through_a_package() {
    for seed in [21u32, 66] {
        let Some(s) = session(64, 64) else { return };
        s.set_brush_size(0.3);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let any = |s: &CanvasSession, k: u32| {
            let layers: Vec<_> = s.layers().into_iter().filter(|l| l.kind == crate::layers::LayerType::Paint).collect();
            layers.get(k as usize % layers.len().max(1)).map_or(0, |l| l.id)
        };
        let names = ["Ink", "Sketch 2", "Läyer ✓", "Background bits", "x"];
        let mut done = Vec::new();
        for _ in 0..25 {
            let op = next(9);
            done.push(op);
            match op {
                0 => {
                    s.stroke_begin(
                        PointerKind::Finger,
                        vec![sample(5.0 + next(50) as f32, 5.0 + next(50) as f32, 0.0)],
                    );
                    s.stroke_end(vec![]);
                }
                1 => _ = s.add_layer(),
                2 => _ = s.rename_layer(any(&s, next(16)), names[next(5) as usize].to_owned()),
                3 => _ = s.set_layer_locked(any(&s, next(16)), next(2) == 0),
                4 => _ = s.set_layer_alpha_lock(any(&s, next(16)), next(2) == 0),
                5 => _ = s.set_layer_reference(any(&s, next(16)), next(2) == 0),
                6 => {
                    _ = s.set_background_color(
                        next(255) as f32 / 255.0,
                        next(255) as f32 / 255.0,
                        next(255) as f32 / 255.0,
                        true,
                    )
                }
                7 => _ = s.set_background_visible(next(3) != 0),
                _ => _ = s.set_dpi([72.0, 132.0, 300.0][next(3) as usize]),
            }
        }
        s.set_author("A. Painter".to_owned());
        let look = |s: &CanvasSession| {
            let layers: Vec<_> = s
                .layers()
                .into_iter()
                .map(|l| (l.name, l.visible, l.alpha_lock, l.locked, l.reference, l.kind))
                .collect();
            let b = s.background();
            let back = (b.visible, [b.r, b.g, b.b].map(|c| (c * 255.0).round() as u8));
            (layers, back, (s.info().dpi * 10.0).round() as i32, s.artwork_info().author)
        };
        let before = look(&s);
        let gpu = s.shared.lock().gpu.clone().unwrap();
        let ready = |opened: std::sync::Arc<CanvasSession>| {
            opened.use_gpu(gpu.clone());
            {
                let mut st = opened.shared.lock();
                st.window_size = (64, 64);
                st.view = Some(View::fit((64, 64), (64, 64)));
                st.ensure_canvas();
            }
            opened
        };
        let dir = std::env::temp_dir().join(format!("procraft-settings-{}-{seed}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let saved = dir.join("saved");
        s.save(saved.to_string_lossy().into_owned()).unwrap();
        let reopened = ready(CanvasSession::open(saved.to_string_lossy().into_owned()).unwrap());
        assert_eq!(look(&reopened), before, "seed {seed}, saved: {done:?}");
        let file = dir.join("out.package");
        s.export_package(file.to_string_lossy().into_owned(), u64::from(seed), None).unwrap();
        let imported = ready(CanvasSession::open_package(file.to_string_lossy().into_owned()).unwrap());
        let after = look(&imported);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(after, before, "seed {seed}, package: {done:?}");
    }
}

#[test]
fn brushes_set_anyhow_paint_smudge_and_erase_without_errors() {
    use crate::{SourceKind, ToolKind};
    const KEYS: [&str; 73] = [
        "count",
        "count_jitter",
        "flow",
        "grain_behavior",
        "grain_blend",
        "grain_depth",
        "grain_depth_jitter",
        "grain_movement",
        "grain_scale",
        "grain_zoom",
        "input_style",
        "jitter_lateral",
        "jitter_linear",
        "jitter_opacity",
        "jitter_size",
        "max_opacity",
        "max_size",
        "min_opacity",
        "min_size",
        "most_size",
        "pen_taper_end",
        "pen_taper_opacity",
        "pen_taper_size",
        "pen_taper_start",
        "pressure_bleed",
        "pressure_brightness",
        "pressure_flow",
        "pressure_hue",
        "pressure_opacity",
        "pressure_roundness",
        "pressure_saturation",
        "pressure_secondary",
        "pressure_size",
        "randomized",
        "rendering_mode",
        "roundness",
        "roundness_jitter_x",
        "roundness_jitter_y",
        "scatter",
        "shape_angle",
        "shape_rotation",
        "smudge_pull",
        "spacing",
        "spacing_jitter",
        "stamp_darkness",
        "stamp_hue",
        "stamp_lightness",
        "stamp_saturation",
        "stamp_secondary",
        "streamline",
        "streamline_pressure",
        "tilt_angle",
        "tilt_bleed",
        "tilt_brightness",
        "tilt_gradation",
        "tilt_hue",
        "tilt_opacity",
        "tilt_roundness",
        "tilt_saturation",
        "tilt_secondary",
        "tilt_size",
        "touch_taper_end",
        "touch_taper_opacity",
        "touch_taper_size",
        "touch_taper_start",
        "wet_attack",
        "wet_blur",
        "wet_blur_jitter",
        "wet_charge",
        "wet_dilution",
        "wet_grade",
        "wet_jitter",
        "wet_pull",
    ];
    let shapes = crate::studio::source_library(false).len() as u32;
    let grains = crate::studio::source_library(true).len() as u32;
    let Some(s) = session(96, 96) else { return };
    let mut state = 77u32;
    let mut next = |n: u32| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) % n.max(1)
    };
    for round in 0..40 {
        s.studio_begin();
        for key in KEYS {
            // past both ends now and then: what the controls can't give is held to what they can
            let v = next(1400) as f32 / 1000.0 - 0.2;
            s.set_studio_setting(key.to_owned(), v);
        }
        s.set_studio_shipped_source(SourceKind::Shape, next(shapes.max(1)));
        s.set_studio_shipped_source(SourceKind::Grain, next(grains.max(1)));
        s.studio_end(true);
        // now and then a dual brush: a second set anyhow too, combined by any mode
        {
            let mut second = procraft_engine::brush::Brush::round_pen();
            for key in KEYS {
                second.set_setting(key, next(1400) as f32 / 1000.0 - 0.2);
            }
            let dual = next(3) == 0;
            let mode = next(29) as f32;
            let mut st = s.shared.lock();
            let tool = st.tool;
            if let Some(brush) = st.brush_mut(tool) {
                brush.secondary = dual.then(|| Box::new(second));
                brush.set_setting("dual_blend", mode);
            }
        }
        s.set_tool([ToolKind::Paint, ToolKind::Smudge, ToolKind::Erase][next(3) as usize]);
        s.set_brush_size(next(100) as f32 / 100.0);
        s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
        let kind = [PointerKind::Stylus, PointerKind::Finger, PointerKind::Mouse][next(3) as usize];
        let (x, y) = (10.0 + next(70) as f32, 10.0 + next(70) as f32);
        let at = |k: u32, pressure: f32, tilt: f32| PenSample {
            x: x + (k as f32 * 0.7).sin() * 30.0,
            y: y + k as f32 * 1.5,
            pressure,
            tilt,
            orientation: k as f32 * 0.2,
            time_ms: f64::from(k) * 8.0,
            roll: 0.0,
        };
        s.stroke_begin(kind, vec![at(0, 0.0, 0.0)]);
        s.stroke_add((1..30).map(|k| at(k, next(100) as f32 / 100.0, next(157) as f32 / 100.0)).collect());
        s.stroke_end(vec![at(30, 0.0, 1.5)]);
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render_finished(gpu, &mut st.doc);
        assert_eq!(gpu.errors(), 0, "GPU validation errors in round {round}");
        let px = canvas.read_composite(gpu).unwrap();
        // the background is opaque white: a stroke never leaves see-through pixels
        assert!(px.chunks(4).all(|p| p[3] == 255), "round {round}: see-through pixels");
    }
}

#[test]
fn transforms_of_every_kind_undo_and_redo_whole() {
    use crate::selection::SelectionJoin;
    use crate::transform::{TransformAction, TransformHandle, TransformKind};
    for seed in [6u32, 52] {
        let Some(s) = session(100, 100) else { return };
        s.shared.lock().canvas.as_mut().unwrap().set_cache_composites(true);
        s.set_brush_size(0.5);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        let can = |s: &CanvasSession, undo: bool| {
            let info = s.info();
            if undo { info.can_undo } else { info.can_redo }
        };
        let blank = composite(&s);
        let mut done = Vec::new();
        for _ in 0..30 {
            let op = next(8);
            done.push(op);
            let (x, y) = (10.0 + next(80) as f32, 10.0 + next(80) as f32);
            match op {
                0..=2 => {
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    let (dx, dy) = (next(50) as f32 - 25.0, next(50) as f32 - 25.0);
                    s.stroke_add(
                        (1..10)
                            .map(|k| sample(x + dx * k as f32 / 9.0, y + dy * k as f32 / 9.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                3 => {
                    let layers = s.layers();
                    if let Some(l) = layers.get(next(layers.len() as u32) as usize) {
                        if next(2) == 0 {
                            s.pick_layer(l.id);
                        } else {
                            s.select_layer(l.id);
                        }
                    }
                    if next(3) == 0 {
                        _ = s.add_layer();
                    }
                }
                4 => {
                    if next(2) == 0 {
                        s.select_shape(
                            next(2) == 0,
                            x,
                            y,
                            x + 10.0 + next(40) as f32,
                            y + 10.0 + next(40) as f32,
                            SelectionJoin::Add,
                        );
                    } else {
                        s.select_none();
                    }
                }
                5 | 6 => {
                    let Some(begun) = s.transform_begin() else { continue };
                    let kind =
                        [TransformKind::Freeform, TransformKind::Uniform, TransformKind::Distort, TransformKind::Warp]
                            [next(4) as usize];
                    let shown = s.transform_set_kind(kind).unwrap_or(begun);
                    for _ in 0..1 + next(3) {
                        let (dx, dy) = (next(30) as f32 - 15.0, next(30) as f32 - 15.0);
                        let (handle, at) = match next(4) {
                            0 => (TransformHandle::Move, (x, y)),
                            1 => {
                                let i = next(4) as usize;
                                (
                                    TransformHandle::Corner { index: i as u8 },
                                    (shown.corners[i * 2], shown.corners[i * 2 + 1]),
                                )
                            }
                            2 if kind == TransformKind::Warp && shown.mesh.len() >= 32 => {
                                let i = next(16) as usize;
                                (TransformHandle::Mesh { index: i as u8 }, (shown.mesh[i * 2], shown.mesh[i * 2 + 1]))
                            }
                            _ => (TransformHandle::Rotate, (x, y)),
                        };
                        s.transform_grab(handle, at.0, at.1);
                        s.transform_drag(at.0 + dx, at.1 + dy);
                        s.transform_release();
                    }
                    match next(5) {
                        0 => _ = s.transform_apply(TransformAction::FlipHorizontal),
                        1 => _ = s.transform_apply(TransformAction::Rotate45),
                        2 => _ = s.transform_apply(TransformAction::Undo),
                        _ => {}
                    }
                    s.transform_end(next(5) != 0);
                }
                _ => {
                    if next(2) == 0 {
                        _ = s.undo();
                    } else {
                        _ = s.redo();
                    }
                }
            }
        }
        while can(&s, false) {
            s.redo();
        }
        let end = composite(&s);
        let mut steps = 0;
        while can(&s, true) && steps < 300 {
            s.undo();
            steps += 1;
        }
        assert!(composite(&s) == blank, "seed {seed}: undone {steps} steps of {done:?}");
        let mut again = 0;
        while can(&s, false) && again < 300 {
            s.redo();
            again += 1;
        }
        assert_eq!(steps, again, "seed {seed}");
        assert!(composite(&s) == end, "seed {seed}: redone {again} steps of {done:?}");
    }
}

#[test]
fn group_work_undoes_and_redoes_whole_and_comes_back_saved() {
    use crate::layers::LayerType;
    for seed in [3u32, 41] {
        let Some(s) = session(64, 64) else { return };
        s.shared.lock().canvas.as_mut().unwrap().set_cache_composites(true);
        s.set_brush_size(0.4);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        let can = |s: &CanvasSession, undo: bool| {
            let info = s.info();
            if undo { info.can_undo } else { info.can_redo }
        };
        // the list as it shows: each row's name, kind, depth and look
        let look = |s: &CanvasSession| {
            s.layers()
                .into_iter()
                .map(|l| (l.name, l.kind, l.depth, l.visible, (l.opacity * 100.0).round() as i32, l.blend))
                .collect::<Vec<_>>()
        };
        let blank = (composite(&s), look(&s));
        let mut done = Vec::new();
        for _ in 0..40 {
            let layers = s.layers();
            let any = |k: u32| layers.get(k as usize % layers.len().max(1)).map_or(0, |l| l.id);
            let groups: Vec<u64> = layers.iter().filter(|l| l.kind == LayerType::Group).map(|l| l.id).collect();
            let group = |k: u32| groups.get(k as usize % groups.len().max(1)).copied();
            let op = next(14);
            done.push(op);
            match op {
                0 | 1 => {
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    let (x, y) = (5.0 + next(50) as f32, 5.0 + next(50) as f32);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    s.stroke_add((1..6).map(|k| sample(x + k as f32 * 2.0, y + k as f32, k as f64 * 8.0)).collect());
                    s.stroke_end(vec![]);
                }
                2 => _ = s.add_layer(),
                3 => _ = s.pick_layer(any(next(16))),
                4 => _ = s.group_picked(),
                5 => {
                    if let Some(g) = group(next(8)) {
                        _ = s.ungroup(g);
                    }
                }
                6 => _ = s.move_layer(any(next(16)), next(layers.len() as u32 + 1)),
                7 => _ = s.merge_down(any(next(16))),
                8 => _ = s.merge_picked(),
                9 => _ = s.duplicate_layer(any(next(16))),
                10 => {
                    if let Some(g) = group(next(8)) {
                        _ = s.flatten_group(g);
                    }
                }
                11 => _ = s.delete_layer(any(next(16))),
                12 => {
                    // a group's (or layer's) look, or two combined into a group
                    let id = group(next(8)).filter(|_| next(3) != 0).unwrap_or_else(|| any(next(16)));
                    match next(4) {
                        0 => _ = s.set_layer_visible(id, next(3) == 0),
                        1 => _ = s.set_layer_opacity(id, 0.2 + next(80) as f32 / 100.0, true),
                        2 => _ = s.set_layer_blend(id, next(20)),
                        _ => _ = s.combine_down(id),
                    }
                }
                _ => {
                    if next(2) == 0 {
                        _ = s.undo();
                    } else {
                        _ = s.redo();
                    }
                }
            }
        }
        while can(&s, false) {
            s.redo();
        }
        let end = (composite(&s), look(&s));
        let mut steps = 0;
        while can(&s, true) && steps < 400 {
            s.undo();
            steps += 1;
        }
        let undone = (composite(&s), look(&s));
        assert!(undone.0 == blank.0, "seed {seed}: undone {steps} steps of {done:?}");
        assert_eq!(undone.1, blank.1, "seed {seed}: undone {steps} steps of {done:?}");
        let mut again = 0;
        while can(&s, false) && again < 400 {
            s.redo();
            again += 1;
        }
        assert_eq!(steps, again, "seed {seed}");
        let redone = (composite(&s), look(&s));
        assert!(redone.0 == end.0, "seed {seed}: redone {again} steps of {done:?}");
        assert_eq!(redone.1, end.1, "seed {seed}: redone {again} steps of {done:?}");
        // saved and opened again: the same list and picture (both composited whole: the caches,
        // 8-bit pictures of what is below and above, round a little differently)
        {
            let mut st = s.shared.lock();
            let canvas = st.canvas.as_mut().unwrap();
            canvas.set_cache_composites(false);
            canvas.invalidate();
        }
        let whole = composite(&s);
        // what the caches showed is what compositing it all shows (but their 8-bit rounding)
        let most = whole.iter().zip(&end.0).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
        assert!(most <= 4, "seed {seed}: cached and whole differ by {most} after {done:?}");
        let gpu = s.shared.lock().gpu.clone().unwrap();
        let dir = std::env::temp_dir().join(format!("procraft-groups-{}-{seed}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        s.save(dir.to_string_lossy().into_owned()).unwrap();
        let opened = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        opened.use_gpu(gpu);
        {
            let mut st = opened.shared.lock();
            st.window_size = (64, 64);
            st.view = Some(View::fit((64, 64), (64, 64)));
            st.ensure_canvas();
            st.canvas.as_mut().unwrap().set_cache_composites(false);
        }
        assert_eq!(look(&opened), end.1, "seed {seed}: reopened, after {done:?}");
        assert!(composite(&opened) == whole, "seed {seed}: reopened, after {done:?}");
    }
}

#[test]
fn painting_in_every_mode_undoes_redoes_and_saves_whole() {
    use crate::adjust::{LiquifyBrush, LiquifyKind};
    use crate::layers::LayerType;
    use crate::selection::SelectionJoin;
    for seed in [5u32, 77] {
        let Some(s) = session(64, 64) else { return };
        s.shared.lock().canvas.as_mut().unwrap().set_cache_composites(true);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        let can = |s: &CanvasSession, undo: bool| {
            let info = s.info();
            if undo { info.can_undo } else { info.can_redo }
        };
        let look = |s: &CanvasSession| {
            s.layers()
                .into_iter()
                .map(|l| (l.name, l.kind, l.depth, l.alpha_lock, l.clipped, l.reference))
                .collect::<Vec<_>>()
        };
        let blank = (composite(&s), look(&s));
        let mut done = Vec::new();
        for _ in 0..40 {
            let layers = s.layers();
            let any = |k: u32| layers.get(k as usize % layers.len().max(1)).map_or(0, |l| l.id);
            let paint = |k: u32| {
                let p: Vec<u64> = layers.iter().filter(|l| l.kind == LayerType::Paint).map(|l| l.id).collect();
                p.get(k as usize % p.len().max(1)).copied().unwrap_or(0)
            };
            let current = layers.iter().find(|l| l.current).map_or(0, |l| l.id);
            let (x, y) = (4.0 + next(56) as f32, 4.0 + next(56) as f32);
            let op = next(14);
            done.push(op);
            match op {
                0..=2 => {
                    s.set_tool([ToolKind::Paint, ToolKind::Paint, ToolKind::Smudge, ToolKind::Erase][next(4) as usize]);
                    s.set_brush_size(0.2 + next(60) as f32 / 100.0);
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    let (dx, dy) = (next(30) as f32 - 15.0, next(30) as f32 - 15.0);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    s.stroke_add(
                        (1..8)
                            .map(|k| sample(x + dx * k as f32 / 7.0, y + dy * k as f32 / 7.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                3 => _ = s.add_layer(),
                4 => _ = s.select_layer(any(next(16))),
                5 => _ = s.add_mask(current),
                6 => _ = s.set_layer_alpha_lock(paint(next(16)), next(2) == 0),
                7 => _ = s.set_layer_clipped(paint(next(16)), next(2) == 0),
                8 => {
                    if next(3) == 0 {
                        s.select_none();
                    } else {
                        let (w, h) = (8.0 + next(40) as f32, 8.0 + next(40) as f32);
                        s.select_shape(next(2) == 0, x, y, x + w, y + h, SelectionJoin::Add);
                    }
                }
                9 => _ = s.set_layer_reference(paint(next(16)), next(2) == 0),
                10 => {
                    s.pour_begin(
                        x,
                        y,
                        next(255) as f32 / 255.0,
                        next(255) as f32 / 255.0,
                        0.3,
                        next(60) as f32 / 100.0,
                    );
                    s.pour_threshold(next(80) as f32 / 100.0);
                    s.pour_end(next(4) != 0, None);
                }
                11 => {
                    s.recolor_start(
                        x,
                        y,
                        next(255) as f32 / 255.0,
                        0.5,
                        next(255) as f32 / 255.0,
                        next(50) as f32 / 100.0,
                    );
                    s.recolor_tap(4.0 + next(56) as f32, 4.0 + next(56) as f32);
                    s.recolor_end();
                }
                12 => {
                    if s.liquify_begin() {
                        let kinds = [
                            LiquifyKind::Push,
                            LiquifyKind::TwirlRight,
                            LiquifyKind::Pinch,
                            LiquifyKind::Expand,
                            LiquifyKind::Crystals,
                        ];
                        let brush =
                            LiquifyBrush { kind: kinds[next(5) as usize], size: 0.2, pressure: 1.0, distortion: 0.3 };
                        s.liquify_stroke(
                            (0..=10).flat_map(|k| [x + k as f32, y, 1.0, 8.0]).collect(),
                            true,
                            false,
                            brush,
                        );
                        s.adjust_end(next(4) != 0);
                    }
                }
                _ => {
                    if next(2) == 0 {
                        _ = s.undo();
                    } else {
                        _ = s.redo();
                    }
                }
            }
        }
        while can(&s, false) {
            s.redo();
        }
        let end = (composite(&s), look(&s));
        let mut steps = 0;
        while can(&s, true) && steps < 400 {
            s.undo();
            steps += 1;
        }
        let undone = (composite(&s), look(&s));
        assert!(undone.0 == blank.0, "seed {seed}: undone {steps} steps of {done:?}");
        assert_eq!(undone.1, blank.1, "seed {seed}: undone {steps} steps of {done:?}");
        let mut again = 0;
        while can(&s, false) && again < 400 {
            s.redo();
            again += 1;
        }
        assert_eq!(steps, again, "seed {seed}");
        let redone = (composite(&s), look(&s));
        assert!(redone.0 == end.0, "seed {seed}: redone {again} steps of {done:?}");
        assert_eq!(redone.1, end.1, "seed {seed}: redone {again} steps of {done:?}");
        // saved and opened again, both composited whole (see the group work's test)
        {
            let mut st = s.shared.lock();
            let canvas = st.canvas.as_mut().unwrap();
            canvas.set_cache_composites(false);
            canvas.invalidate();
        }
        let whole = composite(&s);
        let gpu = s.shared.lock().gpu.clone().unwrap();
        let dir = std::env::temp_dir().join(format!("procraft-modes-{}-{seed}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        s.save(dir.to_string_lossy().into_owned()).unwrap();
        let opened = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        opened.use_gpu(gpu);
        {
            let mut st = opened.shared.lock();
            st.window_size = (64, 64);
            st.view = Some(View::fit((64, 64), (64, 64)));
            st.ensure_canvas();
            st.canvas.as_mut().unwrap().set_cache_composites(false);
        }
        assert_eq!(look(&opened), end.1, "seed {seed}: reopened, after {done:?}");
        assert!(composite(&opened) == whole, "seed {seed}: reopened, after {done:?}");
    }
}

#[test]
fn a_layer_added_after_an_undo_starts_clear() {
    let Some(s) = session(64, 64) else { return };
    s.set_brush_size(0.5);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 30.0, 0.0)]);
    s.stroke_add((1..10).map(|k| sample(10.0 + k as f32 * 4.0, 30.0, k as f64 * 8.0)).collect());
    s.stroke_end(vec![]);
    let first = s.layers()[0].id;
    // a painted copy undone: the next layer added isn't given its id (nor its pixels, kept for redo)
    let copy = s.duplicate_layer(first).unwrap();
    s.undo();
    let added = s.add_layer();
    assert_ne!(added, copy);
    s.set_layer_visible(first, false);
    assert_eq!(pixel(&s, 30, 30), [255, 255, 255, 255]);
    // nor after a document step undone and redone
    s.undo();
    s.redo();
    let again = s.add_layer();
    assert!(again != copy && again != added);
}

#[test]
fn every_studio_setting_comes_back_from_the_library_and_a_brush_file() {
    use procraft_engine::brushfile;
    let mut keys: Vec<&str> = procraft_engine::studio::DRAWN.to_vec();
    for k in [
        "count_jitter",
        "flow",
        "grain_behavior",
        "grain_blend",
        "grain_depth",
        "grain_depth_jitter",
        "grain_movement",
        "grain_scale",
        "grain_zoom",
        "jitter_lateral",
        "jitter_linear",
        "jitter_opacity",
        "jitter_size",
        "max_opacity",
        "max_size",
        "min_opacity",
        "min_size",
        "most_size",
        "pressure_bleed",
        "pressure_brightness",
        "pressure_flow",
        "pressure_hue",
        "pressure_opacity",
        "pressure_roundness",
        "pressure_saturation",
        "pressure_secondary",
        "pressure_size",
        "rendering_mode",
        "roundness",
        "roundness_jitter_x",
        "roundness_jitter_y",
        "shape_angle",
        "smudge_pull",
        "spacing_jitter",
        "stamp_darkness",
        "stamp_hue",
        "stamp_lightness",
        "stamp_saturation",
        "stamp_secondary",
        "tilt_angle",
        "tilt_bleed",
        "tilt_brightness",
        "tilt_gradation",
        "tilt_hue",
        "tilt_opacity",
        "tilt_roundness",
        "tilt_saturation",
        "tilt_secondary",
        "tilt_size",
        "wet_attack",
        "wet_blur",
        "wet_blur_jitter",
        "wet_charge",
        "wet_dilution",
        "wet_grade",
        "wet_jitter",
        "wet_pull",
    ] {
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    let owned: Vec<String> = keys.iter().map(|k| (*k).to_owned()).collect();
    let Some(s) = session(64, 64) else { return };
    let read = |s: &CanvasSession| {
        s.studio_begin();
        let v = s.studio_settings(owned.clone());
        s.studio_end(false);
        v
    };
    let mut state = 9u32;
    let mut next = |n: u32| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) % n.max(1)
    };
    let mut library = Vec::new();
    let mut file = Vec::new();
    for round in 0..6 {
        s.studio_begin();
        for key in &keys {
            s.set_studio_setting((*key).to_owned(), next(1001) as f32 / 1000.0);
        }
        s.studio_end(true);
        let want = read(&s);
        let (set, index) = {
            let st = s.shared.lock();
            st.brush_ref(st.tool)
        };
        // the library kept and loaded again
        let t = CanvasSession::new(64, 64);
        assert!(t.load_brush_library(s.saved_brush_library()));
        assert!(t.select_brush(set as u32, index as u32));
        let got = read(&t);
        for ((k, w), g) in keys.iter().zip(&want).zip(&got) {
            if (w - g).abs() > 1e-5 {
                library.push(format!("round {round} {k}: {w} -> {g}"));
            }
        }
        // shared as a brush file and imported
        let Some(u) = session(64, 64) else { return };
        let bytes = s.brush_as_file(set as u32, index as u32).unwrap();
        assert!(brushfile::read(&bytes).is_some());
        let into = u.import_brushes(bytes).unwrap();
        assert!(u.select_brush(into, 0));
        let got = read(&u);
        for ((k, w), g) in keys.iter().zip(&want).zip(&got) {
            if (w - g).abs() > 2e-3 {
                file.push(format!("round {round} {k}: {w} -> {g}"));
            }
        }
    }
    assert!(library.is_empty(), "library: {} changed:\n{}", library.len(), library.join("\n"));
    assert!(file.is_empty(), "brush file: {} changed:\n{}", file.len(), file.join("\n"));
}

#[test]
fn merging_down_keeps_only_where_the_upper_layer_is_painted() {
    let Some(s) = session(200, 200) else { return };
    let bytes = |s: &CanvasSession| s.shared.lock().canvas.as_ref().unwrap().history_bytes();
    s.set_brush_size(0.2);
    s.set_color(0.0, 0.0, 1.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 20.0, 0.0)]);
    s.stroke_add((1..20).map(|k| sample(20.0 + k as f32 * 8.0, 20.0 + k as f32 * 8.0, k as f64 * 8.0)).collect());
    s.stroke_end(vec![]);
    let lower = s.layers()[0].id;
    let upper = s.add_layer();
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(140.0, 40.0, 0.0)]);
    s.stroke_add((1..4).map(|k| sample(140.0 + k as f32 * 3.0, 40.0, k as f64 * 8.0)).collect());
    s.stroke_end(vec![]);
    let shown = (pixel(&s, 142, 40), pixel(&s, 40, 40));
    let before = bytes(&s);
    assert!(s.merge_down(upper));
    assert_eq!((pixel(&s, 142, 40), pixel(&s, 40, 40)), shown);
    // a dab's worth kept, not the whole lower layer (200 x 200 x 4)
    let kept = bytes(&s) - before;
    assert!(kept > 0 && kept < 200 * 200 * 4 / 10, "{kept}");
    assert_eq!((pixel(&s, 142, 40), pixel(&s, 40, 40)), shown);
    assert_eq!(s.layers().len(), 1);
    // undone, both layers as they were
    s.undo();
    assert_eq!(s.layers().len(), 2);
    s.set_layer_visible(upper, false);
    assert_eq!(pixel(&s, 142, 40), [255, 255, 255, 255]);
    assert_eq!(pixel(&s, 40, 40), shown.1);
    s.set_layer_visible(upper, true);
    s.select_layer(lower);
    s.redo();
    s.redo();
    s.redo();
    assert_eq!((pixel(&s, 142, 40), pixel(&s, 40, 40)), shown);
    // two dabbed layers picked and merged into it: the same
    let dab = |s: &CanvasSession, x: f32| {
        let id = s.add_layer();
        s.stroke_begin(PointerKind::Finger, vec![sample(x, 160.0, 0.0)]);
        s.stroke_end(vec![]);
        id
    };
    let (a, b) = (dab(&s, 30.0), dab(&s, 60.0));
    assert!(s.select_layer(lower) && s.pick_layer(a) && s.pick_layer(b));
    let both = (pixel(&s, 30, 160), pixel(&s, 60, 160));
    let (before, count) = (bytes(&s), s.layers().len());
    assert!(s.merge_picked());
    assert_eq!((pixel(&s, 30, 160), pixel(&s, 60, 160)), both);
    let kept = bytes(&s) - before;
    assert!(kept > 0 && kept < 200 * 200 * 4 / 4, "{kept}");
    s.undo();
    assert_eq!(s.layers().len(), count);
}

#[test]
fn a_blend_mode_inside_a_group_blends_with_what_is_below_the_group() {
    use procraft_engine::document::BlendMode;
    let Some(s) = session(32, 32) else { return };
    // red below, a grey Multiply layer grouped above it
    let bottom = s.layers()[0].id;
    s.set_color(1.0, 0.0, 0.0);
    assert!(s.fill_layer(bottom));
    let grey = s.add_layer();
    s.set_color(0.5, 0.5, 0.5);
    assert!(s.fill_layer(grey));
    assert!(s.set_layer_blend(grey, BlendMode::Multiply.id()));
    let shown = pixel(&s, 16, 16);
    let group = s.group_picked();
    assert!(s.layers().iter().any(|l| l.id == group));
    // the grey multiplies the red as it did ungrouped, not just what is in the group
    let through = pixel(&s, 16, 16);
    assert_eq!(through, shown);
    assert!(through[0].abs_diff(128) <= 2 && through[1] <= 2, "{through:?}");
    // the same while what is below and above is kept aside, the group with its Multiply above
    // the layer worked on as much as around it
    s.shared.lock().canvas.as_mut().unwrap().set_cache_composites(true);
    let top = s.add_layer();
    assert!(s.select_layer(grey));
    assert_eq!(pixel(&s, 16, 16), through);
    assert!(s.select_layer(bottom));
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(4.0, 4.0, 0.0)]);
    s.stroke_end(vec![]);
    assert_eq!(pixel(&s, 16, 16), through);
    assert!(s.select_layer(grey));
    // given an opacity of its own the group is composited first: grey over the red, halved
    assert!(s.set_layer_opacity(group, 0.5, true));
    let alone = pixel(&s, 16, 16);
    assert!(alone[0].abs_diff(192) <= 2 && alone[1].abs_diff(64) <= 2, "{alone:?}");
    s.undo();
    assert_eq!(pixel(&s, 16, 16), through);
    assert!(s.delete_layer(top));
    assert_eq!(pixel(&s, 16, 16), through);
}

#[test]
fn a_mask_pinched_into_its_layer_is_applied_to_it() {
    let Some(s) = session(32, 32) else { return };
    let blue = s.layers()[0].id;
    s.set_color(0.0, 0.0, 1.0);
    assert!(s.fill_layer(blue));
    let red = s.add_layer();
    s.set_color(1.0, 0.0, 0.0);
    assert!(s.fill_layer(red));
    // the mask black at the top half: the red shows only below it
    let mask = s.add_mask(red).unwrap();
    s.set_color(0.0, 0.0, 0.0);
    s.set_brush_size(0.15);
    s.stroke_begin(PointerKind::Finger, vec![sample(0.0, 2.0, 0.0)]);
    s.stroke_add((1..=16).map(|k| sample(k as f32 * 2.0, 2.0, k as f64 * 8.0)).collect());
    s.stroke_end(vec![]);
    let (top, bottom) = (pixel(&s, 16, 2), pixel(&s, 16, 30));
    assert!(top[2] > 200 && top[0] < 50, "{top:?}");
    assert!(bottom[0] > 200 && bottom[2] < 50, "{bottom:?}");
    let count = s.layers().len();
    // pinched (either way round): the mask goes into the layer's pixels, the picture the same
    assert!(s.merge_between(red, mask));
    assert_eq!(s.layers().len(), count - 1);
    assert!(s.layers().iter().all(|l| l.id != mask));
    assert_eq!((pixel(&s, 16, 2), pixel(&s, 16, 30)), (top, bottom));
    // the layer itself is clear where the mask was black
    s.set_layer_visible(blue, false);
    assert_eq!(pixel(&s, 16, 2), [255, 255, 255, 255]);
    s.set_layer_visible(blue, true);
    // undone, the mask is back and the red whole under it
    s.undo();
    s.undo();
    s.undo();
    assert!(s.layers().iter().any(|l| l.id == mask));
    s.set_layer_visible(mask, false);
    assert!(pixel(&s, 16, 2)[0] > 200, "{:?}", pixel(&s, 16, 2));
    s.set_layer_visible(mask, true);
    assert!(s.merge_between(mask, red));
    assert_eq!((pixel(&s, 16, 2), pixel(&s, 16, 30)), (top, bottom));
}

#[test]
fn selection_work_undoes_and_redoes_whole() {
    use crate::selection::SelectionJoin;
    for seed in [4u32, 63] {
        let Some(s) = session(64, 64) else { return };
        s.shared.lock().canvas.as_mut().unwrap().set_cache_composites(true);
        s.set_brush_size(0.4);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        // what shows, and what is selected
        let look = |s: &CanvasSession| {
            let shown = composite(s);
            let selected = s.shared.lock().selection.as_ref().map(|m| m.data.clone());
            (shown, selected, s.layers().len())
        };
        let can = |s: &CanvasSession, undo: bool| {
            let info = s.info();
            if undo { info.can_undo } else { info.can_redo }
        };
        let blank = look(&s);
        let mut done = Vec::new();
        let mut saved = 0u32;
        for _ in 0..40 {
            let (x, y) = (4.0 + next(56) as f32, 4.0 + next(56) as f32);
            let join = if next(3) == 0 { SelectionJoin::Remove } else { SelectionJoin::Add };
            let op = next(15);
            done.push(op);
            match op {
                0..=2 => {
                    s.set_color(next(255) as f32 / 255.0, next(255) as f32 / 255.0, next(255) as f32 / 255.0);
                    let (dx, dy) = (next(40) as f32 - 20.0, next(40) as f32 - 20.0);
                    s.stroke_begin(PointerKind::Finger, vec![sample(x, y, 0.0)]);
                    s.stroke_add(
                        (1..8)
                            .map(|k| sample(x + dx * k as f32 / 7.0, y + dy * k as f32 / 7.0, k as f64 * 8.0))
                            .collect(),
                    );
                    s.stroke_end(vec![]);
                }
                3 => s.select_shape(next(2) == 0, x, y, x + 6.0 + next(30) as f32, y + 6.0 + next(30) as f32, join),
                4 => {
                    let points: Vec<f32> =
                        (0..3 + next(4)).flat_map(|_| [4.0 + next(56) as f32, 4.0 + next(56) as f32]).collect();
                    s.select_polygon(points, join);
                }
                5 => s.select_by_colour(x, y, next(60) as f32 / 100.0, join, false),
                6 => {
                    let layers = s.layers();
                    if let Some(l) = layers.get(next(layers.len() as u32) as usize) {
                        _ = s.select_layer_contents(l.id);
                    }
                }
                7 => s.select_invert(),
                8 => {
                    s.select_feather(next(100) as f32 / 100.0);
                    s.select_feather_done();
                }
                9 => s.select_none(),
                10 => {
                    if next(2) == 0 {
                        saved = s.save_selection().max(saved);
                    } else if saved > 0 {
                        _ = s.load_selection(next(saved + 1));
                    }
                }
                11 => match next(3) {
                    0 => s.clear_layer(),
                    1 => _ = s.fill_layer(s.layers().iter().find(|l| l.current).map_or(0, |l| l.id)),
                    _ => {
                        if let Some(c) = s.copy_layer() {
                            _ = s.paste(c);
                        }
                    }
                },
                12 => _ = s.add_layer(),
                _ => {
                    if next(2) == 0 {
                        _ = s.undo();
                    } else {
                        _ = s.redo();
                    }
                }
            }
        }
        while can(&s, false) {
            s.redo();
        }
        let end = look(&s);
        let mut steps = 0;
        while can(&s, true) && steps < 400 {
            s.undo();
            steps += 1;
        }
        let undone = look(&s);
        assert!(undone.0 == blank.0, "seed {seed}: undone {steps} steps of {done:?}: the picture");
        assert!(undone.1 == blank.1, "seed {seed}: undone {steps} steps of {done:?}: the selection");
        assert_eq!(undone.2, blank.2, "seed {seed}");
        let mut again = 0;
        while can(&s, false) && again < 400 {
            s.redo();
            again += 1;
        }
        assert_eq!(steps, again, "seed {seed}");
        let redone = look(&s);
        assert!(redone.0 == end.0, "seed {seed}: redone {again} steps of {done:?}: the picture");
        assert!(redone.1 == end.1, "seed {seed}: redone {again} steps of {done:?}: the selection");
        assert_eq!(redone.2, end.2, "seed {seed}");
    }
}

#[test]
fn luminance_blending_mixes_a_stroke_in_by_squared_colour() {
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.6);
    s.set_brush_opacity(0.5);
    // white paint on the layer to mix with (the background is apart: a stroke covers it as it is)
    s.set_color(1.0, 1.0, 1.0);
    assert!(s.fill_layer(s.layers()[0].id));
    s.set_color(0.0, 0.0, 0.0);
    let line = |s: &CanvasSession| {
        s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 50.0, 0.0)]);
        s.stroke_add((1..20).map(|k| sample(10.0 + k as f32 * 4.0, 50.0, k as f64 * 8.0)).collect());
        s.stroke_end(vec![]);
        pixel(s, 50, 50)[0]
    };
    let plain = line(&s);
    s.undo();
    assert!(plain > 40 && plain < 215, "{plain}");
    s.studio_begin();
    s.set_studio_setting("luminance_blending".to_owned(), 1.0);
    s.studio_end(true);
    let squared = line(&s);
    // black over white: the white's share of the light, not of the number
    let want = 255.0 * (f32::from(plain) / 255.0).sqrt();
    assert!((f32::from(squared) - want).abs() < 4.0, "{squared} against {want} (plain {plain})");
}

#[test]
fn a_dual_brush_keeps_its_second_through_the_library_and_its_file() {
    use procraft_engine::brush::Brush;
    use procraft_engine::brushfile::{self, Pictures};
    let Some(s) = session(100, 100) else { return };
    let png = crate::project::rgba_png(1, 1, &[255; 4]);
    let pictures = |secondary| Pictures {
        shape_png: &png,
        grain_png: None,
        metallic_png: None,
        roughness_png: None,
        thumbnail_png: &png,
        author_png: None,
        signature_png: None,
        secondary,
    };
    let mut second = Brush { name: "Dabs".to_owned(), ..Brush::round_pen() };
    second.set_setting("spacing", 0.6);
    second.set_setting("scatter", 0.4);
    let mut first =
        Brush { name: "Paired".to_owned(), secondary: Some(Box::new(second.clone())), ..Brush::round_pen() };
    // multiply, by the layers' numbering
    first.set_setting("dual_blend", 1.0);
    let file = brushfile::write(&first, "", &pictures(Some(Box::new(pictures(None)))));
    let read = brushfile::read(&file).unwrap();
    assert_eq!(read.secondary.as_ref().map(|b| b.name.as_str()), Some("Dabs"));
    let set = s.import_brushes(file).unwrap();
    let kept = |s: &CanvasSession| {
        let st = s.shared.lock();
        let b = &st.library[set as usize].brushes[0];
        let second = b.secondary.as_deref().map(|b| (b.name.clone(), b.setting("spacing"), b.setting("scatter")));
        (b.setting("dual_blend"), second)
    };
    let want = (1.0, Some(("Dabs".to_owned(), second.setting("spacing"), second.setting("scatter"))));
    assert_eq!(kept(&s), want);
    // kept with the library
    let t = CanvasSession::new(64, 64);
    assert!(t.load_brush_library(s.saved_brush_library()));
    assert_eq!(kept(&t), want);
    // shared again: the second in its folder
    let again = brushfile::read(&s.brush_as_file(set, 0).unwrap()).unwrap();
    let second_again = again.secondary.unwrap();
    assert_eq!(second_again.name, "Dabs");
    assert!(again.settings.iter().any(|(k, v)| k == "dual_blend" && *v == 1.0));
    // a brush without a second has none, and its file no folder for one
    let plain = brushfile::write(&Brush::round_pen(), "", &pictures(None));
    assert!(brushfile::read(&plain).unwrap().secondary.is_none());
}

#[test]
fn a_dual_brush_draws_its_two_strokes_combined_by_its_mode() {
    use procraft_engine::brush::Brush;
    let Some(s) = session(120, 120) else { return };
    s.set_brush_size(0.5);
    s.set_color(0.0, 0.0, 0.0);
    // how much of the layer one straight stroke covers with the current brush
    let covered = |s: &CanvasSession| {
        s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 60.0, 0.0)]);
        s.stroke_add((1..30).map(|k| sample(10.0 + k as f32 * 3.4, 60.0, k as f64 * 8.0)).collect());
        s.stroke_end(vec![]);
        let n = {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let id = st.doc.current();
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            canvas.read_layer(gpu, id).unwrap().chunks(4).filter(|p| p[3] > 8).count()
        };
        s.undo();
        n
    };
    let plain = covered(&s);
    // the second: wide dabs far apart
    let mut second = Brush { name: "Dabs".to_owned(), ..Brush::round_pen() };
    second.set_setting("spacing", 1.0);
    let set = |s: &CanvasSession, second: &Brush, mode: Option<f32>| {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let tool = st.tool;
        let brush = st.brush_mut(tool).unwrap();
        brush.secondary = mode.map(|_| Box::new(second.clone()));
        if let Some(m) = mode {
            brush.set_setting("dual_blend", m);
        }
    };
    // multiply: only where both cover
    set(&s, &second, Some(1.0));
    let both = covered(&s);
    // normal: where either does
    set(&s, &second, Some(0.0));
    let either = covered(&s);
    // a second with a wet mix of its own is coverage all the same
    let mut wet = second.clone();
    wet.set_setting("wet_dilution", 0.5);
    wet.set_setting("rendering_mode", 2.0);
    assert!(wet.mixes_wet());
    set(&s, &wet, Some(1.0));
    let wet_both = covered(&s);
    set(&s, &second, None);
    assert_eq!(covered(&s), plain, "back to one stroke");
    assert!(both > 0 && both < plain, "multiply {both} of {plain}");
    assert!(either >= plain, "normal {either} of {plain}");
    assert!(wet_both > 0 && wet_both < plain, "a wet second, multiply {wet_both} of {plain}");
}

#[test]
fn touches_anyhow_leave_no_stroke_open_and_undo_whole() {
    use crate::TouchCommand;
    for seed in [12u32, 90] {
        let Some(s) = session(100, 100) else { return };
        s.set_brush_size(0.3);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        // what the apps do with what the touches hand back
        let run = |s: &CanvasSession, commands: Vec<TouchCommand>| {
            for c in commands {
                match c {
                    TouchCommand::Undo => _ = s.undo(),
                    TouchCommand::Redo => _ = s.redo(),
                    TouchCommand::ClearLayer => s.clear_layer(),
                    _ => {}
                }
            }
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        let mut t = 0.0;
        let mut down: Vec<(u64, f32, f32)> = Vec::new();
        let mut ids = 0u64;
        let mut done = Vec::new();
        for _ in 0..300 {
            t += 6.0 + f64::from(next(60));
            let op = next(15);
            done.push(op);
            match op {
                0 | 1 if down.len() < 4 => {
                    ids += 1;
                    let kind = [PointerKind::Finger, PointerKind::Finger, PointerKind::Stylus][next(3) as usize];
                    let (x, y) = (5.0 + next(90) as f32, 5.0 + next(90) as f32);
                    run(&s, s.touch_down(ids, kind, vec![sample(x, y, t)]));
                    down.push((ids, x, y));
                }
                2..=6 if !down.is_empty() => {
                    let i = next(down.len() as u32) as usize;
                    let (id, x, y) = down[i];
                    let (nx, ny) = (x + next(30) as f32 - 15.0, y + next(30) as f32 - 15.0);
                    run(&s, s.touch_move(id, vec![sample(nx, ny, t)]));
                    down[i] = (id, nx, ny);
                }
                7..=9 if !down.is_empty() => {
                    let (id, x, y) = down.remove(next(down.len() as u32) as usize);
                    run(&s, s.touch_up(id, vec![sample(x, y, t)]));
                }
                10 => run(&s, s.touch_tick(t)),
                11 if next(15) == 0 => {
                    run(&s, s.touch_cancel());
                    down.clear();
                }
                // the platform takes one pointer back (a hand on the screen)
                12 if !down.is_empty() && next(4) == 0 => {
                    let (id, _, _) = down.remove(next(down.len() as u32) as usize);
                    run(&s, s.touch_cancel_one(id));
                }
                13 if next(3) == 0 => {
                    let (x, y) = (5.0 + next(90) as f32, 5.0 + next(90) as f32);
                    run(&s, s.touch_pen_button(next(2) == 0, x, y));
                }
                // one finger set to move the canvas, or to paint again
                14 if next(6) == 0 => {
                    let pan = next(2) == 0;
                    s.set_touch_settings(crate::touch::TouchSettings {
                        one_finger_pan: pan,
                        finger_paint: !pan,
                        ..Default::default()
                    });
                }
                _ => {}
            }
        }
        for (id, x, y) in down.drain(..) {
            run(&s, s.touch_up(id, vec![sample(x, y, t)]));
        }
        run(&s, s.touch_tick(t + 5000.0));
        assert!(s.shared.lock().stroke.is_none(), "seed {seed}: a stroke left open after {done:?}");
        // redone to the end (a gesture may have undone), then undone all the way and redone: it
        // comes back as it was
        let can = |s: &CanvasSession, undo: bool| {
            let info = s.info();
            if undo { info.can_undo } else { info.can_redo }
        };
        while can(&s, false) {
            s.redo();
        }
        let end = composite(&s);
        let mut steps = 0;
        while can(&s, true) && steps < 400 {
            s.undo();
            steps += 1;
        }
        let mut again = 0;
        while can(&s, false) && again < 400 {
            s.redo();
            again += 1;
        }
        assert_eq!(steps, again, "seed {seed}");
        assert!(composite(&s) == end, "seed {seed}: redone {again} of {done:?}");
    }
}

#[test]
fn touches_in_every_mode_leave_nothing_open_and_undo_whole() {
    use crate::TouchCommand;
    use crate::touch::{Fill, SelectKind, SelectTouch, TransformTouch};
    for seed in [7u32, 58] {
        let Some(s) = session(100, 100) else { return };
        s.set_brush_size(0.3);
        let mut state = seed;
        let mut next = |n: u32| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) % n.max(1)
        };
        let run = |s: &CanvasSession, commands: Vec<TouchCommand>| {
            for c in commands {
                match c {
                    TouchCommand::Undo => _ = s.undo(),
                    TouchCommand::Redo => _ = s.redo(),
                    TouchCommand::ClearLayer => s.clear_layer(),
                    _ => {}
                }
            }
        };
        let composite = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            canvas.render_finished(gpu, &mut st.doc);
            assert_eq!(gpu.errors(), 0, "GPU validation errors");
            canvas.read_composite(gpu).unwrap()
        };
        // the mode the app has open: none, selecting, transforming, filling, adjusting
        let mut mode = 0;
        let close = |s: &CanvasSession, mode: &mut u32, keep: bool| {
            match *mode {
                1 => s.set_touch_selecting(None),
                2 => {
                    s.set_touch_transforming(None);
                    s.transform_end(keep);
                }
                3 => s.set_touch_filling(None),
                4 => _ = s.adjustment_end(keep),
                _ => {}
            }
            *mode = 0;
        };
        let mut t = 0.0;
        let mut down: Vec<(u64, f32, f32)> = Vec::new();
        let mut ids = 0u64;
        let mut done = Vec::new();
        for _ in 0..300 {
            t += 6.0 + f64::from(next(60));
            let op = next(14);
            done.push(op);
            match op {
                0 | 1 if down.len() < 3 => {
                    ids += 1;
                    let kind = [PointerKind::Finger, PointerKind::Stylus][next(2) as usize];
                    let (x, y) = (5.0 + next(90) as f32, 5.0 + next(90) as f32);
                    run(&s, s.touch_down(ids, kind, vec![sample(x, y, t)]));
                    down.push((ids, x, y));
                }
                2..=5 if !down.is_empty() => {
                    let i = next(down.len() as u32) as usize;
                    let (id, x, y) = down[i];
                    let (nx, ny) = (x + next(30) as f32 - 15.0, y + next(30) as f32 - 15.0);
                    run(&s, s.touch_move(id, vec![sample(nx, ny, t)]));
                    down[i] = (id, nx, ny);
                }
                6..=8 if !down.is_empty() => {
                    let (id, x, y) = down.remove(next(down.len() as u32) as usize);
                    run(&s, s.touch_up(id, vec![sample(x, y, t)]));
                }
                9 => run(&s, s.touch_tick(t)),
                // the app changes mode only with nothing down
                10..=12 if down.is_empty() => {
                    close(&s, &mut mode, next(3) != 0);
                    match next(5) {
                        3 => {
                            use crate::Adjustment as A;
                            let kinds = [
                                A::Hsb,
                                A::ColorBalance,
                                A::Curves,
                                A::GradientMap,
                                A::GaussianBlur,
                                A::MotionBlur,
                                A::PerspectiveBlur,
                                A::Noise,
                                A::Sharpen,
                                A::Bloom,
                                A::Glitch,
                                A::Halftone,
                                A::ChromaticAberration,
                                A::Liquify,
                                A::Clone,
                            ];
                            if s.adjustment_start(kinds[next(kinds.len() as u32) as usize]) {
                                mode = 4;
                            }
                        }
                        0 => {
                            let kinds = [
                                SelectKind::Automatic,
                                SelectKind::Freehand,
                                SelectKind::Rectangle,
                                SelectKind::Ellipse,
                            ];
                            let join =
                                if next(3) == 0 { crate::SelectionJoin::Remove } else { crate::SelectionJoin::Add };
                            s.set_touch_selecting(Some(SelectTouch {
                                kind: kinds[next(4) as usize],
                                join,
                                threshold: next(80) as f32 / 100.0,
                            }));
                            mode = 1;
                        }
                        1 => {
                            if s.transform_begin().is_some() {
                                s.set_touch_transforming(Some(TransformTouch { advanced_mesh: next(2) == 0 }));
                                mode = 2;
                            }
                        }
                        2 => {
                            let fill = Fill {
                                r: next(255) as f32 / 255.0,
                                g: 0.4,
                                b: next(255) as f32 / 255.0,
                                threshold: next(60) as f32 / 100.0,
                            };
                            s.set_touch_filling(Some(fill));
                            mode = 3;
                        }
                        _ => _ = s.set_modify_held(next(2) == 0),
                    }
                }
                13 if next(20) == 0 => {
                    run(&s, s.touch_cancel());
                    down.clear();
                }
                _ => {}
            }
        }
        for (id, x, y) in down.drain(..) {
            run(&s, s.touch_up(id, vec![sample(x, y, t)]));
        }
        run(&s, s.touch_tick(t + 5000.0));
        close(&s, &mut mode, true);
        s.set_modify_held(false);
        assert!(s.shared.lock().stroke.is_none(), "seed {seed}: a stroke left open after {done:?}");
        let can = |s: &CanvasSession, undo: bool| {
            let info = s.info();
            if undo { info.can_undo } else { info.can_redo }
        };
        while can(&s, false) {
            s.redo();
        }
        let end = composite(&s);
        let mut steps = 0;
        while can(&s, true) && steps < 400 {
            s.undo();
            steps += 1;
        }
        let mut again = 0;
        while can(&s, false) && again < 400 {
            s.redo();
            again += 1;
        }
        assert_eq!(steps, again, "seed {seed}");
        assert!(composite(&s) == end, "seed {seed}: redone {again} of {done:?}");
    }
}

#[test]
fn leaving_with_liquify_or_crop_open_keeps_the_work() {
    use crate::adjust::{LiquifyBrush, LiquifyKind};
    let Some(s) = session(100, 100) else { return };
    s.set_brush_size(0.5);
    s.set_color(0.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 50.0, 0.0)]);
    s.stroke_add((1..12).map(|k| sample(20.0 + k as f32 * 5.0, 50.0, k as f64 * 8.0)).collect());
    s.stroke_end(vec![]);
    let gpu = s.shared.lock().gpu.clone().unwrap();
    let composite = |s: &CanvasSession| {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render_finished(gpu, &mut st.doc);
        canvas.read_composite(gpu).unwrap()
    };
    let reopened = |dir: &std::path::Path| {
        let o = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
        o.use_gpu(gpu.clone());
        {
            let mut st = o.shared.lock();
            st.window_size = (100, 100);
            st.view = Some(View::fit((100, 100), (100, 100)));
            st.ensure_canvas();
        }
        composite(&o)
    };
    let dir = std::env::temp_dir().join(format!("procraft-leave-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // liquified, then off to the gallery with Liquify still open: saved as shown
    assert!(s.liquify_begin());
    let brush = LiquifyBrush { kind: LiquifyKind::Push, size: 0.2, pressure: 1.0, distortion: 0.0 };
    s.liquify_stroke((0..=20).flat_map(|i| [40.0, 30.0 + i as f32 * 2.0, 1.0, 8.0]).collect(), true, false, brush);
    let liquified = composite(&s);
    s.save(dir.join("a").to_string_lossy().into_owned()).unwrap();
    assert!(reopened(&dir.join("a")) == liquified, "Liquify's work lost on leaving");
    // undone inside Liquify, it still liquifies
    s.undo();
    s.liquify_stroke((0..=20).flat_map(|i| [60.0, 30.0 + i as f32 * 2.0, 1.0, 8.0]).collect(), true, false, brush);
    assert!(composite(&s) != liquified, "Liquify stopped after an undo");
    s.adjust_end(true);
    // Crop and Resize open, a smaller frame not applied, off to the gallery: the canvas as it was
    let before = composite(&s);
    s.crop_begin();
    s.crop_set_size(Some(60), Some(60));
    s.save(dir.join("b").to_string_lossy().into_owned()).unwrap();
    assert!(reopened(&dir.join("b")) == before, "Crop and Resize lost the work on leaving");
    s.crop_end(false);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_text_is_sized_and_placed_as_it_looks_on_screen() {
    // a canvas shown a pixel a window pixel, two pixels a point (as the reference's Screen Size
    // canvas shows): letters 128 pixels, a box 976.9 wide, centred across, its middle a quarter
    // of the way down, as measured there
    let s = CanvasSession::new(2266, 1488);
    let t = s.new_text_placed(text_info("Text"), 2266.0, 1488.0, 2.0);
    assert!((t.size - 128.0).abs() < 1e-3 && (t.width - 976.896).abs() < 1e-3, "{} {}", t.size, t.width);
    assert!((t.x - 644.552).abs() < 1e-3, "{}", t.x);
    let tall = crate::text_draw::text_box(t.clone()).get(3).copied().unwrap();
    assert!((t.y + tall / 2.0 - 372.0).abs() < 1e-3, "{} {tall}", t.y);
    // a point of half the pixels: half as big on the canvas, the same on screen
    let half = s.new_text_placed(text_info("Text"), 2266.0, 1488.0, 1.0);
    assert!((half.size - 64.0).abs() < 1e-3 && (half.width - 488.448).abs() < 1e-3);
    // a text kept in another typeface: new text starts in it (its size still the screen's)
    let id = s.text_begin(None, t.clone()).unwrap();
    let serif = crate::TextInfo { font: "serif".to_owned(), style: "Bold".to_owned(), ..t.clone() };
    assert!(s.set_text(serif));
    s.text_end(true);
    assert!(s.layer_text(id).is_some());
    let next = s.new_text_placed(text_info("Text"), 2266.0, 1488.0, 2.0);
    assert_eq!((next.font.as_str(), next.style.as_str()), ("serif", "Bold"));
    assert!((next.size - 128.0).abs() < 1e-3);
}

/// On request (`CROSS_OUT=path.png`, `CROSS_FILE=brush file`, `CROSS_SIZE` slider): the brush
/// paints an orange stroke across and a cyan one down through it, as a side-by-side check.
#[test]
fn crossing_strokes_sheet() {
    let (Ok(out), Ok(file)) = (std::env::var("CROSS_OUT"), std::env::var("CROSS_FILE")) else { return };
    let size: f32 = std::env::var("CROSS_SIZE").ok().and_then(|v| v.parse().ok()).unwrap_or(0.3);
    // `CROSS_CANVAS="WxH"`: a canvas that size (else 1200 square), shown at half its size
    let (w, h) = std::env::var("CROSS_CANVAS")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
        .unwrap_or((1200u32, 1200u32));
    let Ok(gpu) = Gpu::headless() else { return };
    let s = CanvasSession::new(w, h);
    {
        let mut st = s.shared.lock();
        st.window_size = (w / 2, h / 2);
        st.view = Some(View::fit((w, h), (w / 2, h / 2)));
        st.gpu = Some(gpu);
        st.ensure_canvas();
    }
    let set = s.import_any_brushes(std::fs::read(&file).unwrap(), Some(file.clone()), 0).unwrap();
    assert!(s.select_brush(set, 0));
    s.set_brush_size(size);
    let main_set = set;
    let line = |x0: f32, y0: f32, x1: f32, y1: f32| {
        let n = 108;
        let at = |i: usize| {
            let t = i as f32 / n as f32;
            sample(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, i as f64 * 8.3)
        };
        s.stroke_begin(PointerKind::Finger, vec![at(0)]);
        for i in 1..n {
            s.stroke_add(vec![at(i)]);
        }
        s.stroke_end(vec![at(n)]);
    };
    // `CROSS_SET="key=value;..."`: studio settings changed first; then one white stroke on dark
    if let Ok(set) = std::env::var("CROSS_SET") {
        s.studio_begin();
        for kv in set.split(';').filter(|kv| !kv.is_empty()) {
            let (k, v) = kv.split_once('=').unwrap();
            s.set_studio_setting(k.to_owned(), v.parse().unwrap());
        }
        // `CROSS_ONE`: one orange stroke across white, at the opacity `CROSS_OPACITY` (else full),
        // 800 canvas px long (400 on the half-size window) in 700 ms (or `CROSS_MS`)
        if std::env::var("CROSS_ONE").is_ok() {
            let opacity: f32 = std::env::var("CROSS_OPACITY").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
            s.set_brush_opacity(opacity);
            s.set_background_color(1.0, 1.0, 1.0, true);
            s.set_color(237.0 / 255.0, 133.0 / 255.0, 68.0 / 255.0);
            let ms: f64 = std::env::var("CROSS_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(700.0);
            // `CROSS_PHASE`: the input that many ms later against the display's frames, and
            // `CROSS_SEED`: strokes counted as already drawn, which seed the stroke's randomness
            let phase: f64 = std::env::var("CROSS_PHASE").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
            if let Some(seed) = std::env::var("CROSS_SEED").ok().and_then(|v| v.parse().ok()) {
                s.shared.lock().strokes = seed;
            }
            // input at 120 a second, as a finger's, along y 150 from `x0` to `x1` in `ms`
            let across = |x0: f32, x1: f32, ms: f64| {
                let n = ((ms / 1000.0 * 120.0).round() as usize).max(2);
                (0..=n)
                    .map(move |i| sample(x0 + (x1 - x0) * i as f32 / n as f32, 150.0, phase + i as f64 * ms / n as f64))
            };
            // `CROSS_FIRST="file|size|x0|x1|ms"`: a stroke with that brush first, the stroke over it
            if let Ok(first) = std::env::var("CROSS_FIRST") {
                let f: Vec<&str> = first.split('|').collect();
                let name = s.brush_library()[main_set as usize].brushes[0].name.clone();
                let other = s.import_any_brushes(std::fs::read(f[0]).unwrap(), Some(f[0].to_owned()), 0).unwrap();
                assert!(s.select_brush(other, 0));
                s.set_brush_size(f[1].parse().unwrap());
                let mut first: Vec<PenSample> =
                    across(f[2].parse().unwrap(), f[3].parse().unwrap(), f[4].parse().unwrap()).collect();
                let last = first.pop().unwrap();
                s.stroke_begin(PointerKind::Finger, vec![first[0]]);
                for p in &first[1..] {
                    s.stroke_add(vec![*p]);
                }
                s.stroke_end(vec![last]);
                // the stroke's own brush again, found by its name (the import moved it)
                let lib = s.brush_library();
                let main = lib
                    .iter()
                    .enumerate()
                    .find_map(|(i, set)| set.brushes.iter().position(|b| b.name == name).map(|j| (i as u32, j as u32)));
                let (main_set, main_index) = main.unwrap();
                assert!(s.select_brush(main_set, main_index));
                s.set_brush_size(size);
                s.set_brush_opacity(opacity);
            }
            // `CROSS_FROM="x0|x1"`: the stroke from x0 to x1 on the window (else 100 to 500)
            let (x0, x1) = std::env::var("CROSS_FROM")
                .ok()
                .and_then(|v| v.split_once('|').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))))
                .unwrap_or((100.0f32, 500.0f32));
            let path: Vec<PenSample> = across(x0, x1, ms).collect();
            let n = path.len() - 1;
            let at = |i: usize| path[i];
            s.stroke_begin(PointerKind::Finger, vec![at(0)]);
            // `CROSS_TAP`: a tap where the stroke would start instead (held 80 ms)
            if std::env::var("CROSS_TAP").is_ok() {
                s.stroke_end(vec![sample(x0, 150.0, 80.0)]);
                s.export_png(out).unwrap();
                return;
            }
            for i in 1..n {
                s.stroke_add(vec![at(i)]);
            }
            s.stroke_end(vec![at(n)]);
            s.export_png(out).unwrap();
            return;
        }
        // `CROSS_TWO`: an orange stroke across white, then a yellow one down through it
        if std::env::var("CROSS_TWO").is_ok() {
            s.set_background_color(1.0, 1.0, 1.0, true);
            s.set_color(237.0 / 255.0, 133.0 / 255.0, 68.0 / 255.0);
            line(100.0, 150.0, 450.0, 150.0);
            s.set_color(1.0, 180.0 / 255.0, 40.0 / 255.0);
            line(275.0, 70.0, 275.0, 250.0);
            s.export_png(out).unwrap();
            return;
        }
        s.set_background_color(0.12, 0.12, 0.12, true);
        s.set_color(1.0, 1.0, 1.0);
        line(100.0, 150.0, 450.0, 150.0);
        s.export_png(out).unwrap();
        return;
    }
    s.set_color(237.0 / 255.0, 133.0 / 255.0, 68.0 / 255.0);
    line(100.0, 150.0, 450.0, 150.0);
    s.set_color(68.0 / 255.0, 209.0 / 255.0, 237.0 / 255.0);
    line(275.0, 70.0, 275.0, 250.0);
    s.export_png(out).unwrap();
}

#[test]
fn the_size_preview_shows_the_brush_s_stamp_soft_where_it_is_soft() {
    let s = CanvasSession::new(64, 64);
    let pick = |name: &str| {
        let found = s
            .brush_library()
            .iter()
            .enumerate()
            .find_map(|(i, set)| set.brushes.iter().position(|b| b.name == name).map(|j| (i as u32, j as u32)));
        if let Some((set, index)) = found {
            s.select_brush(set, index);
        }
        found.is_some()
    };
    assert!(pick("Soft Airbrush"));
    let side = 64usize;
    let soft = s.brush_stamp(side as u32);
    assert_eq!(soft.len(), side * side);
    let at = |p: &[u8], x: usize, y: usize| p[y * side + x];
    // strongest in the middle, fading smoothly out, nothing in the corners
    assert!(at(&soft, 32, 32) > 200, "{}", at(&soft, 32, 32));
    let mid = at(&soft, 48, 32);
    assert!(mid > 10 && mid < 200, "half way out: {mid}");
    assert_eq!(at(&soft, 1, 1), 0);
    // a hard round tip is solid to near its edge
    assert!(pick("Hard Airbrush"));
    let hard = s.brush_stamp(side as u32);
    assert!(at(&hard, 52, 32) > 200, "{}", at(&hard, 52, 32));
    assert_eq!(at(&hard, 1, 1), 0);
}

#[test]
fn a_snapped_ellipse_stays_open_where_its_stroke_did_not_close() {
    use crate::quickshape::QuickShapeKind;
    let Some(s) = session(200, 200) else { return };
    s.set_brush_size(0.2);
    // a circle drawn from the top round to a twelfth short of where it began, held (closed enough
    // to be an ellipse)
    let pts: Vec<PenSample> = (0..=60)
        .map(|i| {
            let t = -std::f32::consts::FRAC_PI_2 + i as f32 / 60.0 * std::f32::consts::TAU * (11.0 / 12.0);
            sample(100.0 + 60.0 * t.cos(), 100.0 + 60.0 * t.sin(), f64::from(i) * 8.0)
        })
        .collect();
    s.stroke_begin(PointerKind::Finger, pts[..1].to_vec());
    s.stroke_add(pts[1..].to_vec());
    let info = s.quickshape_hold().expect("a shape");
    assert_eq!(info.kind, QuickShapeKind::Ellipse);
    s.stroke_end(vec![]);
    s.quickshape_done();
    // painted where it went (its right, bottom and left), not across the gap it left (up and to
    // the left of the top)
    // dark anywhere within 4 pixels
    let dark = |x: u32, y: u32| {
        (x.saturating_sub(4)..=x + 4).any(|x| (y.saturating_sub(4)..=y + 4).any(|y| pixel(&s, x, y)[0] < 128))
    };
    assert!(dark(160, 100) && dark(100, 160) && dark(40, 100));
    let gap = (-std::f32::consts::FRAC_PI_2) - std::f32::consts::TAU / 24.0;
    let (gx, gy) = (100.0 + 60.0 * gap.cos(), 100.0 + 60.0 * gap.sin());
    assert!(!dark(gx.round() as u32, gy.round() as u32), "the gap at {gx},{gy} was closed");
}

#[test]
fn a_pad_is_drawn_again_off_the_caller_s_thread_the_newest_ask_last() {
    let Some(pad) = session(200, 200) else { return };
    let studio = CanvasSession::new(64, 64);
    pad.keep_strokes();
    pad.begin_own_stroke();
    pad.stroke_begin(PointerKind::Finger, vec![sample(10.0, 60.0, 0.0)]);
    pad.stroke_end(vec![sample(190.0, 60.0, 32.0)]);
    // asked many times quickly: each returns at once; the last colour is the one it paints in
    for k in 0..20 {
        std::sync::Arc::clone(&pad).sync_pad(std::sync::Arc::clone(&studio), k as f32 / 19.0, 0.0, 0.0);
    }
    for _ in 0..500 {
        if !pad.pad_syncing() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!pad.pad_syncing());
    {
        let st = pad.shared.lock();
        assert!(!st.holding_frames, "frames go on once it is drawn");
        assert_eq!(st.pad_strokes.as_ref().map(Vec::len), Some(1), "the stroke is kept again");
        assert_eq!(st.color, [1.0, 0.0, 0.0]);
    }
    // the stroke is there again, in the last colour asked for
    assert_eq!(pixel(&pad, 100, 60)[..3], [255, 0, 0]);
}

#[test]
fn the_recent_set_keeps_the_last_eight_brushes_painted_with_and_the_pinned_ones() {
    let Some(s) = session(200, 200) else { return };
    // every brush of the library, by place
    let places: Vec<(u32, u32)> = s
        .brush_library()
        .iter()
        .enumerate()
        .flat_map(|(i, set)| set.brushes.iter().map(move |b| (i as u32, b.index)))
        .collect();
    assert!(places.len() >= 10);
    let paint = |(set, index): (u32, u32)| {
        assert!(s.select_brush(set, index));
        s.stroke_begin(PointerKind::Finger, vec![sample(20.0, 20.0, 0.0)]);
        s.stroke_end(vec![sample(60.0, 20.0, 40.0)]);
    };
    let shown = || s.recent_brushes().iter().map(|r| ((r.set, r.index), r.pinned)).collect::<Vec<_>>();
    // chosen alone it isn't one; painted with, it is
    assert!(s.select_brush(places[0].0, places[0].1));
    assert!(shown().is_empty());
    for &p in &places[..10] {
        paint(p);
    }
    // the last eight, the latest first
    let want: Vec<_> = places[2..10].iter().rev().map(|&p| (p, false)).collect();
    assert_eq!(shown(), want);
    // painted with again, it comes first
    paint(places[5]);
    assert_eq!(shown()[0], (places[5], false));
    assert_eq!(shown().len(), 8);
    // pinned, it stays apart, first; painting doesn't move it; unpinned it is the latest again
    assert!(s.pin_recent_brush(places[3].0, places[3].1, true));
    assert_eq!(shown()[0], (places[3], true));
    assert_eq!(shown().iter().filter(|e| e.0 == places[3]).count(), 1);
    paint(places[3]);
    assert_eq!(shown()[0], (places[3], true));
    // kept between sessions
    let kept = s.saved_brush_choices();
    s.clear_recent_brushes();
    assert!(shown().is_empty());
    assert!(s.load_brush_choices(kept));
    assert_eq!(shown()[0], (places[3], true));
    assert!(s.pin_recent_brush(places[3].0, places[3].1, false));
    assert_eq!(shown()[0], (places[3], false));
    // cleared, it goes
    assert!(s.clear_recent_brush(places[3].0, places[3].1));
    assert!(!shown().iter().any(|e| e.0 == places[3]));
    // a brush deleted leaves it, and those after it in its set move up
    let (set, index) = places[9];
    let after = s.brush_library()[set as usize].brushes.len() as u32 > index + 1;
    assert!(s.delete_brush(set, index));
    assert!(!shown().iter().any(|e| e.0 == (set, index)) || after);
}

#[test]
fn a_quick_menu_brush_follows_its_brush_about_the_library_and_goes_with_it() {
    let Some(s) = session(200, 200) else { return };
    let library = s.brush_library();
    let set = library.iter().position(|set| set.brushes.len() >= 3).expect("a set of three") as u32;
    let name = library[set as usize].brushes[2].name.clone();
    let key = s.quick_brush_key(set, 2);
    assert_ne!(key, 0);
    // the same brush, the same key; no brush there, none
    assert_eq!(s.quick_brush_key(set, 2), key);
    assert_eq!(s.quick_brush_key(set, 9999), 0);
    assert_eq!(s.quick_brush(key).map(|b| (b.index, b.name)), Some((2, name.clone())));
    // moved to the front of its set, it is found there
    assert!(s.move_brush(set, 2, set, 0));
    assert_eq!(s.quick_brush(key).map(|b| (b.index, b.name)), Some((0, name.clone())));
    // kept between sessions
    let kept = s.saved_brush_choices();
    assert!(s.load_brush_choices(kept));
    assert_eq!(s.quick_brush(key).map(|b| b.name), Some(name));
    // deleted, the key stands for nothing
    assert!(s.delete_brush(set, 0));
    assert_eq!(s.quick_brush(key), None);
}

/// With a panel open a tap leaves no dot, and a drag paints from where it came down, though it
/// waited to be a drag.
#[test]
fn a_touch_with_a_panel_open_paints_only_as_a_drag() {
    let Some(s) = session(256, 256) else { return };
    let before = pixel(&s, 60, 60);
    s.set_touch_panel_open(true);
    s.touch_down(1, PointerKind::Finger, vec![sample(60.0, 60.0, 0.0)]);
    s.touch_up(1, vec![sample(60.0, 60.0, 30.0)]);
    assert_eq!(pixel(&s, 60, 60), before, "a tap paints nothing");
    assert!(!s.info().can_undo);

    s.touch_down(1, PointerKind::Finger, vec![sample(60.0, 60.0, 100.0)]);
    s.touch_move(1, vec![sample(70.0, 60.0, 120.0)]);
    assert_eq!(pixel(&s, 60, 60), before, "nothing yet, while it may be a tap");
    let out = s.touch_move(1, vec![sample(100.0, 60.0, 140.0)]);
    assert!(out.contains(&crate::TouchCommand::PanelFades), "{out:?}");
    assert_ne!(pixel(&s, 60, 60), before, "shown from where it came down");
    s.touch_move(1, vec![sample(180.0, 60.0, 200.0)]);
    s.touch_up(1, vec![sample(180.0, 60.0, 220.0)]);
    assert_ne!(pixel(&s, 60, 60), before);
    assert_ne!(pixel(&s, 120, 60), before);
    assert!(s.info().can_undo);
}

/// Lifted moving with momentum, liquify goes on by the touch timer until it runs out, and then
/// the timer stops; without momentum it stops as it lifts.
#[test]
fn liquify_lifted_moving_goes_on_by_the_touch_timer() {
    use crate::Adjustment;
    let drag = |momentum: f32| -> Option<std::sync::Arc<CanvasSession>> {
        let s = session(256, 256)?;
        s.set_brush_size(0.1);
        s.stroke_begin(PointerKind::Finger, vec![sample(128.0, 10.0, 0.0)]);
        s.stroke_add((1..30).map(|i| sample(128.0, 10.0 + i as f32 * 8.0, i as f64 * 8.0)).collect());
        s.stroke_end(vec![]);
        assert!(s.adjustment_start(Adjustment::Liquify));
        s.adjustment_liquify_value(0, 0.2);
        s.adjustment_liquify_value(3, momentum);
        s.touch_down(1, PointerKind::Finger, vec![sample(60.0, 128.0, 1000.0)]);
        for i in 1..=10 {
            s.touch_move(1, vec![sample(60.0 + i as f32 * 6.0, 128.0, 1000.0 + f64::from(i) * 8.0)]);
        }
        s.touch_up(1, vec![sample(120.0, 128.0, 1088.0)]);
        Some(s)
    };
    let Some(still) = drag(0.0) else { return };
    assert_eq!(still.touch_deadline(), None);
    let Some(s) = drag(1.0) else { return };
    let row = |s: &CanvasSession| (100..220).map(|x| pixel(s, x, 128)).collect::<Vec<_>>();
    let lifted = row(&s);
    let mut now = 1088.0f64;
    let mut ticks = 0;
    while let Some(due) = s.touch_deadline() {
        now = now.max(due);
        s.touch_tick(now);
        ticks += 1;
        assert!(ticks < 200, "it never stopped");
    }
    assert!(ticks > 30, "{ticks} ticks");
    assert_ne!(row(&s), lifted, "it went on after the lift");
}

/// Liquify held still works on as the time passes, a stamp at a time adding up to the whole of it.
#[test]
fn liquify_held_still_works_on() {
    use crate::Adjustment;
    use crate::adjust::LiquifyKind;
    // a line across the canvas (or a short bar across it, right of the middle), and the brush held
    // in the middle for half a second
    let held = |kind: LiquifyKind, bar: bool| -> Option<std::sync::Arc<CanvasSession>> {
        let s = session(256, 256)?;
        s.set_color(1.0, 0.0, 0.0);
        s.set_brush_size(0.1);
        if bar {
            s.stroke_begin(PointerKind::Finger, vec![sample(168.0, 112.0, 0.0)]);
            s.stroke_add((1..9).map(|i| sample(168.0, 112.0 + i as f32 * 4.0, i as f64 * 8.0)).collect());
        } else {
            s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 128.0, 0.0)]);
            s.stroke_add((1..30).map(|i| sample(10.0 + i as f32 * 8.0, 128.0, i as f64 * 8.0)).collect());
        }
        s.stroke_end(vec![]);
        assert!(s.adjustment_start(Adjustment::Liquify));
        s.adjustment_liquify_value(0, 0.28);
        s.adjustment_liquify_kind(kind);
        s.touch_down(1, PointerKind::Finger, vec![sample(128.0, 128.0, 1000.0)]);
        assert_eq!(s.touch_deadline(), Some(1016.0));
        for ms in (1016..=1500).step_by(16) {
            s.touch_tick(f64::from(ms));
        }
        s.touch_up(1, vec![sample(128.0, 128.0, 1510.0)]);
        // lifted, it stops
        s.touch_tick(1600.0);
        assert_eq!(s.touch_deadline(), None);
        Some(s)
    };
    let ink = |s: &CanvasSession, x: u32, y: u32| {
        let p = pixel(s, x, y);
        p[0] > 150 && p[1] < 150
    };
    let Some(right) = held(LiquifyKind::TwirlRight, false) else { return };
    // 0.39 s at 4 a second, 50 from the middle of a brush of 101: turned 44 degrees, clockwise
    assert!(ink(&right, 164, 163) && !ink(&right, 164, 93), "turned clockwise on the screen");
    assert!(!ink(&right, 178, 128), "turned away from where it was");
    let Some(left) = held(LiquifyKind::TwirlLeft, false) else { return };
    assert!(ink(&left, 164, 93) && !ink(&left, 164, 163), "turned anticlockwise on the screen");
    let Some(pinch) = held(LiquifyKind::Pinch, true) else { return };
    assert!(ink(&pinch, 152, 128) && !ink(&pinch, 168, 128), "drawn in");
    let Some(expand) = held(LiquifyKind::Expand, true) else { return };
    assert!(ink(&expand, 183, 128) && !ink(&expand, 168, 128), "pushed out");
    // a push held still does nothing more
    let Some(s) = session(256, 256) else { return };
    assert!(s.adjustment_start(Adjustment::Liquify));
    s.touch_down(1, PointerKind::Finger, vec![sample(128.0, 128.0, 0.0)]);
    assert!(s.touch_tick(200.0).is_empty());
    s.touch_up(1, vec![sample(128.0, 128.0, 210.0)]);
    assert_eq!(s.touch_deadline(), None);
}

/// Whether each of `spots` shows a dot in the composite (the background is white).
fn dots_shown(s: &CanvasSession, spots: &[(u32, u32)]) -> Vec<bool> {
    let mut guard = s.shared.lock();
    let st = &mut *guard;
    let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
    canvas.render_finished(gpu, &mut st.doc);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
    let px = canvas.read_composite(gpu).unwrap();
    let w = st.doc.width();
    spots
        .iter()
        .map(|&(x, y)| {
            let i = ((y * w + x) * 4) as usize;
            px[i..i + 3].iter().any(|c| *c < 200)
        })
        .collect()
}

/// Whether each of `spots` should show a dot by the layers panel: some paint layer holding it (as
/// its pixels say) has its box ticked, and so does every group it is in. Checks the boxes too: while
/// a layer or group is shown alone, only it and what is in it may show ticked.
fn dots_listed(s: &CanvasSession, spots: &[(u32, u32)]) -> Vec<bool> {
    use crate::layers::{LayerInfo, LayerType};
    let layers = s.layers();
    let solo = s.solo();
    let by_id = |id: u64| layers.iter().find(|l| l.id == id);
    let ancestors = |l: &LayerInfo| {
        let mut out = Vec::new();
        let mut at = l.parent;
        while let Some(g) = at.and_then(by_id) {
            if out.len() > layers.len() {
                break;
            }
            out.push(g);
            at = g.parent;
        }
        out
    };
    let ticked = |l: &LayerInfo| l.visible && !l.soloed_out;
    let mut guard = s.shared.lock();
    let st = &mut *guard;
    let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
    canvas.render_finished(gpu, &mut st.doc);
    let w = st.doc.width();
    let mut out = vec![false; spots.len()];
    for l in layers.iter().filter(|l| l.kind == LayerType::Paint) {
        let up = ancestors(l);
        let alone = solo.is_none_or(|g| l.id == g || up.iter().any(|a| a.id == g));
        assert_eq!(l.soloed_out, !alone, "{} #{} shown alone or not", l.name, l.id);
        let shows = ticked(l) && up.iter().all(|g| ticked(g));
        let px = canvas.read_layer(gpu, procraft_engine::document::LayerId(l.id)).unwrap();
        for (k, &(x, y)) in spots.iter().enumerate() {
            if shows && px[((y * w + x) * 4 + 3) as usize] > 200 {
                out[k] = true;
            }
        }
    }
    out
}

/// Each row as the panel lists it, for a failure's message.
fn rows(s: &CanvasSession) -> String {
    let rows: Vec<String> = s
        .layers()
        .iter()
        .map(|l| {
            let indent = "  ".repeat(l.depth as usize);
            let flags = [
                (!l.visible, " hidden"),
                (l.soloed_out, " soloed out"),
                (l.current, " current"),
                (l.picked, " picked"),
            ];
            let flags: String = flags.iter().filter(|(on, _)| *on).map(|(_, f)| *f).collect();
            format!("{indent}{} #{} {:?}{flags}", l.name, l.id, l.kind)
        })
        .collect();
    rows.join("\n")
}

/// Layers made, painted, picked, grouped (by Group, Combine Down and into groups below), moved,
/// duplicated, deleted, hidden and shown alone, with undo and redo, then saved and opened again:
/// after every step the picture shows a layer's dot exactly when the panel shows it ticked in
/// ticked groups. A `side` canvas, its layers' textures held to `resident` layers (0: all).
fn picture_follows_the_panel(seed: u32, side: u32, resident: u64, steps: usize) {
    use crate::layers::LayerType;
    let Some(s) = session(side, side) else { return };
    {
        let mut st = s.shared.lock();
        let canvas = st.canvas.as_mut().unwrap();
        canvas.set_cache_composites(true);
        if resident > 0 {
            canvas.set_resident_budget(resident * u64::from(side) * u64::from(side) * 4);
        }
    }
    s.set_brush_size(0.2);
    let mut state = seed;
    let mut next = |n: u32| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) % n.max(1)
    };
    // one dot per new layer, apart (copies share theirs)
    let cell = side / 10;
    let spots: Vec<(u32, u32)> = (0..81).map(|k| ((k % 9 + 1) * cell, (k / 9 + 1) * cell)).collect();
    let dot = |s: &CanvasSession, k: usize| {
        let (x, y) = spots[k];
        let c = (k as f32 * 0.37) % 1.0;
        s.set_color(c * 0.6, 0.2, 0.8 - c * 0.5);
        s.stroke_begin(PointerKind::Finger, vec![sample(x as f32, y as f32, 0.0)]);
        s.stroke_end(vec![]);
    };
    dot(&s, 0);
    let mut used = 1;
    let mut done: Vec<String> = Vec::new();
    for step in 0..steps {
        let layers = s.layers();
        let any = |k: u32| layers.get(k as usize % layers.len().max(1)).map_or(0, |l| l.id);
        let groups: Vec<u64> = layers.iter().filter(|l| l.kind == LayerType::Group).map(|l| l.id).collect();
        let group = |k: u32| groups.get(k as usize % groups.len().max(1)).copied();
        let what = match next(18) {
            0 | 1 => {
                if used < spots.len() && s.add_layer() != 0 {
                    dot(&s, used);
                    used += 1;
                }
                "new layer, painted".to_owned()
            }
            2 => {
                let id = any(next(64));
                format!("select {id}: {}", s.select_layer(id))
            }
            3 | 4 => {
                let id = any(next(64));
                format!("pick {id}: {}", s.pick_layer(id))
            }
            5 | 6 => format!("group picked: {}", s.group_picked()),
            7 => {
                let id = any(next(64));
                format!("combine down {id}: {:?}", s.combine_down(id))
            }
            8 => group(next(8)).map_or_else(String::new, |g| format!("ungroup {g}: {}", s.ungroup(g))),
            9 => {
                let (id, to) = (any(next(64)), next(layers.len() as u32 + 1));
                format!("move {id} to {to}: {}", s.move_layer(id, to))
            }
            10 => {
                let (id, by, above) = (any(next(64)), any(next(64)), next(2) == 0);
                format!("move {id} next to {by} (above {above}): {}", s.move_next_to(id, by, above))
            }
            11 => {
                let id = group(next(8)).filter(|_| next(2) == 0).unwrap_or_else(|| any(next(64)));
                let visible = next(2) == 0;
                format!("visible {id} {visible}: {}", s.set_layer_visible(id, visible))
            }
            12 => {
                let id = any(next(64));
                format!("duplicate {id}: {:?}", s.duplicate_layer(id))
            }
            13 => format!("duplicate picked: {}", s.duplicate_picked()),
            14 => {
                let id = any(next(64));
                format!("delete {id}: {}", s.delete_layer(id))
            }
            15 => {
                if next(2) == 0 {
                    format!("undo: {:?}", s.undo())
                } else {
                    format!("redo: {:?}", s.redo())
                }
            }
            16 => format!("pick all: {}", s.pick_all_layers()),
            _ => {
                let id = group(next(8)).filter(|_| next(2) == 0).unwrap_or_else(|| any(next(64)));
                if next(3) == 0 {
                    format!("box tapped {id}: {:?}", s.tap_visibility(id))
                } else {
                    format!("box held {id}: {:?}", s.hold_visibility(id))
                }
            }
        };
        done.push(format!("{step}: {what}"));
        let (listed, shown) = (dots_listed(&s, &spots[..used]), dots_shown(&s, &spots[..used]));
        let differ: Vec<usize> = (0..used).filter(|k| listed[*k] != shown[*k]).collect();
        assert!(
            differ.is_empty(),
            "seed {seed}, side {side}: dots {differ:?} show otherwise than listed after\n{}\nrows:\n{}",
            done.join("\n"),
            rows(&s)
        );
    }
    let before = dots_shown(&s, &spots[..used]);
    let gpu = s.shared.lock().gpu.clone().unwrap();
    let dir = std::env::temp_dir().join(format!("procraft-panel-{}-{seed}-{side}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    s.save(dir.to_string_lossy().into_owned()).unwrap();
    let opened = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    opened.use_gpu(gpu);
    {
        let mut st = opened.shared.lock();
        st.window_size = (side, side);
        st.view = Some(View::fit((side, side), (side, side)));
        st.ensure_canvas();
        st.canvas.as_mut().unwrap().set_cache_composites(true);
    }
    let after = dots_shown(&opened, &spots[..used]);
    assert_eq!(after, dots_listed(&opened, &spots[..used]), "seed {seed}: opened again\n{}", done.join("\n"));
    // the same picture, unless what was shown alone couldn't show (then every layer shows)
    if opened.solo() == s.solo() {
        assert_eq!(after, before, "seed {seed}: opened again\n{}", done.join("\n"));
    }
}

#[test]
fn the_picture_shows_what_the_layers_panel_shows_through_group_work() {
    for seed in 1..=10 {
        picture_follows_the_panel(seed, 100, 0, 60);
    }
    // layers past what the GPU holds kept compressed
    for seed in 11..=13 {
        picture_follows_the_panel(seed, 400, 3, 60);
    }
}

#[test]
fn while_a_layer_is_shown_alone_the_other_boxes_show_hidden_and_a_tap_ends_it() {
    let Some(s) = session(100, 100) else { return };
    let below = s.layers()[0].id;
    let top = s.add_layer();
    let ticked = |s: &CanvasSession| s.layers().iter().map(|l| l.visible && !l.soloed_out).collect::<Vec<_>>();
    assert_eq!(ticked(&s), [true, true]);
    assert_eq!(s.hold_visibility(top).as_deref(), Some("Solo visibility on"));
    assert_eq!(s.solo(), Some(top));
    // listed top first
    assert_eq!(ticked(&s), [true, false]);
    // a tap on any box shows every layer again, changing none
    assert_eq!(s.tap_visibility(below).as_deref(), Some("Solo visibility off"));
    assert_eq!((s.solo(), ticked(&s)), (None, vec![true, true]));
    assert_eq!(s.hold_visibility(below).as_deref(), Some("Solo visibility on"));
    assert_eq!(s.tap_visibility(below).as_deref(), Some("Solo visibility off"));
    assert!(s.layers().iter().all(|l| l.visible));
    // otherwise a tap shows or hides it, and holding again ends it
    assert_eq!(s.tap_visibility(top).as_deref(), Some("Hide layer"));
    assert_eq!(ticked(&s), [false, true]);
    assert_eq!(s.tap_visibility(top).as_deref(), Some("Show layer"));
    assert_eq!(s.hold_visibility(below).as_deref(), Some("Solo visibility on"));
    assert_eq!(s.hold_visibility(below).as_deref(), Some("Solo visibility off"));
    assert_eq!(s.tap_visibility(999), None);
}
