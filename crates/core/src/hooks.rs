//! The points where an applied patch steps in. Each does nothing with none applied, and holds no
//! feature of its own: what a patch does is in its file. They stay for good, so there are as few as
//! patches need, and each is as narrow as it can be.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::CanvasSession;
use crate::patch_run::{Hook, Running, Stop};
use crate::plugin_file;
use crate::plugins::Plugins;

struct Applied {
    id: String,
    running: Running,
    /// The hooks it is on, as bits.
    hooks: u32,
}

#[derive(Default)]
struct Patches {
    /// In id order, so patches go in the same order however they were applied.
    applied: Vec<Applied>,
    /// What stopped since the list last asked, and why.
    stopped: Vec<(String, Stop)>,
    /// A stop the app hasn't told yet: whoever is drawing sees the stroke change, not the list.
    untold: Option<Stop>,
}

/// The applied patches, shared by a list and the sessions going through it. Not a static: they go
/// with the list, so one comes off at once, no restart.
#[derive(Default)]
pub(crate) struct Hooks {
    /// The hooks with a patch on them, as bits: all a stroke with none pays for is reading this.
    hooked: AtomicU32,
    patches: Mutex<Patches>,
    /// Where a stop is written down, to stay off through a restart.
    store: Option<PathBuf>,
}

impl std::fmt::Debug for Hooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hooks").field("hooked", &self.hooked).finish_non_exhaustive()
    }
}

impl Hooks {
    pub(crate) fn kept_in(store: Option<PathBuf>) -> Hooks {
        Hooks { store, ..Hooks::default() }
    }

    fn patches(&self) -> MutexGuard<'_, Patches> {
        self.patches.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Writes down that `id` stopped, for the list and for the next start.
    fn note_stop(&self, patches: &mut Patches, id: String, stop: Stop) {
        log::warn!("a patch stopped: {id}: {stop:?}");
        if let Some(store) = &self.store {
            plugin_file::note_stopped(store, &id, stop.text());
        }
        patches.stopped.push((id, stop));
        patches.untold = Some(stop);
    }

    fn has(&self, hook: Hook) -> bool {
        self.hooked.load(Ordering::Relaxed) & hook.bit() != 0
    }

    /// The bits go by what is on now.
    fn rehook(&self, patches: &Patches) {
        let hooked = patches.applied.iter().fold(0, |bits, patch| bits | patch.hooks);
        self.hooked.store(hooked, Ordering::Relaxed);
    }

    /// `start` through each patch on `hook`, in turn. One that stops is off the hooks there and then,
    /// and what it was handed goes on as it came to it.
    fn through<T: Copy>(&self, hook: Hook, start: T, mut step: impl FnMut(&mut Running, T) -> Result<T, Stop>) -> T {
        let mut patches = self.patches();
        let mut at = start;
        let mut i = 0;
        while let Some(patch) = patches.applied.get_mut(i) {
            if patch.hooks & hook.bit() == 0 {
                i += 1;
                continue;
            }
            match step(&mut patch.running, at) {
                Ok(to) => {
                    at = to;
                    i += 1;
                }
                Err(stop) => {
                    let gone = patches.applied.remove(i);
                    self.note_stop(&mut patches, gone.id, stop);
                }
            }
        }
        self.rehook(&patches);
        at
    }

    /// The position hook: where a sample in window pixels lands, before the stroke sees it.
    #[inline]
    pub(crate) fn position(&self, x: f32, y: f32) -> (f32, f32) {
        // what isn't a position goes on as it came, and no patch answers for it
        if !self.has(Hook::Position) || !x.is_finite() || !y.is_finite() {
            return (x, y);
        }
        self.through(Hook::Position, (x, y), |running, at| running.position(at.0, at.1))
    }

    /// The pick hook: whether the eyedropper reads the current layer, not the canvas as it is
    /// composited. It only asks: reading either is the app's, and it is a read like any other.
    pub(crate) fn pick_layer(&self) -> bool {
        self.has(Hook::PickLayer)
            && self.through(Hook::PickLayer, false, |running, said| Ok(said | running.pick_layer()?))
    }

    /// The scale hook: how big the interface shows in a window `width` × `height` points at 1. The
    /// number is the patch's, and all the app adds is what keeps every page in reach: see
    /// [`crate::interface_size`].
    pub(crate) fn interface_scale(&self, width: f32, height: f32) -> f32 {
        if !self.has(Hook::InterfaceScale) {
            return 1.0;
        }
        let said = self.through(Hook::InterfaceScale, 1.0, |running, _| running.interface_scale(width, height));
        said.min(crate::interface_size::most_for(width, height))
    }

    /// The paste hook: whether a picture pasted with a mask current goes into the mask.
    pub(crate) fn paste_mask(&self) -> bool {
        self.has(Hook::PasteMask)
            && self.through(Hook::PasteMask, false, |running, said| Ok(said | running.paste_mask()?))
    }

    /// `rgba`, rows of `width` pixels, as the patch on the paste hook makes it; false if it can't,
    /// and what has gone through is no use. The first that takes it has it all: it holds up nothing
    /// that doesn't need the same patch.
    pub(crate) fn paste_rows(&self, rgba: &mut [u8], width: usize) -> bool {
        let mut stopped = false;
        let done = self.through(Hook::PasteMask, false, |running, done| {
            if done || stopped {
                return Ok(done);
            }
            running.paste_rows(rgba, width).inspect_err(|_| stopped = true)?;
            Ok(true)
        });
        done && !stopped
    }

