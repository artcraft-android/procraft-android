//! The points where a plugin that's on steps in. Each does nothing with none on, and holds no
//! feature of its own: what a plugin does is in its file. They stay for good, so there are as few as
//! plugins need, and each is as narrow as it can be.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::CanvasSession;
use crate::plugin_file;
use crate::plugin_run::{Hook, Running, Stop};
use crate::plugins::Plugins;

/// A plugin at work: a lock of its own, so a long call holds up nothing but it.
type Shared = Arc<Mutex<Running>>;

fn at_work(running: &Shared) -> MutexGuard<'_, Running> {
    running.lock().unwrap_or_else(|e| e.into_inner())
}

struct OnHook {
    id: String,
    running: Shared,
    /// The hooks it is on, as bits.
    hooks: u32,
    /// The keys of its rows in the brush studio, in their order.
    brush_keys: Vec<String>,
    /// On the picker hook: its picture's code.
    picture: Option<Arc<str>>,
    /// On an event's hook: a copy of its code of its own for them, so that what an event calls in
    /// the app never waits on this one (its other hooks' copy).
    events: Option<Shared>,
}

/// A plugin of switches that is on: no code, its hooks each answered from its controls (yes while on,
/// or the control at that place's value), whose values it is told as they change.
struct Switched {
    id: String,
    hooks: u32,
    answers: plugin_file::Answers,
    values: Vec<f32>,
}

impl Switched {
    /// What it answers at `hook`, as a number (yes is 1).
    fn answer(&self, hook: Hook) -> f32 {
        match self.answers.iter().find(|(h, _)| *h == hook).and_then(|(_, at)| *at) {
            None => 1.0,
            Some(at) => self.values.get(at).copied().unwrap_or(0.0),
        }
    }
}

#[derive(Default)]
struct Working {
    /// In id order, so plugins go in the same order however they were turned on.
    on: Vec<OnHook>,
    /// The plugins of switches on, in id order.
    switched: Vec<Switched>,
    /// What stopped since the list last asked, and why.
    stopped: Vec<(String, Stop)>,
    /// What the app hasn't told yet: whoever is drawing sees the stroke change, not the list.
    untold: Option<String>,
}

/// The plugins that are on, shared by a list and the sessions going through it. Not a static: they go
/// with the list, so one comes off at once, no restart.
#[derive(Default)]
pub(crate) struct Hooks {
    /// The hooks with a plugin on them, as bits: all a stroke with none pays for is reading this.
    hooked: AtomicU32,
    working: Mutex<Working>,
    /// Where a stop is written down, to stay off through a restart.
    store: Option<PathBuf>,
    /// The plugins' own artworks open, each by its plugin: told its controls as they change.
    canvases: Mutex<Vec<(String, std::sync::Weak<crate::PluginCanvas>)>>,
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

    fn working(&self) -> MutexGuard<'_, Working> {
        self.working.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Writes down that `id` stopped, for the list and for the next start.
    fn note_stop(&self, working: &mut Working, id: String, stop: Stop) {
        log::warn!("a plugin stopped: {id}: {stop:?}");
        if let Some(store) = &self.store {
            plugin_file::note_stopped(store, &id, stop.text());
        }
        working.stopped.push((id, stop));
        working.untold = Some(format!("A plugin stopped. {}", stop.text()));
    }

    fn has(&self, hook: Hook) -> bool {
        self.hooked.load(Ordering::Relaxed) & hook.bit() != 0
    }

    /// The bits go by what is on now.
    fn rehook(&self, working: &Working) {
        let hooked = working.on.iter().fold(0, |bits, plugin| bits | plugin.hooks);
        let hooked = working.switched.iter().fold(hooked, |bits, plugin| bits | plugin.hooks);
        self.hooked.store(hooked, Ordering::Relaxed);
    }

    /// Whether a plugin of switches on `hook` says yes.
    fn switched_yes(&self, hook: Hook) -> bool {
        let working = self.working();
        working.switched.iter().any(|p| p.hooks & hook.bit() != 0 && p.answer(hook) != 0.0)
    }

    /// The number the first plugin of switches on `hook` answers, if one is on it.
    fn switched_number(&self, hook: Hook) -> Option<f32> {
        let working = self.working();
        working.switched.iter().find(|p| p.hooks & hook.bit() != 0).map(|p| p.answer(hook))
    }

    /// Puts the plugin of switches `id` on `hooks`, each answered as `answers` says from `values`
    /// (its controls').
    pub(crate) fn put_switches(&self, id: String, hooks: &[Hook], answers: plugin_file::Answers, values: Vec<f32>) {
        let mut working = self.working();
        working.on.retain(|p| p.id != id);
        working.switched.retain(|p| p.id != id);
        let at = working.switched.partition_point(|p| p.id < id);
        let hooks = hooks.iter().fold(0, |bits, hook| bits | hook.bit());
        working.switched.insert(at, Switched { id, hooks, answers, values });
        self.rehook(&working);
    }

    /// `start` through each plugin on `hook`, in turn. One that stops is off the hooks there and then,
    /// and what it was handed goes on as it came to it.
    fn through<T: Copy>(&self, hook: Hook, start: T, mut step: impl FnMut(&mut Running, T) -> Result<T, Stop>) -> T {
        let mut working = self.working();
        let mut at = start;
        let mut i = 0;
        while let Some(plugin) = working.on.get_mut(i) {
            if plugin.hooks & hook.bit() == 0 {
                i += 1;
                continue;
            }
            let answer = step(&mut at_work(&plugin.running), at);
            match answer {
                Ok(to) => {
                    at = to;
                    i += 1;
                }
                Err(stop) => {
                    let gone = working.on.remove(i);
                    self.note_stop(&mut working, gone.id, stop);
                }
            }
        }
        self.rehook(&working);
        at
    }

    /// The position hook: where a sample in window pixels lands, before the stroke sees it.
    #[inline]
    pub(crate) fn position(&self, x: f32, y: f32) -> (f32, f32) {
        // what isn't a position goes on as it came, and no plugin answers for it
        if !self.has(Hook::Position) || !x.is_finite() || !y.is_finite() {
            return (x, y);
        }
        self.through(Hook::Position, (x, y), |running, at| running.position(at.0, at.1))
    }

    /// The pick hook: whether the eyedropper reads the current layer, not the canvas as it is
    /// composited. It only asks: reading either is the app's, and it is a read like any other.
    pub(crate) fn pick_layer(&self) -> bool {
        self.has(Hook::PickLayer)
            && (self.switched_yes(Hook::PickLayer)
                || self.through(Hook::PickLayer, false, |running, said| Ok(said | running.pick_layer()?)))
    }

    /// The fill-by-all hook: whether ColorDrop measures what it fills on the canvas as composited
    /// (every layer seen) rather than on the current layer or the reference layer. It only asks.
    pub(crate) fn fill_all_layers(&self) -> bool {
        self.has(Hook::FillAllLayers)
            && (self.switched_yes(Hook::FillAllLayers)
                || self.through(Hook::FillAllLayers, false, |running, said| Ok(said | running.fill_all_layers()?)))
    }

    /// The colour hook: the colour (0xRRGGBB) the interface draws `role` in, if a plugin says (the
    /// first that does).
    pub(crate) fn interface_colour(&self, role: i32) -> Option<u32> {
        if !self.has(Hook::InterfaceColour) {
            return None;
        }
        self.through(Hook::InterfaceColour, None, |running, said| match said {
            Some(colour) => Ok(Some(colour)),
            None => running.interface_colour(role),
        })
    }

    /// The sound hook: `frames` samples at `rate` of the stroke as `motion` has it, as the first
    /// plugin on the hook makes them, told `brush` when that changed: for the sound's own thread
    /// (the first at a rate readies the plugin, a long call). None with none on it, or if it stops.
    pub(crate) fn stroke_sound(
        &self,
        frames: usize,
        rate: u32,
        motion: &crate::stroke_sound::SoundMotion,
        brush: &crate::stroke_sound::SoundBrush,
    ) -> Option<Vec<f32>> {
        let (id, running) = self.long_calls(Hook::StrokeSound).into_iter().next()?;
        let made = at_work(&running).stroke_sound(frames, rate, motion, brush);
        match made {
            Ok(samples) => samples,
            Err(stop) => {
                self.stopped(&id, stop);
                None
            }
        }
    }

    /// The keys of the rows in the brush studio of the plugin making the sound, to tell it the
    /// current brush's.
    pub(crate) fn sound_brush_keys(&self) -> Vec<String> {
        let working = self.working();
        let first = working.on.iter().find(|p| p.hooks & Hook::StrokeSound.bit() != 0);
        first.map(|p| p.brush_keys.clone()).unwrap_or_default()
    }

    /// The in-between hook: the frame between `this` and `next` (each `width` × `height`,
    /// premultiplied RGBA8), as the first plugin on the hook draws it: a long call, not for the main
    /// thread. None with none on it, if it draws none, or if it stops.
    pub(crate) fn inbetween(&self, this: &[u8], next: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
        let (id, running) = self.long_calls(Hook::Inbetween).into_iter().next()?;
        match at_work(&running).inbetween(this, next, width, height) {
            Ok(made) => made,
            Err(stop) => {
                self.stopped(&id, stop);
                None
            }
        }
    }

    /// Whether a plugin is on `hook`, an event's (see [`Hooks::tell`]).
    pub(crate) fn listens(&self, hook: Hook) -> bool {
        self.has(hook)
    }

    /// Tells the plugins on `hook` (`stroke_kept` or `layer_added`) that it happened, on the artwork
    /// `session`: each acts as an action does, briefly. One that goes wrong comes off and says why.
    pub(crate) fn tell(&self, hook: Hook, session: Arc<CanvasSession>) {
        let kind = match hook {
            Hook::StrokeKept => 1,
            Hook::LayerAdded => 2,
            _ => return,
        };
        if !self.has(hook) {
            return;
        }
        let told: Vec<(String, Shared)> = {
            let working = self.working();
            let on = working.on.iter().filter(|plugin| plugin.hooks & hook.bit() != 0);
            on.map(|p| (p.id.clone(), Arc::clone(p.events.as_ref().unwrap_or(&p.running)))).collect()
        };
        for (id, running) in told {
            let answer = at_work(&running).event(kind, Arc::clone(&session));
            if let Err(stop) = answer {
                self.stopped(&id, stop);
            }
        }
    }

    /// Whether a plugin draws in-betweens: the frame options then offer In-between.
    pub(crate) fn tweens(&self) -> bool {
        self.has(Hook::Inbetween)
    }

    /// Whether a plugin makes the stroke's sound: the apps play it while one does.
    pub(crate) fn makes_stroke_sound(&self) -> bool {
        self.has(Hook::StrokeSound)
    }

    /// The curve hook: the curve the first plugin on it offers for a snapping stroke of `points`
    /// (window pixels), QuickShape having found `best` first (-1: a curve asked for): its nodes.
    /// None with none on it, if it offers none, or if it stops.
    pub(crate) fn quickshape_curve(&self, points: &[(f32, f32)], best: i32) -> Option<Vec<(f32, f32)>> {
        let (id, running) = self.long_calls(Hook::QuickshapeCurve).into_iter().next()?;
        let offered = at_work(&running).quickshape_curve(points, best);
        match offered {
            Ok(nodes) => nodes,
            Err(stop) => {
                self.stopped(&id, stop);
                None
            }
        }
    }

    /// The triangle hook: whether the colour panel's Classic shows a triangle in place of its
    /// square.
    pub(crate) fn colour_triangle(&self) -> bool {
        self.has(Hook::ColourTriangle)
            && (self.switched_yes(Hook::ColourTriangle)
                || self.through(Hook::ColourTriangle, false, |running, said| Ok(said | running.colour_triangle()?)))
    }

    /// Whether a plugin sets brushes' settings at random (the brush studio shows Randomise).
    pub(crate) fn randomises_brushes(&self) -> bool {
        self.has(Hook::RandomiseBrush)
    }

    /// The randomise hook: where brush setting `setting` goes, as a share of its range, if a plugin
    /// says (the first that does).
    pub(crate) fn randomise(&self, setting: i32, seed: i32) -> Option<f32> {
        if !self.has(Hook::RandomiseBrush) {
            return None;
        }
        self.through(Hook::RandomiseBrush, None, |running, said| match said {
            Some(share) => Ok(Some(share)),
            None => running.randomise(setting, seed),
        })
    }

    /// The opacity hook: whether a layer's opacity changed while it is one of several picked sets
    /// every picked layer's.
    pub(crate) fn opacity_together(&self) -> bool {
        self.has(Hook::OpacityTogether)
            && (self.switched_yes(Hook::OpacityTogether)
                || self.through(Hook::OpacityTogether, false, |running, said| Ok(said | running.opacity_together()?)))
    }

