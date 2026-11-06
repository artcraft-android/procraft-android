//! What the app does beyond its measured look and behaviour, in two lists on two pages. A patch is
//! a file with code in it, run in a sandbox on the [hooks](crate::hooks) it asks for: none is in
//! the app, so with none applied the app is as measured. A plugin is a file that adds something
//! new; the Plugins page also has the extras still bundled, each off (or at 100%) until asked for.
//! See [`crate::plugin_file`] for what is asked of a file.
//!
//! All are plain data, a name, a line on what it does and its controls, so a page draws any of them
//! without knowing it, and a file describes itself the same way. What a bundled one does is the
//! app's: it asks for the values it knows ([`Plugins::fill_gap`]).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::hooks::Hooks;
use crate::patch_run::{Running, Stop};
use crate::plugin_file::{self, Manifest, Refusal};

/// What the bundled extras' values are kept under, as they were before there was a list.
const FILL_GAP: &str = "fill_gap";

/// A setting the app used to have and a patch now brings: what it was kept under, as it started
/// (a switch off is 0), and its name. Its value stays there for the patch, whose control says so.
struct Moved {
    key: &'static str,
    starts: f32,
    name: &'static str,
}

const MOVED: &[Moved] = &[
    Moved { key: "eyedropper_layer", starts: 0.0, name: "Eyedropper picks from layer" },
    Moved { key: "interface_size", starts: 1.0, name: "Interface size" },
    Moved { key: "paste_into_mask", starts: 0.0, name: "Paste into masks" },
];

impl Moved {
    /// Whether a value kept as `value` is one someone changed.
    fn changed(&self, value: &str) -> bool {
        let number = match value {
            "true" => Some(1.0),
            "false" => Some(0.0),
            _ => value.parse::<f32>().ok(),
        };
        number.is_some_and(|number| number != self.starts)
    }
}

/// Whether `key` is a setting the app used to have.
pub(crate) fn moved(key: &str) -> bool {
    MOVED.iter().any(|moved| moved.key == key)
}

/// Why a filter the app ended on, or that stopped the GPU, is off.
pub(crate) const STOPPED_DRIVER: &str = "Switched off: it stopped the graphics driver.";
/// Why a filter whose files no longer hold up can't open.
const FILTER_DAMAGED: &str = "This filter's files don't hold up.";

/// Where one came from. The route decides this, never the plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PluginSource {
    Bundled,
    File,
    Catalogue,
}

/// What one is, and so which page it is on and what its card offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PluginKind {
    /// In the app, its controls always there.
    Extra,
    Filter,
    /// Applied or not, and running on its hooks while it is.
    Patch,
}

/// A setting on or off. `key` is what its value is kept under: one key to a value, across all of
/// them (a plugin's start with its id).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginSwitch {
    pub key: String,
    pub label: String,
    pub on: bool,
    /// As it starts.
    pub starts: bool,
}

/// A setting along a range, in steps from its `least`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginSlider {
    pub key: String,
    pub label: String,
    pub least: f32,
    pub most: f32,
    pub step: f32,
    pub value: f32,
    /// As it starts.
    pub starts: f32,
    /// How it reads: the value times this, then `unit` (0.85 as 85%).
    pub shown_times: f32,
    pub unit: String,
    /// What it reads at its least, if not that ("Off").
    pub at_least: Option<String>,
    /// Its value goes in as it moves; otherwise as it lets go, which what changes the page needs.
    pub live: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PluginControl {
    Switch { control: PluginSwitch },
    Slider { control: PluginSlider },
}

/// One as its page shows it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: PluginSource,
    pub kind: PluginKind,
    /// As its file says; empty for what is bundled.
    pub version: String,
    pub publisher: String,
    /// Tagged as experimental on its page.
    pub experimental: bool,
    /// The real on-off: a patch applied, a filter enabled; always on for what is bundled.
    pub on: bool,
    /// What `on` is kept under; empty for what is bundled, whose controls are all it has.
    pub on_key: String,
    /// Why it does nothing, to show on it.
    pub problem: Option<String>,
    pub controls: Vec<PluginControl>,
}

/// What a plugin file is, for the one screen that asks whether to import it. `refused`: it can't
/// be, and why; nothing else is set.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginOffer {
    pub name: String,
    /// Its version, publisher marked as not checked, and a sentence on what it can do.
    pub message: String,
    pub refused: Option<String>,
}

impl PluginControl {
    pub(crate) fn key(&self) -> &str {
        match self {
            PluginControl::Switch { control } => &control.key,
            PluginControl::Slider { control } => &control.key,
        }
    }

    /// Takes the value `text`, kept as [`Plugins::read_kept`] reads it, if it is its kind of value.
    fn read(&mut self, text: &str) {
        match self {
            PluginControl::Switch { control } => {
                if let Ok(on) = text.parse() {
                    control.on = on;
                }
            }
            PluginControl::Slider { control } => {
                if let Some(read) = text.parse::<f32>().ok().filter(|v| v.is_finite()) {
                    control.value = control.stepped(read);
                }
            }
        }
    }

    /// Back as it starts.
    pub(crate) fn back_to_start(&mut self) {
        match self {
            PluginControl::Switch { control } => control.on = control.starts,
            PluginControl::Slider { control } => control.value = control.starts,
        }
    }

    /// Its value as a patch or a filter is told it.
    pub(crate) fn number(&self) -> f32 {
        match self {
            PluginControl::Switch { control } => f32::from(u8::from(control.on)),
            PluginControl::Slider { control } => control.value,
        }
    }

    /// Its value as kept, read back with [`Plugins::read_kept`].
    fn kept(&self) -> String {
        match self {
            PluginControl::Switch { control } => control.on.to_string(),
            PluginControl::Slider { control } => control.value.to_string(),
        }
    }

    /// `other`'s value taken over, if it is the same kind of control.
    fn take_value(&mut self, other: &PluginControl) {
        match (self, other) {
            (PluginControl::Switch { control }, PluginControl::Switch { control: from }) => control.on = from.on,
            (PluginControl::Slider { control }, PluginControl::Slider { control: from }) => {
                control.value = control.stepped(from.value);
            }
            _ => {}
        }
    }
}

