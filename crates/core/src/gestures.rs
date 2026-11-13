//! Gesture controls, the same in every app: what each way of touching can be set to do (a page of
//! switches for each purpose), what starts on, the delays, and the one rule across pages: a way of
//! touching does one thing at most, so turning it on for one page turns it off on the others. The
//! app keeps the settings between runs (each switch by its name, see [`GestureSettings::kept`]);
//! the touches follow them as [`TouchSettings`].

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::touch::{GestureAction, TouchSettings};

/// What a gesture can do: the pages of Gesture controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum GesturePurpose {
    Smudge,
    Erase,
    Assisted,
    Eyedropper,
    Quickshape,
    Quickmenu,
    FullScreen,
    ClearLayer,
    CopyPaste,
    LayerSelect,
    Hover,
    General,
}

/// A way of touching: each does one thing at most (General's and Hover's settings aside).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Trigger {
    TapModify,
    Touch,
    Pen,
    ModifyTouch,
    ModifyPen,
    ThreeSwipe,
    FourTap,
    /// The pen's side buttons.
    PenDoubleTap,
    PenSqueeze,
    Hold,
    Scrub,
}

/// One switch on a page: its name (a trigger's, or a setting of its own), what it says and does,
/// whether it starts on, whether it has a delay slider under it, and whether it needs a pen's
/// button held down (an app whose pen only taps, as a double-tap does, leaves that switch out).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct GestureOption {
    pub key: String,
    pub label: String,
    pub detail: String,
    pub default: bool,
    pub trigger: Option<Trigger>,
    pub delayed: bool,
    pub held_button: bool,
}

/// The pages in order, with their names as kept, titles and headings.
const PURPOSES: [(GesturePurpose, &str, &str, &str); 12] = [
    (GesturePurpose::Smudge, "SMUDGE", "Smudge", "Customize smudge"),
    (GesturePurpose::Erase, "ERASE", "Erase", "Customize erase"),
    (GesturePurpose::Assisted, "ASSISTED", "Assisted Drawing", "Customize assisted drawing"),
    (GesturePurpose::Eyedropper, "EYEDROPPER", "Eyedropper", "Customize eyedropper"),
    (GesturePurpose::Quickshape, "QUICKSHAPE", "QuickShape", "Customize QuickShape"),
    (GesturePurpose::Quickmenu, "QUICKMENU", "QuickMenu", "Customize QuickMenu"),
    (GesturePurpose::FullScreen, "FULL_SCREEN", "Full Screen", "Customize full screen"),
    (GesturePurpose::ClearLayer, "CLEAR_LAYER", "Clear Layer", "Customize clear layer"),
    (GesturePurpose::CopyPaste, "COPY_PASTE", "Copy & Paste", "Customize copy & paste"),
    (GesturePurpose::LayerSelect, "LAYER_SELECT", "Layer Select", "Customize layer select"),
    (GesturePurpose::Hover, "HOVER", "Hover", "Customize hover"),
    (GesturePurpose::General, "GENERAL", "General", "General"),
];

/// Each trigger's name as kept and its label.
const TRIGGERS: [(Trigger, &str, &str); 11] = [
    (Trigger::TapModify, "TAP_MODIFY", "Tap □"),
    (Trigger::Touch, "TOUCH", "Touch"),
    (Trigger::Pen, "PEN", "Pen"),
    (Trigger::ModifyTouch, "MODIFY_TOUCH", "□ + Touch"),
    (Trigger::ModifyPen, "MODIFY_PEN", "□ + Pen"),
    (Trigger::ThreeSwipe, "THREE_SWIPE", "Three-finger swipe"),
    (Trigger::FourTap, "FOUR_TAP", "Four-finger tap"),
    (Trigger::PenDoubleTap, "PEN_DOUBLE_TAP", "Pen button"),
    (Trigger::PenSqueeze, "PEN_SQUEEZE", "Pen second button"),
    (Trigger::Hold, "HOLD", "Touch and hold"),
    (Trigger::Scrub, "SCRUB", "Scrub"),
];

/// The shortest and longest delay, seconds.
pub const MIN_DELAY_S: f32 = 0.1;
pub const MAX_DELAY_S: f32 = 1.5;