    pub(crate) fn is_on(&self, id: &str) -> bool {
        self.patches().applied.iter().any(|p| p.id == id)
    }

    pub(crate) fn put_on(&self, id: String, running: Running, hooks: &[Hook]) {
        let mut patches = self.patches();
        patches.applied.retain(|p| p.id != id);
        let at = patches.applied.partition_point(|p| p.id < id);
        let hooks = hooks.iter().fold(0, |bits, hook| bits | hook.bit());
        patches.applied.insert(at, Applied { id, running, hooks });
        self.rehook(&patches);
    }

    /// Takes `id` off: from the next sample on, the app is as if it had never been.
    pub(crate) fn take_off(&self, id: &str) {
        let mut patches = self.patches();
        patches.applied.retain(|p| p.id != id);
        self.rehook(&patches);
    }

    /// Tells `id`, if it is on, control `index` has `value`.
    pub(crate) fn control(&self, id: &str, index: usize, value: f32) {
        let mut patches = self.patches();
        let Some(at) = patches.applied.iter().position(|p| p.id == id) else { return };
        let told = patches.applied.get_mut(at).map(|p| p.running.control(index, value));
        if let Some(Err(stop)) = told {
            let gone = patches.applied.remove(at);
            self.note_stop(&mut patches, gone.id, stop);
            self.rehook(&patches);
        }
    }

    /// Writes down that `id` couldn't start, as if it had stopped.
    pub(crate) fn failed(&self, id: &str, stop: Stop) {
        self.note_stop(&mut self.patches(), id.to_owned(), stop);
    }

    /// A stop in words for the app to tell, once.
    pub(crate) fn take_said(&self) -> Option<String> {
        self.patches().untold.take().map(|stop| format!("A patch stopped. {}", stop.text()))
    }