impl PluginSlider {
    /// `value` kept to the range, on a step. In doubles, so a step lands on the number it reads as
    /// (1 exactly, at 100%). A plugin's range comes as it comes: a bad one mustn't panic.
    pub(crate) fn stepped(&self, value: f32) -> f32 {
        let (least, most, step) = (f64::from(self.least), f64::from(self.most), f64::from(self.step));
        let kept = f64::from(value).max(least).min(most);
        if step > 0.0 { (least + ((kept - least) / step).round() * step).min(most) as f32 } else { kept as f32 }
    }
}

/// Where `slider` lands at `along` (0..1) of its way.
#[uniffi::export]
pub fn plugin_slider_at(slider: PluginSlider, along: f32) -> f32 {
    slider.stepped(slider.least + along.clamp(0.0, 1.0) * (slider.most - slider.least))
}

/// How far along its way (0..1) `value` is on `slider`.
#[uniffi::export]
pub fn plugin_slider_along(slider: PluginSlider, value: f32) -> f32 {
    let range = slider.most - slider.least;
    if range > 0.0 { ((value - slider.least) / range).clamp(0.0, 1.0) } else { 0.0 }
}

/// `value` on `slider` as it reads, to as many places as its step needs.
#[uniffi::export]
pub fn plugin_slider_shown(slider: PluginSlider, value: f32) -> String {
    if let Some(text) = slider.at_least.as_ref().filter(|_| value <= slider.least) {
        return text.clone();
    }
    let step = slider.step * slider.shown_times;
    let whole = |v: f32| (v - v.round()).abs() < 1e-3;
    let places = if whole(step) {
        0
    } else if whole(step * 10.0) {
        1
    } else {
        2
    };
    format!("{:.*}{}", places, value * slider.shown_times, slider.unit)
}

/// Whether `slider` holds whole numbers: kept as one where what it is kept as matters.
#[uniffi::export]
pub fn plugin_slider_whole(slider: PluginSlider) -> bool {
    slider.step > 0.0 && slider.step.fract() == 0.0 && slider.least.fract() == 0.0
}

/// The most of a plugin file worth reading, in bytes: one more is enough to refuse it.
#[uniffi::export]
pub fn plugin_file_most_bytes() -> u64 {
    plugin_file::MOST_FILE as u64
}

/// A bundled extra.
fn extra(id: &str, name: &str, description: &str, control: PluginControl) -> PluginInfo {
    PluginInfo {
        id: id.to_owned(),
        name: name.to_owned(),
        description: description.to_owned(),
        source: PluginSource::Bundled,
        kind: PluginKind::Extra,
        version: String::new(),
        publisher: String::new(),
        experimental: true,
        on: true,
        on_key: String::new(),
        problem: None,
        controls: vec![control],
    }
}

/// The bundled extras, each as it starts.
fn extras() -> Vec<PluginInfo> {
    let gap = PluginControl::Slider {
        control: PluginSlider {
            key: FILL_GAP.to_owned(),
            label: "Gap".to_owned(),
            least: 0.0,
            most: procraft_engine::fill::MOST_GAP as f32,
            step: 1.0,
            value: 0.0,
            starts: 0.0,
            shown_times: 1.0,
            unit: " px".to_owned(),
            at_least: Some("Off".to_owned()),
            live: false,
        },
    };
    vec![extra(
        FILL_GAP,
        "ColorDrop gap closing",
        "Keeps ColorDrop from running out through gaps in your lines, up to this wide.",
        gap,
    )]
}

/// `manifest` as a list entry: a patch unapplied, a filter on.
fn from_file(manifest: Manifest) -> PluginInfo {
    let patch = manifest.kind == PluginKind::Patch;
    PluginInfo {
        on_key: format!("{}.{}", manifest.id, if patch { "applied" } else { "enabled" }),
        id: manifest.id,
        name: manifest.name,
        description: manifest.description,
        source: PluginSource::File,
        kind: manifest.kind,
        version: manifest.version,
        publisher: manifest.publisher,
        experimental: manifest.experimental,
        on: !patch,
        problem: None,
        controls: manifest.controls,
    }
}

/// What the offer says `manifest` can do.
fn can_do(manifest: &Manifest) -> String {
    if manifest.kind != PluginKind::Patch {
        return "This adds a filter to Adjustments. It can change only the layer you use it on, and can't reach your files or the internet.".to_owned();
    }
    let sentences: Vec<&str> = manifest.hooks.iter().map(|hook| hook.sentence()).collect();
    format!("{} It can't reach your files, your layers or the internet.", sentences.join(" "))
}

/// A filter whose files have held up again, for opening.
pub(crate) struct FilterToOpen {
    pub name: String,
    pub controls: Vec<PluginControl>,
    pub code: procraft_gpu::FilterCode,
}

#[derive(Debug)]
struct Listed {
    bundled: Vec<PluginInfo>,
    files: Vec<PluginInfo>,
    /// Moved settings read back with no patch yet to take them: key and value, kept as they are.
    moved: Vec<(String, String)>,
    /// The notice about them has been asked for: a start has one chance to say it.
    notice_asked: bool,
}

impl Listed {
    fn every_mut(&mut self) -> impl Iterator<Item = &mut PluginInfo> {
        self.bundled.iter_mut().chain(self.files.iter_mut())
    }

    /// The one holding the control kept under `key`, and the control's place in it.
    fn control_mut(&mut self, key: &str) -> Option<(&mut PluginInfo, usize)> {
        self.every_mut().find_map(|plugin| {
            let at = plugin.controls.iter().position(|c| c.key() == key)?;
            Some((plugin, at))
        })
    }
}

/// The lists, with each control's value as it stands. They live here and in no static, so a
/// plugin goes the moment it is removed. The app keeps the values with its other interface
/// settings, each under its control's key; the files live in `store`.
#[derive(Debug, uniffi::Object)]
pub struct Plugins {
    list: Mutex<Listed>,
    /// The folder imported plugins are kept in, `<id>/<version>/` each. None: none can be.
    store: Option<PathBuf>,
    /// The applied patches, running.
    hooks: Arc<Hooks>,
}