fn purpose_entry(p: GesturePurpose) -> (GesturePurpose, &'static str, &'static str, &'static str) {
    PURPOSES.iter().copied().find(|e| e.0 == p).unwrap_or(PURPOSES[0])
}

fn trigger_entry(t: Trigger) -> (Trigger, &'static str, &'static str) {
    TRIGGERS.iter().copied().find(|e| e.0 == t).unwrap_or(TRIGGERS[0])
}

/// A switch for a trigger (a hold has a delay slider under it).
fn opt(t: Trigger, detail: &str, default: bool) -> GestureOption {
    let (_, key, label) = trigger_entry(t);
    GestureOption {
        key: key.to_owned(),
        label: label.to_owned(),
        detail: detail.to_owned(),
        default,
        trigger: Some(t),
        delayed: t == Trigger::Hold,
        held_button: false,
    }
}

/// A switch for the pen's button held down.
fn held(t: Trigger, detail: &str) -> GestureOption {
    GestureOption { held_button: true, ..opt(t, detail, false) }
}

/// A setting of a page's own.
fn own(key: &str, label: &str, detail: &str, default: bool, delayed: bool) -> GestureOption {
    GestureOption {
        key: key.to_owned(),
        label: label.to_owned(),
        detail: detail.to_owned(),
        default,
        trigger: None,
        delayed,
        held_button: false,
    }
}

/// A page's title (its row in the list) and heading.
#[uniffi::export]
pub fn gesture_purpose_title(purpose: GesturePurpose) -> String {
    purpose_entry(purpose).2.to_owned()
}

#[uniffi::export]
pub fn gesture_purpose_heading(purpose: GesturePurpose) -> String {
    purpose_entry(purpose).3.to_owned()
}

/// The pages in order.
#[uniffi::export]
pub fn gesture_purposes() -> Vec<GesturePurpose> {
    PURPOSES.iter().map(|e| e.0).collect()
}

