//! What the app does beyond its measured look and behaviour: its plugins, on one list and one page.
//! A plugin is a file. The files the app comes with for now ([`crate::bundled`]) are added once, as
//! if imported, off, and are then like any other. With none on the app is as measured. A plugin's
//! manifest says what it holds: code run in a sandbox on the [hooks](crate::hooks) it asks for, a
//! filter, or both. See [`crate::plugin_file`] for what is asked of a file.
//!
//! All are plain data, a name, a line on what it does and its controls, so the page draws any of
//! them without knowing what it holds.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::hooks::Hooks;
use crate::plugin_file::{self, Added, Manifest, Refusal};
use crate::plugin_run::{Hook, Running, Stop};

/// A setting the app used to have and a plugin now brings: what it was kept under, as it started
/// (a switch off is 0), and its name. Its value stays there for the plugin, whose control says so.
struct Moved {
    key: &'static str,
    starts: f32,
    name: &'static str,
}

const MOVED: &[Moved] = &[
    Moved { key: "eyedropper_layer", starts: 0.0, name: "Eyedropper picks from layer" },
    Moved { key: "interface_size", starts: 1.0, name: "Interface size" },
    Moved { key: "paste_into_mask", starts: 0.0, name: "Paste into masks" },
    Moved { key: "fill_gap", starts: 0.0, name: "ColorDrop gap closing" },
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

/// Whether `value`, kept under `key`, is a setting the app used to have that someone changed.
fn moved_and_changed(key: &str, value: &str) -> bool {
    MOVED.iter().any(|moved| moved.key == key && moved.changed(value))
}

/// Why a filter the app ended on, or that stopped the GPU, is off.
pub(crate) const STOPPED_DRIVER: &str = "Switched off: it stopped the graphics driver.";
/// Why a filter whose files no longer hold up can't open.
const FILTER_DAMAGED: &str = "This filter's files don't hold up.";

/// Where one came from. The route decides this, never the plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PluginSource {
    File,
    Catalogue,
}

/// What a plugin holds, as its manifest says: one of these or several.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum PluginPart {
    /// Code, running on its hooks while the plugin is on.
    Hooks,
    /// A filter, under Adjustments while the plugin is on.
    Filter,
}

impl PluginPart {
    /// The part a manifest calls `name`, if this app has it.
    pub(crate) fn named(name: &str) -> Option<PluginPart> {
        match name {
            "hooks" => Some(PluginPart::Hooks),
            "filter" => Some(PluginPart::Filter),
            _ => None,
        }
    }
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
    /// A name for each step from its least, read in place of the number (none: the numbers).
    pub names: Vec<String>,
    /// Its value goes in as it moves; otherwise as it lets go, which what changes the page needs.
    pub live: bool,
}

/// What a file control takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PluginFileKind {
    /// A sound: a recording, kept as a WAV file.
    Sound,
}

/// A file someone picks, kept with the plugin; `chosen` is the name it was picked by, if one is.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginFile {
    pub key: String,
    pub label: String,
    pub accepts: PluginFileKind,
    pub chosen: Option<String>,
}

/// Something a plugin does when its row is tapped: `key` is `<id>.<key>`. Its card lists it, and
/// `place` may list it somewhere else too.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginAction {
    pub key: String,
    pub label: String,
    pub place: PluginPlace,
}

/// Where else a plugin's action is listed, beside its card.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PluginPlace {
    /// Only on its card.
    Card,
    /// In the Adjustments menu's Plugins list, with the filters.
    Adjustments,
    /// In the current layer's options, after the app's own.
    Layer,
    /// In Animation Assist's frame options, after the app's own; run with that frame's layer
    /// current.
    Frame,
}

impl PluginPlace {
    /// The place a manifest names; none for a name this app doesn't have.
    pub(crate) fn named(name: &str) -> Option<PluginPlace> {
        match name {
            "card" => Some(PluginPlace::Card),
            "adjustments" => Some(PluginPlace::Adjustments),
            "layer" => Some(PluginPlace::Layer),
            "frame" => Some(PluginPlace::Frame),
            _ => None,
        }
    }
}

/// A row a plugin adds to the brush studio's Properties: a choice of a few, by their place, kept
/// with each brush under `key` (`plugin:<id>:<key>`; 0 the first choice, as a brush starts).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginBrushControl {
    pub key: String,
    pub label: String,
    pub choices: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PluginControl {
    Switch { control: PluginSwitch },
    Slider { control: PluginSlider },
    File { control: PluginFile },
}

/// One as its page shows it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: PluginSource,
    pub holds: Vec<PluginPart>,
    /// As its file says.
    pub version: String,
    pub publisher: String,
    /// Tagged as experimental on its page.
    pub experimental: bool,
    /// Whether it is doing what it does.
    pub on: bool,
    /// What `on` is kept under.
    pub on_key: String,
    /// Why it does nothing, to show on it.
    pub problem: Option<String>,
    /// What its card shows, while it is on. A filter's own are on its panel.
    pub controls: Vec<PluginControl>,
    /// Rows it adds to the brush studio, while it is on.
    pub brush_controls: Vec<PluginBrushControl>,
    /// What it does when tapped, while it is on, and what it may reach then.
    pub actions: Vec<PluginAction>,
    /// The kinds of artwork it makes, while it is on (the gallery's + lists them).
    pub kinds: Vec<crate::PluginKind>,
    pub permissions: Vec<crate::plugin_commands::PluginPermission>,
}

