//! The plugins: what the app does beyond its measured look and behaviour, each off (or at 100%)
//! until asked for, all on the one list the Plugins page shows. A plugin here is plain data, a name,
//! a line on what it does and its controls, so the page draws any plugin without knowing it, and
//! one from elsewhere can describe itself the same way. What a plugin does is the app's: it asks
//! for the values it knows ([`Plugins::interface_size`]), and one nothing asks about does nothing.

use std::sync::{Arc, Mutex, MutexGuard};

use crate::interface_size;

/// What the bundled plugins' values are kept under, as they were before there was a list.
const INTERFACE_SIZE: &str = "interface_size";
const PASTE_INTO_MASK: &str = "paste_into_mask";

/// A setting on or off. `key` is what its value is kept under: one key to a value, across all
/// plugins.
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
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum PluginControl {
    Switch { control: PluginSwitch },
    Slider { control: PluginSlider },
}

/// A plugin as the page shows it. It is `on` while any of its controls is off where it starts, so
/// what is kept is only their values.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub bundled: bool,
    pub on: bool,
    pub controls: Vec<PluginControl>,
}

impl PluginControl {
    fn key(&self) -> &str {
        match self {
            PluginControl::Switch { control } => &control.key,
            PluginControl::Slider { control } => &control.key,
        }
    }

    /// Whether it is off where it starts.
    fn moved(&self) -> bool {
        match self {
            PluginControl::Switch { control } => control.on != control.starts,
            PluginControl::Slider { control } => control.value != control.starts,
        }
    }

    /// Its value as kept, read back with [`Plugins::read_kept`].
    fn kept(&self) -> String {
        match self {
            PluginControl::Switch { control } => control.on.to_string(),
            PluginControl::Slider { control } => control.value.to_string(),
        }
    }
}