/// A page's switches, in order.
#[uniffi::export]
pub fn gesture_options(purpose: GesturePurpose) -> Vec<GestureOption> {
    use Trigger as T;
    match purpose {
        GesturePurpose::Smudge => vec![
            opt(T::Touch, "Finger strokes smudge", false),
            opt(T::Pen, "Pen strokes smudge", false),
            opt(T::ModifyTouch, "Finger strokes smudge while □ is held down", false),
            opt(T::ModifyPen, "Pen strokes smudge while □ is held down", false),
        ],
        GesturePurpose::Erase => vec![
            opt(T::Touch, "Finger strokes erase", false),
            opt(T::Pen, "Pen strokes erase", false),
            opt(T::ModifyTouch, "Finger strokes erase while □ is held down", false),
            opt(T::ModifyPen, "Pen strokes erase while □ is held down", false),
            // off, the button switches to the eraser and back (as a pen's double-tap does)
            held(T::PenDoubleTap, "Pen strokes erase while the pen button is held down"),
        ],
        GesturePurpose::Assisted => vec![
            opt(T::TapModify, "Turns drawing assist on or off for this layer", false),
            opt(T::Touch, "Finger strokes follow the drawing guide", false),
            opt(T::Pen, "Pen strokes follow the drawing guide", false),
            opt(T::ModifyTouch, "Finger strokes follow the guide while □ is held down", false),
            opt(T::ModifyPen, "Pen strokes follow the guide while □ is held down", false),
            opt(T::PenDoubleTap, "Turns drawing assist on or off for this layer", false),
        ],
        GesturePurpose::Eyedropper => vec![
            opt(T::TapModify, "Opens or closes the eyedropper", true),
            opt(T::Touch, "A finger on the canvas picks a colour", false),
            opt(T::ModifyTouch, "A finger picks a colour while □ is held down", true),
            opt(T::ModifyPen, "The pen picks a colour while □ is held down", true),
            opt(T::PenDoubleTap, "Opens the eyedropper", false),
            opt(T::PenSqueeze, "Opens the eyedropper", false),
            opt(T::Hold, "A finger kept still on the canvas picks a colour", true),
        ],
        GesturePurpose::Quickshape => vec![
            opt(T::TapModify, "Turns the last stroke into a shape", false),
            opt(T::Touch, "A finger touch turns the last stroke into a shape", false),
            opt(T::ThreeSwipe, "Three fingers swiped down turn the last stroke into a shape", false),
            opt(T::FourTap, "Four fingers tapped turn the last stroke into a shape", false),
            opt(T::PenDoubleTap, "Turns the last stroke into a shape", false),
            opt(T::PenSqueeze, "Turns the last stroke into a shape", false),
            own("DRAW_HOLD", "Draw and hold", "Pausing at the end of a stroke turns it into a shape", true, true),
        ],
        GesturePurpose::Quickmenu => vec![
            opt(T::TapModify, "Opens or closes the quick menu", false),
            opt(T::Touch, "A finger touch opens the quick menu", false),
            opt(T::ModifyTouch, "A finger opens the quick menu while □ is held down", false),
            opt(T::ModifyPen, "The pen opens the quick menu while □ is held down", false),
            opt(T::ThreeSwipe, "Three fingers swiped down open the quick menu", false),
            opt(T::FourTap, "Four fingers tapped open or close the quick menu", false),
            opt(T::PenDoubleTap, "Opens the quick menu", false),
            opt(T::PenSqueeze, "Opens the quick menu", false),
            opt(T::Hold, "A finger kept still on the canvas opens the quick menu", false),
        ],
        GesturePurpose::FullScreen => vec![
            opt(T::Touch, "A finger touch shows or hides the interface", false),
            opt(T::ThreeSwipe, "Three fingers swiped down show or hide the interface", false),
            opt(T::FourTap, "Four fingers tapped show or hide the interface", true),
            opt(T::PenDoubleTap, "Shows or hides the interface", false),
            own("AUTO", "Automatic Full Screen", "The interface steps aside while you paint", false, true),
        ],
        GesturePurpose::ClearLayer => vec![
            opt(T::Scrub, "Rubbing three fingers back and forth clears the layer", true),
            opt(T::ThreeSwipe, "Three fingers swiped down clear the layer", false),
            opt(T::FourTap, "Four fingers tapped clear the layer", false),
            opt(T::PenDoubleTap, "Clears the layer", false),
            opt(T::Hold, "A finger kept still on the canvas clears the layer", false),
        ],
        GesturePurpose::CopyPaste => vec![
            opt(T::TapModify, "Opens the copy and paste menu", false),
            opt(T::Touch, "A finger touch opens the copy and paste menu", false),
            opt(T::ModifyTouch, "A finger opens the copy and paste menu while □ is held down", false),
            opt(T::ModifyPen, "The pen opens the copy and paste menu while □ is held down", false),
            opt(T::ThreeSwipe, "Three fingers swiped down open the copy and paste menu", true),
            opt(T::FourTap, "Four fingers tapped open the copy and paste menu", false),
            opt(T::PenDoubleTap, "Opens the copy and paste menu", false),
            opt(T::Hold, "A finger kept still on the canvas opens the copy and paste menu", false),
        ],
        GesturePurpose::LayerSelect => vec![
            opt(T::Touch, "A finger touch picks the layer drawn there", false),
            opt(T::ModifyTouch, "A finger picks the layer drawn there while □ is held down", false),
            opt(T::ModifyPen, "The pen picks the layer drawn there while □ is held down", false),
            opt(T::PenSqueeze, "Picks the layer drawn under the pen", false),
            opt(T::Hold, "A finger kept still picks the layer drawn there", false),
        ],
        GesturePurpose::Hover => vec![own(
            "HOVER_TOUCH",
            "Touch",
            "While the pen hovers, drag a finger for opacity and pinch for size",
            false,
            false,
        )],
        GesturePurpose::General => vec![
            own("FINGER_PAINT", "Painting with a finger", "One finger paints as the pen does", true, false),
            own(
                "ONE_FINGER_PAN",
                "Move the canvas with one finger",
                "One finger drags the canvas instead of painting; the pen still paints",
                false,
                false,
            ),
            own(
                "NO_UNDO",
                "Disable undo and redo gestures",
                "Two- and three-finger taps leave the history alone",
                false,
                false,
            ),
            own("ROTATE", "Rotate with pinch zoom", "Two fingers turn the canvas as they zoom it", true, false),
            own(
                "MODEL_FINGER",
                "3D painting with a finger",
                "A finger paints a model; with □ held it turns it",
                false,
                false,
            ),
        ],
    }
}