    /// The pin hook: whether a touch on the canvas with a panel open paints at once and leaves the
    /// panel up, rather than closing it (a tap) or fading it (a drag).
    pub(crate) fn pin_panels(&self) -> bool {
        self.has(Hook::PinPanels)
            && (self.switched_yes(Hook::PinPanels)
                || self.through(Hook::PinPanels, false, |running, said| Ok(said | running.pin_panels()?)))
    }

    /// The undo notice hook: whether undo and redo go by without the notice naming what they took
    /// back or brought back.
    pub(crate) fn hide_undo_notice(&self) -> bool {
        self.has(Hook::HideUndoNotice)
            && (self.switched_yes(Hook::HideUndoNotice)
                || self.through(Hook::HideUndoNotice, false, |running, said| Ok(said | running.hide_undo_notice()?)))
    }

    /// The radial hook: how many parts radial symmetry makes, if a plugin says (the first that
    /// does); the guide's usual eight otherwise.
    pub(crate) fn radial_segments(&self) -> Option<u32> {
        if !self.has(Hook::RadialSegments) {
            return None;
        }
        // a plugin of switches' slider, held to what the hook takes
        if let Some(n) = self.switched_number(Hook::RadialSegments) {
            return Some((n.round().max(0.0) as u32).clamp(2, 64));
        }
        self.through(Hook::RadialSegments, None, |running, said| match said {
            Some(n) => Ok(Some(n)),
            None => running.radial_segments(),
        })
    }

    /// The scale hook: how big the interface shows in a window `width` × `height` points at 1. The
    /// number is the plugin's, and all the app adds is what keeps every page in reach: see
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

    /// The plugins on `hook`, to be called with the list let go: a long call holds up only them.
    fn long_calls(&self, hook: Hook) -> Vec<(String, Shared)> {
        let working = self.working();
        let on = working.on.iter().filter(|plugin| plugin.hooks & hook.bit() != 0);
        on.map(|plugin| (plugin.id.clone(), Arc::clone(&plugin.running))).collect()
    }

    /// `id` comes off the hooks, having stopped in a long call (or an action).
    pub(crate) fn stopped(&self, id: &str, stop: Stop) {
        let mut working = self.working();
        working.on.retain(|p| p.id != id);
        self.note_stop(&mut working, id.to_owned(), stop);
        self.rehook(&working);
    }

    /// `rgba`, rows of `width` pixels, as the plugin on the paste hook makes it; false if it can't,
    /// and what has gone through is no use. The first that takes it has it all.
    pub(crate) fn paste_rows(&self, rgba: &mut [u8], width: usize) -> bool {
        let Some((id, running)) = self.long_calls(Hook::PasteMask).into_iter().next() else { return false };
        let done = at_work(&running).paste_rows(rgba, width);
        done.inspect_err(|stop| self.stopped(&id, *stop)).is_ok()
    }

    /// The fill hook: whether a fill being let go has anything to go through.
    pub(crate) fn fill_mask(&self) -> bool {
        self.has(Hook::FillMask) && self.through(Hook::FillMask, false, |running, said| Ok(said | running.fill_mask()?))
    }

    /// `rows`, a fill of `width` × `height` from `seed`, a bit a pixel in words of 64, through each
    /// plugin on the fill hook, each taking out what it says: a long call, not for the main thread.
    /// Whether anything came out. One that stops takes nothing out.
    pub(crate) fn fill_rows(&self, rows: &mut Vec<u64>, width: usize, height: usize, seed: (usize, usize)) -> bool {
        let mut changed = false;
        for (id, running) in self.long_calls(Hook::FillMask) {
            let mut left = rows.clone();
            match at_work(&running).fill(&mut left, width, height, seed) {
                Ok(true) => {
                    *rows = left;
                    changed = true;
                }
                Ok(false) => {}
                Err(stop) => self.stopped(&id, stop),
            }
        }
        changed
    }

    /// `words` for the app to tell, once.
    pub(crate) fn say(&self, words: &str) {
        self.working().untold = Some(words.to_owned());
    }

    pub(crate) fn is_on(&self, id: &str) -> bool {
        let working = self.working();
        working.on.iter().any(|p| p.id == id) || working.switched.iter().any(|p| p.id == id)
    }

    pub(crate) fn put_on(&self, id: String, running: Running, hooks: &[Hook]) {
        let mut working = self.working();
        working.on.retain(|p| p.id != id);
        let at = working.on.partition_point(|p| p.id < id);
        let hooks = hooks.iter().fold(0, |bits, hook| bits | hook.bit());
        let running = Arc::new(Mutex::new(running));
        working.on.insert(at, OnHook { id, running, hooks, brush_keys: Vec::new(), picture: None, events: None });
        self.rehook(&working);
    }

    /// `id`'s copy of its code for its events (see [`OnHook::events`]).
    pub(crate) fn give_events(&self, id: &str, running: Running) {
        if let Some(plugin) = self.working().on.iter_mut().find(|p| p.id == id) {
            plugin.events = Some(Arc::new(Mutex::new(running)));
        }
    }

    /// `id`'s picture's code, for the picker hook.
    pub(crate) fn give_picture(&self, id: &str, text: String) {
        if let Some(plugin) = self.working().on.iter_mut().find(|p| p.id == id) {
            plugin.picture = Some(text.into());
        }
    }

    /// The picker hook's plugin (the first on it): its id and its picture's code.
    pub(crate) fn picker(&self) -> Option<(String, Arc<str>)> {
        if !self.has(Hook::Picker) {
            return None;
        }
        let working = self.working();
        let on = working.on.iter().find(|p| p.hooks & Hook::Picker.bit() != 0)?;
        Some((on.id.clone(), Arc::clone(on.picture.as_ref()?)))
    }

    /// The picker hook: the saturation and brightness a point of the picker picks (0..1, y down).
    pub(crate) fn picker_pick(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        if !self.has(Hook::Picker) {
            return None;
        }
        self.through(Hook::Picker, None, |running, got| if got.is_some() { Ok(got) } else { running.picker_pick(x, y) })
    }

    /// The picker hook: where saturation `s` and brightness `v` sit on the picker (0..1, y down).
    pub(crate) fn picker_point(&self, s: f32, v: f32) -> Option<(f32, f32)> {
        if !self.has(Hook::Picker) {
            return None;
        }
        self.through(
            Hook::Picker,
            None,
            |running, got| if got.is_some() { Ok(got) } else { running.picker_point(s, v) },
        )
    }

    /// The keys of `id`'s rows in the brush studio, in their order.
    pub(crate) fn brush_keys(&self, id: &str, keys: Vec<String>) {
        if let Some(plugin) = self.working().on.iter_mut().find(|p| p.id == id) {
            plugin.brush_keys = keys;
        }
    }

    /// The file picked for `id`'s control `index` (none: let go) handed to it: a long call. One
    /// that stops is off its hooks.
    pub(crate) fn give_file(&self, id: &str, index: usize, bytes: Option<&[u8]>) {
        let found = self.working().on.iter().find(|p| p.id == id).map(|p| Arc::clone(&p.running));
        if let Some(running) = found
            && let Err(stop) = at_work(&running).give_file(index, bytes)
        {
            self.stopped(id, stop);
        }
    }

    /// Takes `id` off: from the next sample on, the app is as if it had never been.
    pub(crate) fn take_off(&self, id: &str) {
        let mut working = self.working();
        working.on.retain(|p| p.id != id);
        working.switched.retain(|p| p.id != id);
        self.rehook(&working);
    }

    /// Tells `id`, if it is on, control `index` has `value`.
    pub(crate) fn control(&self, id: &str, index: usize, value: f32) {
        {
            let mut working = self.working();
            // a plugin of switches keeps the value, to answer from
            if let Some(value_at) =
                working.switched.iter_mut().find(|p| p.id == id).and_then(|p| p.values.get_mut(index))
            {
                *value_at = value;
            }
            if let Some(at) = working.on.iter().position(|p| p.id == id) {
                // its events' copy too
                if let Some(events) = working.on.get(at).and_then(|p| p.events.as_ref()) {
                    let _ = at_work(events).control(index, value);
                }
                let told = working.on.get(at).map(|p| at_work(&p.running).control(index, value));
                if let Some(Err(stop)) = told {
                    let gone = working.on.remove(at);
                    self.note_stop(&mut working, gone.id, stop);
                    self.rehook(&working);
                }
            }
        }
        // its artworks open too, once the list is let go (a canvas telling of a stop takes it)
        let open: Vec<Arc<crate::PluginCanvas>> = {
            let mut canvases = self.canvases.lock().unwrap_or_else(|e| e.into_inner());
            canvases.retain(|(_, canvas)| canvas.strong_count() > 0);
            canvases.iter().filter(|(of, _)| of == id).filter_map(|(_, canvas)| canvas.upgrade()).collect()
        };
        for canvas in open {
            canvas.control(index, value);
        }
    }

    /// `canvas`, an artwork of `id`'s open, told `id`'s controls as they change from now on.
    pub(crate) fn keep_canvas(&self, id: &str, canvas: &Arc<crate::PluginCanvas>) {
        let mut canvases = self.canvases.lock().unwrap_or_else(|e| e.into_inner());
        canvases.retain(|(_, canvas)| canvas.strong_count() > 0);
        canvases.push((id.to_owned(), Arc::downgrade(canvas)));
    }

    /// Writes down that `id` couldn't start, as if it had stopped.
    pub(crate) fn failed(&self, id: &str, stop: Stop) {
        self.note_stop(&mut self.working(), id.to_owned(), stop);
    }

    /// What there is for the app to tell, once.
    pub(crate) fn take_said(&self) -> Option<String> {
        self.working().untold.take()
    }

