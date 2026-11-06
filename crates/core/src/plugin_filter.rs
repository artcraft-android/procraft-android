//! A plugin's filter open on the current layer. It goes the way the adjustments do: the layer is
//! copied, each change redraws it from the copy, Apply keeps that as one undo step and Cancel puts
//! the copy back. Its values are the controls its manifest lists, so one panel in each app draws
//! any filter. Nothing of it goes in the artwork: what it did is pixels, and an artwork never
//! depends on a plugin.
//!
//! What keeps a filter from running away is in procraft-gpu (`plugin_filter.rs`). This is what is
//! done about one that gets past that, or that a driver falls over on: the artwork is saved before
//! the first filter of a session, a filter on trial is written down until it has drawn, and one
//! that stops is switched off with its reason on its card, for someone to turn on again. It is
//! never tried again by itself: on the tablet tested, it is the third or fourth hang that loses the
//! device and every layer with it.

use std::sync::Arc;
use std::time::Duration;

use procraft_gpu::{Canvas, FilterProgram, FilterRun};

use crate::plugins::{PluginControl, PluginSlider, Plugins, STOPPED_DRIVER};
use crate::{CanvasSession, State};

/// A draw that takes longer than this can't follow a finger, so it waits for the finger to let go.
const FOLLOWS_A_FINGER: Duration = Duration::from_millis(150);

const STOPPED_SLOW: &str = "Switched off: it took too long.";
const STOPPED_DEVICE: &str = "Switched off: it can't run on this device.";
const TOO_BIG: &str = "This filter takes too long on a canvas this big.";
const NO_DEVICE: &str = "The canvas isn't ready for a filter yet.";

/// What a filter's panel shows.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginFilterShown {
    pub name: String,
    pub controls: Vec<PluginControl>,
}

/// A filter as it stands while open, and after, to open again.
struct OpenFilter {
    plugins: Arc<Plugins>,
    id: String,
    name: String,
    program: Arc<FilterProgram>,
    controls: Vec<PluginControl>,
    /// The last draw took too long to follow a finger.
    slow: bool,
    /// It hasn't drawn yet, so a crash now is put down to it.
    trying: bool,
}

#[derive(Default)]
pub(crate) struct Filters {
    open: Option<OpenFilter>,
    /// The last one open, for Reset and Undo to open it again.
    last: Option<OpenFilter>,
    /// What stopped one, for the app to tell, once.
    said: Option<String>,
    /// The artwork was saved ahead of a first filter.
    saved: bool,
}

impl Filters {
    pub(crate) fn shown(&self) -> Option<PluginFilterShown> {
        self.open.as_ref().map(|open| PluginFilterShown { name: open.name.clone(), controls: open.controls.clone() })
    }

    pub(crate) fn take_said(&mut self) -> Option<String> {
        self.said.take()
    }

    /// The one open ends, and what its undo step is called.
    pub(crate) fn close(&mut self) -> Option<String> {
        let closed = self.open.take()?;
        let name = closed.name.clone();
        self.last = Some(closed);
        Some(name)
    }
}