/// A trigger's label.
#[uniffi::export]
pub fn gesture_trigger_label(trigger: Trigger) -> String {
    trigger_entry(trigger).2.to_owned()
}

/// A page's delay at first, seconds: holds 0.40 s, QuickShape 0.65 s, automatic full screen 0.66 s.
fn default_delay(p: GesturePurpose) -> f32 {
    match p {
        GesturePurpose::Quickshape => 0.65,
        GesturePurpose::FullScreen => 0.66,
        _ => 0.40,
    }
}

/// The settings as kept: each switch by its name (`PAGE.KEY`, as `EYEDROPPER.HOLD`) and each page's
/// delay by `PAGE.delay`, seconds.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct KeptGestures {
    pub switches: HashMap<String, bool>,
    pub delays: HashMap<String, f32>,
}

#[derive(Default)]
struct State {
    on: HashMap<String, bool>,
    delays: HashMap<GesturePurpose, f32>,
}

/// The gesture settings: the app holds one, made from what it kept, and keeps what changes.
#[derive(uniffi::Object)]
pub struct GestureSettings {
    state: Mutex<State>,
}

#[uniffi::export]
impl GestureSettings {
    /// The settings from what was `kept` (anything not there as the app ships).
    #[uniffi::constructor]
    pub fn new(kept: KeptGestures) -> Arc<GestureSettings> {
        let mut st = State::default();
        for &(p, page, _, _) in &PURPOSES {
            for o in gesture_options(p) {
                let name = format!("{page}.{}", o.key);
                let on = kept.switches.get(&name).copied().unwrap_or(o.default);
                st.on.insert(name, on);
            }
            let delay = kept.delays.get(&format!("{page}.delay")).copied().unwrap_or_else(|| default_delay(p));
            st.delays.insert(p, delay.clamp(MIN_DELAY_S, MAX_DELAY_S));
        }
        Arc::new(GestureSettings { state: Mutex::new(st) })
    }

    pub fn is_on(&self, purpose: GesturePurpose, key: String) -> bool {
        self.st().on.get(&name(purpose, &key)).copied().unwrap_or(false)
    }

    /// Turns a switch on or off; on, its trigger goes off on every other page. Returns the
    /// switches that changed, by name, to keep.
    pub fn set(&self, purpose: GesturePurpose, key: String, value: bool) -> HashMap<String, bool> {
        let mut st = self.st();
        let mut changed = HashMap::new();
        let trigger = gesture_options(purpose).into_iter().find(|o| o.key == key).and_then(|o| o.trigger);
        if let (true, Some(t)) = (value, trigger) {
            for &(other, _, _, _) in &PURPOSES {
                if other == purpose {
                    continue;
                }
                for o in gesture_options(other).into_iter().filter(|o| o.trigger == Some(t)) {
                    let n = name(other, &o.key);
                    if st.on.get(&n) == Some(&true) {
                        st.on.insert(n.clone(), false);
                        changed.insert(n, false);
                    }
                }
            }
        }
        // one finger moves the canvas, paints, or does what a page's Touch is set to: one at a time
        if value {
            let general = |key: &str| name(GesturePurpose::General, key);
            let off: Vec<String> = if purpose == GesturePurpose::General && key == "ONE_FINGER_PAN" {
                PURPOSES
                    .iter()
                    .flat_map(|&(p, _, _, _)| {
                        gesture_options(p)
                            .into_iter()
                            .filter(|o| o.trigger == Some(Trigger::Touch))
                            .map(move |o| name(p, &o.key))
                    })
                    .chain([general("FINGER_PAINT")])
                    .collect()
            } else if (purpose == GesturePurpose::General && key == "FINGER_PAINT") || trigger == Some(Trigger::Touch) {
                vec![general("ONE_FINGER_PAN")]
            } else {
                Vec::new()
            };
            for n in off {
                if st.on.get(&n) == Some(&true) {
                    st.on.insert(n.clone(), false);
                    changed.insert(n, false);
                }
            }
        }
        let n = name(purpose, &key);
        st.on.insert(n.clone(), value);
        changed.insert(n, value);
        changed
    }