    /// What stopped since this was last asked.
    pub(crate) fn take_stopped(&self) -> Vec<(String, Stop)> {
        std::mem::take(&mut self.working().stopped)
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Strokes go through the plugins on in `plugins`, as they come and go, from here on.
    pub fn use_plugins(&self, plugins: Arc<Plugins>) {
        let hooks = plugins.hooks();
        self.shared.lock().hooks = Some(hooks);
    }

    /// The picker plugin's picture for `hue` (0..1), `side` pixels square: premultiplied RGBA rows,
    /// drawn on the GPU (it is compiled once). None with no picker plugin on or no GPU yet, or if it
    /// couldn't be drawn: one that runs away or takes too long goes off, and says why.
    pub fn picker_picture(&self, hue: f32, side: u32) -> Option<Vec<u8>> {
        let (hooks, gpu, cached) = {
            let st = self.shared.lock();
            (st.hooks.clone()?, st.gpu.clone()?, st.picker_program.clone())
        };
        let (id, text) = hooks.picker()?;
        let program = match cached.filter(|(was, code, _)| *was == id && *code == text) {
            Some((_, _, program)) => program,
            None => {
                let code = procraft_gpu::read_picture(&text).map_err(|why| log::warn!("a picture was refused: {why}"));
                let made = code.ok().and_then(|code| gpu.make_picture(code).ok());
                let Some(program) = made else {
                    hooks.stopped(&id, Stop::Damaged);
                    return None;
                };
                self.shared.lock().picker_program = Some((id.clone(), Arc::clone(&text), Arc::clone(&program)));
                program
            }
        };
        match gpu.draw_picture(&program, hue, side) {
            Ok(rgba) => Some(rgba),
            // asked bigger than it can be drawn, or the device gone: not the plugin's doing
            Err(procraft_gpu::PictureStop::TooBig | procraft_gpu::PictureStop::Lost) => None,
            Err(procraft_gpu::PictureStop::Spent | procraft_gpu::PictureStop::Cut) => {
                hooks.stopped(&id, Stop::TooSlow);
                None
            }
        }
    }

    /// The notice an undo (`undo`) or a redo of the step `name` shows: "Undo Brush stroke", or
    /// "Redo" for a step with no name; none while a plugin hides it.
    pub fn step_notice(&self, undo: bool, name: Option<String>) -> Option<String> {
        let hooks = self.shared.lock().hooks.clone();
        if hooks.is_some_and(|hooks| hooks.hide_undo_notice()) {
            return None;
        }
        let word = if undo { "Undo" } else { "Redo" };
        Some(name.map_or_else(|| word.to_owned(), |name| format!("{word} {name}")))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::Instant;

    use procraft_engine::view::View;
    use procraft_gpu::Gpu;
    use serde_json::{Value, json};

    use super::*;
    use crate::plugin_file::tests::{coded, pack};
    use crate::plugin_run::tests::{ACTS, FILL_AT, OFFSET, TAKES_MOST, fill_module, position_module};
    use crate::plugin_run::{MODULE, Running};
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
        let mut manifest = coded(ID, &offset());
        manifest["controls"] = json!([
            { "key": "x", "type": "slider", "label": "X", "least": -100, "most": 100, "step": 1, "starts": 0 },
            { "key": "y", "type": "slider", "label": "Y", "least": -100, "most": 100, "step": 1, "starts": 0 },
        ]);
        manifest
    }

    /// A list holding the offset plugin, in `dir`.
    fn with_offset(dir: &str) -> Arc<Plugins> {
        let plugins = Plugins::with_store(dir.to_owned());
        assert_eq!(plugins.install(file(&offset_manifest(), &offset())), None);
        plugins
    }

    /// A plugin of no controls whose `procraft_position` does `body`, imported and on.
    fn turned_on(dir: &str, before: &str, body: &str) -> Arc<Plugins> {
        let module = position_module(before, body);
        let plugins = Plugins::with_store(dir.to_owned());
        assert_eq!(plugins.install(file(&coded(ID, &module), &module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        plugins
    }

    fn card(plugins: &Plugins) -> crate::plugins::PluginInfo {
        plugins.plugins().remove(0)
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
        // whatever it is, even what no plugin could hand on
        let (x, y) = hooks.position(f32::NAN, f32::INFINITY);
        assert!(x.is_nan() && y == f32::INFINITY);
    }

    #[test]
    fn a_sample_that_isn_t_a_position_doesn_t_stop_a_plugin() {
        let hooks = Hooks::default();
        hooks.put_on("offset".to_owned(), Running::start(&offset(), &[]).unwrap(), &[Hook::Position]);
        assert!(hooks.position(f32::NAN, 2.0).0.is_nan());
        assert_eq!(hooks.position(1.0, f32::INFINITY), (1.0, f32::INFINITY));
        assert!(hooks.is_on("offset"));
        assert_eq!(hooks.take_stopped(), []);
    }

    /// A plugin that says the pick reads the layer as its one switch says, in a store at `dir`.
    fn picking(dir: &str) -> Arc<Plugins> {
        let module = wat::parse_str(PICKS).unwrap();
        let plugins = Plugins::with_store(dir.to_owned());
        let mut manifest = coded(ID, &module);
        manifest["hooks"] = json!(["pick_layer"]);
        manifest["controls"] = json!([
            { "key": "layer", "type": "switch", "label": "Layer", "keeps": "eyedropper_layer" },
        ]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        plugins
    }

    #[test]
    fn the_pick_reads_the_layer_only_while_a_plugin_says_so() {
        let dir = store("pick");
        let plugins = picking(&dir);
        let hooks = plugins.hooks();
        assert!(!hooks.pick_layer(), "imported, it says nothing");
        assert!(plugins.set_on(ID.to_owned(), true));
        assert!(!hooks.pick_layer(), "on, it goes by its switch");
        assert!(plugins.set_switch("eyedropper_layer".to_owned(), true));
        assert!(hooks.pick_layer());
        assert!(plugins.set_on(ID.to_owned(), false));
        assert!(!hooks.pick_layer());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A plugin that says ColorDrop fills by every layer, in a store at `dir`.
    /// A plugin on whose action is `module`'s, reaching what `permissions` give.
    fn acting(dir: &str, module: &[u8], permissions: Value) -> Arc<Plugins> {
        let plugins = Plugins::with_store(dir.to_owned());
        let mut manifest = coded(ID, module);
        manifest["compat"]["procraft"]["interface"] = json!(2);
        manifest["hooks"] = json!([]);
        manifest["permissions"] = permissions;
        manifest["actions"] = json!([{ "key": "paint", "label": "Paint a square" }]);
        assert_eq!(plugins.install(file(&manifest, module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        plugins
    }

    #[test]
    fn an_action_paints_a_layer_of_its_own_and_one_undo_takes_it_back() {
        let Some(s) = session() else { return };
        let dir = store("action-session");
        let plugins = acting(&dir, &wat::parse_str(ACTS).unwrap(), json!(["artwork.write"]));
        let before = s.layers().len();
        assert!(plugins.run_action(format!("{ID}.paint"), Arc::clone(&s)));
        let layers = s.layers();
        assert_eq!(layers.len(), before + 1);
        let grid = layers.iter().find(|l| l.name == "Grid").expect("named");
        assert!(grid.current, "the layer added is the current one");
        let rgba = picture(&s);
        let at = |x: usize, y: usize| rgba[(y * 200 + x) * 4..(y * 200 + x) * 4 + 4].to_vec();
        assert_eq!(at(25, 35), [255, 0, 0, 255], "painted inside the square");
        assert_eq!(at(19, 35)[3], 0, "and nowhere else");
        assert_eq!(at(30, 35)[3], 0);
        // one undo takes the layer, its name and its paint back together
        s.undo();
        assert_eq!(s.layers().len(), before);
        assert!(s.layers().iter().all(|l| l.name != "Grid"));
        // a key it doesn't have does nothing, and nor does it off
        assert!(!plugins.run_action(format!("{ID}.other"), Arc::clone(&s)));
        assert!(plugins.set_on(ID.to_owned(), false));
        assert!(!plugins.run_action(format!("{ID}.paint"), Arc::clone(&s)));
        assert_eq!(s.layers().len(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plugin_of_switches_answers_from_its_controls_while_on() {
        let dir = store("switches");
        let plugins = Plugins::with_store(dir.clone());
        let hooks = plugins.hooks();
        let mut manifest = crate::plugin_file::tests::switches(ID, json!(["radial_segments", "hide_undo_notice"]));
        manifest["controls"] = json!([
            { "key": "parts", "type": "slider", "label": "Segments", "least": 2, "most": 32, "step": 1, "starts": 8 },
        ]);
        manifest["answers"] = json!({ "radial_segments": "parts" });
        assert_eq!(plugins.install(pack(&manifest, &[])), None);
        assert_eq!((hooks.radial_segments(), hooks.hide_undo_notice()), (None, false), "imported, it says nothing");
        assert!(plugins.set_on(ID.to_owned(), true));
        assert_eq!((hooks.radial_segments(), hooks.hide_undo_notice()), (Some(8), true));
        // its slider moved: the answer follows
        assert!(plugins.set_slider(format!("{ID}.parts"), 12.0).is_some());
        assert_eq!(hooks.radial_segments(), Some(12));
        // read back as the next start reads it, still on and as it was left
        let next = restarted(&dir, &plugins);
        assert_eq!(next.hooks().radial_segments(), Some(12));
        assert!(plugins.set_on(ID.to_owned(), false));
        assert_eq!((hooks.radial_segments(), hooks.hide_undo_notice()), (None, false));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Told an event, adds a layer named "After".
    const TOLD: &str = r#"(module
        (import "procraft" "add_layer" (func $add (result i64)))
        (import "procraft" "set_layer_name" (func $name (param i64 i32 i32) (result i32)))
        (memory (export "memory") 1)
        (data (i32.const 0) "After")
        (func (export "procraft_event") (param i32) (result i32)
            (drop (call $name (call $add) (i32.const 0) (i32.const 5)))
            (i32.const 0)))"#;

    #[test]
    fn a_plugin_is_told_a_stroke_was_kept_and_a_layer_added_and_acts_as_one_step() {
        let Some(s) = session() else { return };
        let dir = store("events");
        let module = wat::parse_str(TOLD).unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = coded(ID, &module);
        manifest["compat"]["procraft"]["interface"] = json!(2);
        manifest["hooks"] = json!(["stroke_kept", "layer_added"]);
        manifest["permissions"] = json!(["artwork.write"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        s.use_plugins(Arc::clone(&plugins));
        let afters = |s: &CanvasSession| s.layers().iter().filter(|l| l.name == "After").count();
        let before = s.layers().len();
        // a stroke kept: told once
        draw(&s);
        assert_eq!(afters(&s), 1);
        assert_eq!(s.layers().len(), before + 1);
        // a layer added from the panel: told, and the layer the plugin adds itself tells nothing
        s.add_layer();
        assert_eq!(afters(&s), 2);
        assert_eq!(s.layers().len(), before + 3);
        // undo takes the plugin's step back first, then the layer added
        s.undo();
        assert_eq!((afters(&s), s.layers().len()), (1, before + 2));
        s.undo();
        assert_eq!((afters(&s), s.layers().len()), (1, before + 1));
        // off: told nothing
        assert!(plugins.set_on(ID.to_owned(), false));
        s.add_layer();
        assert_eq!(afters(&s), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// On `stroke_kept` and `opacity_together`: told a stroke, it sets the current layer's opacity
    /// to a half, which asks its own `opacity_together`.
    const HALVES: &str = r#"(module
        (import "procraft" "current_layer" (func $current (result i64)))
        (import "procraft" "set_layer_opacity" (func $opacity (param i64 f32) (result i32)))
        (func (export "procraft_opacity_together") (result i32) (i32.const 1))
        (func (export "procraft_event") (param i32) (result i32)
            (drop (call $opacity (call $current) (f32.const 0.5)))
            (i32.const 0)))"#;

    #[test]
    fn an_event_that_calls_into_its_own_other_hook_doesn_t_wait_on_itself() {
        let Some(s) = session() else { return };
        let dir = store("events-own-hook");
        let module = wat::parse_str(HALVES).unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = coded(ID, &module);
        manifest["compat"]["procraft"]["interface"] = json!(2);
        manifest["hooks"] = json!(["stroke_kept", "opacity_together"]);
        manifest["permissions"] = json!(["artwork.read", "artwork.write"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        s.use_plugins(Arc::clone(&plugins));
        // (on one copy of its code this waited on itself for good)
        draw(&s);
        let current = s.layers().into_iter().find(|l| l.current).unwrap();
        assert!((current.opacity - 0.5).abs() < 1e-3, "{}", current.opacity);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_layer_the_app_adds_for_its_own_work_tells_no_plugin() {
        let Some(s) = session() else { return };
        let dir = store("events-quiet");
        let module = wat::parse_str(TOLD).unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = coded(ID, &module);
        manifest["compat"]["procraft"]["interface"] = json!(2);
        manifest["hooks"] = json!(["layer_added"]);
        manifest["permissions"] = json!(["artwork.write"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        s.use_plugins(Arc::clone(&plugins));
        draw(&s);
        s.select_shape(false, 0.0, 0.0, 100.0, 100.0, crate::SelectionJoin::Add);
        let before = s.layers().len();
        assert!(s.copy_selection_to_layer().is_some());
        assert_eq!(s.layers().len(), before + 1, "a selection copied to a layer: no plugin told");
        assert!(s.layers().iter().all(|l| l.name != "After"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_grid_plugin_adds_a_grid_layer_in_the_brush_colour() {
        let Some(s) = session() else { return };
        let dir = store("grid-layer");
        let plugins = Plugins::with_store(dir.clone());
        let (_, file) = crate::bundled::BUNDLED.iter().find(|(name, _)| *name == "grid-layer").unwrap();
        assert_eq!(plugins.install(file.to_vec()), None);
        let id = "io.github.artcraftandroid.grid-layer";
        assert!(plugins.set_on(id.to_owned(), true));
        // lines 50 apart, 3 wide, in blue
        let card = plugins.plugins().into_iter().find(|p| p.id == id).unwrap();
        assert_eq!(card.actions.len(), 1);
        // on its card only
        assert!(plugins.actions_in(crate::plugins::PluginPlace::Adjustments).is_empty());
        assert_eq!(plugins.actions_in(crate::plugins::PluginPlace::Card), card.actions);
        assert!(plugins.set_slider(format!("{id}.spacing"), 50.0).is_some());
        assert!(plugins.set_slider(format!("{id}.line"), 3.0).is_some());
        s.set_color(0.0, 0.0, 1.0);
        assert!(plugins.run_action(card.actions[0].key.clone(), Arc::clone(&s)));
        assert!(s.layers().iter().any(|l| l.name == "Grid" && l.current));
        let rgba = picture(&s);
        let at = |x: usize, y: usize| rgba[(y * 200 + x) * 4..(y * 200 + x) * 4 + 4].to_vec();
        for (x, y) in [(0, 10), (2, 10), (50, 77), (152, 199), (10, 0), (77, 102), (199, 150)] {
            assert_eq!(at(x, y), [0, 0, 255, 255], "a line through ({x}, {y})");
        }
        for (x, y) in [(3, 10), (49, 77), (53, 77), (25, 25), (175, 175)] {
            assert_eq!(at(x, y)[3], 0, "between lines at ({x}, {y})");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_action_s_panel_stays_open_its_touches_told_to_the_same_code() {
        let Some(s) = session() else { return };
        let dir = store("action-panel");
        let json =
            r#"{"title":"Squares","rows":[{"type":"button","label":"Add a layer"},{"type":"button","label":"Done"}]}"#;
        let text = format!(
            r#"(module
                (import "procraft" "show_panel" (func $show (param i32 i32) (result i32)))
                (import "procraft" "close_panel" (func $close))
                (import "procraft" "add_layer" (func $add (result i64)))
                (memory (export "memory") 1)
                (data (i32.const 0) "{}")
                (func (export "procraft_action") (param i32) (result i32)
                    (drop (call $show (i32.const 0) (i32.const {}))) (i32.const 0))
                (func (export "procraft_panel_event") (param $row i32) (param f32) (result i32)
                    (if (i32.eqz (local.get $row)) (then (drop (call $add))) (else (call $close)))
                    (i32.const 0)))"#,
            json.replace('"', "\\\""),
            json.len()
        );
        let plugins = acting(&dir, &wat::parse_str(text).unwrap(), json!(["artwork.write", "panel"]));
        assert_eq!(plugins.panel(), None);
        assert!(plugins.run_action(format!("{ID}.paint"), Arc::clone(&s)));
        let panel = plugins.panel().expect("shown");
        assert_eq!(panel.title, "Squares");
        assert_eq!(panel.rows.len(), 2);
        // its first row adds a layer, one step to undo; the panel stays
        let before = s.layers().len();
        assert!(plugins.panel_touched(0, 1.0, Arc::clone(&s)));
        assert_eq!(s.layers().len(), before + 1);
        s.undo();
        assert_eq!(s.layers().len(), before);
        assert!(plugins.panel().is_some());
        // its second closes it
        assert!(!plugins.panel_touched(1, 1.0, Arc::clone(&s)));
        assert_eq!(plugins.panel(), None);
        assert!(!plugins.panel_touched(0, 1.0, Arc::clone(&s)));
        assert_eq!(s.layers().len(), before);
        // turned off, its panel goes with it
        assert!(plugins.run_action(format!("{ID}.paint"), Arc::clone(&s)));
        assert!(plugins.panel().is_some());
        assert!(plugins.set_on(ID.to_owned(), false));
        assert_eq!(plugins.panel(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_picker_plugin_draws_classic_and_says_where_touches_land() {
        let Some(s) = session() else { return };
        let dir = store("picker");
        let plugins = Plugins::with_store(dir.clone());
        let file = crate::plugin_file::tests::picker_file(ID, Some(crate::plugin_run::tests::PICTURE));
        assert_eq!(plugins.install(file), None);
        assert!(!plugins.picker_on(), "off until turned on");
        assert!(plugins.set_on(ID.to_owned(), true));
        s.use_plugins(Arc::clone(&plugins));
        assert!(plugins.picker_on());
        assert_eq!(plugins.picker_pick(0.25, 0.75), Some(vec![0.25, 0.25]));
        assert_eq!(plugins.picker_point(0.5, 0.0), Some(vec![0.5, 1.0]));
        // its picture, for the hue asked: red across, green down, the hue in blue
        let rgba = s.picker_picture(0.5, 8).expect("drawn");
        assert_eq!(rgba.len(), 8 * 8 * 4);
        let last = &rgba[rgba.len() - 4..];
        assert!(last[0] > 230 && last[1] > 230 && (last[2] as i32 - 128).abs() <= 1 && last[3] == 255, "{last:?}");
        // off, there is none
        assert!(plugins.set_on(ID.to_owned(), false));
        assert!(!plugins.picker_on());
        assert_eq!(s.picker_picture(0.5, 8), None);
        assert_eq!(plugins.picker_pick(0.25, 0.75), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_picker_picture_that_never_ends_switches_its_plugin_off() {
        let Some(s) = session() else { return };
        let dir = store("picker-loops");
        let plugins = Plugins::with_store(dir.clone());
        let loops =
            "fn picture(at: vec2<f32>) -> vec4<f32> { var x = at.x; loop { x = x + 1.0; } return vec4<f32>(x); }";
        assert_eq!(plugins.install(crate::plugin_file::tests::picker_file(ID, Some(loops))), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        s.use_plugins(Arc::clone(&plugins));
        assert_eq!(s.picker_picture(0.0, 64), None);
        let card = plugins.plugins().into_iter().find(|p| p.id == ID).unwrap();
        assert!(!card.on);
        assert_eq!(card.problem.as_deref(), Some("Switched off: it took too long."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_session_showing_a_plugin_s_artwork_sends_it_strokes_and_undoes_by_its_steps() {
        let Some(s) = session() else { return };
        let dir = store("plugin-artwork");
        let plugins = Plugins::with_store(dir.clone());
        let file = crate::plugin_file::tests::dots_file(ID, json!([{ "key": "dots", "label": "Dots" }]));
        assert_eq!(plugins.install(file), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        let kind = format!("{ID}.dots");
        let canvas = plugins.open_canvas(kind.clone(), 200, 200, None).unwrap();
        // only a canvas its size
        let small = plugins.open_canvas(kind.clone(), 100, 100, None).unwrap();
        assert!(!s.attach_plugin_canvas(kind.clone(), small));
        assert!(s.attach_plugin_canvas(kind.clone(), canvas));
        assert_eq!(s.plugin_kind(), Some(kind));
        let red_at = |s: &CanvasSession, x: usize, y: usize| {
            picture(s)[(y * 200 + x) * 4..(y * 200 + x) * 4 + 4] == [255, 0, 0, 255]
        };
        // a stroke goes to it, not to the brush: the dot follows the pen and the brush draws nothing
        s.stroke_begin(PointerKind::Stylus, vec![sample(40.0, 40.0, 0.0)]);
        s.stroke_add(vec![sample(60.0, 40.0, 8.0)]);
        s.stroke_end(vec![sample(100.0, 120.0, 16.0)]);
        assert!(red_at(&s, 100, 120), "the dot where the stroke ended");
        assert!(!red_at(&s, 40, 40) && !red_at(&s, 60, 40), "nothing else drawn");
        assert!(s.info().can_undo);
        let first = s.plugin_data().unwrap();
        // another, then undone back to the first, and redone
        s.stroke_begin(PointerKind::Stylus, vec![sample(30.0, 160.0, 0.0)]);
        s.stroke_end(vec![]);
        assert!(red_at(&s, 30, 160) && !red_at(&s, 100, 120));
        s.undo();
        assert!(red_at(&s, 100, 120) && !red_at(&s, 30, 160));
        assert_eq!(s.plugin_data().unwrap(), first);
        assert!(s.info().can_redo);
        s.redo();
        assert!(red_at(&s, 30, 160));
        // saved, its kind named and its data beside; opened again, its plugin takes it back
        let folder = store("plugin-artwork-saved");
        s.save(folder.clone()).unwrap();
        let json = std::fs::read_to_string(std::path::Path::new(&folder).join("artwork.json")).unwrap();
        assert!(json.contains(&format!("\"plugin\":\"{ID}.dots\"")), "{json}");
        let data = std::fs::read(std::path::Path::new(&folder).join(crate::project::PLUGIN_DATA)).unwrap();
        assert_eq!(Some(data.clone()), s.plugin_data());
        let opened = CanvasSession::open(folder.clone()).unwrap();
        assert_eq!(opened.plugin_kind(), Some(format!("{ID}.dots")));
        assert!(opened.open_plugin_artwork(Arc::clone(&plugins)));
        assert_eq!(opened.plugin_data(), Some(data));
        // its plugin off, it opens as what it last drew, the plugin's kind and data kept through a save
        let again = CanvasSession::open(folder.clone()).unwrap();
        assert!(plugins.set_on(ID.to_owned(), false));
        assert!(!again.open_plugin_artwork(Arc::clone(&plugins)));
        again.save(folder.clone()).unwrap();
        let json = std::fs::read_to_string(std::path::Path::new(&folder).join("artwork.json")).unwrap();
        assert!(json.contains(&format!("{ID}.dots")));
        let _ = std::fs::remove_dir_all(&folder);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_vector_canvas_plugin_draws_strokes_in_the_brush_colour_and_undoes_them() {
        let Some(s) = session() else { return };
        let dir = store("vector-canvas");
        let plugins = Plugins::with_store(dir.clone());
        let (_, file) = crate::bundled::BUNDLED.iter().find(|(name, _)| *name == "vector-canvas").unwrap();
        assert_eq!(plugins.install(file.to_vec()), None);
        let id = "io.github.artcraftandroid.vector-canvas";
        assert!(plugins.set_on(id.to_owned(), true));
        let kind = plugins.kinds().into_iter().find(|k| k.key == format!("{id}.vector")).expect("listed").key;
        let canvas = plugins.open_canvas(kind.clone(), 200, 200, None).unwrap();
        assert!(s.attach_plugin_canvas(kind, canvas));
        s.set_color(0.0, 0.0, 1.0);
        s.set_brush_size(0.5);
        s.stroke_begin(PointerKind::Stylus, vec![sample(40.0, 100.0, 0.0)]);
        s.stroke_add((1..30).map(|i| sample(40.0 + i as f32 * 4.0, 100.0, f64::from(i) * 8.0)).collect());
        s.stroke_end(vec![sample(160.0, 100.0, 248.0)]);
        let rgba = picture(&s);
        let at = |x: usize, y: usize| rgba[(y * 200 + x) * 4..(y * 200 + x) * 4 + 4].to_vec();
        assert_eq!(at(100, 100), [0, 0, 255, 255], "a blue stroke through the middle");
        assert_eq!(at(100, 40)[3], 0, "and nothing far from it");
        // a second stroke is drawn over the first's picture: both show, as drawn whole
        s.set_color(1.0, 0.0, 0.0);
        s.stroke_begin(PointerKind::Stylus, vec![sample(100.0, 40.0, 0.0)]);
        s.stroke_add((1..30).map(|i| sample(100.0, 40.0 + i as f32 * 4.0, f64::from(i) * 8.0)).collect());
        s.stroke_end(vec![sample(100.0, 160.0, 248.0)]);
        let both = picture(&s);
        let at2 = |x: usize, y: usize| both[(y * 200 + x) * 4..(y * 200 + x) * 4 + 4].to_vec();
        assert_eq!(at2(60, 100), [0, 0, 255, 255], "the first still there");
        assert_eq!(at2(100, 60), [255, 0, 0, 255], "the second over it");
        s.undo();
        assert_eq!(picture(&s)[(60 * 200 + 100) * 4 + 3], 0, "the second undone");
        // shared as SVG, its shapes kept in its colour
        assert!(s.exports_svg());
        let svg = std::path::Path::new(&dir).join("artwork.svg");
        s.export_svg(svg.to_string_lossy().into_owned()).unwrap();
        let text = std::fs::read_to_string(&svg).unwrap();
        assert!(text.starts_with("<svg ") && text.contains("fill=\"#0000ff\""), "{text}");
        s.undo();
        assert_eq!(picture(&s)[(100 * 200 + 100) * 4 + 3], 0, "undone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plugin_s_artwork_keeps_its_layer_its_steps_and_its_data_honestly() {
        let Some(s) = session() else { return };
        let dir = store("plugin-artwork-honest");
        let plugins = Plugins::with_store(dir.clone());
        let (_, file) = crate::bundled::BUNDLED.iter().find(|(name, _)| *name == "vector-canvas").unwrap();
        assert_eq!(plugins.install(file.to_vec()), None);
        let id = "io.github.artcraftandroid.vector-canvas";
        assert!(plugins.set_on(id.to_owned(), true));
        let kind = format!("{id}.vector");
        let canvas = plugins.open_canvas(kind.clone(), 200, 200, None).unwrap();
        assert!(s.attach_plugin_canvas(kind.clone(), canvas));
        let art_layer = s.layers().iter().find(|l| l.current).unwrap().id;
        // a stroke called off leaves no step to undo
        s.stroke_begin(PointerKind::Stylus, vec![sample(40.0, 100.0, 0.0)]);
        s.stroke_add(vec![sample(60.0, 100.0, 8.0)]);
        s.stroke_cancel();
        assert!(!s.info().can_undo, "nothing to undo after a stroke called off");
        // with no step of its own, undo takes back the canvas's (a layer added)
        let before = s.layers().len();
        s.add_layer();
        assert_eq!(s.layers().len(), before + 1);
        s.undo();
        assert_eq!(s.layers().len(), before, "the layer added undone");
        // a layer added keeps the plugin's picture where it was, through a save
        let added = s.add_layer();
        s.set_color(0.0, 0.0, 1.0);
        s.set_brush_size(0.5);
        s.stroke_begin(PointerKind::Stylus, vec![sample(40.0, 100.0, 0.0)]);
        s.stroke_add((1..30).map(|i| sample(40.0 + i as f32 * 4.0, 100.0, f64::from(i) * 8.0)).collect());
        s.stroke_end(vec![sample(160.0, 100.0, 248.0)]);
        let alpha_on = |s: &CanvasSession, layer: u64| {
            let mut st = s.shared.lock();
            st.layer_pixels(procraft_engine::document::LayerId(layer)).unwrap()[(100 * 200 + 100) * 4 + 3]
        };
        assert_eq!(alpha_on(&s, art_layer), 255, "drawn on the plugin's layer");
        assert_eq!(alpha_on(&s, added), 0, "not on the layer added");
        let folder = store("plugin-artwork-honest-saved");
        s.save(folder.clone()).unwrap();
        let opened = CanvasSession::open(folder.clone()).unwrap();
        assert!(opened.open_plugin_artwork(Arc::clone(&plugins)));
        assert_eq!(opened.shared.lock().plugin_artwork.as_ref().map(|a| a.layer.0), Some(art_layer));
        // its data gone, it opens as what it last drew, not as a new, empty artwork
        std::fs::remove_file(std::path::Path::new(&folder).join(crate::project::PLUGIN_DATA)).unwrap();
        let bare = CanvasSession::open(folder.clone()).unwrap();
        assert!(!bare.open_plugin_artwork(Arc::clone(&plugins)));
        let _ = std::fs::remove_dir_all(&folder);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_eraser_cuts_a_vector_stroke_on_the_canvas() {
        let Some(s) = session() else { return };
        let dir = store("vector-eraser");
        let plugins = Plugins::with_store(dir.clone());
        let (_, file) = crate::bundled::BUNDLED.iter().find(|(name, _)| *name == "vector-canvas").unwrap();
        assert_eq!(plugins.install(file.to_vec()), None);
        let id = "io.github.artcraftandroid.vector-canvas";
        assert!(plugins.set_on(id.to_owned(), true));
        let kind = format!("{id}.vector");
        assert!(s.attach_plugin_canvas(kind.clone(), plugins.open_canvas(kind, 200, 200, None).unwrap()));
        s.set_color(0.0, 0.0, 1.0);
        s.set_brush_size(0.5);
        s.stroke_begin(PointerKind::Stylus, vec![sample(40.0, 100.0, 0.0)]);
        s.stroke_add((1..30).map(|i| sample(40.0 + i as f32 * 4.0, 100.0, f64::from(i) * 8.0)).collect());
        s.stroke_end(vec![sample(160.0, 100.0, 248.0)]);
        let alpha = |s: &CanvasSession, x: usize| picture(s)[(100 * 200 + x) * 4 + 3];
        assert_eq!((alpha(&s, 60), alpha(&s, 100), alpha(&s, 140)), (255, 255, 255));
        // the eraser down across the middle: the middle goes, the ends stay; one undo brings it back
        s.set_tool(crate::ToolKind::Erase);
        s.stroke_begin(PointerKind::Stylus, vec![sample(100.0, 80.0, 0.0)]);
        s.stroke_add((1..10).map(|i| sample(100.0, 80.0 + i as f32 * 4.0, f64::from(i) * 8.0)).collect());
        s.stroke_end(vec![sample(100.0, 120.0, 88.0)]);
        assert_eq!((alpha(&s, 60), alpha(&s, 100), alpha(&s, 140)), (255, 0, 255));
        s.undo();
        assert_eq!(alpha(&s, 100), 255);
        // its switch turned on with the artwork open: the eraser takes the whole stroke at once
        assert!(plugins.set_switch(format!("{id}.whole"), true));
        s.stroke_begin(PointerKind::Stylus, vec![sample(100.0, 80.0, 0.0)]);
        s.stroke_add((1..10).map(|i| sample(100.0, 80.0 + i as f32 * 4.0, f64::from(i) * 8.0)).collect());
        s.stroke_end(vec![sample(100.0, 120.0, 88.0)]);
        assert_eq!((alpha(&s, 60), alpha(&s, 100), alpha(&s, 140)), (0, 0, 0), "all of it gone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_action_that_crashes_goes_off_and_leaves_one_step_to_undo() {
        let Some(s) = session() else { return };
        let dir = store("action-crash");
        let crashes = r#"(module
            (import "procraft" "add_layer" (func $add (result i64)))
            (func (export "procraft_action") (param i32) (result i32)
                (drop (call $add)) (unreachable)))"#;
        let plugins = acting(&dir, &wat::parse_str(crashes).unwrap(), json!(["artwork.write"]));
        let before = s.layers().len();
        assert!(!plugins.run_action(format!("{ID}.paint"), Arc::clone(&s)));
        let plugin = plugins.plugins().into_iter().find(|p| p.id == ID).unwrap();
        assert!(!plugin.on);
        assert_eq!(plugin.problem.as_deref(), Some("Switched off: it crashed."));
        // what it did before is still one step
        assert_eq!(s.layers().len(), before + 1);
        s.undo();
        assert_eq!(s.layers().len(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn filling_by_all(dir: &str) -> Arc<Plugins> {
        let module =
            wat::parse_str(r#"(module (func (export "procraft_fill_all_layers") (result i32) (i32.const 1)))"#)
                .unwrap();
        let plugins = Plugins::with_store(dir.to_owned());
        let mut manifest = coded(ID, &module);
        manifest["hooks"] = json!(["fill_all_layers"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        plugins
    }

    #[test]
    fn a_fill_goes_by_every_layer_only_while_a_plugin_says_so() {
        let dir = store("fill-all");
        let plugins = filling_by_all(&dir);
        let hooks = plugins.hooks();
        assert!(!hooks.fill_all_layers(), "imported, it says nothing");
        assert!(plugins.set_on(ID.to_owned(), true));
        assert!(hooks.fill_all_layers());
        assert!(plugins.set_on(ID.to_owned(), false));
        assert!(!hooks.fill_all_layers());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn colordrop_on_an_empty_layer_stops_at_another_layers_lines_with_the_plugin_on() {
        let Some(s) = session() else { return };
        let dir = store("fill-all-session");
        // a line across the canvas on the first layer, then an empty layer over it
        s.set_brush_size(0.5);
        s.stroke_begin(PointerKind::Finger, vec![sample(100.0, -10.0, 0.0)]);
        s.stroke_add((1..=44).map(|i| sample(100.0, -10.0 + i as f32 * 5.0, f64::from(i) * 8.0)).collect());
        s.stroke_end(vec![]);
        let line = s.layers()[0].id;
        let empty = s.add_layer();
        assert_ne!(empty, line);
        let red_at = |s: &CanvasSession, x: usize| {
            s.await_pour();
            let rgba = picture(s);
            let at = (100 * 200 + x) * 4;
            rgba[at] > 200 && rgba[at + 3] > 200
        };
        // as the app is: the empty layer fills whole
        assert!(s.pour_begin(40.0, 100.0, 1.0, 0.0, 0.0, 0.3));
        s.pour_end(true, None);
        assert!(red_at(&s, 40) && red_at(&s, 160), "fills the empty layer whole");
        s.undo();
        // with the plugin on: as far as the line on the layer under it
        let plugins = filling_by_all(&dir);
        assert!(plugins.set_on(ID.to_owned(), true));
        s.use_plugins(Arc::clone(&plugins));
        assert!(s.pour_begin(40.0, 100.0, 1.0, 0.0, 0.0, 0.3));
        s.pour_end(true, None);
        assert!(red_at(&s, 40), "filled where it was dropped");
        assert!(!red_at(&s, 160), "not past the other layer's line");
        assert_eq!(s.layers().iter().find(|l| l.current).map(|l| l.id), Some(empty), "on the current layer");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_interface_takes_a_plugin_s_colour_while_it_is_on() {
        let dir = store("interface-colour");
        // red for the accent, nothing said for any other role
        let module = wat::parse_str(
            r#"(module (func (export "procraft_interface_colour") (param $role i32) (result i32)
                (if (result i32) (i32.eqz (local.get $role)) (then (i32.const 0xff0000)) (else (i32.const -1)))))"#,
        )
        .unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = coded(ID, &module);
        manifest["hooks"] = json!(["interface_colour"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        assert_eq!(plugins.accent_colour(), None, "imported, it says nothing");
        assert!(plugins.set_on(ID.to_owned(), true));
        assert_eq!(plugins.accent_colour(), Some(0xFF0000));
        assert_eq!(plugins.hooks().interface_colour(1), None);
        assert!(plugins.set_on(ID.to_owned(), false));
        assert_eq!(plugins.accent_colour(), None);
        // a colour past 24 bits stops it
        let wild = wat::parse_str(
            r#"(module (func (export "procraft_interface_colour") (param i32) (result i32) (i32.const 0x1000000)))"#,
        )
        .unwrap();
        let dir2 = store("interface-colour-wild");
        let plugins = Plugins::with_store(dir2.clone());
        let mut manifest = coded(ID, &wild);
        manifest["hooks"] = json!(["interface_colour"]);
        assert_eq!(plugins.install(file(&manifest, &wild)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        assert_eq!(plugins.accent_colour(), None);
        assert!(card(&plugins).problem.is_some());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    /// The Brush sounds plugin the app comes with, in a store of its own, not on yet.
    fn brush_sounds(dir: &str) -> Arc<Plugins> {
        let plugins = Plugins::with_store(dir.to_owned());
        let (_, bytes) = crate::bundled::BUNDLED.iter().find(|(name, _)| *name == "brush-sounds").unwrap();
        assert_eq!(plugins.install(bytes.to_vec()), None);
        plugins
    }

    const SOUNDS: &str = "io.github.artcraftandroid.brush-sounds";

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
    }

    #[test]
    fn a_stroke_sounds_as_fast_as_it_moves_while_the_sound_plugin_is_on() {
        let Some(s) = session() else { return };
        let dir = store("sound");
        let plugins = brush_sounds(&dir);
        s.use_plugins(Arc::clone(&plugins));
        // a fast stroke under way
        let fast: Vec<_> = (0..20).map(|i| sample(10.0 + i as f32 * 10.0, 50.0, f64::from(i) * 4.0)).collect();
        s.stroke_begin(PointerKind::Finger, fast[..1].to_vec());
        s.stroke_add(fast[1..].to_vec());
        assert!(!plugins.makes_stroke_sound());
        assert_eq!(rms(&s.stroke_sound(2_400, 48_000)), 0.0, "off: silent");
        assert!(plugins.set_on(SOUNDS.to_owned(), true));
        assert!(plugins.makes_stroke_sound());
        // (the first readies the plugin's sounds)
        s.stroke_add(vec![sample(220.0, 50.0, 84.0)]);
        let _ = s.stroke_sound(2_400, 48_000);
        s.stroke_add(vec![sample(240.0, 50.0, 88.0)]);
        let heard = rms(&s.stroke_sound(2_400, 48_000));
        let problem = plugins.plugins().into_iter().find(|p| p.id == SOUNDS).and_then(|p| p.problem);
        assert!(heard > 0.005, "a fast stroke is heard: {heard}, {problem:?}");
        // its volume down: silent
        assert!(plugins.set_slider(format!("{SOUNDS}.volume"), 0.0).is_some());
        s.stroke_add(vec![sample(260.0, 50.0, 92.0)]);
        let _ = s.stroke_sound(2_400, 48_000);
        s.stroke_add(vec![sample(280.0, 50.0, 96.0)]);
        assert!(rms(&s.stroke_sound(2_400, 48_000)) < 1e-4, "at no volume");
        // it ends: quiet again
        assert!(plugins.set_slider(format!("{SOUNDS}.volume"), 1.0).is_some());
        s.stroke_end(vec![]);
        let _ = s.stroke_sound(9_600, 48_000);
        assert!(rms(&s.stroke_sound(2_400, 48_000)) < 0.002);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_recording_picked_is_kept_with_its_plugin_and_played_by_it() {
        let Some(s) = session() else { return };
        let dir = store("recording");
        let plugins = brush_sounds(&dir);
        assert!(plugins.set_on(SOUNDS.to_owned(), true));
        s.use_plugins(Arc::clone(&plugins));
        // every brush the airbrush, which hisses while pressed; its recording a 440 Hz tone
        assert!(plugins.set_slider(format!("{SOUNDS}.sound"), 5.0).is_some());
        let tone: Vec<f32> =
            (0..22_050).map(|i| 0.7 * (std::f32::consts::TAU * 440.0 * i as f32 / 44_100.0).sin()).collect();
        let key = format!("{SOUNDS}.airbrush_recording");
        assert!(!plugins.set_sound_file(key.clone(), "x".to_owned(), b"not a sound".to_vec()));
        assert!(plugins.set_sound_file(
            key.clone(),
            "Tone.wav".to_owned(),
            crate::sound_file::write_wav(&tone, 44_100)
        ));
        s.stroke_begin(PointerKind::Finger, vec![sample(100.0, 100.0, 0.0)]);
        let _ = s.stroke_sound(9_600, 48_000);
        s.stroke_add(vec![sample(101.0, 100.0, 4.0)]);
        let heard = s.stroke_sound(48_000, 48_000);
        let crossings = heard.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((800..960).contains(&crossings), "the tone: {crossings}");
        s.stroke_end(vec![]);
        // kept with the plugin, read back as it was, not among the settings
        let again = Plugins::with_store(dir.clone());
        let chosen = again.plugins().into_iter().flat_map(|p| p.controls).find_map(|c| match c {
            crate::plugins::PluginControl::File { control } if control.key == key => control.chosen,
            _ => None,
        });
        assert_eq!(chosen.as_deref(), Some("Tone.wav"));
        assert!(again.kept_lines().iter().all(|line| !line.starts_with(&key)));
        // let go: the file gone
        let (file, _) = crate::plugin_file::chosen_paths(std::path::Path::new(&dir), SOUNDS, &key);
        assert!(file.is_file());
        assert!(plugins.clear_file(key));
        assert!(!file.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sound_plugin_s_row_in_the_brush_studio_is_kept_with_the_brush() {
        let dir = store("rows");
        let plugins = brush_sounds(&dir);
        let rows = plugins.plugins().into_iter().find(|p| p.id == SOUNDS).unwrap().brush_controls;
        assert_eq!(rows.len(), 1);
        assert_eq!(
            (rows[0].key.as_str(), rows[0].choices.len()),
            ("plugin:io.github.artcraftandroid.brush-sounds:sound", 8)
        );
        let mut brush = procraft_engine::brush::Brush::round_pen();
        brush.set_setting(&rows[0].key, 5.0);
        assert_eq!(brush.setting(&rows[0].key), 5.0);
        brush.set_setting(&rows[0].key, 300.0);
        assert_eq!(brush.setting(&rows[0].key), 255.0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stroke_bending_twice_snaps_to_a_curve_while_a_plugin_says_so() {
        use crate::{QuickShapeAnchor, QuickShapeKind};
        let Some(s) = session() else { return };
        let dir = store("curves");
        let plugins = Plugins::with_store(dir.clone());
        let (_, bytes) = crate::bundled::BUNDLED.iter().find(|(name, _)| *name == "quickshape-curves").unwrap();
        assert_eq!(plugins.install(bytes.to_vec()), None);
        const ID: &str = "io.github.artcraftandroid.quickshape-curves";
        s.use_plugins(Arc::clone(&plugins));
        // a zigzag: across, back and across again, as a hand draws one
        let zigzag = |s: &CanvasSession, t0: f64| {
            let points = [(20.0, 20.0), (180.0, 30.0), (30.0, 100.0), (180.0, 170.0)];
            let mut samples = Vec::new();
            for (k, pair) in points.windows(2).enumerate() {
                let (a, b) = (pair[0], pair[1]);
                for i in 0..20 {
                    let f = i as f32 / 20.0;
                    let at = (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
                    samples.push(sample(at.0, at.1, t0 + ((k * 20 + i) * 8) as f64));
                }
            }
            s.stroke_begin(PointerKind::Finger, samples[..1].to_vec());
            s.stroke_add(samples[1..].to_vec());
            s.quickshape_hold()
        };
        // off: a polyline, as the app is
        let shape = zigzag(&s, 0.0).expect("it snaps");
        assert_eq!(shape.kind, QuickShapeKind::Polyline);
        s.quickshape_done();
        s.stroke_end(vec![]);
        // on: a curve, the polyline to be shown instead
        assert!(plugins.set_on(ID.to_owned(), true));
        let shape = zigzag(&s, 1000.0).expect("it snaps");
        assert_eq!(shape.kind, QuickShapeKind::Curve, "{shape:?}");
        assert!(shape.choices.contains(&QuickShapeKind::Polyline));
        assert!(shape.anchors.len() >= 3 && shape.anchors.iter().all(|a| *a == QuickShapeAnchor::Smooth));
        // a tap on an anchor in the middle steps it on: broken, then a corner
        let shape = s.quickshape_cycle_anchor(3).expect("an anchor");
        assert_eq!(shape.anchors[1], QuickShapeAnchor::Broken);
        let shape = s.quickshape_cycle_anchor(3).expect("an anchor");
        assert_eq!(shape.anchors[1], QuickShapeAnchor::Corner);
        // a control is no anchor
        assert!(s.quickshape_cycle_anchor(1).is_none());
        // shown as the polyline, then as a curve again: the plugin fits it afresh
        assert_eq!(s.quickshape_choose(QuickShapeKind::Polyline).map(|i| i.kind), Some(QuickShapeKind::Polyline));
        assert_eq!(s.quickshape_choose(QuickShapeKind::Curve).map(|i| i.kind), Some(QuickShapeKind::Curve));
        s.quickshape_done();
        s.stroke_end(vec![]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn randomise_sets_the_studio_brush_as_a_plugin_says() {
        let Some(s) = session() else { return };
        let dir = store("randomise");
        // every setting to the middle of its range, but the spacing (0) left alone
        let module = wat::parse_str(
            r#"(module (func (export "procraft_randomise") (param i32 i32) (result f32)
                (if (result f32) (i32.eqz (local.get 0)) (then (f32.const -1)) (else (f32.const 0.5)))))"#,
        )
        .unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = coded(ID, &module);
        manifest["hooks"] = json!(["randomise_brush"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        s.use_plugins(Arc::clone(&plugins));
        let _ = s.studio_begin();
        let keys = |keys: &[&str]| keys.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>();
        let before = s.studio_settings(keys(&["spacing", "streamline", "shape_rotation", "rendering_mode"]));
        assert!(!plugins.randomises_brushes());
        assert!(!s.studio_randomise(), "off, nothing changes");
        assert!(plugins.set_on(ID.to_owned(), true));
        assert!(plugins.randomises_brushes());
        assert!(s.studio_randomise());
        let after = s.studio_settings(keys(&["spacing", "streamline", "shape_rotation", "rendering_mode"]));
        assert_eq!(after[0], before[0], "the spacing left as it was");
        assert!((after[1] - 0.5).abs() < 1e-4, "{after:?}");
        assert!(after[2].abs() < 1e-4, "the middle of -1 to 1: {after:?}");
        assert_eq!(after[3].round(), 3.0, "a choice made whole: {after:?}");
        s.studio_end(false);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn picked_layers_take_one_opacity_together_while_a_plugin_says_so() {
        let Some(s) = session() else { return };
        let dir = store("opacity-together");
        let module =
            wat::parse_str(r#"(module (func (export "procraft_opacity_together") (result i32) (i32.const 1)))"#)
                .unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = coded(ID, &module);
        manifest["hooks"] = json!(["opacity_together"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        s.use_plugins(Arc::clone(&plugins));
        let first = s.layers()[0].id;
        let second = s.add_layer();
        let third = s.add_layer();
        assert!(s.pick_layer(first));
        let opacity = |s: &CanvasSession, id: u64| s.layers().iter().find(|l| l.id == id).map(|l| l.opacity);
        // off: only the one
        assert!(s.set_layer_opacity(third, 0.5, true));
        assert_eq!((opacity(&s, first), opacity(&s, third)), (Some(1.0), Some(0.5)));
        // on: every picked layer, as one step; one not picked keeps its own
        assert!(plugins.set_on(ID.to_owned(), true));
        assert!(s.set_layer_opacity(first, 0.25, false));
        assert!(s.set_layer_opacity(first, 0.3, true));
        assert_eq!((opacity(&s, first), opacity(&s, third)), (Some(0.3), Some(0.3)));
        assert_eq!(opacity(&s, second), Some(1.0));
        s.undo();
        assert_eq!((opacity(&s, first), opacity(&s, third)), (Some(1.0), Some(0.5)));
        // a layer not among the picked goes alone
        assert!(s.set_layer_opacity(second, 0.7, true));
        assert_eq!((opacity(&s, first), opacity(&s, second), opacity(&s, third)), (Some(1.0), Some(0.7), Some(0.5)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_touch_with_a_panel_open_paints_at_once_and_leaves_it_while_a_plugin_pins_it() {
        use crate::touch::{TouchCommand, TouchMove};
        let Some(s) = session() else { return };
        let dir = store("pin-panels");
        let module =
            wat::parse_str(r#"(module (func (export "procraft_pin_panels") (result i32) (i32.const 1)))"#).unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = coded(ID, &module);
        manifest["hooks"] = json!(["pin_panels"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        s.use_plugins(Arc::clone(&plugins));
        s.set_touch_panel_open(true);
        let closes = |out: &[TouchCommand]| {
            out.iter().any(|c| matches!(c, TouchCommand::PanelTapped | TouchCommand::PanelFades))
        };
        // off: a tap only closes the panel
        let mut out = s.touch_down(1, PointerKind::Finger, vec![sample(100.0, 100.0, 0.0)]);
        out.extend(s.touch_up(1, vec![sample(100.0, 100.0, 40.0)]));
        assert!(out.contains(&TouchCommand::PanelTapped), "{out:?}");
        // on: a tap and a drag paint, and the panel stays
        assert!(plugins.set_on(ID.to_owned(), true));
        let mut out = s.touch_down(2, PointerKind::Finger, vec![sample(100.0, 100.0, 100.0)]);
        out.extend(s.touch_up(2, vec![sample(100.0, 100.0, 140.0)]));
        assert!(!closes(&out), "{out:?}");
        let mut out = s.touch_down(3, PointerKind::Finger, vec![sample(100.0, 100.0, 200.0)]);
        out.extend(s.touch_moves(vec![TouchMove { id: 3, samples: vec![sample(140.0, 100.0, 220.0)] }]));
        out.extend(s.touch_moves(vec![TouchMove { id: 3, samples: vec![sample(180.0, 100.0, 240.0)] }]));
        out.extend(s.touch_up(3, vec![sample(200.0, 100.0, 260.0)]));
        assert!(!closes(&out), "{out:?}");
        assert!(out.contains(&TouchCommand::StrokeEnded), "{out:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_and_redo_show_no_notice_while_a_plugin_hides_it() {
        let Some(s) = session() else { return };
        let dir = store("undo-notice");
        assert_eq!(s.step_notice(true, Some("Brush stroke".into())).as_deref(), Some("Undo Brush stroke"));
        assert_eq!(s.step_notice(false, None).as_deref(), Some("Redo"));
        let module =
            wat::parse_str(r#"(module (func (export "procraft_hide_undo_notice") (result i32) (i32.const 1)))"#)
                .unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = coded(ID, &module);
        manifest["hooks"] = json!(["hide_undo_notice"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        s.use_plugins(Arc::clone(&plugins));
        assert!(s.step_notice(true, Some("Brush stroke".into())).is_some(), "imported, it says nothing");
        assert!(plugins.set_on(ID.to_owned(), true));
        assert_eq!(s.step_notice(true, Some("Brush stroke".into())), None);
        assert_eq!(s.step_notice(false, None), None);
        assert!(plugins.set_on(ID.to_owned(), false));
        assert_eq!(s.step_notice(false, Some("Fill".into())).as_deref(), Some("Redo Fill"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn radial_symmetry_goes_by_a_plugin_s_count_while_it_is_on() {
        let dir = store("radial");
        let module =
            wat::parse_str(r#"(module (func (export "procraft_radial_segments") (result i32) (i32.const 6)))"#)
                .unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = coded(ID, &module);
        manifest["hooks"] = json!(["radial_segments"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        let hooks = plugins.hooks();
        assert_eq!(hooks.radial_segments(), None, "imported, it says nothing");
        assert!(plugins.set_on(ID.to_owned(), true));
        assert_eq!(hooks.radial_segments(), Some(6));
        assert!(plugins.set_on(ID.to_owned(), false));
        assert_eq!(hooks.radial_segments(), None);
        // a count past what is allowed stops it
        let wild =
            wat::parse_str(r#"(module (func (export "procraft_radial_segments") (result i32) (i32.const 1000)))"#)
                .unwrap();
        let dir2 = store("radial-wild");
        let plugins = Plugins::with_store(dir2.clone());
        let mut manifest = coded(ID, &wild);
        manifest["hooks"] = json!(["radial_segments"]);
        assert_eq!(plugins.install(file(&manifest, &wild)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        assert_eq!(plugins.hooks().radial_segments(), None);
        assert!(card(&plugins).problem.is_some());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn a_pick_plugin_that_crashes_leaves_the_pick_on_the_canvas() {
        let dir = store("pick-crash");
        let module =
            wat::parse_str(r#"(module (func (export "procraft_pick_layer") (result i32) (unreachable)))"#).unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = coded(ID, &module);
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
    fn the_eyedropper_reads_the_current_layer_while_a_plugin_says_so() {
        let Some(s) = session() else { return };
        // paint on the first layer, then an empty one over it
        draw(&s);
        s.add_layer();
        let shown = s.pick(100.0, 50.0, 21).unwrap().rgb;
        assert!(shown.is_some());
        let dir = store("pick-session");
        let plugins = picking(&dir);
        s.use_plugins(Arc::clone(&plugins));
        assert_eq!(s.pick(100.0, 50.0, 21).unwrap().rgb, shown, "with none on, the canvas");
        plugins.set_on(ID.to_owned(), true);
        plugins.set_switch("eyedropper_layer".to_owned(), true);
        assert_eq!(s.pick(100.0, 50.0, 21).unwrap().rgb, None, "the current layer is clear there");
        plugins.set_on(ID.to_owned(), false);
        assert_eq!(s.pick(100.0, 50.0, 21).unwrap().rgb, shown);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A plugin that scales the interface by its one slider, in a store at `dir`.
    fn scaling(dir: &str) -> Arc<Plugins> {
        let module = wat::parse_str(SCALES).unwrap();
        let plugins = Plugins::with_store(dir.to_owned());
        let mut manifest = coded(ID, &module);
        manifest["hooks"] = json!(["interface_scale"]);
        manifest["controls"] = json!([{
            "key": "size", "type": "slider", "label": "Size", "least": 0.5, "most": 2, "step": 0.05, "starts": 1,
            "keeps": "interface_size",
        }]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        plugins
    }

    #[test]
    fn the_interface_shows_as_a_plugin_says_and_as_measured_without_one() {
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
    fn whatever_a_plugin_says_every_page_stays_in_reach() {
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
        // smaller is the plugin's to say, whatever the window
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
    fn a_scale_plugin_that_goes_wrong_leaves_the_interface_as_measured() {
        let dir = store("scale-wrong");
        let module = wat::parse_str(
            r#"(module (func (export "procraft_interface_scale") (param f32 f32) (result f32) (f32.const 40)))"#,
        )
        .unwrap();
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = coded(ID, &module);
        manifest["hooks"] = json!(["interface_scale"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        assert_eq!(plugins.interface_scale(1600.0, 1000.0), 1.0);
        assert_eq!(card(&plugins).problem.as_deref(), Some("Switched off: it gave an answer that makes no sense."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A list holding the paste plugin, its switch kept as the setting it took over, in `dir`.
    fn pasting(dir: &str) -> Arc<Plugins> {
        let module = wat::parse_str(PASTES).unwrap();
        let plugins = Plugins::with_store(dir.to_owned());
        let mut manifest = coded(ID, &module);
        manifest["hooks"] = json!(["paste_mask"]);
        manifest["controls"] = json!([
            { "key": "into", "type": "switch", "label": "Into", "keeps": "paste_into_mask" },
        ]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        plugins
    }

    /// `s` with the paste plugin on, its switch too, and where its files are.
    pub(crate) fn paste_into_masks(s: &CanvasSession, name: &str) -> (Arc<Plugins>, String) {
        let dir = store(name);
        let plugins = pasting(&dir);
        assert!(plugins.set_on(ID.to_owned(), true));
        assert!(plugins.set_switch("paste_into_mask".to_owned(), true));
        s.use_plugins(Arc::clone(&plugins));
        (plugins, dir)
    }

    #[test]
    fn a_picture_goes_into_the_mask_only_while_a_plugin_says_so() {
        let dir = store("paste");
        let plugins = pasting(&dir);
        let hooks = plugins.hooks();
        assert!(!hooks.paste_mask(), "imported, it says nothing");
        assert!(plugins.set_on(ID.to_owned(), true));
        assert!(!hooks.paste_mask(), "on, it goes by its switch");
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
    fn a_paste_plugin_that_stops_leaves_the_picture_to_paste_as_any_is() {
        let Some(s) = session() else { return };
        let layer = s.layers().iter().find(|l| l.current).unwrap().id;
        let mask = s.add_mask(layer).unwrap();
        s.select_layer(mask);
        let dir = store("paste-stops");
        let module = crate::plugin_run::tests::paste_module("(i32.const 65536)", "(unreachable)");
        let plugins = Plugins::with_store(dir.clone());
        let mut manifest = coded(ID, &module);
        manifest["hooks"] = json!(["paste_mask"]);
        assert_eq!(plugins.install(file(&manifest, &module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        s.use_plugins(Arc::clone(&plugins));
        let layers = s.layers().len();
        let picture = crate::layers::ThumbnailImage { width: 200, height: 200, rgba: vec![255; 200 * 200 * 4] };
        let pasted = s.paste(picture).unwrap();
        assert_ne!(pasted, mask, "a new layer, as anything pasted");
        assert_eq!(s.layers().len(), layers + 1);
        assert_eq!(s.take_failure().as_deref(), Some("A plugin stopped. Switched off: it crashed."));
        assert_eq!(card(&plugins).problem.as_deref(), Some("Switched off: it crashed."));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Takes out everything past the first two words of each row, as a plugin of real use might.
    const RIGHT_OF_128: &str = "(local $i i32) (local.set $i (i32.const 16))
        (i64.store (i32.const 65536) (i64.const 0))
        (i64.store (i32.const 65544) (i64.const 0))
        (block $done (loop $next
            (br_if $done (i32.ge_u (local.get $i) (i32.const 32)))
            (i64.store (i32.add (i32.const 65536) (local.get $i)) (i64.const -1))
            (local.set $i (i32.add (local.get $i) (i32.const 8)))
            (br $next)))";

    /// A session with `module` on the fill hook, and the list holding it.
    fn filling(s: &CanvasSession, dir: &str, module: &[u8]) -> Arc<Plugins> {
        let plugins = Plugins::with_store(dir.to_owned());
        let mut manifest = coded(ID, module);
        manifest["hooks"] = json!(["fill_mask"]);
        assert_eq!(plugins.install(file(&manifest, module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        s.use_plugins(Arc::clone(&plugins));
        plugins
    }

    /// A layer's pixel, once the fill being finished is done.
    fn red_at(s: &CanvasSession, x: usize, y: usize) -> bool {
        s.await_pour();
        let rgba = picture(s);
        let at = (y * 200 + x) * 4;
        rgba[at] > 200 && rgba[at + 3] > 200
    }

    fn pour_at(s: &CanvasSession, x: f32, y: f32) {
        assert!(s.pour_begin(x, y, 1.0, 0.0, 0.0, 0.3));
        s.pour_end(true, None);
    }

    #[test]
    fn a_fill_is_finished_by_the_plugin_on_its_own_thread_and_is_one_step() {
        let Some(s) = session() else { return };
        let dir = store("fill-real");
        let plugins = filling(&s, &dir, &fill_module(FILL_AT, "(i32.const 1)", RIGHT_OF_128));
        pour_at(&s, 30.0, 30.0);
        assert!(red_at(&s, 30, 30) && red_at(&s, 120, 190));
        assert!(!red_at(&s, 130, 30) && !red_at(&s, 199, 199), "past where the plugin keeps it");
        assert!(!s.pour_finishing());
        // one undo takes the whole fill back
        s.undo();
        assert!(!red_at(&s, 30, 30));
        // off, the same fill runs on through
        plugins.set_on(ID.to_owned(), false);
        pour_at(&s, 30.0, 30.0);
        assert!(red_at(&s, 30, 30) && red_at(&s, 199, 199));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fill_keeps_to_a_selection_with_a_plugin_as_it_does_without() {
        let Some(s) = session() else { return };
        let dir = store("fill-selection");
        filling(&s, &dir, &fill_module(FILL_AT, "(i32.const 1)", RIGHT_OF_128));
        // what is selected: a band across, on both sides of where the plugin cuts
        let mut mask = vec![0u8; 200 * 200];
        for y in 60..120 {
            mask[y * 200..(y + 1) * 200].fill(255);
        }
        s.shared.lock().selection = Some(procraft_engine::selection::Mask { width: 200, height: 200, data: mask });
        pour_at(&s, 30.0, 80.0);
        assert!(red_at(&s, 30, 80), "in the selection, this side of the cut");
        assert!(!red_at(&s, 30, 30), "out of the selection");
        assert!(!red_at(&s, 150, 80), "in the selection, past the cut");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fill_box_goes_through_alone_with_a_pixel_round_it() {
        // a square of paper in a filled layer: the fill's box is a corner of the canvas. The plugin
        // says how much it was handed in the fill's own terms
        let Some(s) = session() else { return };
        let dir = store("fill-box");
        s.set_color(0.0, 0.0, 1.0);
        let layer = s.layers().iter().find(|l| l.current).unwrap().id;
        assert!(s.fill_layer(layer));
        {
            let mut st = s.shared.lock();
            let id = st.doc.current();
            let mut pixels = st.layer_pixels(id).unwrap();
            for y in 10..50 {
                for x in 20..70 {
                    pixels[(y * 200 + x) * 4..(y * 200 + x + 1) * 4].fill(0);
                }
            }
            st.canvas.as_mut().unwrap().write_layer(id, pixels);
        }
        // the row it is told of: 52 wide, 42 tall (50 x 40 and a pixel each way), the seed in terms
        // of that: anything else and it says nothing comes out
        let module = fill_module(
            "(if (i32.or (i32.ne (local.get 0) (i32.const 52)) (i32.or (i32.ne (local.get 1) (i32.const 42))
                (i32.or (i32.ne (local.get 2) (i32.const 11)) (i32.ne (local.get 3) (i32.const 11)))))
                (then (unreachable)))
             (i32.const 65536)",
            "(i32.const 1)",
            "(i64.store (i32.const 65536) (i64.const 0x00ffff0000000000))",
        );
        let plugins = filling(&s, &dir, &module);
        pour_at(&s, 30.0, 20.0);
        assert!(red_at(&s, 30, 20));
        assert!(!red_at(&s, 19 + 45, 20), "taken out, at its place in the box");
        assert_eq!(card(&plugins).problem, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What a plugin that goes wrong on a fill must come to: the fill as it was, one step, and why.
    fn fill_goes_wrong(name: &str, module: &[u8], reason: &str) {
        let Some(s) = session() else { return };
        let dir = store(name);
        let plugins = filling(&s, &dir, module);
        pour_at(&s, 30.0, 30.0);
        assert!(red_at(&s, 30, 30) && red_at(&s, 199, 199), "the fill stays unclosed");
        assert_eq!(s.take_failure().as_deref(), Some(format!("A plugin stopped. {reason}").as_str()));
        let stopped = card(&plugins);
        assert!(!stopped.on);
        assert_eq!(stopped.problem.as_deref(), Some(reason));
        s.undo();
        assert!(!red_at(&s, 30, 30), "and it is one step");
        // the next fill takes no time over it
        pour_at(&s, 30.0, 30.0);
        assert!(!s.pour_finishing());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fill_plugin_that_never_ends_leaves_the_fill_unclosed() {
        let module = fill_module(FILL_AT, "(loop (br 0)) (i32.const 1)", "");
        fill_goes_wrong("fill-loop", &module, "Switched off: it took too long.");
    }

    #[test]
    fn a_fill_plugin_that_crashes_leaves_the_fill_unclosed() {
        let module = fill_module(FILL_AT, "(unreachable)", "");
        fill_goes_wrong("fill-trap", &module, "Switched off: it crashed.");
    }

    #[test]
    fn a_fill_plugin_that_takes_the_seed_leaves_the_fill_unclosed() {
        let module = fill_module(FILL_AT, "(i32.const 1)", "(i64.store (i32.const 65536) (i64.const -1))");
        fill_goes_wrong("fill-seed", &module, "Switched off: it gave an answer that makes no sense.");
    }

    #[test]
    fn a_fill_plugin_that_asks_too_much_leaves_the_fill_unclosed() {
        let module = fill_module(FILL_AT, "(drop (memory.grow (i32.const 2000))) (i32.const 1)", "");
        fill_goes_wrong("fill-memory", &module, "Switched off: it asked for too much memory.");
    }

    #[test]
    fn a_fill_too_big_for_a_plugin_stays_unclosed_and_says_so() {
        let hooks = Hooks::default();
        let module = fill_module(FILL_AT, "(i32.const 1)", TAKES_MOST);
        hooks.put_on("fill".to_owned(), Running::start(&module, &[]).unwrap(), &[Hook::FillMask]);
        let mask = crate::pour::tests::mask_of_box(9000, 9000);
        assert!(crate::pour::closed_by(&hooks, mask, (5, 5)).is_none());
        let said = hooks.take_said().unwrap();
        assert!(said.contains("too big"), "{said}");
        assert!(hooks.is_on("fill"), "it isn't the plugin's doing");
    }

    #[test]
    fn a_long_call_holds_up_no_other_hook() {
        // a plugin on the fill hook at work, and another on the position hook goes on answering
        let hooks = Arc::new(Hooks::default());
        let body = "(local $i i32) (local.set $i (i32.const 30000000))
            (loop (local.set $i (i32.sub (local.get $i) (i32.const 1))) (br_if 0 (local.get $i)))
            (i32.const 0)";
        let slow = fill_module(FILL_AT, body, "");
        hooks.put_on("fill".to_owned(), Running::start(&slow, &[]).unwrap(), &[Hook::FillMask]);
        let mut moves = Running::start(&offset(), &[]).unwrap();
        moves.control(0, 1.0).unwrap();
        hooks.put_on("offset".to_owned(), moves, &[Hook::Position]);
        let working = Arc::clone(&hooks);
        let long = std::thread::spawn(move || {
            let mut rows = vec![!0_u64; 2048 * 32];
            working.fill_rows(&mut rows, 2048, 2048, (0, 0))
        });
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(hooks.position(1.0, 1.0), (2.0, 1.0));
        assert!(!long.is_finished(), "answered while the other was at work");
        assert!(!long.join().unwrap());
    }

    /// What whole-canvas fills cost going through a real plugin file, up to the most that can: with
    /// `PROCRAFT_PLUGIN_FILE` naming the gap-closing plugin, `--ignored --nocapture`, on a device.
    #[test]
    #[ignore]
    fn a_built_gap_plugin_costs() {
        let bytes = std::fs::read(std::env::var("PROCRAFT_PLUGIN_FILE").unwrap()).unwrap();
        let dir = store("built-gap-cost");
        let plugins = Plugins::with_store(dir.clone());
        assert_eq!(plugins.install(bytes), None);
        let found = card(&plugins);
        assert!(plugins.set_on(found.id.clone(), true));
        plugins.set_slider("fill_gap".to_owned(), 8.0);
        let hooks = plugins.hooks();
        assert!(hooks.fill_mask());
        for side in [512_usize, 2048, 4096, 8192] {
            // two rooms, a wall down the middle with an opening 4 tall
            let mut data = vec![255_u8; side * side];
            for y in (0..side).filter(|y| !(side / 2..side / 2 + 4).contains(y)) {
                data[y * side + side / 2..y * side + side / 2 + 6].fill(0);
            }
            let filled = data.iter().filter(|&&v| v > 0).count();
            let mask = procraft_engine::selection::Mask { width: side as u32, height: side as u32, data };
            let start = Instant::now();
            let closed = crate::pour::closed_by(&hooks, mask, (side as u32 / 4, side as u32 / 4));
            let took = start.elapsed().as_secs_f64() * 1e3;
            let kept = closed.map(|m| m.data.iter().filter(|&&v| v > 0).count());
            let memory = hooks.long_calls(Hook::FillMask).first().map_or(0, |(_, r)| at_work(r).memory_used());
            println!(
                "fill {side}: {took:.0} ms, {:.1} MiB so far, kept {kept:?} of {filled}",
                memory as f64 / (1 << 20) as f64
            );
        }
        println!("{:?}", hooks.take_said());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A gap closed by a real plugin file: with `PROCRAFT_PLUGIN_FILE` naming the gap-closing plugin and
    /// `--ignored`. Two rooms, the same fill through a gap, with the plugin on and off.
    #[test]
    #[ignore]
    fn a_built_gap_plugin_closes_a_real_fill() {
        let bytes = std::fs::read(std::env::var("PROCRAFT_PLUGIN_FILE").unwrap()).unwrap();
        let Some(s) = session() else { return };
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.1);
        for (y0, y1) in [(-10.0, 90.0), (110.0, 210.0)] {
            s.stroke_begin(PointerKind::Finger, vec![sample(100.0, y0, 0.0)]);
            s.stroke_add(
                (1..20).map(|i| sample(100.0, y0 + (y1 - y0) * i as f32 / 20.0, f64::from(i) * 8.0)).collect(),
            );
            s.stroke_end(vec![]);
        }
        let dir = store("built-gap");
        let plugins = Plugins::with_store(dir.clone());
        assert_eq!(plugins.install(bytes), None);
        let found = card(&plugins);
        assert!(plugins.set_on(found.id.clone(), true));
        assert_eq!(plugins.set_slider("fill_gap".to_owned(), 32.0), Some(32.0));
        s.use_plugins(Arc::clone(&plugins));
        assert!(s.pour_begin(40.0, 100.0, 1.0, 0.0, 0.0, 0.3));
        s.pour_end(true, None);
        assert!(red_at(&s, 40, 100) && red_at(&s, 40, 30));
        assert!(!red_at(&s, 160, 100) && !red_at(&s, 160, 30), "kept to its side");
        s.undo();
        s.await_pour();
        assert!(plugins.set_on(found.id.clone(), false));
        assert!(s.pour_begin(40.0, 100.0, 1.0, 0.0, 0.0, 0.3));
        s.pour_end(true, None);
        assert!(red_at(&s, 40, 100) && red_at(&s, 160, 100), "off, it runs through the gap");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plugin_goes_on_as_applied_and_off_as_unapplied_with_no_restart() {
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
        assert!(keys.contains(&format!("{ID}.x")) && keys.contains(&format!("{ID}.on")), "{keys:?}");
        assert_eq!(hooks.position(1.0, 1.0), (1.0, 1.0));
        assert!(plugins.plugins().is_empty());
        assert!(Plugins::with_store(dir.clone()).plugins().is_empty());
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
        assert_eq!(hooks.position(1.0, 1.0), (8.0, 1.0), "on still, with what was set");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What a plugin that goes wrong on a sample must come to, whatever went wrong.
    fn stops(name: &str, before: &str, body: &str, reason: &str) {
        let dir = store(name);
        let plugins = turned_on(&dir, before, body);
        let hooks = plugins.hooks();
        assert_eq!(hooks.position(3.0, 4.0), (3.0, 4.0), "the sample goes through unchanged");
        let stopped = card(&plugins);
        assert!(!stopped.on);
        assert_eq!(stopped.problem.as_deref(), Some(reason));
        assert_eq!(hooks.position(5.0, 6.0), (5.0, 6.0), "and it is off the hook");
        assert_eq!(hooks.hooked.load(Ordering::Relaxed), 0);
        // it stays off through a restart, even with its flag kept as on, and says why
        let next = Plugins::with_store(dir.clone());
        next.read_kept(format!("{ID}.on"), "true".to_owned());
        assert_eq!(next.hooks().position(3.0, 4.0), (3.0, 4.0));
        let kept = card(&next);
        assert!(!kept.on);
        assert_eq!(kept.problem.as_deref(), Some(reason));
        // off, it says nothing more; asked again, it gets its chance
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
    fn a_plugin_that_loops_for_ever_is_stopped_and_switched_off() {
        stops("loop", "", "(loop (br 0)) (i64.const 0)", "Switched off: it took too long.");
    }

    #[test]
    fn a_plugin_that_traps_is_switched_off() {
        stops("trap", "", "(unreachable)", "Switched off: it crashed.");
    }

    #[test]
    fn a_plugin_that_returns_garbage_is_switched_off() {
        // two NaNs
        let nan = "(i64.const 0x7fc000007fc00000)";
        stops("garbage", "", nan, "Switched off: it gave an answer that makes no sense.");
    }

    #[test]
    fn a_plugin_over_the_memory_cap_is_switched_off() {
        let grows = "(drop (memory.grow (i32.const 1000))) (i64.const 0)";
        stops("memory", "(memory 1)", grows, "Switched off: it asked for too much memory.");
    }

    #[test]
    fn a_plugin_stopped_on_a_stroke_is_told_once_by_the_session() {
        let dir = store("told");
        let plugins = turned_on(&dir, "", "(unreachable)");
        let s = CanvasSession::new(200, 200);
        s.use_plugins(Arc::clone(&plugins));
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
        assert_eq!(s.take_failure().as_deref(), Some("A plugin stopped. Switched off: it crashed."));
        assert_eq!(s.take_failure(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plugin_that_can_t_start_stays_off_and_says_why() {
        let dir = store("start");
        let module = position_module("(memory 1000)", "(i64.const 0)");
        let plugins = Plugins::with_store(dir.clone());
        assert_eq!(plugins.install(file(&coded(ID, &module), &module)), None);
        assert!(plugins.set_on(ID.to_owned(), true));
        let stopped = card(&plugins);
        assert!(!stopped.on);
        assert_eq!(stopped.problem.as_deref(), Some("Switched off: it asked for too much memory."));
        assert_eq!(plugins.hooks().hooked.load(Ordering::Relaxed), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stopped_plugin_leaves_the_others_on() {
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
    fn plugins_go_in_id_order_however_they_were_applied() {
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
    fn a_stroke_lands_where_the_plugin_says_and_as_it_did_once_it_is_gone() {
        let Some(plain) = session() else { return };
        draw(&plain);
        let drawn = picture(&plain);
        assert!(!rows_drawn(&drawn).is_empty());

        // with the list on the session and nothing on, nothing differs at all
        let dir = store("stroke");
        let plugins = with_offset(&dir);
        let Some(s) = session() else { return };
        s.use_plugins(Arc::clone(&plugins));
        draw(&s);
        assert!(picture(&s) == drawn);

        let Some(s) = session() else { return };
        s.use_plugins(Arc::clone(&plugins));
        plugins.set_on(ID.to_owned(), true);
        plugins.set_slider(format!("{ID}.y"), 40.0);
        draw(&s);
        let moved = rows_drawn(&picture(&s));
        let want: Vec<usize> = rows_drawn(&drawn).iter().map(|y| y + 40).collect();
        assert_eq!(moved, want, "the same stroke, 40 pixels down");

        // off it is as if it had never been
        let Some(s) = session() else { return };
        s.use_plugins(Arc::clone(&plugins));
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

    /// What the hook costs a sample, and what a plugin that spends all its fuel costs: run on a
    /// device with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn hook_cost() {
        let none = Hooks::default();
        println!("position hook, none on: {:.2} ns a sample", (0..5).map(|_| cost(&none)).fold(f64::MAX, f64::min));

        let with_one = Hooks::default();
        let mut running = Running::start(&offset(), &[]).unwrap();
        running.control(0, 3.0).unwrap();
        with_one.put_on("offset".to_owned(), running, &[Hook::Position]);
        println!("the offset plugin on: {:.1} ns a sample", (0..5).map(|_| cost(&with_one)).fold(f64::MAX, f64::min));

        // the worst a plugin can do to a sample before it is stopped, over and over
        let mut slowest = f64::MAX;
        for _ in 0..50 {
            let mut running = Running::start(&position_module("", "(loop (br 0)) (i64.const 0)"), &[]).unwrap();
            let start = Instant::now();
            assert_eq!(running.position(1.0, 2.0), Err(Stop::TooSlow));
            slowest = slowest.min(start.elapsed().as_secs_f64() * 1000.0);
        }
        println!("a plugin spending all its fuel on a sample: {slowest:.3} ms");

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
        println!("the pick hook, a plugin on it: {:.1} ns a pick", start.elapsed().as_secs_f64() * 1000.0);

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
        println!("the scale hook, a plugin on it: {:.1} ns a frame", start.elapsed().as_secs_f64() * 1000.0);

        let start = Instant::now();
        let mut running = Running::start(&offset(), &[]).unwrap();
        println!("starting a plugin: {:.3} ms", start.elapsed().as_secs_f64() * 1000.0);
        running.control(0, 1.0).unwrap();
    }

    /// What a 4096 square costs going through a paste plugin. With `PROCRAFT_PLUGIN_FILE` naming one
    /// it goes through that, otherwise through [`PASTES`]: run on a device with
    /// `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_paste_of_a_4096_square_costs() {
        let dir = store("paste-cost");
        let plugins = match std::env::var("PROCRAFT_PLUGIN_FILE") {
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
                "4096 square through the plugin: {:.0} ms, {:.0} ns a pixel",
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
    fn a_stroke_costs_the_same_with_nothing_on() {
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
            println!("to_canvas a sample: {bare:.2} ns without the list, {hooked:.2} ns with it, none on");
        }
    }

    /// A real plugin file, built by `tools/pack_plugin.py`, end to end on the hook. Run with
    /// `--ignored` and `PROCRAFT_PLUGIN_FILE` naming it.
    #[test]
    #[ignore]
    fn a_built_plugin_file_works() {
        let bytes = std::fs::read(std::env::var("PROCRAFT_PLUGIN_FILE").unwrap()).unwrap();
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
        assert!(next.plugins().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