    /// What stopped since this was last asked.
    pub(crate) fn take_stopped(&self) -> Vec<(String, Stop)> {
        std::mem::take(&mut self.patches().stopped)
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Strokes go through the patches `plugins` has applied, as they come and go, from here on.
    pub fn use_patches(&self, plugins: Arc<Plugins>) {
        let hooks = plugins.hooks();
        self.shared.lock().hooks = Some(hooks);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Instant;

    use procraft_engine::view::View;
    use procraft_gpu::Gpu;
    use serde_json::{Value, json};

    use super::*;
    use crate::patch_run::tests::{OFFSET, position_module};
    use crate::patch_run::{MODULE, Running};
    use crate::plugin_file::tests::{pack, patch};
    use crate::{PenSample, PointerKind};

    const ID: &str = "org.example.offset";
    /// Turns a row to its brightness, as the app used to.
    const PASTES: &str = r#"(module
        (memory (export "memory") 2)
        (global $on (mut i32) (i32.const 0))
        (func (export "procraft_control") (param i32 f32)
            (global.set $on (f32.ne (local.get 1) (f32.const 0))))
        (func (export "procraft_paste_mask") (result i32) (global.get $on))
        (func (export "procraft_row_buffer") (param i32) (result i32) (i32.const 65536))
        (func (export "procraft_paste_row") (param $at i32) (param $width i32)
            (local $end i32) (local $p i32) (local $grey i32)
            (local.set $end (i32.add (local.get $at) (i32.shl (local.get $width) (i32.const 2))))
            (block $done (loop $next
                (br_if $done (i32.ge_u (local.get $at) (local.get $end)))
                (local.set $p (i32.load (local.get $at)))
                (local.set $grey (i32.shr_u (i32.add (i32.add (i32.add
                    (i32.mul (i32.and (local.get $p) (i32.const 255)) (i32.const 54))
                    (i32.mul (i32.and (i32.shr_u (local.get $p) (i32.const 8)) (i32.const 255)) (i32.const 183)))
                    (i32.mul (i32.and (i32.shr_u (local.get $p) (i32.const 16)) (i32.const 255)) (i32.const 19)))
                    (i32.const 128)) (i32.const 8)))
                (i32.store (local.get $at)
                    (i32.or (i32.const 0xff000000) (i32.mul (local.get $grey) (i32.const 0x010101))))
                (local.set $at (i32.add (local.get $at) (i32.const 4)))
                (br $next)))))"#;
    /// Answers the scale hook by its one slider.
    const SCALES: &str = r#"(module
        (global $size (mut f32) (f32.const 1))
        (func (export "procraft_control") (param i32 f32) (global.set $size (local.get 1)))
        (func (export "procraft_interface_scale") (param f32 f32) (result f32) (global.get $size)))"#;
    /// Answers the pick hook by its one switch.
    const PICKS: &str = r#"(module
        (global $layer (mut i32) (i32.const 0))
        (func (export "procraft_control") (param i32 f32)
            (global.set $layer (f32.ne (local.get 1) (f32.const 0))))
        (func (export "procraft_pick_layer") (result i32) (global.get $layer)))"#;

    fn store(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("procraft-hooks-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.to_string_lossy().into_owned()
    }

    fn file(manifest: &Value, module: &[u8]) -> Vec<u8> {
        pack(manifest, &[(MODULE, module)])
    }

    fn offset() -> Vec<u8> {
        wat::parse_str(OFFSET).unwrap()
    }

    fn offset_manifest() -> Value {
        let mut manifest = patch(ID, &offset());
        manifest["controls"] = json!([
            { "key": "x", "type": "slider", "label": "X", "least": -100, "most": 100, "step": 1, "starts": 0 },
            { "key": "y", "type": "slider", "label": "Y", "least": -100, "most": 100, "step": 1, "starts": 0 },
        ]);
        manifest
    }

    /// A list holding the offset patch, in `dir`.
    fn with_offset(dir: &str) -> Arc<Plugins> {
        let plugins = Plugins::with_store(dir.to_owned());
        assert_eq!(plugins.install(file(&offset_manifest(), &offset())), None);
        plugins
    }

    /// A patch of no controls whose `procraft_position` does `body`, imported and applied.
    fn applied(dir: &str, before: &str, body: &str) -> Arc<Plugins> {
        let module = position_module(before, body);
        let plugins = Plugins::with_store(dir.to_owned());
        assert_eq!(plugins.install(file(&patch(ID, &module), &module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        plugins
    }

    fn card(plugins: &Plugins) -> crate::plugins::PluginInfo {
        plugins.patches().remove(0)
    }

    /// A list as the next start reads it back.
    fn restarted(dir: &str, plugins: &Plugins) -> Arc<Plugins> {
        let next = Plugins::with_store(dir.to_owned());
        for line in plugins.kept_lines() {
            let (key, value) = line.split_once(' ').unwrap();
            next.read_kept(key.to_owned(), value.to_owned());
        }
        next
    }

    #[test]
    fn with_nothing_applied_a_sample_goes_through_as_it_came() {
        let hooks = Hooks::default();
        assert_eq!(hooks.position(1.5, -2.5), (1.5, -2.5));
        // whatever it is, even what no patch could hand on
        let (x, y) = hooks.position(f32::NAN, f32::INFINITY);
        assert!(x.is_nan() && y == f32::INFINITY);
    }

    #[test]
    fn a_sample_that_isn_t_a_position_doesn_t_stop_a_patch() {
        let hooks = Hooks::default();
        hooks.put_on("offset".to_owned(), Running::start(&offset(), &[]).unwrap(), &[Hook::Position]);
        assert!(hooks.position(f32::NAN, 2.0).0.is_nan());
        assert_eq!(hooks.position(1.0, f32::INFINITY), (1.0, f32::INFINITY));
        assert!(hooks.is_on("offset"));
        assert_eq!(hooks.take_stopped(), []);
    }

    /// A patch that says the pick reads the layer as its one switch says, in a store at `dir`.
    fn picking(dir: &str) -> Arc<Plugins> {
        let module = wat::parse_str(PICKS).unwrap();
        let plugins = Plugins::with_store(dir.to_owned());
        let mut manifest = patch(ID, &module);
        manifest["hooks"] = json!(["pick_layer"]);
        manifest["controls"] = json!([
            { "key": "layer", "type": "switch", "label": "Layer", "keeps": "eyedropper_layer" },
        ]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        plugins
    }

    #[test]
    fn the_pick_reads_the_layer_only_while_a_patch_says_so() {
        let dir = store("pick");
        let plugins = picking(&dir);
        let hooks = plugins.hooks();
        assert!(!hooks.pick_layer(), "imported, it says nothing");
        assert!(plugins.set_on(ID.to_owned(), true));
        assert!(!hooks.pick_layer(), "applied, it goes by its switch");
        assert!(plugins.set_switch("eyedropper_layer".to_owned(), true));
        assert!(hooks.pick_layer());
        assert!(plugins.set_on(ID.to_owned(), false));
        assert!(!hooks.pick_layer());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_pick_patch_that_crashes_leaves_the_pick_on_the_canvas() {
        let dir = store("pick-crash");
        let module =
            wat::parse_str(r#"(module (func (export "procraft_pick_layer") (result i32) (unreachable)))"#).unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = patch(ID, &module);
        manifest["hooks"] = json!(["pick_layer"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        let hooks = plugins.hooks();
        assert!(!hooks.pick_layer());
        assert_eq!(card(&plugins).problem.as_deref(), Some("Switched off: it crashed."));
        assert!(!hooks.has(Hook::PickLayer));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_eyedropper_reads_the_current_layer_while_a_patch_says_so() {
        let Some(s) = session() else { return };
        // paint on the first layer, then an empty one over it
        draw(&s);
        s.add_layer();
        let shown = s.pick(100.0, 50.0, 21).unwrap().rgb;
        assert!(shown.is_some());
        let dir = store("pick-session");
        let plugins = picking(&dir);
        s.use_patches(Arc::clone(&plugins));
        assert_eq!(s.pick(100.0, 50.0, 21).unwrap().rgb, shown, "with none applied, the canvas");
        plugins.set_on(ID.to_owned(), true);
        plugins.set_switch("eyedropper_layer".to_owned(), true);
        assert_eq!(s.pick(100.0, 50.0, 21).unwrap().rgb, None, "the current layer is clear there");
        plugins.set_on(ID.to_owned(), false);
        assert_eq!(s.pick(100.0, 50.0, 21).unwrap().rgb, shown);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A patch that scales the interface by its one slider, in a store at `dir`.
    fn scaling(dir: &str) -> Arc<Plugins> {
        let module = wat::parse_str(SCALES).unwrap();
        let plugins = Plugins::with_store(dir.to_owned());
        let mut manifest = patch(ID, &module);
        manifest["hooks"] = json!(["interface_scale"]);
        manifest["controls"] = json!([{
            "key": "size", "type": "slider", "label": "Size", "least": 0.5, "most": 2, "step": 0.05, "starts": 1,
            "keeps": "interface_size",
        }]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        plugins
    }

    #[test]
    fn the_interface_shows_as_a_patch_says_and_as_measured_without_one() {
        let dir = store("scale");
        let plugins = scaling(&dir);
        assert_eq!(plugins.interface_scale(1600.0, 1000.0), 1.0, "imported, it says nothing");
        assert!(plugins.set_on(ID.to_owned(), true));
        assert_eq!(plugins.set_slider("interface_size".to_owned(), 1.2), Some(1.2));
        assert_eq!(plugins.interface_scale(1600.0, 1000.0), 1.2);
        assert_eq!(plugins.set_slider("interface_size".to_owned(), 0.75), Some(0.75));
        assert_eq!(plugins.interface_scale(1600.0, 1000.0), 0.75);
        assert!(plugins.set_on(ID.to_owned(), false));
        assert_eq!(plugins.interface_scale(1600.0, 1000.0), 1.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn whatever_a_patch_says_every_page_stays_in_reach() {
        let dir = store("scale-room");
        let plugins = scaling(&dir);
        assert!(plugins.set_on(ID.to_owned(), true));
        plugins.set_slider("interface_size".to_owned(), 2.0);
        assert_eq!(plugins.interface_scale(2000.0, 1500.0), 2.0);
        // the panels need 640 x 480 points: past what leaves them that, it goes no further
        assert_eq!(plugins.interface_scale(960.0, 1500.0), 1.5);
        assert_eq!(plugins.interface_scale(2000.0, 600.0), 1.25);
        // a window with less shows as measured
        assert_eq!(plugins.interface_scale(640.0, 480.0), 1.0);
        assert_eq!(plugins.interface_scale(500.0, 400.0), 1.0);
        // smaller is the patch's to say, whatever the window
        plugins.set_slider("interface_size".to_owned(), 0.75);
        assert_eq!(plugins.interface_scale(500.0, 400.0), 0.75);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_kept_interface_size_comes_back_through_a_restart() {
        let dir = store("scale-restart");
        let plugins = scaling(&dir);
        plugins.set_on(ID.to_owned(), true);
        plugins.set_slider("interface_size".to_owned(), 1.25);
        let next = restarted(&dir, &plugins);
        assert_eq!(next.interface_scale(1600.0, 1000.0), 1.25);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_scale_patch_that_goes_wrong_leaves_the_interface_as_measured() {
        let dir = store("scale-wrong");
        let module = wat::parse_str(
            r#"(module (func (export "procraft_interface_scale") (param f32 f32) (result f32) (f32.const 40)))"#,
        )
        .unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = patch(ID, &module);
        manifest["hooks"] = json!(["interface_scale"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        assert_eq!(plugins.interface_scale(1600.0, 1000.0), 1.0);
        assert_eq!(card(&plugins).problem.as_deref(), Some("Switched off: it gave an answer that makes no sense."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A list holding the paste patch, its switch kept as the setting it took over, in `dir`.
    fn pasting(dir: &str) -> Arc<Plugins> {
        let module = wat::parse_str(PASTES).unwrap();
        let plugins = Plugins::with_store(dir.to_owned());
        let mut manifest = patch(ID, &module);
        manifest["hooks"] = json!(["paste_mask"]);
        manifest["controls"] = json!([
            { "key": "into", "type": "switch", "label": "Into", "keeps": "paste_into_mask" },
        ]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        plugins
    }

    /// `s` with the paste patch applied and on, and where its files are.
    pub(crate) fn paste_into_masks(s: &CanvasSession, name: &str) -> (Arc<Plugins>, String) {
        let dir = store(name);
        let plugins = pasting(&dir);
        assert!(plugins.set_on(ID.to_owned(), true));
        assert!(plugins.set_switch("paste_into_mask".to_owned(), true));
        s.use_patches(Arc::clone(&plugins));
        (plugins, dir)
    }

    #[test]
    fn a_picture_goes_into_the_mask_only_while_a_patch_says_so() {
        let dir = store("paste");
        let plugins = pasting(&dir);
        let hooks = plugins.hooks();
        assert!(!hooks.paste_mask(), "imported, it says nothing");
        assert!(plugins.set_on(ID.to_owned(), true));
        assert!(!hooks.paste_mask(), "applied, it goes by its switch");
        assert!(plugins.set_switch("paste_into_mask".to_owned(), true));
        assert!(hooks.paste_mask());
        assert!(plugins.set_on(ID.to_owned(), false));
        assert!(!hooks.paste_mask());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rows_come_back_as_the_brightness_the_app_used_to_make() {
        let dir = store("paste-rows");
        let plugins = pasting(&dir);
        plugins.set_on(ID.to_owned(), true);
        let hooks = plugins.hooks();
        let colours: [[u8; 4]; 6] = [
            [255, 255, 255, 255],
            [0, 0, 0, 0],
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [10, 200, 90, 200],
        ];
        let mut rgba: Vec<u8> = colours.concat();
        assert!(hooks.paste_rows(&mut rgba, 3));
        for (px, [r, g, b, _]) in rgba.chunks(4).zip(colours.map(|c| c.map(u32::from))) {
            let grey = ((54 * r + 183 * g + 19 * b + 128) >> 8).min(255) as u8;
            assert_eq!(px, [grey, grey, grey, 255]);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_paste_patch_that_stops_leaves_the_picture_to_paste_as_any_is() {
        let Some(s) = session() else { return };
        let layer = s.layers().iter().find(|l| l.current).unwrap().id;
        let mask = s.add_mask(layer).unwrap();
        s.select_layer(mask);
        let dir = store("paste-stops");
        let module = crate::patch_run::tests::paste_module("(i32.const 65536)", "(unreachable)");
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = patch(ID, &module);
        manifest["hooks"] = json!(["paste_mask"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        s.use_patches(Arc::clone(&plugins));
        let layers = s.layers().len();
        let picture = crate::layers::ThumbnailImage { width: 200, height: 200, rgba: vec![255; 200 * 200 * 4] };
        let pasted = s.paste(picture).unwrap();
        assert_ne!(pasted, mask, "a new layer, as anything pasted");
        assert_eq!(s.layers().len(), layers + 1);
        assert_eq!(s.take_failure().as_deref(), Some("A patch stopped. Switched off: it crashed."));
        assert_eq!(card(&plugins).problem.as_deref(), Some("Switched off: it crashed."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_patch_goes_on_as_applied_and_off_as_unapplied_with_no_restart() {
        let dir = store("apply");
        let plugins = with_offset(&dir);
        let hooks = plugins.hooks();
        assert_eq!(hooks.position(10.0, 10.0), (10.0, 10.0), "imported, it does nothing");
        assert!(plugins.set_on(ID.to_owned(), true));
        assert_eq!(plugins.set_slider(format!("{ID}.x"), 7.0), Some(7.0));
        assert_eq!(plugins.set_slider(format!("{ID}.y"), -3.0), Some(-3.0));
        assert_eq!(hooks.position(10.0, 10.0), (17.0, 7.0));
        assert!(plugins.set_on(ID.to_owned(), false));
        assert_eq!(hooks.position(10.0, 10.0), (10.0, 10.0));
        // and back as it was left
        assert!(plugins.set_on(ID.to_owned(), true));
        assert_eq!(hooks.position(10.0, 10.0), (17.0, 7.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn applied_it_comes_back_through_a_restart_whatever_order_values_are_read_in() {
        let dir = store("restart");
        let plugins = with_offset(&dir);
        plugins.set_on(ID.to_owned(), true);
        plugins.set_slider(format!("{ID}.x"), 7.0);
        let mut lines = plugins.kept_lines();
        for _ in 0..2 {
            let next = Plugins::with_store(dir.to_owned());
            for line in &lines {
                let (key, value) = line.split_once(' ').unwrap();
                next.read_kept(key.to_owned(), value.to_owned());
            }
            assert_eq!(next.hooks().position(1.0, 1.0), (8.0, 1.0));
            lines.reverse();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removed_it_is_off_the_hook_and_gone() {
        let dir = store("remove");
        let plugins = with_offset(&dir);
        plugins.set_on(ID.to_owned(), true);
        plugins.set_slider(format!("{ID}.x"), 7.0);
        let hooks = plugins.hooks();
        assert_eq!(hooks.position(1.0, 1.0), (8.0, 1.0));
        let keys = plugins.remove(ID.to_owned()).unwrap();
        assert!(keys.contains(&format!("{ID}.x")) && keys.contains(&format!("{ID}.applied")), "{keys:?}");
        assert_eq!(hooks.position(1.0, 1.0), (1.0, 1.0));
        assert!(plugins.patches().is_empty());
        assert!(Plugins::with_store(dir.clone()).patches().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_version_takes_over_on_the_hook() {
        let dir = store("update");
        let plugins = with_offset(&dir);
        plugins.set_on(ID.to_owned(), true);
        plugins.set_slider(format!("{ID}.x"), 7.0);
        let hooks = plugins.hooks();
        let mut newer = offset_manifest();
        newer["version"] = json!("1.1.0");
        assert_eq!(plugins.install(file(&newer, &offset())), None);
        assert_eq!(card(&plugins).version, "1.1.0");
        assert!(card(&plugins).on);
        assert_eq!(hooks.position(1.0, 1.0), (8.0, 1.0), "applied still, with what was set");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What a patch that goes wrong on a sample must come to, whatever went wrong.
    fn stops(name: &str, before: &str, body: &str, reason: &str) {
        let dir = store(name);
        let plugins = applied(&dir, before, body);
        let hooks = plugins.hooks();
        assert_eq!(hooks.position(3.0, 4.0), (3.0, 4.0), "the sample goes through unchanged");
        let stopped = card(&plugins);
        assert!(!stopped.on);
        assert_eq!(stopped.problem.as_deref(), Some(reason));
        assert_eq!(hooks.position(5.0, 6.0), (5.0, 6.0), "and it is off the hook");
        assert_eq!(hooks.hooked.load(Ordering::Relaxed), 0);
        // it stays off through a restart, even with its flag kept as applied, and says why
        let next = Plugins::with_store(dir.clone());
        next.read_kept(format!("{ID}.applied"), "true".to_owned());
        assert_eq!(next.hooks().position(3.0, 4.0), (3.0, 4.0));
        let kept = card(&next);
        assert!(!kept.on);
        assert_eq!(kept.problem.as_deref(), Some(reason));
        // unapplied, it says nothing more; asked again, it gets its chance
        assert!(next.set_on(ID.to_owned(), false));
        assert_eq!(card(&next).problem, None);
        assert_eq!(card(&Plugins::with_store(dir.clone())).problem, None);
        assert!(next.set_on(ID.to_owned(), true));
        assert!(card(&next).on);
        // and removed, nothing of it is left
        assert!(next.remove(ID.to_owned()).is_some());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_patch_that_loops_for_ever_is_stopped_and_switched_off() {
        stops("loop", "", "(loop (br 0)) (i64.const 0)", "Switched off: it took too long.");
    }

    #[test]
    fn a_patch_that_traps_is_switched_off() {
        stops("trap", "", "(unreachable)", "Switched off: it crashed.");
    }

    #[test]
    fn a_patch_that_returns_garbage_is_switched_off() {
        // two NaNs
        let nan = "(i64.const 0x7fc000007fc00000)";
        stops("garbage", "", nan, "Switched off: it gave an answer that makes no sense.");
    }

    #[test]
    fn a_patch_over_the_memory_cap_is_switched_off() {
        let grows = "(drop (memory.grow (i32.const 1000))) (i64.const 0)";
        stops("memory", "(memory 1)", grows, "Switched off: it asked for too much memory.");
    }

    #[test]
    fn a_patch_stopped_on_a_stroke_is_told_once_by_the_session() {
        let dir = store("told");
        let plugins = applied(&dir, "", "(unreachable)");
        let s = CanvasSession::new(200, 200);
        s.use_patches(Arc::clone(&plugins));
        assert_eq!(s.take_failure(), None);
        {
            let mut st = s.shared.lock();
            st.view = Some(View::fit((200, 200), (200, 200)));
            let at = st.to_canvas(&[sample(10.0, 20.0, 0.0)]);
            assert_eq!(at.len(), 1);
            let plain = st
                .view
                .and_then(|v| v.to_screen().inverse())
                .unwrap()
                .apply(procraft_engine::geom::Vec2::new(10.0, 20.0));
            assert_eq!(at[0].pos, plain, "the sample goes through unchanged");
        }
        assert_eq!(s.take_failure().as_deref(), Some("A patch stopped. Switched off: it crashed."));
        assert_eq!(s.take_failure(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_patch_that_can_t_start_stays_off_and_says_why() {
        let dir = store("start");
        let module = position_module("(memory 1000)", "(i64.const 0)");
        let plugins = Plugins::with_store(dir.clone());
        assert_eq!(plugins.install(file(&patch(ID, &module), &module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        let stopped = card(&plugins);
        assert!(!stopped.on);
        assert_eq!(stopped.problem.as_deref(), Some("Switched off: it asked for too much memory."));
        assert_eq!(plugins.hooks().hooked.load(Ordering::Relaxed), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stopped_patch_leaves_the_others_on() {
        let bad = position_module("", "(unreachable)");
        let hooks = Hooks::default();
        let mut good = Running::start(&offset(), &[]).unwrap();
        good.control(0, 2.0).unwrap();
        hooks.put_on("b".to_owned(), good, &[Hook::Position]);
        hooks.put_on("a".to_owned(), Running::start(&bad, &[]).unwrap(), &[Hook::Position]);
        assert_eq!(hooks.position(1.0, 1.0), (3.0, 1.0));
        assert!(!hooks.is_on("a") && hooks.is_on("b"));
        assert_eq!(hooks.take_stopped(), [("a".to_owned(), Stop::Crashed)]);
        assert_eq!(hooks.take_stopped(), []);
    }

    #[test]
    fn patches_go_in_id_order_however_they_were_applied() {
        // the second moves what the first gave it, so the order shows
        let doubles = position_module(
            "",
            "(i64.or
                (i64.extend_i32_u (i32.reinterpret_f32 (f32.mul (local.get 0) (f32.const 2))))
                (i64.shl (i64.extend_i32_u (i32.reinterpret_f32 (local.get 1))) (i64.const 32)))",
        );
        let offset_by = |x: f32| {
            let mut running = Running::start(&offset(), &[]).unwrap();
            running.control(0, x).unwrap();
            running
        };
        let hooks = Hooks::default();
        hooks.put_on("b".to_owned(), Running::start(&doubles, &[]).unwrap(), &[Hook::Position]);
        hooks.put_on("a".to_owned(), offset_by(1.0), &[Hook::Position]);
        assert_eq!(hooks.position(1.0, 0.0), (4.0, 0.0));
    }

    /// A session on a headless GPU drawing just where the input goes, or none without a GPU.
    fn session() -> Option<Arc<CanvasSession>> {
        let gpu = match Gpu::headless() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipped: {e}");
                return None;
            }
        };
        let s = CanvasSession::new(200, 200);
        {
            let mut st = s.shared.lock();
            st.window_size = (200, 200);
            st.view = Some(View::fit((200, 200), (200, 200)));
            st.gpu = Some(gpu);
            st.ensure_canvas();
        }
        s.studio_begin();
        s.set_studio_setting("streamline".to_owned(), 0.0);
        s.studio_end(true);
        Some(s)
    }

    fn sample(x: f32, y: f32, ms: f64) -> PenSample {
        PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: ms, roll: 0.0 }
    }

    fn draw(s: &CanvasSession) {
        s.stroke_begin(PointerKind::Stylus, vec![sample(30.0, 50.0, 0.0)]);
        s.stroke_add((1..20).map(|i| sample(30.0 + i as f32 * 6.0, 50.0, f64::from(i) * 8.0)).collect());
        s.stroke_end(vec![sample(150.0, 50.0, 170.0)]);
    }

    /// What the canvas looks like, after a render.
    fn picture(s: &CanvasSession) -> Vec<u8> {
        let mut st = s.shared.lock();
        let layer = st.doc.current();
        st.layer_pixels(layer).unwrap()
    }

    /// The rows of `rgba`, 200 wide, that anything was drawn on.
    fn rows_drawn(rgba: &[u8]) -> Vec<usize> {
        let rows = rgba.chunks(200 * 4).enumerate();
        rows.filter(|(_, row)| row.chunks(4).any(|p| p[3] > 0)).map(|(y, _)| y).collect()
    }

    #[test]
    fn a_stroke_lands_where_the_patch_says_and_as_it_did_once_it_is_gone() {
        let Some(plain) = session() else { return };
        draw(&plain);
        let drawn = picture(&plain);
        assert!(!rows_drawn(&drawn).is_empty());

        // with the list on the session and nothing applied, nothing differs at all
        let dir = store("stroke");
        let plugins = with_offset(&dir);
        let Some(s) = session() else { return };
        s.use_patches(Arc::clone(&plugins));
        draw(&s);
        assert!(picture(&s) == drawn);

        let Some(s) = session() else { return };
        s.use_patches(Arc::clone(&plugins));
        plugins.set_on(ID.to_owned(), true);
        plugins.set_slider(format!("{ID}.y"), 40.0);
        draw(&s);
        let moved = rows_drawn(&picture(&s));
        let want: Vec<usize> = rows_drawn(&drawn).iter().map(|y| y + 40).collect();
        assert_eq!(moved, want, "the same stroke, 40 pixels down");

        // unapplied it is as if it had never been
        let Some(s) = session() else { return };
        s.use_patches(Arc::clone(&plugins));
        plugins.set_on(ID.to_owned(), false);
        draw(&s);
        assert!(picture(&s) == drawn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nanoseconds a sample takes through `hooks`, over a run long enough to read.
    fn cost(hooks: &Hooks) -> f64 {
        const SAMPLES: u32 = 2_000_000;
        let start = Instant::now();
        let mut sum = 0.0_f32;
        for i in 0..SAMPLES {
            sum += hooks.position(std::hint::black_box(i as f32 % 2000.0), 300.0).0;
        }
        std::hint::black_box(sum);
        start.elapsed().as_secs_f64() * 1e9 / f64::from(SAMPLES)
    }

    /// What the hook costs a sample, and what a patch that spends all its fuel costs: run on a
    /// device with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn hook_cost() {
        let none = Hooks::default();
        println!(
            "position hook, none applied: {:.2} ns a sample",
            (0..5).map(|_| cost(&none)).fold(f64::MAX, f64::min)
        );

        let applied = Hooks::default();
        let mut running = Running::start(&offset(), &[]).unwrap();
        running.control(0, 3.0).unwrap();
        applied.put_on("offset".to_owned(), running, &[Hook::Position]);
        println!(
            "the offset patch applied: {:.1} ns a sample",
            (0..5).map(|_| cost(&applied)).fold(f64::MAX, f64::min)
        );

        // the worst a patch can do to a sample before it is stopped, over and over
        let mut slowest = f64::MAX;
        for _ in 0..50 {
            let mut running = Running::start(&position_module("", "(loop (br 0)) (i64.const 0)"), &[]).unwrap();
            let start = Instant::now();
            assert_eq!(running.position(1.0, 2.0), Err(Stop::TooSlow));
            slowest = slowest.min(start.elapsed().as_secs_f64() * 1000.0);
        }
        println!("a patch spending all its fuel on a sample: {slowest:.3} ms");

        let picks = Hooks::default();
        picks.put_on(
            "pick".to_owned(),
            Running::start(&wat::parse_str(PICKS).unwrap(), &[1.0]).unwrap(),
            &[Hook::PickLayer],
        );
        let start = Instant::now();
        for _ in 0..1_000_000 {
            std::hint::black_box(picks.pick_layer());
        }
        println!("the pick hook, a patch on it: {:.1} ns a pick", start.elapsed().as_secs_f64() * 1000.0);

        let scales = Hooks::default();
        scales.put_on(
            "scale".to_owned(),
            Running::start(&wat::parse_str(SCALES).unwrap(), &[1.1]).unwrap(),
            &[Hook::InterfaceScale],
        );
        let start = Instant::now();
        for _ in 0..1_000_000 {
            std::hint::black_box(scales.interface_scale(std::hint::black_box(1600.0), 1000.0));
        }
        println!("the scale hook, a patch on it: {:.1} ns a frame", start.elapsed().as_secs_f64() * 1000.0);

        let start = Instant::now();
        let mut running = Running::start(&offset(), &[]).unwrap();
        println!("starting a patch: {:.3} ms", start.elapsed().as_secs_f64() * 1000.0);
        running.control(0, 1.0).unwrap();
    }

    /// What a 4096 square costs going through a paste patch. With `PROCRAFT_PATCH_FILE` naming one
    /// it goes through that, otherwise through [`PASTES`]: run on a device with
    /// `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_paste_of_a_4096_square_costs() {
        let dir = store("paste-cost");
        let plugins = match std::env::var("PROCRAFT_PATCH_FILE") {
            Ok(path) => {
                let plugins = Plugins::with_store(dir.clone());
                assert_eq!(plugins.install(std::fs::read(path).unwrap()), None);
                let id = card(&plugins).id;
                assert!(plugins.set_on(id, true));
                plugins
            }
            Err(_) => {
                let plugins = pasting(&dir);
                plugins.set_on(ID.to_owned(), true);
                plugins
            }
        };
        let hooks = plugins.hooks();
        const SIDE: usize = 4096;
        let picture: Vec<u8> = (0..SIDE * SIDE * 4).map(|i| (i % 251) as u8).collect();
        for _ in 0..3 {
            let mut rgba = picture.clone();
            let start = Instant::now();
            assert!(hooks.paste_rows(&mut rgba, SIDE));
            let took = start.elapsed().as_secs_f64();
            println!(
                "4096 square through the patch: {:.0} ms, {:.0} ns a pixel",
                took * 1e3,
                took * 1e9 / (SIDE * SIDE) as f64
            );
            let mut native = picture.clone();
            let start = Instant::now();
            for px in native.as_chunks_mut::<4>().0 {
                let [r, g, b, _] = px.map(u32::from);
                let grey = ((54 * r + 183 * g + 19 * b + 128) >> 8).min(255) as u8;
                *px = [grey, grey, grey, 255];
            }
            println!("the same, native: {:.0} ms", start.elapsed().as_secs_f64() * 1e3);
            assert!(rgba == native);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The same stroke, with the list on the session and without: run on a device with
    /// `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_stroke_costs_the_same_with_nothing_applied() {
        let to_canvas_ns = |hooks: Option<Arc<Hooks>>| {
            let s = CanvasSession::new(200, 200);
            let mut st = s.shared.lock();
            st.view = Some(View::fit((200, 200), (200, 200)));
            st.hooks = hooks;
            let samples: Vec<PenSample> = (0..2000).map(|i| sample(i as f32 % 200.0, 50.0, f64::from(i))).collect();
            (0..7)
                .map(|_| {
                    let start = Instant::now();
                    for _ in 0..500 {
                        std::hint::black_box(st.to_canvas(std::hint::black_box(&samples)));
                    }
                    start.elapsed().as_secs_f64() * 1e9 / (500.0 * 2000.0)
                })
                .fold(f64::MAX, f64::min)
        };
        for _ in 0..3 {
            let bare = to_canvas_ns(None);
            let hooked = to_canvas_ns(Some(Arc::new(Hooks::default())));
            println!("to_canvas a sample: {bare:.2} ns without the list, {hooked:.2} ns with it, none applied");
        }
    }

    /// A real patch file, built by `tools/pack_patch.py`, end to end on the hook. Run with
    /// `--ignored` and `PROCRAFT_PATCH_FILE` naming it.
    #[test]
    #[ignore]
    fn a_built_patch_file_works() {
        let bytes = std::fs::read(std::env::var("PROCRAFT_PATCH_FILE").unwrap()).unwrap();
        let dir = store("built");
        let plugins = Plugins::with_store(dir.clone());
        let offer = plugins.inspect(bytes.clone());
        println!("{offer:?}");
        assert_eq!(offer.refused, None);
        assert_eq!(plugins.install(bytes), None);
        let found = card(&plugins);
        assert!(plugins.set_on(found.id.clone(), true));
        let hooks = plugins.hooks();
        assert_eq!(hooks.position(100.0, 100.0), (100.0, 100.0), "at its start it moves nothing");
        for control in &found.controls {
            let crate::plugins::PluginControl::Slider { control } = control else { panic!() };
            plugins.set_slider(control.key.clone(), if control.label == "X" { 12.0 } else { -8.0 });
        }
        assert_eq!(hooks.position(100.0, 100.0), (112.0, 92.0));
        let next = restarted(&dir, &plugins);
        assert_eq!(next.hooks().position(100.0, 100.0), (112.0, 92.0));
        assert!(next.set_on(found.id.clone(), false));
        assert_eq!(next.hooks().position(100.0, 100.0), (100.0, 100.0));
        assert!(next.remove(found.id).is_some());
        assert!(next.patches().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