    /// The page a trigger is set to, if any.
    pub fn purpose_of(&self, trigger: Trigger) -> Option<GesturePurpose> {
        let st = self.st();
        PURPOSES.iter().map(|e| e.0).find(|&p| {
            gesture_options(p).iter().any(|o| o.trigger == Some(trigger) && st.on.get(&name(p, &o.key)) == Some(&true))
        })
    }

    /// A page's delay, seconds.
    pub fn delay(&self, purpose: GesturePurpose) -> f32 {
        self.st().delays.get(&purpose).copied().unwrap_or_else(|| default_delay(purpose))
    }

    /// Sets a page's delay (kept within its range); returns it as set.
    pub fn set_delay(&self, purpose: GesturePurpose, seconds: f32) -> f32 {
        let s = if seconds.is_finite() { seconds.clamp(MIN_DELAY_S, MAX_DELAY_S) } else { default_delay(purpose) };
        self.st().delays.insert(purpose, s);
        s
    }

    /// Every switch and delay as the app ships them.
    pub fn reset_all(&self) {
        let mut st = self.st();
        for &(p, _, _, _) in &PURPOSES {
            for o in gesture_options(p) {
                st.on.insert(name(p, &o.key), o.default);
            }
            st.delays.insert(p, default_delay(p));
        }
    }

    /// Everything, to keep.
    pub fn kept(&self) -> KeptGestures {
        let st = self.st();
        KeptGestures {
            switches: st.on.clone(),
            delays: st.delays.iter().map(|(&p, &d)| (format!("{}.delay", purpose_entry(p).1), d)).collect(),
        }
    }

    /// The settings as the canvas's touches take them; `rapid_undo_ms` is how long two fingers are
    /// held before undo repeats (a setting of its own).
    pub fn touch_settings(&self, rapid_undo_ms: f64) -> TouchSettings {
        let action = |p: Option<GesturePurpose>| {
            Some(match p? {
                GesturePurpose::Smudge => GestureAction::Smudge,
                GesturePurpose::Erase => GestureAction::Erase,
                GesturePurpose::Assisted => GestureAction::Assisted,
                GesturePurpose::Eyedropper => GestureAction::Eyedropper,
                GesturePurpose::Quickshape => GestureAction::QuickShape,
                GesturePurpose::Quickmenu => GestureAction::QuickMenu,
                GesturePurpose::FullScreen => GestureAction::FullScreen,
                GesturePurpose::ClearLayer => GestureAction::ClearLayer,
                GesturePurpose::CopyPaste => GestureAction::CopyPaste,
                GesturePurpose::LayerSelect => GestureAction::LayerSelect,
                GesturePurpose::Hover | GesturePurpose::General => return None,
            })
        };
        let general = |key: &str| self.is_on(GesturePurpose::General, key.to_owned());
        let hold = self.purpose_of(Trigger::Hold);
        // moving the canvas with one finger, the finger neither paints nor does a page's Touch
        let pan = general("ONE_FINGER_PAN");
        TouchSettings {
            finger_paint: general("FINGER_PAINT") && !pan,
            one_finger_pan: pan,
            undo_taps: !general("NO_UNDO"),
            rotate: general("ROTATE"),
            model_finger_paint: general("MODEL_FINGER"),
            draw_and_hold: self.is_on(GesturePurpose::Quickshape, "DRAW_HOLD".to_owned()),
            touch: if pan { None } else { action(self.purpose_of(Trigger::Touch)) },
            pen: action(self.purpose_of(Trigger::Pen)),
            hold: action(hold),
            modify_touch: action(self.purpose_of(Trigger::ModifyTouch)),
            modify_pen: action(self.purpose_of(Trigger::ModifyPen)),
            four_tap: action(self.purpose_of(Trigger::FourTap)),
            three_swipe: action(self.purpose_of(Trigger::ThreeSwipe)),
            pen_button: action(self.purpose_of(Trigger::PenDoubleTap)),
            pen_second_button: action(self.purpose_of(Trigger::PenSqueeze)),
            scrub: self.is_on(GesturePurpose::ClearLayer, "SCRUB".to_owned()),
            hover_touch: self.is_on(GesturePurpose::Hover, "HOVER_TOUCH".to_owned()),
            hold_ms: f64::from(self.delay(hold.unwrap_or(GesturePurpose::Eyedropper))) * 1000.0,
            quickshape_ms: f64::from(self.delay(GesturePurpose::Quickshape)) * 1000.0,
            rapid_undo_ms,
        }
    }
}