impl PluginSlider {
    /// `value` kept to the range, on a step. In doubles, so a step lands on the number it reads as
    /// (1 exactly, at 100%). A plugin's range comes as it comes: a bad one mustn't panic.
    fn stepped(&self, value: f32) -> f32 {
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

/// The plugins the app comes with, as they start.
fn bundled() -> Vec<PluginInfo> {
    vec![
        PluginInfo {
            id: INTERFACE_SIZE.to_owned(),
            name: "Interface size".to_owned(),
            description: "Makes the bars, panels and text bigger or smaller to suit your screen.".to_owned(),
            bundled: true,
            on: false,
            controls: vec![PluginControl::Slider {
                control: PluginSlider {
                    key: INTERFACE_SIZE.to_owned(),
                    label: "Size".to_owned(),
                    least: interface_size::LEAST / 100.0,
                    most: interface_size::MOST / 100.0,
                    step: interface_size::STEP / 100.0,
                    value: 1.0,
                    starts: 1.0,
                    shown_times: 100.0,
                    unit: "%".to_owned(),
                },
            }],
        },
        PluginInfo {
            id: PASTE_INTO_MASK.to_owned(),
            name: "Paste into masks".to_owned(),
            description: "With a mask selected, Paste puts the picture's brightness into the mask.".to_owned(),
            bundled: true,
            on: false,
            controls: vec![PluginControl::Switch {
                control: PluginSwitch {
                    key: PASTE_INTO_MASK.to_owned(),
                    label: "Paste into the mask".to_owned(),
                    on: false,
                    starts: false,
                },
            }],
        },
    ]
}

/// The list, with each control's value as it stands. The app keeps the values where it keeps its
/// other interface settings, each under its control's key.
#[derive(Debug, uniffi::Object)]
pub struct Plugins {
    list: Mutex<Vec<PluginInfo>>,
}

impl Plugins {
    fn listed(&self) -> MutexGuard<'_, Vec<PluginInfo>> {
        self.list.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What `change` makes of the control kept under `key`, if there is one; its plugin's `on`
    /// follows.
    fn change<T>(&self, key: &str, change: impl FnOnce(&mut PluginControl) -> Option<T>) -> Option<T> {
        let mut list = self.listed();
        let plugin = list.iter_mut().find(|p| p.controls.iter().any(|c| c.key() == key))?;
        let done = plugin.controls.iter_mut().find(|c| c.key() == key).and_then(change);
        plugin.on = plugin.controls.iter().any(PluginControl::moved);
        done
    }

    fn control(&self, key: &str) -> Option<PluginControl> {
        self.listed().iter().flat_map(|p| &p.controls).find(|c| c.key() == key).cloned()
    }

    /// Each value as a line of a settings file, its key and then what [`Plugins::read_kept`] reads.
    pub fn kept_lines(&self) -> Vec<String> {
        self.listed().iter().flat_map(|p| &p.controls).map(|c| format!("{} {}", c.key(), c.kept())).collect()
    }
}

#[uniffi::export]
impl Plugins {
    /// The bundled plugins, as they start.
    #[uniffi::constructor]
    pub fn new() -> Arc<Plugins> {
        Arc::new(Plugins { list: Mutex::new(bundled()) })
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        self.listed().clone()
    }

    /// A value read back from where it was kept, as text. A key no control has, or anything that
    /// isn't its kind of value, leaves things as they are.
    pub fn read_kept(&self, key: String, value: String) {
        self.change(&key, |control| {
            match control {
                PluginControl::Switch { control } => control.on = value.parse().ok()?,
                PluginControl::Slider { control } => {
                    let read = value.parse::<f32>().ok().filter(|v| v.is_finite())?;
                    control.value = control.stepped(read);
                }
            }
            Some(())
        });
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

    /// How big the whole interface is set to show (1: as measured).
    pub fn interface_size(&self) -> f32 {
        match self.control(INTERFACE_SIZE) {
            Some(PluginControl::Slider { control }) => control.value,
            _ => 1.0,
        }
    }

    /// Whether what is pasted with a mask current goes into the mask.
    pub fn paste_into_mask(&self) -> bool {
        matches!(self.control(PASTE_INTO_MASK), Some(PluginControl::Switch { control }) if control.on)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size_slider(plugins: &Plugins) -> PluginSlider {
        match plugins.control(INTERFACE_SIZE) {
            Some(PluginControl::Slider { control }) => control,
            other => panic!("{other:?}"),
        }
    }

    fn plugin(plugins: &Plugins, id: &str) -> PluginInfo {
        plugins.list().into_iter().find(|p| p.id == id).unwrap()
    }

    /// One from elsewhere: nothing in the app asks about it.
    fn stranger() -> PluginInfo {
        PluginInfo {
            id: "someone.stranger".to_owned(),
            name: "Stranger".to_owned(),
            description: "No code answers to it.".to_owned(),
            bundled: false,
            on: false,
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
                    },
                },
            ],
        }
    }

    #[test]
    fn the_list_starts_with_everything_off_or_at_100() {
        let plugins = Plugins::new();
        let list = plugins.list();
        let ids: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["interface_size", "paste_into_mask"]);
        for p in &list {
            assert!(p.bundled && !p.on, "{p:?}");
            assert!(!p.name.is_empty() && !p.description.is_empty() && !p.controls.is_empty(), "{p:?}");
        }
        assert_eq!(plugins.interface_size(), 1.0);
        assert!(!plugins.paste_into_mask());
        let size = size_slider(&plugins);
        assert_eq!((size.least, size.most, size.value), (0.75, 1.25, 1.0));
    }