impl Plugins {
    /// The lists, with whatever stopped on a hook since they were last read shown as stopped.
    fn listed(&self) -> MutexGuard<'_, Listed> {
        let mut list = self.list.lock().unwrap_or_else(|e| e.into_inner());
        for (id, stop) in self.hooks.take_stopped() {
            if let Some(patch) = list.files.iter_mut().find(|p| p.id == id) {
                patch.on = false;
                patch.problem = Some(stop.text().to_owned());
            }
        }
        list
    }

    fn kept_in(store: Option<PathBuf>) -> Arc<Plugins> {
        let files = store.as_deref().map(plugin_file::read_all).unwrap_or_default();
        let mut files: Vec<PluginInfo> = files.into_iter().map(from_file).collect();
        if let Some(store) = &store {
            plugin_file::settle_trying(store, STOPPED_DRIVER);
            for plugin in &mut files {
                plugin.problem = plugin_file::stopped_why(store, &plugin.id);
                plugin.on &= plugin.kind == PluginKind::Patch || plugin.problem.is_none();
            }
        }
        let list = Listed { bundled: extras(), files, moved: Vec::new(), notice_asked: false };
        Arc::new(Plugins { list: Mutex::new(list), hooks: Arc::new(Hooks::kept_in(store.clone())), store })
    }

    /// Puts `patch` on its hooks, if it is applied and not there yet. One that can't start goes
    /// off, and says why.
    fn start(&self, patch: &mut PluginInfo) {
        if patch.kind != PluginKind::Patch || !patch.on || self.hooks.is_on(&patch.id) {
            return;
        }
        let values: Vec<f32> = patch.controls.iter().map(PluginControl::number).collect();
        let store = self.store.as_deref();
        let code = store.and_then(|store| plugin_file::read_code(store, &patch.id, &patch.version));
        let started =
            code.ok_or(Stop::Damaged).and_then(|(hooks, bytes)| Ok((hooks, Running::start(&bytes, &values)?)));
        match started {
            Ok((hooks, running)) => self.hooks.put_on(patch.id.clone(), running, &hooks),
            Err(stop) => {
                self.hooks.failed(&patch.id, stop);
                patch.on = false;
                patch.problem = Some(stop.text().to_owned());
            }
        }
    }

    /// The hooks, every applied patch on them: a list read back from where it was kept starts its
    /// patches here, once all its values are in and something will go through them.
    pub(crate) fn hooks(&self) -> Arc<Hooks> {
        let mut list = self.listed();
        for patch in &mut list.files {
            self.start(patch);
        }
        Arc::clone(&self.hooks)
    }

    /// What `change` makes of the control kept under `key`, if there is one. A running patch is
    /// told the new value.
    fn change<T>(&self, key: &str, change: impl FnOnce(&mut PluginControl) -> Option<T>) -> Option<T> {
        let mut list = self.listed();
        let (plugin, at) = list.control_mut(key)?;
        let changed = plugin.controls.get_mut(at).and_then(change)?;
        if let Some(control) = plugin.controls.get(at) {
            self.hooks.control(&plugin.id, at, control.number());
        }
        Some(changed)
    }

    fn control(&self, key: &str) -> Option<PluginControl> {
        let mut list = self.listed();
        let (plugin, at) = list.control_mut(key)?;
        plugin.controls.get(at).cloned()
    }

    /// Each value as a line of a settings file, its key and then what [`Plugins::read_kept`] reads.
    pub fn kept_lines(&self) -> Vec<String> {
        let list = self.listed();
        let every = list.bundled.iter().chain(&list.files);
        let values = every.flat_map(|p| &p.controls).map(|c| format!("{} {}", c.key(), c.kept()));
        let flags = list.files.iter().map(|p| format!("{} {}", p.on_key, p.on));
        values.chain(flags).chain(list.moved.iter().map(|(key, value)| format!("{key} {value}"))).collect()
    }

    /// Takes `bytes` in, as `install` says.
    fn take_in(&self, bytes: &[u8]) -> Result<(), Refusal> {
        let package = plugin_file::read_package(bytes)?;
        self.can_take(&package.manifest)?;
        let store = self.store.as_ref().ok_or(Refusal::NotKept)?;
        let folder = plugin_file::keep(store, &package)?;
        // as the next start will find it, or not at all
        let (id, version) = (&package.manifest.id, &package.manifest.version);
        let manifest = plugin_file::read_folder(&folder, id, version).ok_or(Refusal::NotKept)?;
        let mut fresh = from_file(manifest);
        let mut list = self.listed();
        // a moved setting read back goes to the patch that takes it over
        for control in &mut fresh.controls {
            if let Some(at) = list.moved.iter().position(|(key, _)| key == control.key()) {
                let (_, value) = list.moved.remove(at);
                control.read(&value);
            }
        }
        match list.files.iter_mut().find(|p| p.id == fresh.id) {
            Some(old) => {
                // an update keeps what was set, for what it still has
                for control in &mut fresh.controls {
                    if let Some(was) = old.controls.iter().find(|c| c.key() == control.key()) {
                        control.take_value(was);
                    }
                }
                fresh.on = old.on;
                // what stopped gets its chance with the new file
                self.hooks.take_off(&fresh.id);
                plugin_file::clear_stopped(store, &fresh.id);
                self.start(&mut fresh);
                *old = fresh;
            }
            None => list.files.push(fresh),
        }
        Ok(())
    }

    /// The filter `id` as it can be opened; what to tell if it can't.
    pub(crate) fn filter_to_open(&self, id: &str) -> Result<FilterToOpen, String> {
        let list = self.listed();
        let plugin = list.files.iter().find(|p| p.id == id && p.kind == PluginKind::Filter);
        let plugin = plugin.ok_or_else(|| FILTER_DAMAGED.to_owned())?;
        if let Some(problem) = &plugin.problem {
            return Err(problem.clone());
        }
        let store = self.store.as_deref().ok_or_else(|| FILTER_DAMAGED.to_owned())?;
        let read = plugin_file::read_filter_code(store, id, &plugin.version);
        let (manifest, text) = read.ok_or_else(|| FILTER_DAMAGED.to_owned())?;
        let code = procraft_gpu::read_filter(&text, &plugin_file::value_names(&manifest.controls));
        let code = code.map_err(|why| {
            log::warn!("a kept filter's code was refused: {why}");
            FILTER_DAMAGED.to_owned()
        })?;
        Ok(FilterToOpen { name: manifest.name, controls: manifest.controls, code })
    }

    /// Filter `id` is on trial: if the app ends before it has drawn, it is put down to it.
    pub(crate) fn filter_trying(&self, id: &str) {
        if let Some(store) = &self.store {
            plugin_file::note_trying(store, id);
        }
    }

    /// No filter is on trial any more.
    pub(crate) fn filter_tried(&self) {
        if let Some(store) = &self.store {
            plugin_file::clear_trying(store);
        }
    }

    /// Switches filter `id` off, `why` on its card and through a restart, until someone turns it on.
    pub(crate) fn stop_filter(&self, id: &str, why: &str) {
        if let Some(store) = &self.store {
            plugin_file::note_stopped(store, id, why);
        }
        if let Some(filter) = self.listed().files.iter_mut().find(|p| p.id == id) {
            filter.on = false;
            filter.problem = Some(why.to_owned());
        }
    }

    /// Whether what is here lets a plugin of `manifest` in: an older file doesn't replace a newer,
    /// and a moved setting has one holder.
    fn can_take(&self, manifest: &Manifest) -> Result<(), Refusal> {
        let list = self.listed();
        let others = || list.files.iter().filter(|p| p.id != manifest.id).flat_map(|p| &p.controls);
        if manifest.controls.iter().any(|control| others().any(|other| other.key() == control.key())) {
            return Err(Refusal::Clash);
        }
        let kept = list.files.iter().find(|p| p.id == manifest.id);
        if kept.is_some_and(|k| plugin_file::version_is_older(&manifest.version, &k.version)) {
            return Err(Refusal::NewerInstalled);
        }
        Ok(())
    }
}