impl PluginInfo {
    fn holds(&self, part: PluginPart) -> bool {
        self.holds.contains(&part)
    }
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
            PluginControl::File { control } => &control.key,
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
            // what was picked is kept with the plugin, not with the settings
            PluginControl::File { .. } => {}
        }
    }

    /// Back as it starts.
    pub(crate) fn back_to_start(&mut self) {
        match self {
            PluginControl::Switch { control } => control.on = control.starts,
            PluginControl::Slider { control } => control.value = control.starts,
            PluginControl::File { .. } => {}
        }
    }

    /// Its value as a plugin or a filter is told it.
    pub(crate) fn number(&self) -> f32 {
        match self {
            PluginControl::Switch { control } => f32::from(u8::from(control.on)),
            PluginControl::Slider { control } => control.value,
            PluginControl::File { control } => f32::from(u8::from(control.chosen.is_some())),
        }
    }

    /// Its value as kept, read back with [`Plugins::read_kept`].
    fn kept(&self) -> String {
        match self {
            PluginControl::Switch { control } => control.on.to_string(),
            PluginControl::Slider { control } => control.value.to_string(),
            PluginControl::File { control } => control.chosen.is_some().to_string(),
        }
    }

    /// `other`'s value taken over, if it is the same kind of control.
    fn take_value(&mut self, other: &PluginControl) {
        match (self, other) {
            (PluginControl::Switch { control }, PluginControl::Switch { control: from }) => control.on = from.on,
            (PluginControl::Slider { control }, PluginControl::Slider { control: from }) => {
                control.value = control.stepped(from.value);
            }
            (PluginControl::File { control }, PluginControl::File { control: from }) => {
                control.chosen.clone_from(&from.chosen)
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
    let place = ((value - slider.least) / slider.step).round();
    if let Some(name) = (place >= 0.0).then_some(place as usize).and_then(|i| slider.names.get(i)) {
        return name.clone();
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

/// `manifest` as a list entry. One that only adds a filter starts on, as that changes nothing
/// already there; any other starts off, for someone to turn on.
/// The names of the files picked for `plugin`'s file controls, as kept in `store`.
fn read_chosen(store: &std::path::Path, plugin: &mut PluginInfo) {
    let id = plugin.id.clone();
    for control in &mut plugin.controls {
        if let PluginControl::File { control } = control {
            let (file, name) = plugin_file::chosen_paths(store, &id, &control.key);
            let named = || std::fs::read_to_string(&name).ok().map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
            control.chosen = file.is_file().then(|| named().unwrap_or_else(|| "Recording".to_owned()));
        }
    }
}

fn from_file(manifest: Manifest) -> PluginInfo {
    let only_filter = manifest.holds == [PluginPart::Filter];
    PluginInfo {
        on_key: format!("{}.on", manifest.id),
        id: manifest.id,
        name: manifest.name,
        description: manifest.description,
        source: PluginSource::File,
        holds: manifest.holds,
        version: manifest.version,
        publisher: manifest.publisher,
        experimental: manifest.experimental,
        on: only_filter,
        problem: None,
        controls: if only_filter { Vec::new() } else { manifest.controls },
        brush_controls: manifest.brush_controls,
        actions: manifest.actions,
        kinds: manifest.kinds,
        permissions: manifest.permissions,
    }
}

/// What the offer says `manifest` can do.
fn can_do(manifest: &Manifest) -> String {
    let filter = manifest.holds.contains(&PluginPart::Filter);
    let adds =
        filter.then_some("This plugin adds a filter to Adjustments, which changes only the layer you use it on.");
    let sentences: Vec<&str> = adds
        .into_iter()
        .chain(manifest.hooks.iter().map(|hook| hook.sentence()))
        .chain(manifest.permissions.iter().map(|permission| permission.sentence()))
        .collect();
    let artwork = manifest.permissions.iter().any(|p| {
        matches!(
            p,
            crate::plugin_commands::PluginPermission::ArtworkRead
                | crate::plugin_commands::PluginPermission::ArtworkWrite
        )
    });
    let reach =
        if filter || artwork { "your files or the internet" } else { "your files, your layers or the internet" };
    format!("{} It can't reach {reach}.", sentences.join(" "))
}

/// A filter whose files have held up again, for opening.
pub(crate) struct FilterToOpen {
    pub name: String,
    pub controls: Vec<PluginControl>,
    pub code: procraft_gpu::FilterCode,
}

#[derive(Debug)]
struct Listed {
    files: Vec<PluginInfo>,
    /// Moved settings read back with no plugin yet to take them: key and value, kept as they are.
    moved: Vec<(String, String)>,
    /// The notice about them has been asked for: a start has one chance to say it.
    notice_asked: bool,
    /// The plugins whose on or off has been read back.
    flagged: HashSet<String>,
    /// What the app came with and added, as written down in the store.
    added: Vec<Added>,
}

impl Listed {
    /// Whether plugin `id` is one the app added that starts off until someone turns it on.
    fn added_off(&self, id: &str) -> bool {
        self.added.iter().any(|added| added.off && added.id == id)
    }

    /// The one holding the control kept under `key`, and the control's place in it.
    fn control_mut(&mut self, key: &str) -> Option<(&mut PluginInfo, usize)> {
        self.files.iter_mut().find_map(|plugin| {
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
    /// The plugins that are on, running.
    hooks: Arc<Hooks>,
    /// A plugin's panel showing, with the copy of its code that showed it.
    panel: Mutex<Option<OpenPanel>>,
}

/// A plugin's panel open: whose, what it shows, and the copy of its code its touches go to (under a
/// lock of its own: what the panel shows is read on every frame, while a touch may take seconds).
struct OpenPanel {
    id: String,
    shows: crate::PluginPanel,
    running: Arc<Mutex<Running>>,
}

impl std::fmt::Debug for OpenPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenPanel").field("id", &self.id).field("shows", &self.shows).finish_non_exhaustive()
    }
}

impl Plugins {
    /// The lists, with whatever stopped on a hook since they were last read shown as stopped.
    fn listed(&self) -> MutexGuard<'_, Listed> {
        let mut list = self.list.lock().unwrap_or_else(|e| e.into_inner());
        for (id, stop) in self.hooks.take_stopped() {
            if let Some(plugin) = list.files.iter_mut().find(|p| p.id == id) {
                plugin.on = false;
                plugin.problem = Some(stop.text().to_owned());
            }
        }
        list
    }

    fn kept_in(store: Option<PathBuf>) -> Arc<Plugins> {
        let files = store.as_deref().map(plugin_file::read_all).unwrap_or_default();
        let mut files: Vec<PluginInfo> = files.into_iter().map(from_file).collect();
        let added = store.as_deref().map(plugin_file::read_added).unwrap_or_default();
        if let Some(store) = &store {
            plugin_file::settle_trying(store, STOPPED_DRIVER);
            for plugin in &mut files {
                plugin.problem = plugin_file::stopped_why(store, &plugin.id);
                plugin.on &= plugin.problem.is_none();
                read_chosen(store, plugin);
            }
        }
        // what the app added starts off until someone turns it on, whatever it holds
        for plugin in &mut files {
            plugin.on &= !added.iter().any(|added| added.off && added.id == plugin.id);
        }
        let list = Listed { files, moved: Vec::new(), notice_asked: false, flagged: HashSet::new(), added };
        let hooks = Arc::new(Hooks::kept_in(store.clone()));
        Arc::new(Plugins { list: Mutex::new(list), hooks, store, panel: Mutex::new(None) })
    }

    /// Adds each of `files`, which the app comes with, the first time it comes with it: as if
    /// imported, but off, and from then on like any other plugin. One the store holds older takes
    /// the newer file, as an import of it would, keeping what was set. It is written down that each
    /// was added, before it is, so one removed since stays removed.
    pub(crate) fn add_bundled(&self, files: &[(&str, &[u8])]) {
        let Some(store) = &self.store else { return };
        // each to take in, and whether it is new
        let mut taking = Vec::new();
        {
            let mut list = self.listed();
            let mut added = list.added.clone();
            for (name, bytes) in files {
                let manifest = match plugin_file::read_package(bytes) {
                    Ok(package) => package.manifest,
                    Err(refusal) => {
                        log::warn!("{name}, which comes with the app, doesn't hold up: {refusal:?}");
                        continue;
                    }
                };
                let kept = list.files.iter().find(|p| p.id == manifest.id).map(|p| p.version.as_str());
                let noted = added.iter().any(|added| added.id == manifest.id);
                if !noted {
                    // one imported before the app came with it goes on as it was
                    added.push(Added { id: manifest.id.clone(), off: kept.is_none() });
                }
                let take = match kept {
                    // removed since it was added
                    None => !noted,
                    // taken again only if this one is newer
                    Some(kept) => plugin_file::version_is_older(kept, &manifest.version),
                };
                if take {
                    taking.push((*name, *bytes, manifest.id, kept.is_none()));
                }
            }
            if added != list.added {
                if !plugin_file::note_added(store, &added) {
                    return;
                }
                list.added = added;
            }
        }
        let mut failed = Vec::new();
        for (name, bytes, id, new) in taking {
            if let Err(refusal) = self.take_in(bytes) {
                log::warn!("{name}, which comes with the app, couldn't be added: {refusal:?}");
                if new {
                    failed.push(id);
                }
            }
        }
        // to be tried again on the next start
        if !failed.is_empty() {
            let mut list = self.listed();
            let mut added = list.added.clone();
            added.retain(|added| !failed.contains(&added.id));
            if plugin_file::note_added(store, &added) {
                list.added = added;
            }
        }
    }

    /// `id` off its hooks, and its panel closed.
    fn take_off(&self, id: &str) {
        self.hooks.take_off(id);
        let mut panel = self.open_panel();
        if panel.as_ref().is_some_and(|open| open.id == id) {
            *panel = None;
        }
    }

    fn open_panel(&self) -> MutexGuard<'_, Option<OpenPanel>> {
        self.panel.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Puts `plugin` on its hooks, if it has code, is on and isn't there yet. One that can't start
    /// goes off, and says why.
    fn start(&self, plugin: &mut PluginInfo) {
        if !plugin.holds(PluginPart::Hooks) || !plugin.on || self.hooks.is_on(&plugin.id) {
            return;
        }
        let values: Vec<f32> = plugin.controls.iter().map(PluginControl::number).collect();
        let store = self.store.as_deref();
        // a plugin of switches: no code, its hooks answered from its controls
        if let Some((hooks, answers)) =
            store.and_then(|store| plugin_file::read_switches(store, &plugin.id, &plugin.version))
        {
            self.hooks.put_switches(plugin.id.clone(), &hooks, answers, values);
            return;
        }
        let code = store.and_then(|store| plugin_file::read_code(store, &plugin.id, &plugin.version));
        let started = code
            .ok_or(Stop::Damaged)
            .and_then(|(hooks, bytes)| Ok((Running::start_on(&bytes, &values, &hooks, &plugin.permissions)?, hooks)));
        match started {
            Ok((running, hooks)) => {
                self.hooks.put_on(plugin.id.clone(), running, &hooks);
                // a copy of its own for its events
                if hooks.iter().any(|h| matches!(h, Hook::StrokeKept | Hook::LayerAdded))
                    && let Some((_, bytes)) =
                        store.and_then(|store| plugin_file::read_code(store, &plugin.id, &plugin.version))
                    && let Ok(events) = Running::start_on(&bytes, &values, &hooks, &plugin.permissions)
                {
                    self.hooks.give_events(&plugin.id, events);
                }
                self.hooks.brush_keys(&plugin.id, plugin.brush_controls.iter().map(|c| c.key.clone()).collect());
                // a picker's picture, for the sessions to draw
                if hooks.contains(&Hook::Picker)
                    && let Some(text) =
                        store.and_then(|store| plugin_file::read_picture_code(store, &plugin.id, &plugin.version))
                {
                    self.hooks.give_picture(&plugin.id, text);
                }
                // the files picked for it, handed over
                for (at, control) in plugin.controls.iter().enumerate() {
                    if let (Some(store), PluginControl::File { control }) = (store, control)
                        && control.chosen.is_some()
                        && let Ok(bytes) = std::fs::read(plugin_file::chosen_paths(store, &plugin.id, &control.key).0)
                    {
                        self.hooks.give_file(&plugin.id, at, Some(&bytes));
                    }
                }
            }
            Err(stop) => {
                self.hooks.failed(&plugin.id, stop);
                plugin.on = false;
                plugin.problem = Some(stop.text().to_owned());
            }
        }
    }

    /// The hooks, every plugin that's on on them: a list read back from where it was kept starts its
    /// plugins here, once all its values are in and something will go through them.
    pub(crate) fn hooks(&self) -> Arc<Hooks> {
        let mut list = self.listed();
        for plugin in &mut list.files {
            self.start(plugin);
        }
        Arc::clone(&self.hooks)
    }

    /// What `change` makes of the control kept under `key`, if there is one. A running plugin is
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

    #[cfg(test)]
    fn control(&self, key: &str) -> Option<PluginControl> {
        let mut list = self.listed();
        let (plugin, at) = list.control_mut(key)?;
        plugin.controls.get(at).cloned()
    }

    /// Each value as a line of a settings file, its key and then what [`Plugins::read_kept`] reads.
    pub fn kept_lines(&self) -> Vec<String> {
        let list = self.listed();
        // (a file picked is kept with its plugin)
        let values = list.files.iter().flat_map(|p| &p.controls).filter(|c| !matches!(c, PluginControl::File { .. }));
        let values = values.map(|c| format!("{} {}", c.key(), c.kept()));
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
        read_chosen(store, &mut fresh);
        let mut list = self.listed();
        fresh.on &= !list.added_off(&fresh.id);
        // a moved setting read back goes to the plugin that takes it over
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
                self.take_off(&fresh.id);
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
        let plugin = list.files.iter().find(|p| p.id == id && p.holds(PluginPart::Filter));
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

    /// Switches off the plugin of filter `id`, all of it, `why` on its card and through a restart,
    /// until someone turns it on.
    pub(crate) fn stop_filter(&self, id: &str, why: &str) {
        if let Some(store) = &self.store {
            plugin_file::note_stopped(store, id, why);
        }
        self.take_off(id);
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
    /// Nothing, and nowhere to keep a file.
    #[uniffi::constructor]
    pub fn new() -> Arc<Plugins> {
        Plugins::kept_in(None)
    }

    /// Every file kept in the folder `dir` that holds up.
    #[uniffi::constructor]
    pub fn with_store(dir: String) -> Arc<Plugins> {
        Plugins::kept_in(Some(PathBuf::from(dir)))
    }

    /// The app's plugins: every file kept in the folder `dir` that holds up, with the files the app
    /// comes with added there first if it never added them before.
    #[uniffi::constructor]
    pub fn with_bundled(dir: String) -> Arc<Plugins> {
        let plugins = Plugins::kept_in(Some(PathBuf::from(dir)));
        plugins.add_bundled(crate::bundled::BUNDLED);
        plugins
    }

    /// What the Plugins page shows: every plugin imported.
    pub fn plugins(&self) -> Vec<PluginInfo> {
        self.listed().files.clone()
    }

    /// The actions listed in `place` now, of the plugins that are on.
    pub fn actions_in(&self, place: PluginPlace) -> Vec<PluginAction> {
        let list = self.listed();
        let on = list.files.iter().filter(|p| p.on && p.problem.is_none());
        on.flat_map(|p| p.actions.iter().filter(|a| a.place == place).cloned()).collect()
    }

    /// The filters that can be opened now: the Adjustments menu's, which has none to show with none
    /// imported.
    pub fn filters(&self) -> Vec<PluginInfo> {
        let list = self.listed();
        let ready = |p: &&PluginInfo| p.holds(PluginPart::Filter) && p.on && p.problem.is_none();
        list.files.iter().filter(ready).cloned().collect()
    }

    /// A value read back from where it was kept, as text. A key nothing has, or anything that
    /// isn't its kind of value, leaves things as they are. A plugin that stopped stays off, whatever
    /// its flag says, until someone asks it to run.
    ///
    /// A setting the app used to have, that someone changed, starts the plugin the app added to
    /// hold it on, so the app goes on as they left it: unless that plugin's own on or off is read
    /// back too, in whatever order, which says instead.
    pub fn read_kept(&self, key: String, value: String) {
        let mut list = self.listed();
        let Listed { files, moved: unheld, flagged, added, .. } = &mut *list;
        if let Some(plugin) = files.iter_mut().find(|p| p.on_key == key) {
            if let Ok(on) = value.parse::<bool>() {
                plugin.on = on && plugin.problem.is_none();
                flagged.insert(plugin.id.clone());
            }
            return;
        }
        let held = files.iter_mut().find_map(|plugin| {
            let at = plugin.controls.iter().position(|c| c.key() == key)?;
            Some((plugin, at))
        });
        if let Some((plugin, at)) = held {
            if let Some(control) = plugin.controls.get_mut(at) {
                control.read(&value);
            }
            let added_off = added.iter().any(|added| added.off && added.id == plugin.id);
            if added_off && moved_and_changed(&key, &value) && !flagged.contains(&plugin.id) {
                plugin.on = plugin.problem.is_none();
            }
        } else if moved(&key) {
            unheld.retain(|(kept, _)| *kept != key);
            unheld.push((key, value));
        }
    }

    /// Words for someone who changed a setting that now comes as a plugin, once: it is written down
    /// that they were told. Their value stays where it was kept, for the plugin to pick up.
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
                "{name} now comes as a plugin. Your setting is kept: import the plugin and it picks it up."
            ));
        }
        let names = rest.iter().map(|moved| moved.name).collect::<Vec<_>>().join(", ");
        let name = last.name;
        Some(format!(
            "{names} and {name} now come as plugins. Your settings are kept: import each plugin and it picks them up."
        ))
    }

    /// Sets the switch kept under `key`; false if there is none.
    pub fn set_switch(&self, key: String, on: bool) -> bool {
        self.change(&key, |control| match control {
            PluginControl::Switch { control } => {
                control.on = on;
                Some(())
            }
            _ => None,
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

    /// The file control kept under `key` takes the sound `wav` (a WAV file's bytes) picked by `name`:
    /// kept with its plugin (a minute of it at most, in one channel) for its plugin to play. False
    /// if there is no such control, the plugins can't be kept, or `wav` is no sound this reads.
    pub fn set_sound_file(&self, key: String, name: String, wav: Vec<u8>) -> bool {
        let Some(store) = self.store.as_deref() else { return false };
        let Some((samples, rate)) = crate::sound_file::read_wav(&wav) else { return false };
        let name: String = name.chars().filter(|c| !c.is_control()).take(80).collect();
        let name = if name.trim().is_empty() { "Recording".to_owned() } else { name.trim().to_owned() };
        let mut list = self.listed();
        let Some((plugin, at)) = list.control_mut(&key) else { return false };
        let id = plugin.id.clone();
        let Some(PluginControl::File { control }) = plugin.controls.get_mut(at) else { return false };
        let (file, named) = plugin_file::chosen_paths(store, &id, &key);
        let kept_wav = crate::sound_file::write_wav(&samples, rate);
        let written =
            |tmp: &std::path::Path| std::fs::write(tmp, &kept_wav).map_err(|e| crate::project::file_error("saving", e));
        let kept = file.parent().is_some_and(|folder| std::fs::create_dir_all(folder).is_ok())
            && crate::project::write_atomically(&file, written).is_ok()
            && std::fs::write(&named, &name).is_ok();
        if !kept {
            log::warn!("a sound for {key} couldn't be kept");
            return false;
        }
        control.chosen = Some(name);
        self.hooks.control(&id, at, 1.0);
        self.hooks.give_file(&id, at, Some(&kept_wav));
        true
    }

    /// The file control kept under `key` lets its file go. False if there is no such control.
    pub fn clear_file(&self, key: String) -> bool {
        let mut list = self.listed();
        let Some((plugin, at)) = list.control_mut(&key) else { return false };
        let id = plugin.id.clone();
        let Some(PluginControl::File { control }) = plugin.controls.get_mut(at) else { return false };
        if let Some(store) = self.store.as_deref() {
            let (file, named) = plugin_file::chosen_paths(store, &id, &key);
            let _ = std::fs::remove_file(&file);
            let _ = std::fs::remove_file(&named);
        }
        control.chosen = None;
        self.hooks.control(&id, at, 0.0);
        self.hooks.give_file(&id, at, None);
        true
    }

    /// Runs the action `key` (`<id>.<key>`) of a plugin that is on, on the artwork `session`: what
    /// it changes is one step undo takes back. It runs in a copy of the plugin of its own, told its
    /// controls, so its hooks go on meanwhile. A long call, not for the main thread. False if no
    /// plugin on has it, or the plugin stopped (it goes off, and says why).
    pub fn run_action(&self, key: String, session: Arc<crate::CanvasSession>) -> bool {
        let found = {
            let list = self.listed();
            list.files.iter().filter(|p| p.on && p.problem.is_none()).find_map(|p| {
                let index = p.actions.iter().position(|a| a.key == key)?;
                let values: Vec<f32> = p.controls.iter().map(PluginControl::number).collect();
                Some((p.id.clone(), p.version.clone(), index, values, p.permissions.clone()))
            })
        };
        let Some((id, version, index, values, permissions)) = found else { return false };
        // another action closes a panel open
        *self.open_panel() = None;
        let code = self.store.as_deref().and_then(|store| plugin_file::read_code(store, &id, &version));
        let started = code
            .ok_or(Stop::Damaged)
            .and_then(|(hooks, bytes)| Running::start_on(&bytes, &values, &hooks, &permissions));
        let ran = started.and_then(|mut running| {
            let step = session.plugin_step();
            let ran = running.action(index, Arc::clone(&session));
            drop(step);
            ran.map(|()| running)
        });
        match ran {
            Ok(mut running) => {
                // a panel it showed stays open, with this copy of its code
                if let (Some(shows), false) = running.take_panel() {
                    let running = Arc::new(Mutex::new(running));
                    *self.open_panel() = Some(OpenPanel { id, shows, running });
                }
                true
            }
            Err(stop) => {
                // (the list takes it off when next read)
                self.hooks.stopped(&id, stop);
                false
            }
        }
    }

    /// The kinds of artwork the plugins on make, for the gallery's +.
    pub fn kinds(&self) -> Vec<crate::PluginKind> {
        let list = self.listed();
        let on = list.files.iter().filter(|p| p.on && p.problem.is_none());
        on.flat_map(|p| p.kinds.iter().cloned()).collect()
    }

    /// An artwork of kind `kind` (`<id>.<key>`) opened, in a copy of its plugin's code of its own:
    /// a new one `width` × `height`, or the one its saved `data` holds. None if no plugin on makes
    /// that kind, or it couldn't (one that stops goes off, and says why).
    pub fn open_canvas(
        &self,
        kind: String,
        width: u32,
        height: u32,
        data: Option<Vec<u8>>,
    ) -> Option<Arc<crate::PluginCanvas>> {
        let found = {
            let list = self.listed();
            list.files.iter().filter(|p| p.on && p.problem.is_none()).find_map(|p| {
                p.kinds.iter().any(|k| k.key == kind).then(|| {
                    let values: Vec<f32> = p.controls.iter().map(PluginControl::number).collect();
                    (p.id.clone(), p.version.clone(), values, p.permissions.clone())
                })
            })
        };
        let (id, version, values, permissions) = found?;
        let code = self.store.as_deref().and_then(|store| plugin_file::read_code(store, &id, &version));
        let started = code
            .ok_or(Stop::Damaged)
            .and_then(|(hooks, bytes)| Running::start_on(&bytes, &values, &hooks, &permissions));
        let running = match started {
            Ok(running) => running,
            Err(stop) => {
                self.hooks.stopped(&id, stop);
                return None;
            }
        };
        match crate::PluginCanvas::open(id.clone(), Arc::clone(&self.hooks), running, width, height, data.as_deref()) {
            Ok(canvas) => {
                let canvas = Arc::new(canvas);
                // told its plugin's controls as they change
                self.hooks.keep_canvas(&id, &canvas);
                Some(canvas)
            }
            Err(stop) => {
                if let Some(stop) = stop {
                    self.hooks.stopped(&id, stop);
                }
                None
            }
        }
    }

    /// The panel a plugin's action showed, while it is open.
    pub fn panel(&self) -> Option<crate::PluginPanel> {
        self.open_panel().as_ref().map(|open| open.shows.clone())
    }

    /// Row `row` of the panel open touched with `value` (a slider's, a switch's 1 or 0, a choice's
    /// place, 1 for a button), on the artwork `session`: the plugin is told, and what it changes is
    /// one step undo takes back. It may show the panel changed, or close it. A long call, not for the
    /// main thread. False once the panel is closed (by the plugin, or as it stopped).
    pub fn panel_touched(&self, row: u32, value: f32, session: Arc<crate::CanvasSession>) -> bool {
        let row = row as usize;
        // the touch shows at once, and the code is fetched to be told
        let (id, running, value) = {
            let mut open = self.open_panel();
            let Some(panel) = open.as_mut() else { return false };
            let Some(value) = crate::plugin_panel::touched(&panel.shows, row, value) else { return true };
            panel.shows = crate::plugin_panel::with_value(panel.shows.clone(), row, value);
            (panel.id.clone(), Arc::clone(&panel.running), value)
        };
        let mut code = running.lock().unwrap_or_else(|e| e.into_inner());
        let step = session.plugin_step();
        let ran = code.panel_event(row, value, Arc::clone(&session));
        drop(step);
        let answer = ran.map(|()| code.take_panel());
        drop(code);
        // what it answered, unless that panel was closed meanwhile (or another took its place)
        let mut open = self.open_panel();
        let same = open.as_ref().is_some_and(|panel| Arc::ptr_eq(&panel.running, &running));
        match answer {
            Ok((shows, false)) => {
                if let (true, Some(shows), Some(panel)) = (same, shows, open.as_mut()) {
                    panel.shows = shows;
                }
                same
            }
            Ok((_, true)) => {
                if same {
                    *open = None;
                }
                false
            }
            Err(stop) => {
                if same {
                    *open = None;
                }
                drop(open);
                self.hooks.stopped(&id, stop);
                false
            }
        }
    }

    /// The panel open closed (its close button, or a tap away from it).
    pub fn close_panel(&self) {
        *self.open_panel() = None;
    }

    /// Turns a plugin on or off; false if no file is `id`. It goes on its hooks or off them at once,
    /// no restart: off them the app is as it was, and its values wait for it. One that can't start
    /// stays off, and says why.
    pub fn set_on(&self, id: String, on: bool) -> bool {
        let mut list = self.listed();
        let Some(plugin) = list.files.iter_mut().find(|p| p.id == id) else { return false };
        plugin.on = on;
        plugin.problem = None;
        if let Some(store) = &self.store {
            plugin_file::clear_stopped(store, &id);
        }
        self.take_off(&id);
        self.start(plugin);
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
        self.take_off(&id);
        let gone = list.files.remove(at);
        list.flagged.remove(&id);
        // one the app added stays written down, so it isn't added again; imported again, it is as
        // any import
        if list.added_off(&id)
            && let Some(store) = &self.store
        {
            let mut added = list.added.clone();
            added.iter_mut().filter(|added| added.id == id).for_each(|added| added.off = false);
            if plugin_file::note_added(store, &added) {
                list.added = added;
            }
        }
        let keys = gone.controls.iter().map(|c| c.key().to_owned());
        Some(keys.chain([gone.on_key.clone()]).collect())
    }

    /// How big the interface shows in a window `width` × `height` points at 1: as a plugin says, or
    /// as measured.
    pub fn interface_scale(&self, width: f32, height: f32) -> f32 {
        self.hooks().interface_scale(width, height)
    }

    /// Whether a plugin draws in-betweens: Animation Assist's frame options then offer In-between
    /// ([`crate::CanvasSession::add_inbetween`]).
    pub fn tweens(&self) -> bool {
        self.hooks().tweens()
    }

    /// Whether a plugin makes the sound of the stroke being drawn: the app plays
    /// [`crate::CanvasSession::stroke_sound`] while one does.
    pub fn makes_stroke_sound(&self) -> bool {
        self.hooks().makes_stroke_sound()
    }

    /// Whether a plugin on draws the colour panel's Classic square its own way: its picture
    /// ([`crate::CanvasSession::picker_picture`]) and where touches land on it.
    pub fn picker_on(&self) -> bool {
        self.hooks().picker().is_some()
    }

    /// The saturation and brightness the picker plugin says point (`x`, `y`) of the Classic square
    /// picks (0..1, y down): `[s, v]`, or none with no picker plugin on.
    pub fn picker_pick(&self, x: f32, y: f32) -> Option<Vec<f32>> {
        self.hooks().picker_pick(x, y).map(|(s, v)| vec![s, v])
    }

    /// Where the picker plugin says saturation `s` and brightness `v` sit on the Classic square
    /// (0..1, y down): `[x, y]`, or none with no picker plugin on.
    pub fn picker_point(&self, s: f32, v: f32) -> Option<Vec<f32>> {
        self.hooks().picker_point(s, v).map(|(x, y)| vec![x, y])
    }

    /// Whether a plugin has the colour panel's Classic show a triangle (saturation and brightness
    /// between the pure hue, white and black) in place of its square.
    pub fn colour_triangle(&self) -> bool {
        self.hooks().colour_triangle()
    }

    /// Whether a plugin sets brushes' settings at random: the brush studio shows Randomise.
    pub fn randomises_brushes(&self) -> bool {
        self.hooks().randomises_brushes()
    }

    /// The accent (0xRRGGBB), the blue of what is chosen and switched on, if a plugin gives the
    /// interface another; none: as measured.
    pub fn accent_colour(&self) -> Option<u32> {
        self.hooks().interface_colour(0)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::plugin_file::tests::{SHADER, coded, filter, pack};
    use crate::plugin_run::tests::position_module;

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
            names: Vec::new(),
            live: false,
        }
    }

    /// A slider of whole pixels, from Off.
    fn gap_slider() -> PluginSlider {
        PluginSlider {
            key: "gap".to_owned(),
            label: "Gap".to_owned(),
            least: 0.0,
            most: 32.0,
            step: 1.0,
            value: 0.0,
            starts: 0.0,
            shown_times: 1.0,
            unit: " px".to_owned(),
            at_least: Some("Off".to_owned()),
            names: Vec::new(),
            live: false,
        }
    }

    /// The filters imported.
    fn filters(plugins: &Plugins) -> Vec<PluginInfo> {
        plugins.plugins().into_iter().filter(|p| p.holds.contains(&PluginPart::Filter)).collect()
    }

    /// One from elsewhere: nothing in the app asks about it.
    fn stranger() -> PluginInfo {
        PluginInfo {
            brush_controls: Vec::new(),
            actions: Vec::new(),
            kinds: Vec::new(),
            permissions: Vec::new(),
            id: "someone.stranger".to_owned(),
            name: "Stranger".to_owned(),
            description: "No code answers to it.".to_owned(),
            source: PluginSource::File,
            holds: vec![PluginPart::Hooks],
            version: "1.0.0".to_owned(),
            publisher: "Someone".to_owned(),
            experimental: false,
            on: true,
            on_key: "someone.stranger.on".to_owned(),
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
                        names: Vec::new(),
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
    fn nothing_is_in_the_app_until_a_file_is_imported() {
        let plugins = Plugins::new();
        assert!(plugins.plugins().is_empty());
        assert_eq!(plugins.interface_scale(1600.0, 1000.0), 1.0);
        assert!(plugins.kept_lines().is_empty());
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
        plugins.listed().files.push(stranger());
        assert_eq!(plugins.set_slider("someone.stranger.slider".to_owned(), 7.0), Some(7.0));
        let lines = plugins.kept_lines();
        assert!(lines.contains(&"someone.stranger.slider 7".to_owned()), "{lines:?}");
        let again = Plugins::new();
        again.listed().files.push(stranger());
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
    fn a_slider_can_read_as_a_word_at_its_least() {
        let gap = gap_slider();
        assert_eq!(plugin_slider_shown(gap.clone(), 0.0), "Off");
        assert_eq!(plugin_slider_shown(gap.clone(), 1.0), "1 px");
        assert_eq!(plugin_slider_shown(gap.clone(), 32.0), "32 px");
        assert_eq!(plugin_slider_at(gap.clone(), 0.5), 16.0);
        assert_eq!(plugin_slider_along(gap, 8.0), 0.25);
    }

    #[test]
    fn a_slider_can_name_each_step() {
        let names = ["By brush", "Eraser", "Pencil"].map(str::to_owned).to_vec();
        let sound = PluginSlider { least: 0.0, most: 2.0, step: 1.0, at_least: None, names, ..gap_slider() };
        assert_eq!(plugin_slider_shown(sound.clone(), 0.0), "By brush");
        assert_eq!(plugin_slider_shown(sound.clone(), 1.2), "Eraser");
        assert_eq!(plugin_slider_shown(sound, 2.0), "Pencil");
    }

    #[test]
    fn a_count_is_kept_as_a_whole_number() {
        assert!(plugin_slider_whole(gap_slider()));
        assert!(!plugin_slider_whole(size_slider()));
    }

    #[test]
    fn a_key_nothing_has_is_left_alone() {
        let plugins = Plugins::new();
        plugins.listed().files.push(stranger());
        let before = plugins.plugins();
        assert!(!plugins.set_switch("nobody".to_owned(), true));
        assert_eq!(plugins.set_slider("nobody".to_owned(), 1.0), None);
        plugins.read_kept("nobody".to_owned(), "true".to_owned());
        plugins.read_kept("nobody.on".to_owned(), "false".to_owned());
        assert!(!plugins.set_on("nobody".to_owned(), true));
        assert_eq!(plugins.remove("nobody".to_owned()), None);
        // nor does a value of the wrong kind reach a control
        assert!(!plugins.set_switch("someone.stranger.slider".to_owned(), true));
        assert_eq!(plugins.set_slider("someone.stranger.slider".to_owned(), f32::NAN), None);
        assert_eq!(plugins.plugins(), before);
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
            names: Vec::new(),
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
        assert_eq!(plugins.plugins().len(), 1);
        // its values change and are kept like any other's, and touch nothing else
        assert!(plugins.set_switch("someone.stranger.switch".to_owned(), true));
        assert_eq!(plugins.set_slider("someone.stranger.slider".to_owned(), 3.3), Some(3.5));
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
        assert!(card.controls.is_empty(), "its own are on its panel");
        assert_eq!(card.on_key, "org.example.filter.on");
        assert!(dir_holds(&dir, "org.example.filter/1.0.0/filter.wgsl"));
        // through a restart
        let next = at(&dir);
        assert_eq!(filters(&next).len(), 1);
        for line in plugins.kept_lines() {
            let (key, value) = line.split_once(' ').unwrap();
            next.read_kept(key.to_owned(), value.to_owned());
        }
        assert_eq!(filters(&next), filters(&plugins));
        // removed: gone at once, no restart, and from the disk
        let keys = next.remove("org.example.filter".to_owned()).unwrap();
        assert!(keys.contains(&"org.example.filter.on".to_owned()));
        assert!(filters(&next).is_empty());
        assert!(!dir_holds(&dir, "org.example.filter"));
        assert!(filters(&at(&dir)).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file of a plugin with code and the slider `key`, at `version`.
    fn slider_file(id: &str, key: &str, version: &str) -> Vec<u8> {
        let module = position_module(r#"(func (export "procraft_control") (param i32 f32))"#, "(i64.const 0)");
        let mut manifest = coded(id, &module);
        manifest["version"] = json!(version);
        manifest["controls"] = json!([{
            "key": key, "type": "slider", "label": "Cut-off", "least": 0, "most": 20, "step": 0.25, "starts": 3
        }]);
        pack(&manifest, &[(crate::plugin_run::MODULE, &module)])
    }

    #[test]
    fn a_plugin_can_t_reach_a_moved_value_without_saying_so() {
        let dir = store("collide");
        let plugins = at(&dir);
        plugins.read_kept("fill_gap".to_owned(), "11".to_owned());
        assert_eq!(plugins.install(slider_file("org.example.plugin", "fill_gap", "1.0.0")), None);
        let key = "org.example.plugin.fill_gap";
        let value = |plugins: &Plugins| match plugins.control(key) {
            Some(PluginControl::Slider { control }) => control.value,
            other => panic!("{other:?}"),
        };
        assert_eq!(value(&plugins), 3.0, "its own value is its own");
        assert!(plugins.kept_lines().contains(&"fill_gap 11".to_owned()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_filter_is_enabled_and_disabled_and_it_is_kept() {
        let dir = store("enabled");
        let plugins = at(&dir);
        plugins.install(filter_file(&filter("org.example.filter")));
        assert!(plugins.set_on("org.example.filter".to_owned(), false));
        assert!(!filters(&plugins)[0].on);
        assert!(plugins.kept_lines().contains(&"org.example.filter.on false".to_owned()));
        let next = at(&dir);
        assert!(filters(&next)[0].on, "as imported");
        next.read_kept("org.example.filter.on".to_owned(), "false".to_owned());
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
    fn the_offer_of_a_plugin_says_what_it_can_and_can_t_do() {
        let module = position_module("", "(i64.const 0)");
        let mut manifest = coded("org.example.plugin", &module);
        manifest["name"] = json!("Offset");
        let plugins = at(&store("plugin-offer"));
        let offer = plugins.inspect(pack(&manifest, &[(crate::plugin_run::MODULE, &module)]));
        assert_eq!(offer.refused, None);
        assert_eq!(offer.name, "Offset");
        assert!(offer.message.contains("Someone (not checked)"), "{offer:?}");
        assert!(offer.message.contains("where your strokes land"), "{offer:?}");
        assert!(offer.message.contains("can't reach your files"), "{offer:?}");
        assert!(plugins.plugins().is_empty());
    }

    /// A plugin taking over the moved eyedropper setting, as a file.
    fn eyedropper_file(id: &str) -> Vec<u8> {
        let text = r#"(module (func (export "procraft_control") (param i32 f32))
            (func (export "procraft_pick_layer") (result i32) (i32.const 0)))"#;
        let module = wat::parse_str(text).unwrap();
        let mut manifest = coded(id, &module);
        manifest["hooks"] = json!(["pick_layer"]);
        manifest["controls"] = json!([
            { "key": "layer", "type": "switch", "label": "Layer", "keeps": "eyedropper_layer" },
        ]);
        pack(&manifest, &[(crate::plugin_run::MODULE, &module)])
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
        assert!(said.starts_with("Eyedropper picks from layer now comes as a plugin."), "{said}");
        // once, in this run and the next
        assert_eq!(plugins.take_moved_notice(), None);
        let next = at(&dir);
        next.read_kept("eyedropper_layer".to_owned(), "true".to_owned());
        assert_eq!(next.take_moved_notice(), None);
        // nothing is installed for them, and the value goes on being kept
        assert!(next.plugins().is_empty());
        assert!(next.kept_lines().contains(&"eyedropper_layer true".to_owned()));
        // the plugin picks it up on import, and keeps it where it was, through a restart
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
    fn a_moved_setting_with_its_plugin_here_says_nothing() {
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
    fn a_plugin_with_code_and_a_filter_share_one_list() {
        let dir = store("pages");
        let plugins = at(&dir);
        let module = position_module("", "(i64.const 0)");
        let file = pack(&coded("org.example.plugin", &module), &[(crate::plugin_run::MODULE, &module)]);
        assert_eq!(plugins.install(file), None);
        assert_eq!(plugins.install(filter_file(&filter("org.example.filter"))), None);
        let cards = plugins.plugins();
        assert_eq!(cards.len(), 2);
        assert!(cards[0].holds == [PluginPart::Hooks] && cards[0].source == PluginSource::File);
        assert!(!cards[0].on && cards[0].problem.is_none(), "it starts off: {:?}", cards[0]);
        assert_eq!(cards[0].on_key, "org.example.plugin.on");
        assert!(cards[1].holds == [PluginPart::Filter] && cards[1].on, "it only adds, so it starts on");
        assert_eq!(filters(&plugins).len(), 1);
        // and through a restart
        let next = at(&dir);
        assert_eq!((next.plugins().len(), filters(&next).len()), (2, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    const BOTH: &str = "org.example.both";

    /// A file holding a filter and the code `answer` for every sample, with the filter's controls.
    fn both_file(answer: &str) -> Vec<u8> {
        let module = position_module(r#"(func (export "procraft_control") (param i32 f32))"#, answer);
        let mut manifest = filter(BOTH);
        manifest["holds"] = json!(["filter", "hooks"]);
        manifest["hooks"] = json!(["position"]);
        let sha256 = crate::plugin_file::sha256;
        manifest["files"] = json!([
            { "path": "filter.wgsl", "sha256": sha256(SHADER), "size": SHADER.len() },
            { "path": crate::plugin_run::MODULE, "sha256": sha256(&module), "size": module.len() },
        ]);
        pack(&manifest, &[("filter.wgsl", SHADER), (crate::plugin_run::MODULE, &module)])
    }

    /// Where a sample ends up: (5, 6) with no code on it, the corner with.
    const UNTOUCHED: (f32, f32) = (5.0, 6.0);
    const CORNER: (f32, f32) = (0.0, 0.0);

    fn landing(plugins: &Plugins) -> (f32, f32) {
        plugins.hooks().position(UNTOUCHED.0, UNTOUCHED.1)
    }

    #[test]
    fn a_plugin_holding_a_filter_and_code_is_one_card_with_one_on_off() {
        let dir = store("both");
        let plugins = at(&dir);
        assert_eq!(plugins.install(both_file("(i64.const 0)")), None);
        let cards = plugins.plugins();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].holds, [PluginPart::Filter, PluginPart::Hooks]);
        assert_eq!(cards[0].controls.len(), 2, "its code has controls, so they are on its card");
        // as anything that changes what is there, it starts off, both parts
        assert!(!cards[0].on);
        assert!(plugins.filters().is_empty());
        assert_eq!(landing(&plugins), UNTOUCHED);
        // on, both are
        assert!(plugins.set_on(BOTH.to_owned(), true));
        assert_eq!(plugins.filters().len(), 1);
        assert_eq!(landing(&plugins), CORNER);
        // what is set on the card is its code's: the filter's panel opens as it starts
        assert_eq!(plugins.set_slider(format!("{BOTH}.cut"), 0.25), Some(0.25));
        let opened = plugins.filter_to_open(BOTH).map_err(|why| why.to_string()).map(|open| open.controls);
        assert_eq!(opened.unwrap()[0].number(), 0.5);
        // through a restart, with its flag kept
        let lines = plugins.kept_lines();
        let next = at(&dir);
        for line in &lines {
            let (key, value) = line.split_once(' ').unwrap();
            next.read_kept(key.to_owned(), value.to_owned());
        }
        assert_eq!(next.filters().len(), 1);
        assert_eq!(landing(&next), CORNER);
        // off, neither is, and the app is as it was
        assert!(plugins.set_on(BOTH.to_owned(), false));
        assert!(plugins.filters().is_empty());
        assert_eq!(landing(&plugins), UNTOUCHED);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plugin_is_stopped_whole_whichever_part_stopped() {
        let dir = store("both-stops");
        let plugins = at(&dir);
        assert_eq!(plugins.install(both_file("(i64.const 0)")), None);
        assert!(plugins.set_on(BOTH.to_owned(), true));
        assert_eq!(landing(&plugins), CORNER);
        // its filter stops the driver: its code doesn't run on regardless
        plugins.stop_filter(BOTH, STOPPED_DRIVER);
        assert_eq!(landing(&plugins), UNTOUCHED);
        assert!(plugins.filters().is_empty());
        let card = plugins.plugins().remove(0);
        assert_eq!((card.on, card.problem.as_deref()), (false, Some(STOPPED_DRIVER)));
        // nor through a restart, whatever its flag says
        let next = at(&dir);
        next.read_kept(card.on_key.clone(), "true".to_owned());
        assert_eq!(landing(&next), UNTOUCHED);
        assert!(next.filters().is_empty());
        // asked to, both come back
        assert!(plugins.set_on(BOTH.to_owned(), true));
        assert_eq!(landing(&plugins), CORNER);
        assert_eq!(plugins.filters().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        // its code stops on a sample, an answer that isn't a number: its filter goes with it
        let dir = store("both-stops-code");
        let plugins = at(&dir);
        assert_eq!(plugins.install(both_file("(i64.const 0x7fc00000)")), None);
        assert!(plugins.set_on(BOTH.to_owned(), true));
        assert_eq!(plugins.filters().len(), 1);
        assert_eq!(landing(&plugins), UNTOUCHED, "what it was handed goes on as it came");
        assert!(plugins.filters().is_empty());
        assert!(!plugins.plugins()[0].on);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_offer_of_both_says_all_it_can_do_and_no_more() {
        let plugins = at(&store("both-offer"));
        let offer = plugins.inspect(both_file("(i64.const 0)"));
        assert_eq!(offer.refused, None);
        assert!(offer.message.contains("adds a filter to Adjustments"), "{offer:?}");
        assert!(offer.message.contains("where your strokes land"), "{offer:?}");
        assert!(offer.message.contains("can't reach your files or the internet"), "{offer:?}");
        assert!(!offer.message.contains("your layers"), "its filter changes one: {offer:?}");
    }

    /// The files in the folder `PROCRAFT_PLUGIN_DIR`, as the packer leaves them: each is taken, and
    /// turns on and off. `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn packed_files_are_taken_and_turn_on_and_off() {
        let Ok(folder) = std::env::var("PROCRAFT_PLUGIN_DIR") else { return };
        let dir = store("packed");
        let plugins = at(&dir);
        let mut files = 0;
        for file in std::fs::read_dir(folder).unwrap() {
            let bytes = std::fs::read(file.unwrap().path()).unwrap();
            let offer = plugins.inspect(bytes.clone());
            println!("{}: {}", offer.name, offer.message.replace('\n', " | "));
            assert_eq!(offer.refused, None);
            assert_eq!(plugins.install(bytes), None, "{}", offer.name);
            files += 1;
        }
        let cards = plugins.plugins();
        assert_eq!(cards.len(), files);
        for card in cards {
            assert!(plugins.set_on(card.id.clone(), true));
            let on = plugins.plugins().into_iter().find(|p| p.id == card.id).unwrap();
            assert!(on.on && on.problem.is_none(), "{on:?}");
            assert!(plugins.set_on(card.id.clone(), false));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_update_keeps_what_was_set_and_an_older_file_is_refused() {
        let dir = store("update");
        let plugins = at(&dir);
        let id = "org.example.plugin";
        assert_eq!(plugins.install(slider_file(id, "cut", "1.2.0")), None);
        plugins.set_slider("org.example.plugin.cut".to_owned(), 0.25);
        // older: refused, on the screen and if asked anyway
        let older = slider_file(id, "cut", "1.0.0");
        let said = Some("A newer version of this plugin is already imported.".to_owned());
        assert_eq!(plugins.inspect(older.clone()).refused, said);
        assert_eq!(plugins.install(older), said);
        // newer: takes over, what was set stays
        assert_eq!(plugins.install(slider_file(id, "cut", "1.3.0")), None);
        assert_eq!(plugins.plugins()[0].version, "1.3.0");
        let cut = plugins.control("org.example.plugin.cut");
        assert!(matches!(cut, Some(PluginControl::Slider { control }) if (control.value - 0.25).abs() < 1e-6));
        assert_eq!(at(&dir).plugins().len(), 1);
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

    const POSTERISE: &str = "io.github.artcraftandroid.posterise";
    const GAP: &str = "io.github.artcraftandroid.gap-closing";
    const SIZE: &str = "io.github.artcraftandroid.interface-size";
    const EYEDROPPER: &str = "io.github.artcraftandroid.eyedropper-layer";
    const PASTE: &str = "io.github.artcraftandroid.paste-into-masks";
    const TOUCH: &str = "io.github.artcraftandroid.touch-offset";
    const COLOUR: &str = "io.github.artcraftandroid.interface-colour";

    /// The app's plugins kept in `dir`, what it comes with added.
    fn app_at(dir: &str) -> Arc<Plugins> {
        Plugins::with_bundled(dir.to_owned())
    }

    /// The app's plugins kept in `dir`, as an app that comes with `files` adds them.
    fn app_with(dir: &str, files: &[(&str, Vec<u8>)]) -> Arc<Plugins> {
        let plugins = Plugins::kept_in(Some(PathBuf::from(dir)));
        let files: Vec<(&str, &[u8])> = files.iter().map(|(name, bytes)| (*name, bytes.as_slice())).collect();
        plugins.add_bundled(&files);
        plugins
    }

    /// The file of `name` that the app comes with, said to be `version`.
    fn bundled_file(name: &str, version: &str) -> Vec<u8> {
        let (_, bytes) = crate::bundled::BUNDLED.iter().find(|(n, _)| *n == name).unwrap();
        let package = plugin_file::read_package(bytes).unwrap();
        let mut manifest: Value = serde_json::from_slice(package.manifest_text()).unwrap();
        manifest["version"] = json!(version);
        let inside = [plugin_file::FILTER_CODE, crate::plugin_run::MODULE];
        let files: Vec<(&str, &[u8])> =
            inside.into_iter().filter_map(|path| Some((path, package.file(path)?))).collect();
        pack(&manifest, &files)
    }

    /// Every file the app comes with, as it comes.
    fn all_bundled() -> Vec<(&'static str, Vec<u8>)> {
        crate::bundled::BUNDLED.iter().map(|(name, bytes)| (*name, bytes.to_vec())).collect()
    }

    fn card(plugins: &Plugins, id: &str) -> PluginInfo {
        plugins.plugins().into_iter().find(|p| p.id == id).unwrap()
    }

    fn has(plugins: &Plugins, id: &str) -> bool {
        plugins.plugins().iter().any(|p| p.id == id)
    }

    fn slider_value(plugins: &Plugins, key: &str) -> f32 {
        match plugins.control(key) {
            Some(PluginControl::Slider { control }) => control.value,
            other => panic!("{other:?}"),
        }
    }

    fn read_lines(plugins: &Plugins, lines: &[String]) {
        for line in lines {
            let (key, value) = line.split_once(' ').unwrap();
            plugins.read_kept(key.to_owned(), value.to_owned());
        }
    }

    #[test]
    fn the_first_start_adds_what_the_app_comes_with_as_files_all_off() {
        let dir = store("bundled");
        let plugins = app_at(&dir);
        let cards = plugins.plugins();
        assert_eq!(cards.len(), crate::bundled::BUNDLED.len());
        for card in &cards {
            // as an import: a card like any other, kept in the store
            assert_eq!(card.source, PluginSource::File, "{card:?}");
            assert!(!card.on && card.problem.is_none(), "{card:?}");
            assert!(dir_holds(&dir, &format!("{}/{}", card.id, card.version)), "{}", card.id);
        }
        // off, the app is as it was
        assert!(plugins.filters().is_empty());
        assert_eq!(plugins.interface_scale(1600.0, 1000.0), 1.0);
        assert_eq!(landing(&plugins), UNTOUCHED);
        assert!(!plugins.hooks().pick_layer());
        assert!(!plugins.hooks().paste_mask() && !plugins.hooks().fill_mask());
        assert_eq!(plugins.take_moved_notice(), None);
        // and so on the next start, the filters too, with nothing kept for them
        let next = app_at(&dir);
        assert_eq!(next.plugins(), cards);
        assert!(next.filters().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_artwork_of_a_plugin_s_kind_takes_touches_draws_saves_and_loads() {
        let dir = store("canvas-kind");
        let plugins = Plugins::with_store(dir.clone());
        let id = "org.example.dots";
        let file = crate::plugin_file::tests::dots_file(id, serde_json::json!([{ "key": "dots", "label": "Dots" }]));
        assert_eq!(plugins.install(file), None);
        assert!(plugins.kinds().is_empty(), "off until turned on");
        assert!(plugins.set_on(id.to_owned(), true));
        let kind = format!("{id}.dots");
        assert_eq!(plugins.kinds(), [crate::PluginKind { key: kind.clone(), label: "Dots".to_owned() }]);
        let canvas = plugins.open_canvas(kind.clone(), 40, 30, None).expect("opened");
        assert_eq!((canvas.width(), canvas.height()), (40, 30));
        let red_at = |c: &crate::PluginCanvas, x: usize, y: usize| {
            let rgba = c.picture();
            rgba[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4] == [255, 0, 0, 255]
        };
        assert!(!red_at(&canvas, 20, 15), "nothing yet");
        assert!(canvas.touch(0, 20.0, 15.0, 1.0));
        assert!(red_at(&canvas, 20, 15) && !red_at(&canvas, 2, 2));
        // what it saved goes back, as undo does
        let saved = canvas.save().expect("saved");
        assert_eq!(saved.len(), 8);
        assert!(canvas.touch(0, 6.0, 6.0, 1.0));
        assert!(red_at(&canvas, 6, 6) && !red_at(&canvas, 20, 15));
        assert!(canvas.load(saved.clone()));
        assert!(red_at(&canvas, 20, 15) && !red_at(&canvas, 6, 6));
        // and opens as it was
        let again = plugins.open_canvas(kind.clone(), 40, 30, Some(saved)).expect("reopened");
        assert!(red_at(&again, 20, 15));
        // a kind no plugin on makes opens nothing
        assert!(plugins.open_canvas(format!("{id}.other"), 40, 30, None).is_none());
        assert!(plugins.set_on(id.to_owned(), false));
        assert!(plugins.open_canvas(kind, 40, 30, None).is_none());
        assert!(plugins.kinds().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plugin_whose_artwork_crashes_goes_off_and_says_why() {
        let dir = store("canvas-crash");
        let plugins = Plugins::with_store(dir.clone());
        let id = "org.example.dots";
        let touch = "(f32.store (i32.const 0) (local.get 1)) (f32.store (i32.const 4) (local.get 2)) (i32.const 1))";
        let text = crate::plugin_run::tests::DOTS.replace(touch, "(unreachable))");
        assert_ne!(text, crate::plugin_run::tests::DOTS);
        let module = wat::parse_str(text).unwrap();
        let mut manifest = crate::plugin_file::tests::coded(id, &module);
        manifest["compat"]["procraft"]["interface"] = serde_json::json!(2);
        manifest["hooks"] = serde_json::json!([]);
        manifest["kinds"] = serde_json::json!([{ "key": "dots", "label": "Dots" }]);
        let file = crate::plugin_file::tests::pack(&manifest, &[(crate::plugin_run::MODULE, &module)]);
        assert_eq!(plugins.install(file), None);
        assert!(plugins.set_on(id.to_owned(), true));
        let canvas = plugins.open_canvas(format!("{id}.dots"), 40, 30, None).unwrap();
        assert!(!canvas.touch(0, 5.0, 5.0, 1.0));
        assert_eq!(canvas.problem().as_deref(), Some("Switched off: it crashed."));
        let card = plugins.plugins().into_iter().find(|p| p.id == id).unwrap();
        assert!(!card.on);
        assert_eq!(card.problem.as_deref(), Some("Switched off: it crashed."));
        // and it takes nothing more
        assert!(!canvas.touch(0, 6.0, 6.0, 1.0));
        assert!(canvas.save().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn each_the_app_added_runs_as_any_imported() {
        let dir = store("bundled-run");
        let plugins = app_at(&dir);
        for card in plugins.plugins() {
            assert!(plugins.set_on(card.id.clone(), true));
            let on = self::card(&plugins, &card.id);
            assert!(on.on && on.problem.is_none(), "{on:?}");
            if card.holds.contains(&PluginPart::Filter) {
                let opened = plugins.filter_to_open(&card.id).map(|open| open.name);
                assert_eq!(opened.as_deref(), Ok(card.name.as_str()));
            }
            if card.holds.contains(&PluginPart::Hooks) {
                assert!(plugins.hooks().is_on(&card.id), "{}", card.name);
            }
        }
        assert_eq!(plugins.filters().len(), 4);
        // each does what it says, by its controls
        assert_eq!(plugins.set_slider(format!("{TOUCH}.x"), 10.0), Some(10.0));
        assert_eq!(landing(&plugins), (UNTOUCHED.0 + 10.0, UNTOUCHED.1));
        assert!(plugins.set_slider("interface_size".to_owned(), 0.9).is_some());
        assert!((plugins.interface_scale(1600.0, 1000.0) - 0.9).abs() < 1e-6);
        assert!(plugins.set_switch("eyedropper_layer".to_owned(), true));
        assert!(plugins.hooks().pick_layer());
        assert!(plugins.set_switch("paste_into_mask".to_owned(), true));
        assert!(plugins.hooks().paste_mask());
        assert_eq!(plugins.set_slider("fill_gap".to_owned(), 4.0), Some(4.0));
        assert!(plugins.hooks().fill_mask());
        // the accent as it was at the slider's start, then red
        assert_eq!(plugins.accent_colour(), Some(0x0076FD));
        assert!(plugins.set_slider(format!("{COLOUR}.hue"), 0.0).is_some());
        assert_eq!(plugins.accent_colour(), Some(0xFD0000));
        assert!(plugins.set_slider(format!("{COLOUR}.saturation"), 0.0).is_some());
        assert_eq!(plugins.accent_colour(), Some(0xFDFDFD));
        // the triangle draws Classic, and says where a touch lands: its right corner is the pure hue
        assert!(plugins.picker_on());
        let corner = plugins.picker_pick(0.933, 0.5).unwrap();
        assert!(corner.iter().all(|share| (share - 1.0).abs() < 1e-3), "{corner:?}");
        // kept, so through a restart
        let next = app_at(&dir);
        read_lines(&next, &plugins.kept_lines());
        assert_eq!(next.plugins(), plugins.plugins());
        assert_eq!(landing(&next), (UNTOUCHED.0 + 10.0, UNTOUCHED.1));
        assert_eq!(next.filters().len(), 4);
        // and off again, none does
        for card in plugins.plugins() {
            assert!(plugins.set_on(card.id, false));
        }
        assert!(plugins.filters().is_empty());
        assert_eq!(landing(&plugins), UNTOUCHED);
        assert_eq!(plugins.interface_scale(1600.0, 1000.0), 1.0);
        assert_eq!(plugins.accent_colour(), None);
        assert!(!plugins.picker_on());
        assert!(!plugins.hooks().pick_layer() && !plugins.hooks().paste_mask() && !plugins.hooks().fill_mask());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_the_app_added_and_someone_removed_stays_removed() {
        let dir = store("bundled-removed");
        let plugins = app_at(&dir);
        assert!(plugins.set_on(GAP.to_owned(), true));
        assert_eq!(plugins.set_slider("fill_gap".to_owned(), 6.0), Some(6.0));
        assert_eq!(plugins.remove(POSTERISE.to_owned()), Some(vec![format!("{POSTERISE}.on")]));
        let keys = plugins.remove(GAP.to_owned()).unwrap();
        assert!(keys.contains(&"fill_gap".to_owned()) && keys.contains(&format!("{GAP}.on")), "{keys:?}");
        assert!(!has(&plugins, POSTERISE) && !has(&plugins, GAP) && !plugins.hooks().is_on(GAP));
        assert!(!dir_holds(&dir, POSTERISE) && !dir_holds(&dir, GAP));
        // through a restart, and an app that comes with a newer one
        let lines = plugins.kept_lines();
        let next = app_at(&dir);
        read_lines(&next, &lines);
        assert!(!has(&next, POSTERISE) && !has(&next, GAP));
        assert_eq!(next.plugins().len(), crate::bundled::BUNDLED.len() - 2);
        let mut newer = all_bundled();
        newer.retain(|(name, _)| *name != "posterise");
        newer.push(("posterise", bundled_file("posterise", "1.1.0")));
        assert!(!has(&app_with(&dir, &newer), POSTERISE));
        // imported again, it is as any import: a filter starts on, and stays so
        assert_eq!(next.install(bundled_file("posterise", "1.0.0")), None);
        assert!(card(&next, POSTERISE).on);
        assert!(card(&app_at(&dir), POSTERISE).on);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_app_adds_what_it_never_added_and_updates_what_is_there() {
        let dir = store("bundled-newer");
        let mut first = all_bundled();
        first.retain(|(name, _)| *name != "touch-offset");
        let plugins = app_with(&dir, &first);
        assert!(!has(&plugins, TOUCH));
        assert!(plugins.set_on(POSTERISE.to_owned(), true));
        assert!(plugins.set_on(GAP.to_owned(), true));
        assert_eq!(plugins.set_slider("fill_gap".to_owned(), 7.0), Some(7.0));
        let lines = plugins.kept_lines();
        // the next comes with touch offset, and newer ones of these two
        let mut next = all_bundled();
        next.retain(|(name, _)| !["posterise", "gap-closing"].contains(name));
        next.push(("posterise", bundled_file("posterise", "1.1.0")));
        next.push(("gap-closing", bundled_file("gap-closing", "1.0.1")));
        let updated = app_with(&dir, &next);
        read_lines(&updated, &lines);
        assert!(!card(&updated, TOUCH).on);
        let (posterise, gap) = (card(&updated, POSTERISE), card(&updated, GAP));
        assert_eq!((posterise.version.as_str(), posterise.on), ("1.1.0", true));
        assert_eq!((gap.version.as_str(), gap.on), ("1.0.1", true));
        assert_eq!(slider_value(&updated, "fill_gap"), 7.0);
        // one newer than the app's stays as it is
        let older = app_at(&dir);
        assert_eq!(card(&older, POSTERISE).version, "1.1.0");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plugin_imported_before_the_app_came_with_it_is_left_as_it_was() {
        let dir = store("bundled-imported");
        // a filter, so it started on
        assert_eq!(at(&dir).install(bundled_file("posterise", "1.0.0")), None);
        assert_eq!(at(&dir).install(bundled_file("touch-offset", "0.5.0")), None);
        let plugins = app_at(&dir);
        assert!(card(&plugins, POSTERISE).on);
        assert!(card(&app_at(&dir), POSTERISE).on);
        // an older one is updated
        let touch = card(&plugins, TOUCH);
        assert_eq!((touch.version.as_str(), touch.on), ("1.0.0", false));
        assert!(!card(&plugins, "io.github.artcraftandroid.outline").on);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_moved_setting_someone_changed_starts_what_the_app_added_on_with_it() {
        let dir = store("bundled-moved");
        // as an earlier version kept them: the gap and the eyedropper changed, the rest not
        let plugins = app_at(&dir);
        plugins.read_kept("fill_gap".to_owned(), "5".to_owned());
        plugins.read_kept("eyedropper_layer".to_owned(), "true".to_owned());
        plugins.read_kept("interface_size".to_owned(), "1.0".to_owned());
        plugins.read_kept("paste_into_mask".to_owned(), "false".to_owned());
        assert!(card(&plugins, GAP).on && card(&plugins, EYEDROPPER).on);
        assert_eq!(slider_value(&plugins, "fill_gap"), 5.0);
        assert!(plugins.hooks().is_on(GAP) && plugins.hooks().pick_layer());
        assert!(!card(&plugins, SIZE).on && !card(&plugins, PASTE).on);
        // nothing to import: it's here, and on
        assert_eq!(plugins.take_moved_notice(), None);
        let lines = plugins.kept_lines();
        assert!(lines.contains(&format!("{GAP}.on true")), "{lines:?}");
        assert_eq!(lines.iter().filter(|line| line.starts_with("fill_gap ")).count(), 1, "{lines:?}");
        // on the next start with only the setting kept, as one platform keeps them
        let next = app_at(&dir);
        next.read_kept("fill_gap".to_owned(), "5".to_owned());
        assert!(card(&next, GAP).on);
        // the size too, from where it was kept as a number
        next.read_kept("interface_size".to_owned(), "0.85".to_owned());
        assert!(card(&next, SIZE).on);
        assert!((next.interface_scale(1600.0, 1000.0) - 0.85).abs() < 1e-6);
        // its own on or off says instead, read before or after
        for flag_first in [true, false] {
            let next = app_at(&dir);
            let flag = || next.read_kept(format!("{GAP}.on"), "false".to_owned());
            if flag_first {
                flag();
            }
            next.read_kept("fill_gap".to_owned(), "5".to_owned());
            if !flag_first {
                flag();
            }
            assert!(!card(&next, GAP).on, "flag first: {flag_first}");
            assert_eq!(slider_value(&next, "fill_gap"), 5.0);
        }
        // nor does one that stopped come back on for it
        app_at(&dir).stop_filter(GAP, STOPPED_DRIVER);
        let again = app_at(&dir);
        again.read_kept("fill_gap".to_owned(), "5".to_owned());
        assert!(!card(&again, GAP).on);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_nowhere_to_keep_them_nothing_is_added() {
        let plugins = Plugins::new();
        plugins.add_bundled(crate::bundled::BUNDLED);
        assert!(plugins.plugins().is_empty());
    }

    #[test]
    fn a_kept_plugin_tampered_with_counts_as_absent_and_nothing_breaks() {
        let dir = store("tampered");
        let plugins = at(&dir);
        assert_eq!(plugins.install(filter_file(&filter("org.example.filter"))), None);
        let shader = std::path::Path::new(&dir).join("org.example.filter/1.0.0/filter.wgsl");
        std::fs::write(shader, b"fn pixel() {}").unwrap();
        let next = at(&dir);
        assert!(filters(&next).is_empty());
        // a value kept for it finds nothing to go to
        next.read_kept("org.example.filter.cut".to_owned(), "0.1".to_owned());
        assert!(filters(&next).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