impl State {
    /// Redraws the filter open with its values as they are.
    pub(crate) fn draw_filter(&mut self) -> bool {
        let Some(open) = self.filters.open.as_ref() else { return false };
        let alpha_locked = self.doc.layer(self.doc.current()).is_some_and(|layer| layer.alpha_lock);
        let values: Vec<f32> = open.controls.iter().map(PluginControl::number).collect();
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.set_plugin_filter(&open.program, &values, alpha_locked);
        }
        true
    }

    /// Takes in how the filter's last draw went. One that failed has put the layer back, and it
    /// ends the adjustment here; true if so.
    pub(crate) fn heed_filter_run(&mut self) -> bool {
        let Some(run) = self.canvas.as_mut().and_then(Canvas::take_filter_run) else { return false };
        let Some(open) = self.filters.open.as_mut() else { return false };
        let trying = std::mem::take(&mut open.trying);
        if trying {
            open.plugins.filter_tried();
        }
        let (stopped, said) = match run {
            FilterRun::Drew(took) => {
                open.slow = took > FOLLOWS_A_FINGER;
                return false;
            }
            FilterRun::Spent => (Some(STOPPED_SLOW), STOPPED_SLOW),
            FilterRun::Cut | FilterRun::Lost => (Some(STOPPED_DRIVER), STOPPED_DRIVER),
            FilterRun::TooLong => (None, TOO_BIG),
        };
        log::warn!("a plugin filter stopped: {}: {run:?}", open.id);
        if let Some(why) = stopped {
            open.plugins.stop_filter(&open.id, why);
        }
        self.filters.said = Some(match stopped {
            Some(_) => format!("{} stopped. {said}", open.name),
            None => said.to_owned(),
        });
        self.filters.open = None;
        self.filters.last = None;
        self.adjust.close_plugin();
        self.end_adjustment(false);
        self.needs_frame = true;
        true
    }
}

impl CanvasSession {
    /// Opens `open` on the current layer, drawn at once; false if the layer can't be adjusted.
    fn open_filter(&self, open: OpenFilter) -> bool {
        let tried = open.trying.then(|| Arc::clone(&open.plugins));
        if !self.adjust_begin() {
            if let Some(plugins) = tried {
                plugins.filter_tried();
            }
            return false;
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.adjust.open_plugin();
        st.filters.open = Some(open);
        st.draw_filter();
        self.shared.frame(st);
        true
    }

    /// The filter last open, again, its values as they start: its Reset, and its Undo.
    pub(crate) fn plugin_filter_again(&self) -> bool {
        let last = self.shared.lock().filters.last.take();
        let Some(mut filter) = last else { return false };
        for control in &mut filter.controls {
            control.back_to_start();
        }
        self.open_filter(filter)
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Opens the filter `id` from `plugins` on the current layer, keeping any adjustment open first.
    /// What to tell if it can't be; nothing if it opened, or if the layer can't be adjusted, as with
    /// [`CanvasSession::adjustment_start`]. Slow, as it saves the artwork (in `saved_in`, the first
    /// time in a session) and compiles the filter: call it off the main thread.
    pub fn plugin_filter_start(&self, plugins: Arc<Plugins>, id: String, saved_in: Option<String>) -> Option<String> {
        self.adjustment_end(true);
        let ready = match plugins.filter_to_open(&id) {
            Ok(ready) => ready,
            Err(why) => return Some(why),
        };
        let (gpu, first) = {
            let mut st = self.shared.lock();
            (st.gpu.clone(), !std::mem::replace(&mut st.filters.saved, true))
        };
        let Some(gpu) = gpu else { return Some(NO_DEVICE.to_owned()) };
        // all a hang or a crash can then cost is what came after this
        if first
            && let Some(dir) = saved_in
            && let Err(e) = self.save(dir)
        {
            log::warn!("saving ahead of a filter: {e}");
        }
        // a driver that falls over compiling it takes the app with it, leaving only this
        plugins.filter_trying(&id);
        let program = match gpu.make_filter(ready.code) {
            Ok(program) => program,
            Err(e) => {
                log::warn!("a plugin filter wouldn't compile: {e}");
                plugins.filter_tried();
                plugins.stop_filter(&id, STOPPED_DEVICE);
                return Some(format!("{} stopped. {STOPPED_DEVICE}", ready.name));
            }
        };
        let open =
            OpenFilter { plugins, id, name: ready.name, program, controls: ready.controls, slow: false, trying: true };
        self.open_filter(open);
        None
    }

    /// Moves the open filter's slider `key` to `value`; where it landed. It redraws as it goes,
    /// unless a draw takes too long for that: then `done` (let go) is what redraws it.
    pub fn plugin_filter_slider(&self, key: String, value: f32, done: bool) -> Option<f32> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.heed_filter_run();
        let open = st.filters.open.as_mut()?;
        let slider = open.controls.iter_mut().find_map(|control| match control {
            PluginControl::Slider { control } if control.key == key => Some(control),
            _ => None,
        })?;
        let landed = PluginSlider::stepped(slider, value);
        slider.value = landed;
        let waits = open.slow && !done;
        st.adjust.touch();
        if !waits && st.draw_filter() {
            self.shared.frame(st);
        }
        Some(landed)
    }

    /// Sets the open filter's switch `key`, and redraws it.
    pub fn plugin_filter_switch(&self, key: String, on: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.heed_filter_run();
        let Some(open) = st.filters.open.as_mut() else { return };
        let switch = open.controls.iter_mut().find_map(|control| match control {
            PluginControl::Switch { control } if control.key == key => Some(control),
            _ => None,
        });
        let Some(switch) = switch else { return };
        switch.on = on;
        st.adjust.touch();
        if st.draw_filter() {
            self.shared.frame(st);
        }
    }
}

#[cfg(test)]
mod tests {
    use procraft_engine::view::View;
    use procraft_gpu::Gpu;
    use serde_json::{Value, json};