#[uniffi::export]
impl Plugins {
    /// The bundled extras, with nowhere to keep a file.
    #[uniffi::constructor]
    pub fn new() -> Arc<Plugins> {
        Plugins::kept_in(None)
    }

    /// The bundled extras, and every file kept in the folder `dir` that holds up.
    #[uniffi::constructor]
    pub fn with_store(dir: String) -> Arc<Plugins> {
        Plugins::kept_in(Some(PathBuf::from(dir)))
    }

    /// What the Patches page shows: the patches imported.
    pub fn patches(&self) -> Vec<PluginInfo> {
        self.listed().files.iter().filter(|p| p.kind == PluginKind::Patch).cloned().collect()
    }

    /// What the Plugins page shows: the bundled extras and the filters imported.
    pub fn plugins(&self) -> Vec<PluginInfo> {
        let list = self.listed();
        let filters = list.files.iter().filter(|p| p.kind != PluginKind::Patch);
        list.bundled.iter().chain(filters).cloned().collect()
    }

    /// The filters that can be opened now: the Adjustments menu's, which has none to show with none
    /// imported.
    pub fn filters(&self) -> Vec<PluginInfo> {
        let list = self.listed();
        let ready = |p: &&PluginInfo| p.kind == PluginKind::Filter && p.on && p.problem.is_none();
        list.files.iter().filter(ready).cloned().collect()
    }

    /// A value read back from where it was kept, as text. A key nothing has, or anything that
    /// isn't its kind of value, leaves things as they are. A patch that stopped stays off, whatever
    /// its flag says, until someone asks it to run.
    pub fn read_kept(&self, key: String, value: String) {
        let mut list = self.listed();
        if let Some(plugin) = list.files.iter_mut().find(|p| p.on_key == key) {
            if let Ok(on) = value.parse::<bool>() {
                plugin.on = on && plugin.problem.is_none();
            }
            return;
        }
        if let Some((plugin, at)) = list.control_mut(&key) {
            if let Some(control) = plugin.controls.get_mut(at) {
                control.read(&value);
            }
        } else if moved(&key) {
            list.moved.retain(|(kept, _)| *kept != key);
            list.moved.push((key, value));
        }
    }

    /// Words for someone who changed a setting that now comes as a patch, once: it is written down
    /// that they were told. Their value stays where it was kept, for the patch to pick up.
    pub fn take_moved_notice(&self) -> Option<String> {
        let store = self.store.as_deref()?;
        let mut list = self.listed();
        if std::mem::replace(&mut list.notice_asked, true) {
            return None;
        }
        let mut told = plugin_file::read_told(store);
        let untold = MOVED.iter().filter(|moved| {
            let changed = list.moved.iter().any(|(key, value)| key == moved.key && moved.changed(value));
            changed && !told.iter().any(|key| key == moved.key)
        });
        let untold: Vec<&Moved> = untold.collect();
        let (last, rest) = untold.split_last()?;
        told.extend(untold.iter().map(|moved| moved.key.to_owned()));
        plugin_file::note_told(store, &told);
        if rest.is_empty() {
            let name = last.name;
            return Some(format!(
                "{name} now comes as a patch. Your setting is kept: import the patch and it picks it up."
            ));
        }
        let names = rest.iter().map(|moved| moved.name).collect::<Vec<_>>().join(", ");
        let name = last.name;
        Some(format!(
            "{names} and {name} now come as patches. Your settings are kept: import each patch and it picks them up."
        ))
    }

    /// Sets the switch kept under `key`; false if there is none.
    pub fn set_switch(&self, key: String, on: bool) -> bool {
        self.change(&key, |control| match control {
            PluginControl::Switch { control } => {
                control.on = on;
                Some(())
            }
            PluginControl::Slider { .. } => None,
        })
        .is_some()
    }

    /// Sets the slider kept under `key`; what it landed on, to keep, if there is one.
    pub fn set_slider(&self, key: String, value: f32) -> Option<f32> {
        self.change(&key, |control| match control {
            PluginControl::Slider { control } if value.is_finite() => {
                control.value = control.stepped(value);
                Some(control.value)
            }
            _ => None,
        })
    }

    /// Applies or unapplies a patch, enables or disables a filter; false if no file is `id`. A
    /// patch goes on its hooks or off them at once, no restart: off them the app is as it was, and
    /// its values wait for it. One that can't start stays off, and says why.
    pub fn set_on(&self, id: String, on: bool) -> bool {
        let mut list = self.listed();
        let Some(plugin) = list.files.iter_mut().find(|p| p.id == id) else { return false };
        plugin.on = on;
        plugin.problem = None;
        if let Some(store) = &self.store {
            plugin_file::clear_stopped(store, &id);
        }
        if plugin.kind == PluginKind::Patch {
            self.hooks.take_off(&id);
            self.start(plugin);
        }
        true
    }

    /// What the plugin file `bytes` is, for the screen that asks whether to import it. Nothing is
    /// kept, and nothing in it runs.
    pub fn inspect(&self, bytes: Vec<u8>) -> PluginOffer {
        let offered = plugin_file::read_package(&bytes).and_then(|package| {
            self.can_take(&package.manifest)?;
            Ok(package.manifest)
        });
        match offered {
            Ok(manifest) => {
                let publisher = if manifest.publisher.is_empty() { "none given" } else { &manifest.publisher };
                let sentence = can_do(&manifest);
                PluginOffer {
                    message: format!("Version {}\nPublisher: {publisher} (not checked)\n{sentence}", manifest.version),
                    name: manifest.name,
                    refused: None,
                }
            }
            Err(refusal) => {
                log::warn!("a plugin file was refused: {refusal:?}");
                PluginOffer { name: String::new(), message: String::new(), refused: Some(refusal.text().to_owned()) }
            }
        }
    }