impl GestureSettings {
    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn name(p: GesturePurpose, key: &str) -> String {
    format!("{}.{key}", purpose_entry(p).1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_shipped_the_touches_get_their_defaults() {
        let g = GestureSettings::new(KeptGestures::default());
        let mut t = g.touch_settings(650.0);
        let shipped = TouchSettings::default();
        // the defaults round 0.40 s and 0.65 s through f32
        assert!((t.hold_ms - shipped.hold_ms).abs() < 0.01 && (t.quickshape_ms - shipped.quickshape_ms).abs() < 0.01);
        t.hold_ms = shipped.hold_ms;
        t.quickshape_ms = shipped.quickshape_ms;
        assert_eq!(t, shipped);
    }

    #[test]
    fn a_trigger_does_one_thing() {
        let g = GestureSettings::new(KeptGestures::default());
        assert_eq!(g.purpose_of(Trigger::Hold), Some(GesturePurpose::Eyedropper));
        let changed = g.set(GesturePurpose::Quickmenu, "HOLD".into(), true);
        assert_eq!(changed.get("EYEDROPPER.HOLD"), Some(&false));
        assert_eq!(changed.get("QUICKMENU.HOLD"), Some(&true));
        assert_eq!(g.purpose_of(Trigger::Hold), Some(GesturePurpose::Quickmenu));
        assert_eq!(g.touch_settings(650.0).hold, Some(GestureAction::QuickMenu));
        // a page's own setting touches nothing else
        assert_eq!(g.set(GesturePurpose::General, "NO_UNDO".into(), true).len(), 1);
        assert!(!g.touch_settings(650.0).undo_taps);
    }

    #[test]
    fn erase_s_pen_button_erases_while_held_one_switch_kept_like_the_rest() {
        let g = GestureSettings::new(KeptGestures::default());
        // as shipped, off: the button switches to the eraser and back
        assert!(!g.is_on(GesturePurpose::Erase, "PEN_DOUBLE_TAP".into()));
        assert_eq!(g.touch_settings(650.0).pen_button, None);
        g.set(GesturePurpose::Quickmenu, "PEN_DOUBLE_TAP".into(), true);
        let changed = g.set(GesturePurpose::Erase, "PEN_DOUBLE_TAP".into(), true);
        assert_eq!(changed.get("QUICKMENU.PEN_DOUBLE_TAP"), Some(&false));
        assert_eq!(changed.get("ERASE.PEN_DOUBLE_TAP"), Some(&true));
        assert_eq!(g.touch_settings(650.0).pen_button, Some(GestureAction::Erase));
        let again = GestureSettings::new(g.kept());
        assert!(again.is_on(GesturePurpose::Erase, "PEN_DOUBLE_TAP".into()));
        assert_eq!(again.touch_settings(650.0).pen_button, Some(GestureAction::Erase));
        again.reset_all();
        assert_eq!(again.touch_settings(650.0).pen_button, None);
        // the only switch that needs a pen's button held down
        let held: Vec<(GesturePurpose, String)> = gesture_purposes()
            .into_iter()
            .flat_map(|p| gesture_options(p).into_iter().filter(|o| o.held_button).map(move |o| (p, o.key)))
            .collect();
        assert_eq!(held, [(GesturePurpose::Erase, "PEN_DOUBLE_TAP".to_owned())]);
    }

    #[test]
    fn what_was_kept_comes_back() {
        let g = GestureSettings::new(KeptGestures::default());
        g.set(GesturePurpose::Smudge, "PEN".into(), true);
        assert_eq!(g.set_delay(GesturePurpose::Eyedropper, 9.0), MAX_DELAY_S);
        let again = GestureSettings::new(g.kept());
        assert!(again.is_on(GesturePurpose::Smudge, "PEN".into()));
        assert_eq!(again.delay(GesturePurpose::Eyedropper), MAX_DELAY_S);
        again.reset_all();
        assert!(!again.is_on(GesturePurpose::Smudge, "PEN".into()));
        assert_eq!(again.delay(GesturePurpose::Quickshape), 0.65);
    }
}