    use super::*;
    use crate::plugin_file::sha256;
    use crate::plugin_file::tests::{filter as manifest, pack};
    use crate::{Adjustment, PenSample, PointerKind};

    /// Clears what isn't dark, by the amount of `cut`.
    const LINES: &str = "fn filter(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
        let dark = 1.0 - dot(colour.rgb, vec3<f32>(0.299, 0.587, 0.114));
        let shade = select(vec3<f32>(0.0), colour.rgb, v.keep_colour > 0.5);
        return vec4<f32>(shade, step(v.cut, dark) * colour.a);
    }";

    const ID: &str = "org.example.lines";

    fn store(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("procraft-filters-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.to_string_lossy().into_owned()
    }

    /// A plugin file of `code`, with the controls every manifest here has.
    fn file_of(id: &str, code: &str) -> Vec<u8> {
        let mut manifest = manifest(id);
        manifest["files"] = json!([{ "path": "filter.wgsl", "sha256": sha256(code.as_bytes()), "size": code.len() }]);
        pack(&manifest, &[("filter.wgsl", code.as_bytes())])
    }

    /// A session on a headless GPU with a black stroke down the middle, or none without a GPU.
    fn session() -> Option<Arc<CanvasSession>> {
        let gpu = match Gpu::headless() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipped: {e}");
                return None;
            }
        };
        let s = CanvasSession::new(64, 64);
        {
            let mut st = s.shared.lock();
            st.window_size = (64, 64);
            st.view = Some(View::fit((64, 64), (64, 64)));
            st.gpu = Some(gpu);
            st.ensure_canvas();
        }
        let sample = |x: f32, y: f32, t: f64| PenSample {
            x,
            y,
            pressure: 1.0,
            tilt: 0.0,
            orientation: 0.0,
            time_ms: t,
            roll: 0.0,
        };
        s.set_color(0.0, 0.0, 0.0);
        s.stroke_begin(PointerKind::Finger, vec![sample(32.0, 4.0, 0.0)]);
        s.stroke_add((1..16).map(|i| sample(32.0, 4.0 + i as f32 * 4.0, f64::from(i) * 8.0)).collect());
        s.stroke_end(vec![]);
        Some(s)
    }

    fn layer(s: &CanvasSession) -> Vec<u8> {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let current = st.doc.current();
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        assert_eq!(gpu.errors(), 0, "GPU validation errors");
        let read = canvas.read_layer(gpu, current).unwrap();
        st.heed_filter_run();
        read
    }

    fn at(px: &[u8], x: usize, y: usize) -> [u8; 4] {
        let i = (y * 64 + x) * 4;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    /// The layer of `s` made opaque white under its stroke, as a scan would be, and read.
    fn on_paper(s: &CanvasSession) -> Vec<u8> {
        let mut px = layer(s);
        for pixel in px.as_chunks_mut::<4>().0 {
            if pixel[3] < 255 {
                *pixel = [255; 4];
            }
        }
        {
            let mut st = s.shared.lock();
            let current = st.doc.current();
            st.canvas.as_mut().unwrap().put_layer(current, px);
        }
        layer(s)
    }

    #[test]
    fn a_filter_opens_redraws_as_it_slides_and_cancel_puts_the_layer_back() {
        let Some(s) = session() else { return };
        let dir = store("cancel");
        let plugins = Plugins::with_store(dir.clone());
        assert!(plugins.filters().is_empty());
        assert_eq!(plugins.install(file_of(ID, LINES)), None);
        assert_eq!(plugins.filters().len(), 1);
        let scan = on_paper(&s);
        assert_eq!(at(&scan, 2, 2), [255, 255, 255, 255]);

        assert_eq!(s.plugin_filter_start(Arc::clone(&plugins), ID.to_owned(), None), None);
        let shown = s.adjustment_shown();
        assert_eq!(shown.adjustment, Some(Adjustment::Plugin));
        let panel = shown.plugin.unwrap();
        assert_eq!(panel.name, "A filter");
        assert_eq!(panel.controls.len(), 2);
        // shown at once, at its start: the paper is gone and the line stays
        let px = layer(&s);
        assert_eq!(at(&px, 2, 2)[3], 0);
        assert_eq!(at(&px, 32, 32), [0, 0, 0, 255]);
        // a value goes in, kept to its range, and redraws from the copy
        assert_eq!(s.plugin_filter_slider(format!("{ID}.cut"), 2.0, false), Some(1.0));
        assert_eq!(at(&layer(&s), 32, 32)[3], 255);
        assert_eq!(s.plugin_filter_slider(format!("{ID}.cut"), -1.0, true), Some(0.0));
        assert_eq!(at(&layer(&s), 2, 2)[3], 255, "nothing is too light now");
        assert_eq!(s.plugin_filter_slider("nothing".to_owned(), 1.0, true), None);
        // Cancel
        assert!(s.adjustment_end(false));
        assert_eq!(layer(&s), scan);
        assert_eq!(s.adjustment_shown().plugin, None);
        assert_eq!(s.take_failure(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn apply_is_one_undo_step_named_after_the_plugin_and_ends_it() {
        let Some(s) = session() else { return };
        let dir = store("apply");
        let plugins = Plugins::with_store(dir.clone());
        assert_eq!(plugins.install(file_of(ID, LINES)), None);
        let scan = on_paper(&s);
        assert_eq!(s.plugin_filter_start(Arc::clone(&plugins), ID.to_owned(), None), None);
        s.plugin_filter_switch(format!("{ID}.keep_colour"), true);
        s.plugin_filter_slider(format!("{ID}.cut"), 0.5, true);
        let filtered = layer(&s);
        s.adjustment_apply();
        assert_eq!(s.adjustment_shown().adjustment, None);
        assert_eq!(layer(&s), filtered);
        assert_eq!(s.undo().as_deref(), Some("A filter"));
        assert_eq!(layer(&s), scan);
        assert_eq!(s.redo().as_deref(), Some("A filter"));
        assert_eq!(layer(&s), filtered);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reset_preview_and_undo_keep_it_open_at_its_start() {
        let Some(s) = session() else { return };
        let dir = store("reset");
        let plugins = Plugins::with_store(dir.clone());
        assert_eq!(plugins.install(file_of(ID, LINES)), None);
        let scan = on_paper(&s);
        assert_eq!(s.plugin_filter_start(Arc::clone(&plugins), ID.to_owned(), None), None);
        let starts = layer(&s);
        s.plugin_filter_slider(format!("{ID}.cut"), 0.0, true);
        let moved = layer(&s);
        assert_ne!(moved, starts);
        // Preview shows what was, and back
        s.adjustment_preview(true);
        assert_eq!(layer(&s), scan);
        s.adjustment_preview(false);
        assert_eq!(layer(&s), moved);
        // Reset: its values and its picture as it opened
        s.adjustment_reset();
        assert_eq!(s.adjustment_shown().adjustment, Some(Adjustment::Plugin));
        assert_eq!(layer(&s), starts);
        let cut = s.adjustment_shown().plugin.unwrap().controls.into_iter().find_map(|control| match control {
            PluginControl::Slider { control } => Some(control.value),
            PluginControl::Switch { .. } => None,
        });
        assert_eq!(cut, Some(0.5));
        // Undo the same, and it stays open
        s.plugin_filter_slider(format!("{ID}.cut"), 0.0, true);
        s.adjustment_undo();
        assert_eq!(s.adjustment_shown().adjustment, Some(Adjustment::Plugin));
        assert_eq!(layer(&s), starts);
        assert!(s.adjustment_end(false));
        assert_eq!(layer(&s), scan);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_filter_on_trial_when_the_app_ended_comes_back_off_until_turned_on() {
        let dir = store("trial");
        let plugins = Plugins::with_store(dir.clone());
        assert_eq!(plugins.install(file_of(ID, LINES)), None);
        // the app ends with it compiling, or before it has drawn
        plugins.filter_trying(ID);
        let next = Plugins::with_store(dir.clone());
        assert!(next.filters().is_empty());
        let card = next.plugins().into_iter().find(|p| p.id == ID).unwrap();
        assert_eq!(card.problem.as_deref(), Some(STOPPED_DRIVER));
        assert!(!card.on);
        // a value kept as on doesn't bring it back, nor a restart
        next.read_kept(card.on_key.clone(), "true".to_owned());
        assert!(next.filters().is_empty());
        assert!(Plugins::with_store(dir.clone()).filters().is_empty());
        // someone asking does
        assert!(next.set_on(ID.to_owned(), true));
        assert_eq!(next.filters().len(), 1);
        assert_eq!(Plugins::with_store(dir.clone()).filters().len(), 1);
        // a draw that went well leaves nothing standing
        next.filter_trying(ID);
        next.filter_tried();
        assert_eq!(Plugins::with_store(dir.clone()).filters().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_filter_that_stops_is_switched_off_told_once_and_the_layer_goes_back() {
        let Some(s) = session() else { return };
        let dir = store("stops");
        let plugins = Plugins::with_store(dir.clone());
        assert_eq!(plugins.install(file_of(ID, LINES)), None);
        let scan = on_paper(&s);
        assert_eq!(s.plugin_filter_start(Arc::clone(&plugins), ID.to_owned(), None), None);
        layer(&s);
        // as the canvas tells of a draw it had to stop, having put the layer back
        {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            st.canvas.as_mut().unwrap().tell_filter_run(FilterRun::Cut);
            assert!(st.heed_filter_run());
        }
        assert_eq!(s.adjustment_shown().adjustment, None);
        assert_eq!(layer(&s), scan);
        assert!(s.take_failure().is_some_and(|said| said.contains("A filter stopped")));
        assert_eq!(s.take_failure(), None);
        assert!(plugins.filters().is_empty());
        let card = plugins.plugins().into_iter().find(|p| p.id == ID).unwrap();
        assert_eq!(card.problem.as_deref(), Some(STOPPED_DRIVER));
        // it won't open, and it stays so through a restart
        assert_eq!(s.plugin_filter_start(Arc::clone(&plugins), ID.to_owned(), None).as_deref(), Some(STOPPED_DRIVER));
        assert!(Plugins::with_store(dir.clone()).filters().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_filter_the_app_cant_read_is_refused_at_the_door() {
        let refused = |code: &str| {
            let dir = store("door");
            let plugins = Plugins::with_store(dir.clone());
            let told = plugins.install(file_of(ID, code));
            assert!(plugins.filters().is_empty());
            let _ = std::fs::remove_dir_all(dir);
            told
        };
        for code in [
            "fn filter(",
            "@group(0) @binding(0) var t: texture_2d<f32>;\nfn filter(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> { return colour; }",
            "@fragment fn main() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }",
        ] {
            assert_eq!(refused(code).as_deref(), Some("This filter's code can't run in ProCraft."), "{code}");
        }
        // the text is held to its size at the door too
        let huge = format!(
            "// {}\nfn filter(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {{ return colour; }}",
            "x".repeat(70_000)
        );
        assert!(refused(&huge).is_some());
    }

    /// Every sample in the repo's plugins/ folder, as a file imported and opened on a real canvas:
    /// a wgpu upgrade can change what WGSL is accepted, and this is what would show it.
    #[test]
    fn the_samples_compile_and_draw() {
        let Some(s) = session() else { return };
        {
            let st = s.shared.lock();
            let info = st.gpu.as_ref().unwrap().adapter.get_info();
            println!("samples on {} ({:?})", info.name, info.backend);
        }
        // (on a device the repo isn't there: the folder is pushed, and named)
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let root = std::env::var_os("PROCRAFT_SAMPLES").map_or(here, std::path::PathBuf::from);
        let mut count = 0;
        // greys across, and clear at the top
        let ramp = (0..64 * 64).flat_map(|i: usize| {
            let (x, y) = (i % 64, i / 64);
            let grey = (x * 4) as u8;
            if y < 8 { [0, 0, 0, 0] } else { [grey, grey, grey, 255] }
        });
        {
            let mut st = s.shared.lock();
            let current = st.doc.current();
            st.canvas.as_mut().unwrap().put_layer(current, ramp.collect());
        }
        for sample in std::fs::read_dir(root).unwrap() {
            let folder = sample.unwrap().path();
            let code = std::fs::read(folder.join("filter.wgsl")).unwrap();
            let manifest = std::fs::read(folder.join("craft-extension.json")).unwrap();
            let mut manifest: Value = serde_json::from_slice(&manifest).unwrap();
            manifest["files"] = json!([{ "path": "filter.wgsl", "sha256": sha256(&code), "size": code.len() }]);
            let id = manifest["id"].as_str().unwrap().to_owned();
            let dir = store(&format!("sample-{count}"));
            let plugins = Plugins::with_store(dir.clone());
            assert_eq!(plugins.install(pack(&manifest, &[("filter.wgsl", &code)])), None, "{}", folder.display());
            let before = layer(&s);
            assert_eq!(s.plugin_filter_start(Arc::clone(&plugins), id.clone(), None), None, "{id}");
            let after = layer(&s);
            assert_eq!(s.adjustment_shown().adjustment, Some(Adjustment::Plugin), "{id}");
            assert_ne!(before, after, "{id} changed nothing");
            // and for each control, a draw at each end of it
            for control in s.adjustment_shown().plugin.unwrap().controls {
                match control {
                    PluginControl::Slider { control } => {
                        s.plugin_filter_slider(control.key.clone(), control.least, true);
                        layer(&s);
                        s.plugin_filter_slider(control.key, control.most, true);
                        layer(&s);
                    }
                    PluginControl::Switch { control } => {
                        s.plugin_filter_switch(control.key, !control.starts);
                        layer(&s);
                    }
                }
            }
            assert_eq!(s.adjustment_shown().adjustment, Some(Adjustment::Plugin), "{id} stopped");
            assert_eq!(s.take_failure(), None, "{id}");
            assert!(s.adjustment_end(false));
            assert_eq!(layer(&s), before);
            let _ = std::fs::remove_dir_all(dir);
            count += 1;
        }
        assert!(count >= 3, "{count} samples");
    }
}