    /// Imports the plugin file `bytes`, read in full again, so nothing needs to have gone before;
    /// why not, if it wasn't. It shows at once, no restart. A file of a plugin already here
    /// replaces it, keeping what was set, unless it is older.
    pub fn install(&self, bytes: Vec<u8>) -> Option<String> {
        let taken = self.take_in(&bytes);
        taken.err().map(|refusal| {
            log::warn!("a plugin file wasn't imported: {refusal:?}");
            refusal.text().to_owned()
        })
    }

    /// Removes the imported plugin `id`: its folder goes, at once, no restart. The keys to take out
    /// of where values are kept; none if it can't be removed.
    pub fn remove(&self, id: String) -> Option<Vec<String>> {
        let mut list = self.listed();
        let at = list.files.iter().position(|p| p.id == id)?;
        if let Some(store) = &self.store
            && !plugin_file::forget(store, &id)
        {
            return None;
        }
        self.hooks.take_off(&id);
        let gone = list.files.remove(at);
        let keys = gone.controls.iter().map(|c| c.key().to_owned());
        Some(keys.chain([gone.on_key.clone()]).collect())
    }

    /// How big the interface shows in a window `width` × `height` points at 1: as a patch says, or
    /// as measured.
    pub fn interface_scale(&self, width: f32, height: f32) -> f32 {
        self.hooks().interface_scale(width, height)
    }

    /// How wide a gap in its lines ColorDrop keeps out of, canvas pixels (0: none).
    pub fn fill_gap(&self) -> u32 {
        match self.control(FILL_GAP) {
            Some(PluginControl::Slider { control }) => control.value as u32,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::patch_run::tests::position_module;
    use crate::plugin_file::tests::{SHADER, filter, pack, patch};

    /// A slider in steps of 5% from 75% to 125%.
    fn size_slider() -> PluginSlider {
        PluginSlider {
            key: "size".to_owned(),
            label: "Size".to_owned(),
            least: 0.75,
            most: 1.25,
            step: 0.05,
            value: 1.0,
            starts: 1.0,
            shown_times: 100.0,
            unit: "%".to_owned(),
            at_least: None,
            live: false,
        }
    }

    fn gap_slider(plugins: &Plugins) -> PluginSlider {
        match plugins.control(FILL_GAP) {
            Some(PluginControl::Slider { control }) => control,
            other => panic!("{other:?}"),
        }
    }

    /// The filters imported.
    fn filters(plugins: &Plugins) -> Vec<PluginInfo> {
        plugins.plugins().into_iter().filter(|p| p.kind == PluginKind::Filter).collect()
    }

    /// One from elsewhere: nothing in the app asks about it.
    fn stranger() -> PluginInfo {
        PluginInfo {
            id: "someone.stranger".to_owned(),
            name: "Stranger".to_owned(),
            description: "No code answers to it.".to_owned(),
            source: PluginSource::File,
            kind: PluginKind::Filter,
            version: "1.0.0".to_owned(),
            publisher: "Someone".to_owned(),
            experimental: false,
            on: true,
            on_key: "someone.stranger.enabled".to_owned(),
            problem: None,
            controls: vec![
                PluginControl::Switch {
                    control: PluginSwitch {
                        key: "someone.stranger.switch".to_owned(),
                        label: "On".to_owned(),
                        on: false,
                        starts: false,
                    },
                },
                PluginControl::Slider {
                    control: PluginSlider {
                        key: "someone.stranger.slider".to_owned(),
                        label: "Amount".to_owned(),
                        least: 0.0,
                        most: 10.0,
                        step: 0.5,
                        value: 2.0,
                        starts: 2.0,
                        shown_times: 1.0,
                        unit: " px".to_owned(),
                        at_least: None,
                        live: true,
                    },
                },
            ],
        }
    }

    /// A folder of its own, empty, for a test's plugins.
    fn store(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("procraft-plugins-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.to_string_lossy().into_owned()
    }

    fn at(dir: &str) -> Arc<Plugins> {
        Plugins::with_store(dir.to_owned())
    }

    fn filter_file(manifest: &Value) -> Vec<u8> {
        pack(manifest, &[("filter.wgsl", SHADER)])
    }

    fn dir_holds(dir: &str, path: &str) -> bool {
        std::path::Path::new(dir).join(path).exists()
    }

    #[test]
    fn the_extras_are_on_the_plugins_page_and_no_patch_is_in_the_app() {
        let plugins = Plugins::new();
        assert!(plugins.patches().is_empty());
        let list = plugins.plugins();
        let ids: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["fill_gap"]);
        for p in &list {
            assert!(p.source == PluginSource::Bundled && p.kind == PluginKind::Extra, "{p:?}");
            assert!(p.experimental && p.on && p.on_key.is_empty(), "{p:?}");
            assert!(!p.name.is_empty() && !p.description.is_empty() && !p.controls.is_empty(), "{p:?}");
            assert!(!p.id.contains('.'), "an extra's id can't be a file's key's start: {p:?}");
        }
        assert_eq!(plugins.interface_scale(1600.0, 1000.0), 1.0);
        assert_eq!(plugins.fill_gap(), 0);
    }

    #[test]
    fn no_two_controls_share_a_key() {
        let plugins = Plugins::new();
        let list = plugins.plugins();
        let mut keys: Vec<String> = list.iter().flat_map(|p| &p.controls).map(|c| c.key().into()).collect();
        let all = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), all);
    }

    #[test]
    fn values_kept_before_there_was_a_list_carry_over() {
        // as both apps wrote them: the desktop's `interface` lines, Android's preferences as text
        let plugins = Plugins::new();
        plugins.read_kept("fill_gap".to_owned(), "12".to_owned());
        assert_eq!(plugins.fill_gap(), 12);
    }