    #[test]
    fn no_two_controls_share_a_key() {
        let plugins = Plugins::new();
        let mut keys: Vec<String> = plugins.list().iter().flat_map(|p| &p.controls).map(|c| c.key().into()).collect();
        let all = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), all);
    }

    #[test]
    fn values_kept_before_there_was_a_list_carry_over() {
        // as both apps wrote them: the desktop's `interface` lines, Android's preferences as text
        let plugins = Plugins::new();
        plugins.read_kept("interface_size".to_owned(), "0.85".to_owned());
        plugins.read_kept("paste_into_mask".to_owned(), "true".to_owned());
        assert_eq!(plugins.interface_size(), 0.85);
        assert!(plugins.paste_into_mask());
        assert!(plugin(&plugins, INTERFACE_SIZE).on);
        assert!(plugin(&plugins, PASTE_INTO_MASK).on);
        // kept at 100% and off, they are as they start
        let plugins = Plugins::new();
        plugins.read_kept("interface_size".to_owned(), "1.0".to_owned());
        plugins.read_kept("paste_into_mask".to_owned(), "false".to_owned());
        assert_eq!(plugins.interface_size(), 1.0);
        assert!(!plugins.paste_into_mask());
        assert!(plugins.list().iter().all(|p| !p.on));
    }

    #[test]
    fn a_kept_value_comes_back_usable() {
        let read = |text: &str| {
            let plugins = Plugins::new();
            plugins.read_kept("interface_size".to_owned(), text.to_owned());
            plugins.interface_size()
        };
        assert_eq!(read("0.86"), 0.85);
        assert_eq!(read("9"), 1.25);
        assert_eq!(read("0"), 0.75);
        for bad in ["NaN", "inf", "-inf", "", "big", "true"] {
            assert_eq!(read(bad), 1.0, "{bad}");
        }
        let plugins = Plugins::new();
        plugins.read_kept("paste_into_mask".to_owned(), "yes".to_owned());
        assert!(!plugins.paste_into_mask());
    }

    #[test]
    fn what_is_kept_reads_back_the_same() {
        let plugins = Plugins::new();
        assert_eq!(plugins.set_slider(INTERFACE_SIZE.to_owned(), 1.15), Some(1.15));
        assert!(plugins.set_switch(PASTE_INTO_MASK.to_owned(), true));
        let lines = plugins.kept_lines();
        assert_eq!(lines, ["interface_size 1.15", "paste_into_mask true"]);
        let again = Plugins::new();
        for line in &lines {
            let (key, value) = line.split_once(' ').unwrap();
            again.read_kept(key.to_owned(), value.to_owned());
        }
        assert_eq!(again.list(), plugins.list());
    }

    #[test]
    fn a_plugin_is_on_only_while_a_control_is_off_where_it_starts() {
        let plugins = Plugins::new();
        plugins.set_slider(INTERFACE_SIZE.to_owned(), 0.9);
        assert!(plugin(&plugins, INTERFACE_SIZE).on);
        assert!(!plugin(&plugins, PASTE_INTO_MASK).on);
        plugins.set_slider(INTERFACE_SIZE.to_owned(), 1.0);
        assert!(!plugin(&plugins, INTERFACE_SIZE).on);
    }

    #[test]
    fn a_slider_moves_in_steps_with_100_in_the_middle() {
        let size = size_slider(&Plugins::new());
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
        let size = size_slider(&Plugins::new());
        assert_eq!(plugin_slider_shown(size.clone(), 0.85), "85%");
        assert_eq!(plugin_slider_shown(size, 1.0), "100%");
        let PluginControl::Slider { control } = stranger().controls[1].clone() else { panic!() };
        assert_eq!(plugin_slider_shown(control.clone(), 2.5), "2.5 px");
        let hundredths = PluginSlider { step: 0.05, ..control };
        assert_eq!(plugin_slider_shown(hundredths, 2.25), "2.25 px");
    }

    #[test]
    fn a_plugin_nothing_answers_to_shows_and_does_nothing() {
        let plugins = Plugins::new();
        plugins.listed().push(stranger());
        assert_eq!(plugins.list().len(), 3);
        // its values change and are kept like any other's, and touch nothing else
        assert!(plugins.set_switch("someone.stranger.switch".to_owned(), true));
        assert_eq!(plugins.set_slider("someone.stranger.slider".to_owned(), 3.3), Some(3.5));
        assert!(plugin(&plugins, "someone.stranger").on);
        assert_eq!(plugins.interface_size(), 1.0);
        assert!(!plugins.paste_into_mask());
        assert_eq!(plugins.kept_lines().len(), 4);
    }

    #[test]
    fn a_key_no_control_has_is_left_alone() {
        let plugins = Plugins::new();
        let before = plugins.list();
        assert!(!plugins.set_switch("nobody".to_owned(), true));
        assert_eq!(plugins.set_slider("nobody".to_owned(), 1.0), None);
        plugins.read_kept("nobody".to_owned(), "true".to_owned());
        // nor does a value of the wrong kind reach a control
        assert!(!plugins.set_switch(INTERFACE_SIZE.to_owned(), true));
        assert_eq!(plugins.set_slider(PASTE_INTO_MASK.to_owned(), 1.0), None);
        assert_eq!(plugins.set_slider(INTERFACE_SIZE.to_owned(), f32::NAN), None);
        assert_eq!(plugins.list(), before);
    }

    #[test]
    fn without_a_bundled_plugin_the_app_goes_on_as_it_starts() {
        let plugins = Plugins::new();
        plugins.listed().clear();
        assert_eq!(plugins.interface_size(), 1.0);
        assert!(!plugins.paste_into_mask());
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
        };
        for slider in [bad(5.0, 1.0, 1.0), bad(1.0, 1.0, 0.0), bad(0.0, 1.0, -1.0), bad(f32::NAN, f32::NAN, f32::NAN)] {
            let _ = slider.stepped(0.5);
            let _ = plugin_slider_at(slider.clone(), f32::NAN);
            let _ = plugin_slider_along(slider.clone(), 0.5);
            let _ = plugin_slider_shown(slider, 0.5);
        }
    }
}