    #[test]
    fn a_kept_value_comes_back_usable() {
        let read = |text: &str| {
            let plugins = Plugins::new();
            plugins.listed().files.push(stranger());
            plugins.read_kept("someone.stranger.slider".to_owned(), text.to_owned());
            match plugins.control("someone.stranger.slider") {
                Some(PluginControl::Slider { control }) => control.value,
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(read("3.3"), 3.5);
        assert_eq!(read("90"), 10.0);
        assert_eq!(read("-1"), 0.0);
        for bad in ["NaN", "inf", "-inf", "", "big", "true"] {
            assert_eq!(read(bad), 2.0, "{bad}");
        }
        let plugins = Plugins::new();
        plugins.listed().files.push(stranger());
        plugins.read_kept("someone.stranger.switch".to_owned(), "yes".to_owned());
        assert!(
            matches!(plugins.control("someone.stranger.switch"), Some(PluginControl::Switch { control }) if !control.on)
        );
    }

    #[test]
    fn what_is_kept_reads_back_the_same() {
        let plugins = Plugins::new();
        assert_eq!(plugins.set_slider(FILL_GAP.to_owned(), 7.0), Some(7.0));
        let lines = plugins.kept_lines();
        assert!(lines.contains(&"fill_gap 7".to_owned()), "{lines:?}");
        assert_eq!(lines.len(), 1, "the extras have their controls and nothing else: {lines:?}");
        let again = Plugins::new();
        for line in &lines {
            let (key, value) = line.split_once(' ').unwrap();
            again.read_kept(key.to_owned(), value.to_owned());
        }
        assert_eq!(again.plugins(), plugins.plugins());
    }

    #[test]
    fn a_slider_moves_in_steps_with_100_in_the_middle() {
        let size = size_slider();
        assert_eq!(plugin_slider_at(size.clone(), 0.0), 0.75);
        assert_eq!(plugin_slider_at(size.clone(), 0.5), 1.0);
        assert_eq!(plugin_slider_at(size.clone(), 1.0), 1.25);
        // the nearest step, from anywhere along it
        assert_eq!(plugin_slider_at(size.clone(), 0.51), 1.0);
        assert_eq!(plugin_slider_at(size.clone(), 0.56), 1.05);
        assert_eq!(plugin_slider_along(size.clone(), 1.0), 0.5);
        // every step exactly the number it reads as, and back to where it was
        let steps = [0.75, 0.8, 0.85, 0.9, 0.95, 1.0, 1.05, 1.1, 1.15, 1.2, 1.25];
        for (i, step) in steps.into_iter().enumerate() {
            let along = i as f32 / 10.0;
            assert_eq!(plugin_slider_at(size.clone(), along), step);
            assert!((plugin_slider_along(size.clone(), step) - along).abs() < 1e-5, "{along}");
        }
    }

    #[test]
    fn a_slider_reads_as_its_step_needs() {
        let size = size_slider();
        assert_eq!(plugin_slider_shown(size.clone(), 0.85), "85%");
        assert_eq!(plugin_slider_shown(size, 1.0), "100%");
        let PluginControl::Slider { control } = stranger().controls[1].clone() else { panic!() };
        assert_eq!(plugin_slider_shown(control.clone(), 2.5), "2.5 px");
        let hundredths = PluginSlider { step: 0.05, ..control };
        assert_eq!(plugin_slider_shown(hundredths, 2.25), "2.25 px");
    }

    #[test]
    fn the_gap_keeps_its_range_and_how_it_reads() {
        let plugins = Plugins::new();
        let gap = gap_slider(&plugins);
        assert_eq!((gap.least, gap.most, gap.step), (0.0, 32.0, 1.0));
        assert_eq!(plugin_slider_shown(gap.clone(), 0.0), "Off");
        assert_eq!(plugin_slider_shown(gap.clone(), 1.0), "1 px");
        assert_eq!(plugin_slider_shown(gap.clone(), 32.0), "32 px");
        assert_eq!(plugin_slider_at(gap.clone(), 0.5), 16.0);
        assert_eq!(plugin_slider_along(gap, 8.0), 0.25);
        // what was kept past its ends comes back inside them, as it always did
        plugins.read_kept(FILL_GAP.to_owned(), "40".to_owned());
        assert_eq!(plugins.fill_gap(), 32);
        plugins.read_kept(FILL_GAP.to_owned(), "-3".to_owned());
        assert_eq!(plugins.fill_gap(), 0);
    }

    #[test]
    fn a_count_is_kept_as_a_whole_number() {
        let plugins = Plugins::new();
        assert!(plugin_slider_whole(gap_slider(&plugins)));
        assert!(!plugin_slider_whole(size_slider()));
        plugins.set_slider(FILL_GAP.to_owned(), 12.0);
        // as the desktop has always written it
        assert!(plugins.kept_lines().contains(&"fill_gap 12".to_owned()));
    }

    #[test]
    fn a_key_nothing_has_is_left_alone() {
        let plugins = Plugins::new();
        let before = plugins.plugins();
        assert!(!plugins.set_switch("nobody".to_owned(), true));
        assert_eq!(plugins.set_slider("nobody".to_owned(), 1.0), None);
        plugins.read_kept("nobody".to_owned(), "true".to_owned());
        plugins.read_kept("nobody.enabled".to_owned(), "false".to_owned());
        assert!(!plugins.set_on("nobody".to_owned(), true));
        assert_eq!(plugins.remove("nobody".to_owned()), None);
        // nor does a value of the wrong kind reach a control
        assert!(!plugins.set_switch(FILL_GAP.to_owned(), true));
        assert_eq!(plugins.set_slider(FILL_GAP.to_owned(), f32::NAN), None);
        // and an extra can't be turned off or removed
        assert!(!plugins.set_on(FILL_GAP.to_owned(), false));
        assert_eq!(plugins.remove(FILL_GAP.to_owned()), None);
        assert_eq!(plugins.plugins(), before);
        assert_eq!(plugins.fill_gap(), 0);
    }

    #[test]
    fn without_an_extra_the_app_goes_on_as_it_starts() {
        let plugins = Plugins::new();
        plugins.listed().bundled.clear();
        assert_eq!(plugins.fill_gap(), 0);
    }

    #[test]
    fn a_slider_described_badly_panics_nowhere() {
        let bad = |least, most, step| PluginSlider {
            key: "bad".to_owned(),
            label: String::new(),
            least,
            most,
            step,
            value: 0.0,
            starts: 0.0,
            shown_times: 1.0,
            unit: String::new(),
            at_least: None,
            live: false,
        };
        for slider in [bad(5.0, 1.0, 1.0), bad(1.0, 1.0, 0.0), bad(0.0, 1.0, -1.0), bad(f32::NAN, f32::NAN, f32::NAN)] {
            let _ = slider.stepped(0.5);
            let _ = plugin_slider_at(slider.clone(), f32::NAN);
            let _ = plugin_slider_along(slider.clone(), 0.5);
            let _ = plugin_slider_shown(slider, 0.5);
        }
    }

    #[test]
    fn a_plugin_nothing_answers_to_shows_and_does_nothing() {
        let plugins = Plugins::new();
        plugins.listed().files.push(stranger());
        assert_eq!(filters(&plugins).len(), 1);
        // its values change and are kept like any other's, and touch nothing else
        assert!(plugins.set_switch("someone.stranger.switch".to_owned(), true));
        assert_eq!(plugins.set_slider("someone.stranger.slider".to_owned(), 3.3), Some(3.5));
        assert_eq!(plugins.fill_gap(), 0);
    }

    #[test]
    fn a_filter_is_imported_kept_through_a_restart_and_removed() {
        let dir = store("filter");
        let plugins = at(&dir);
        assert!(filters(&plugins).is_empty());
        // it shows at once, and runs nothing
        assert_eq!(plugins.install(filter_file(&filter("org.example.filter"))), None);
        let card = filters(&plugins).remove(0);
        assert_eq!(card.source, PluginSource::File);
        assert_eq!((card.version.as_str(), card.publisher.as_str()), ("1.0.0", "Someone"));
        assert_eq!(card.problem, None);
        assert!(card.on && card.experimental);
        assert_eq!(card.controls.len(), 2);
        assert_eq!(card.on_key, "org.example.filter.enabled");
        assert!(dir_holds(&dir, "org.example.filter/1.0.0/filter.wgsl"));
        // through a restart, its value with it
        plugins.set_slider("org.example.filter.cut".to_owned(), 0.25);
        let next = at(&dir);
        assert_eq!(filters(&next).len(), 1);
        for line in plugins.kept_lines() {
            let (key, value) = line.split_once(' ').unwrap();
            next.read_kept(key.to_owned(), value.to_owned());
        }
        assert_eq!(filters(&next), filters(&plugins));
        // removed: gone at once, no restart, and from the disk
        let keys = next.remove("org.example.filter".to_owned()).unwrap();
        assert!(keys.contains(&"org.example.filter.cut".to_owned()));
        assert!(keys.contains(&"org.example.filter.enabled".to_owned()));
        assert!(filters(&next).is_empty());
        assert!(!dir_holds(&dir, "org.example.filter"));
        assert!(filters(&at(&dir)).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plugin_can_t_reach_an_extra_s_value() {
        let dir = store("collide");
        let plugins = at(&dir);
        plugins.set_slider(FILL_GAP.to_owned(), 11.0);
        let mut manifest = filter("org.example.filter");
        manifest["controls"] = json!([{
            "key": "fill_gap", "type": "slider", "label": "Gap", "least": 0, "most": 5, "step": 1, "starts": 3,
        }]);
        assert_eq!(plugins.install(filter_file(&manifest)), None);
        let key = "org.example.filter.fill_gap".to_owned();
        assert_eq!(plugins.set_slider(key.clone(), 4.0), Some(4.0));
        assert_eq!(plugins.fill_gap(), 11, "its own value is its own");
        plugins.read_kept(key, "2".to_owned());
        assert_eq!(plugins.fill_gap(), 11);
        // and the extra's key goes on reaching the extra's value
        assert_eq!(plugins.set_slider(FILL_GAP.to_owned(), 8.0), Some(8.0));
        assert_eq!(plugins.fill_gap(), 8);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_filter_is_enabled_and_disabled_and_it_is_kept() {
        let dir = store("enabled");
        let plugins = at(&dir);
        plugins.install(filter_file(&filter("org.example.filter")));
        assert!(plugins.set_on("org.example.filter".to_owned(), false));
        assert!(!filters(&plugins)[0].on);
        assert!(plugins.kept_lines().contains(&"org.example.filter.enabled false".to_owned()));
        let next = at(&dir);
        assert!(filters(&next)[0].on, "as imported");
        next.read_kept("org.example.filter.enabled".to_owned(), "false".to_owned());
        assert!(!filters(&next)[0].on);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_offer_says_plainly_what_it_is() {
        let plugins = at(&store("offer"));
        let offer = plugins.inspect(filter_file(&filter("org.example.filter")));
        assert_eq!(offer.refused, None);
        assert_eq!(offer.name, "A filter");
        assert!(offer.message.contains("Version 1.0.0"), "{offer:?}");
        assert!(offer.message.contains("Someone (not checked)"), "{offer:?}");
        assert!(offer.message.contains("only the layer you use it on"), "{offer:?}");
        // nothing was kept by asking
        assert!(filters(&plugins).is_empty());
        let refused = plugins.inspect(b"nothing".to_vec());
        assert_eq!(refused.refused.as_deref(), Some("This isn't a ProCraft plugin."));
        assert!(refused.name.is_empty());
    }

    #[test]
    fn the_offer_of_a_patch_says_what_it_can_and_can_t_do() {
        let module = position_module("", "(i64.const 0)");
        let mut manifest = patch("org.example.patch", &module);
        manifest["name"] = json!("Offset");
        let plugins = at(&store("patch-offer"));
        let offer = plugins.inspect(pack(&manifest, &[(crate::patch_run::MODULE, &module)]));
        assert_eq!(offer.refused, None);
        assert_eq!(offer.name, "Offset");
        assert!(offer.message.contains("Someone (not checked)"), "{offer:?}");
        assert!(offer.message.contains("where your strokes land"), "{offer:?}");
        assert!(offer.message.contains("can't reach your files"), "{offer:?}");
        assert!(plugins.patches().is_empty());
    }

    /// A patch taking over the moved eyedropper setting, as a file.
    fn eyedropper_file(id: &str) -> Vec<u8> {
        let text = r#"(module (func (export "procraft_control") (param i32 f32))
            (func (export "procraft_pick_layer") (result i32) (i32.const 0)))"#;
        let module = wat::parse_str(text).unwrap();
        let mut manifest = patch(id, &module);
        manifest["hooks"] = json!(["pick_layer"]);
        manifest["controls"] = json!([
            { "key": "layer", "type": "switch", "label": "Layer", "keeps": "eyedropper_layer" },
        ]);
        pack(&manifest, &[(crate::patch_run::MODULE, &module)])
    }

    fn switch_on(plugins: &Plugins, key: &str) -> Option<bool> {
        match plugins.control(key) {
            Some(PluginControl::Switch { control }) => Some(control.on),
            _ => None,
        }
    }

    #[test]
    fn someone_who_changed_a_moved_setting_is_told_once_and_keeps_it() {
        let dir = store("moved");
        let plugins = at(&dir);
        plugins.read_kept("eyedropper_layer".to_owned(), "true".to_owned());
        let said = plugins.take_moved_notice().unwrap();
        assert!(said.starts_with("Eyedropper picks from layer now comes as a patch."), "{said}");
        // once, in this run and the next
        assert_eq!(plugins.take_moved_notice(), None);
        let next = at(&dir);
        next.read_kept("eyedropper_layer".to_owned(), "true".to_owned());
        assert_eq!(next.take_moved_notice(), None);
        // nothing is installed for them, and the value goes on being kept
        assert!(next.patches().is_empty());
        assert!(next.kept_lines().contains(&"eyedropper_layer true".to_owned()));
        // the patch picks it up on import, and keeps it where it was, through a restart
        assert_eq!(next.install(eyedropper_file("org.example.pick")), None);
        assert_eq!(switch_on(&next, "eyedropper_layer"), Some(true));
        let lines = next.kept_lines();
        assert_eq!(lines.iter().filter(|line| line.starts_with("eyedropper_layer ")).count(), 1, "{lines:?}");
        let again = at(&dir);
        again.read_kept("eyedropper_layer".to_owned(), "true".to_owned());
        assert_eq!(switch_on(&again, "eyedropper_layer"), Some(true));
        // removed, the value goes with it
        let keys = again.remove("org.example.pick".to_owned()).unwrap();
        assert!(keys.contains(&"eyedropper_layer".to_owned()), "{keys:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_moved_setting_nobody_changed_says_nothing() {
        let dir = store("moved-unchanged");
        let plugins = at(&dir);
        plugins.read_kept("eyedropper_layer".to_owned(), "false".to_owned());
        assert_eq!(plugins.take_moved_notice(), None);
        assert_eq!(at(&dir).take_moved_notice(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_moved_setting_with_its_patch_here_says_nothing() {
        let dir = store("moved-held");
        let plugins = at(&dir);
        assert_eq!(plugins.install(eyedropper_file("org.example.pick")), None);
        let next = at(&dir);
        next.read_kept("eyedropper_layer".to_owned(), "true".to_owned());
        assert_eq!(next.take_moved_notice(), None);
        assert_eq!(switch_on(&next, "eyedropper_layer"), Some(true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_moved_setting_has_one_holder() {
        let dir = store("moved-clash");
        let plugins = at(&dir);
        assert_eq!(plugins.install(eyedropper_file("org.example.pick")), None);
        let said = plugins.inspect(eyedropper_file("org.example.other")).refused;
        assert_eq!(said.as_deref(), Some("Another plugin already holds a setting this one needs."));
        assert_eq!(plugins.install(eyedropper_file("org.example.other")), said);
        // and a newer version of the holder is still welcome
        assert_eq!(plugins.install(eyedropper_file("org.example.pick")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_patch_and_a_filter_each_go_on_their_own_page() {
        let dir = store("pages");
        let plugins = at(&dir);
        let module = position_module("", "(i64.const 0)");
        let file = pack(&patch("org.example.patch", &module), &[(crate::patch_run::MODULE, &module)]);
        assert_eq!(plugins.install(file), None);
        assert_eq!(plugins.install(filter_file(&filter("org.example.filter"))), None);
        let patches = plugins.patches();
        assert_eq!(patches.len(), 1);
        assert!(patches[0].kind == PluginKind::Patch && patches[0].source == PluginSource::File);
        assert!(!patches[0].on && patches[0].problem.is_none(), "it starts unapplied: {:?}", patches[0]);
        assert_eq!(patches[0].on_key, "org.example.patch.applied");
        assert_eq!(filters(&plugins).len(), 1);
        assert!(plugins.plugins().iter().all(|p| p.kind != PluginKind::Patch));
        // and through a restart
        let next = at(&dir);
        assert_eq!((next.patches().len(), filters(&next).len()), (1, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_update_keeps_what_was_set_and_an_older_file_is_refused() {
        let dir = store("update");
        let plugins = at(&dir);
        let mut newer = filter("org.example.filter");
        newer["version"] = json!("1.2.0");
        assert_eq!(plugins.install(filter_file(&newer)), None);
        plugins.set_slider("org.example.filter.cut".to_owned(), 0.25);
        // older: refused, on the screen and if asked anyway
        let older = filter_file(&filter("org.example.filter"));
        let said = Some("A newer version of this plugin is already imported.".to_owned());
        assert_eq!(plugins.inspect(older.clone()).refused, said);
        assert_eq!(plugins.install(older), said);
        // newer: takes over, what was set stays
        newer["version"] = json!("1.3.0");
        assert_eq!(plugins.install(filter_file(&newer)), None);
        assert_eq!(filters(&plugins)[0].version, "1.3.0");
        let cut = plugins.control("org.example.filter.cut");
        assert!(matches!(cut, Some(PluginControl::Slider { control }) if (control.value - 0.25).abs() < 1e-6));
        assert_eq!(filters(&at(&dir)).len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refused_file_leaves_nothing_behind() {
        let dir = store("refused");
        let plugins = at(&dir);
        let mut asks = filter("org.example.filter");
        asks["permissions"] = json!(["network"]);
        let said = plugins.install(filter_file(&asks));
        assert_eq!(said.as_deref(), Some("This plugin asks for permissions ProCraft doesn't give."));
        assert!(filters(&plugins).is_empty());
        assert!(!dir_holds(&dir, "org.example.filter"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_nowhere_to_keep_it_a_file_isn_t_taken() {
        let file = filter_file(&filter("org.example.filter"));
        assert_eq!(Plugins::new().install(file).as_deref(), Some("This plugin couldn't be saved."));
    }

    #[test]
    fn a_kept_plugin_tampered_with_counts_as_absent_and_nothing_breaks() {
        let dir = store("tampered");
        let plugins = at(&dir);
        assert_eq!(plugins.install(filter_file(&filter("org.example.filter"))), None);
        let shader = std::path::Path::new(&dir).join("org.example.filter/1.0.0/filter.wgsl");
        std::fs::write(shader, b"fn filter() {}").unwrap();
        let next = at(&dir);
        assert!(filters(&next).is_empty());
        // a value kept for it finds nothing to go to
        next.read_kept("org.example.filter.cut".to_owned(), "0.1".to_owned());
        assert!(filters(&next).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
