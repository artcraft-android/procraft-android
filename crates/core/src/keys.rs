//! A hardware keyboard's shortcuts on the canvas: which key does what. The platforms name the key
//! and do what it says (as they do the touches' commands); typing in a text field never comes
//! here. A mouse dragged with a key held moves the view instead of painting (the platform says
//! which drag, and hands it the pointer): Space with the command key zooms the canvas, R turns
//! it (see [`KeyDrag`]); R tapped turns it a step.

use std::f32::consts::{PI, TAU};

use crate::touch::COMPANION_ZOOM_DISTANCE;
use crate::{CanvasSession, TransformBox};

/// How far R tapped turns the canvas, and the steps a turning drag with Shift lands on (radians,
/// 15°).
const TURN_STEP: f32 = 15.0 * PI / 180.0;
/// A turning drag this near the window's middle (points) turns nothing: there its angle jumps
/// about.
const TURN_STILL: f32 = 8.0;

/// What a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum KeyShortcut {
    Undo,
    Redo,
    CopyLayer,
    CutLayer,
    Paste,
    /// The paint tool, or the eraser.
    Paint,
    Erase,
    /// The next touch picks a colour (again: it doesn't).
    Eyedropper,
    /// Selection, Transform, or Hue, Saturation, Brightness opened (or closed).
    Selection,
    Transform,
    HueSaturationBrightness,
    /// The colour panel, Actions, or Layers opened (or closed).
    Colours,
    Actions,
    Layers,
    SwapColours,
    Deselect,
    DuplicateLayer,
    /// The interface hidden, or shown again.
    Interface,
    ClearLayer,
    /// The whole canvas copied, as it shows.
    CopyCanvas,
    ColourBalance,
    /// The perspective guide on, or off.
    PerspectiveGuide,
    /// The QuickMenu opened, or closed.
    QuickMenu,
    /// The canvas shown in perceptual greys, or in colour again (the window's view only).
    GreyView,
    /// The brush's size changed by `by` (of the slider's whole run).
    BrushSize {
        by: f32,
    },
    /// What is being transformed moved a pixel (`dx`, `dy`: -1, 0 or 1 across and down the
    /// window): [`CanvasSession::transform_arrow`].
    Nudge {
        dx: f32,
        dy: f32,
    },
}

/// The shortcut on `key` with the presses the app starts with (see [`KeyBindings`]): a letter or
/// digit (either case), "[", "]", ";", "space", "delete", "backspace", or an arrow ("left",
/// "right", "up", "down"); `command` is Ctrl (⌘), and the arrows nudge only while `transforming`.
///
/// With the command key: Z undoes (with Shift redoes), C, X and V copy, cut and paste the layer,
/// A copies the canvas, B opens Color Balance, U Hue, Saturation, Brightness, D deselects, J
/// duplicates the layer, K Actions, 0 the interface, ";" the perspective guide, Delete
/// (Backspace) clears the layer, [ and ] the brush's size by 1%, G shows the canvas in perceptual
/// greys (as Krita's soft proofing does). Alone: B paints, E erases, M the eyedropper, S
/// selection, V transform, C the colours, X swaps them, L Layers, Space the QuickMenu, [ and ] the
/// brush's size by 5% (with Shift 10%); U, D, J, K and 0 as with the command key.
#[uniffi::export]
pub fn key_shortcut(key: String, command: bool, shift: bool, alt: bool, transforming: bool) -> Option<KeyShortcut> {
    key_shortcut_with(KeyBindings::default(), key, command, shift, alt, transforming)
}

/// The shortcut on `key` with `bindings` in place: a press a binding names exactly, or else the
/// key alone (so Shift with D deselects where only D is bound, and AltGr typing a bracket is the
/// bracket alone); then the presses that take a step from the modifiers: the brush's size and an
/// arrow nudging a transform. `command` is Ctrl (⌘), and Ctrl with Alt is AltGr typing a key.
#[uniffi::export]
pub fn key_shortcut_with(
    bindings: KeyBindings,
    key: String,
    command: bool,
    shift: bool,
    alt: bool,
    transforming: bool,
) -> Option<KeyShortcut> {
    key_shortcut_bound(&bindings, &key, command, shift, alt, transforming)
}

/// [`key_shortcut_with`], with the bindings seen rather than taken.
pub fn key_shortcut_bound(
    bindings: &KeyBindings,
    key: &str,
    command: bool,
    shift: bool,
    alt: bool,
    transforming: bool,
) -> Option<KeyShortcut> {
    use KeyShortcut::*;
    let key = key.to_lowercase();
    if let Some(action) = bindings.lookup(&key, command, shift, alt) {
        return Some(action.shortcut());
    }
    let step = if command && !alt {
        0.01
    } else if shift {
        0.10
    } else {
        0.05
    };
    let nudge = |dx: f32, dy: f32| transforming.then_some(Nudge { dx, dy });
    match key.as_str() {
        "[" => Some(BrushSize { by: -step }),
        "]" => Some(BrushSize { by: step }),
        "left" => nudge(-1.0, 0.0),
        "right" => nudge(1.0, 0.0),
        "up" => nudge(0.0, -1.0),
        "down" => nudge(0.0, 1.0),
        _ => None,
    }
}

/// Something a key does that a press can be bound to: the shortcuts' list's commands. What takes a
/// step from the press itself (the brush's size, an arrow nudging a transform) is not among them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum KeyAction {
    Undo,
    Redo,
    CopyLayer,
    CutLayer,
    Paste,
    Paint,
    Erase,
    Eyedropper,
    Selection,
    Transform,
    HueSaturationBrightness,
    Colours,
    Actions,
    Layers,
    SwapColours,
    Deselect,
    DuplicateLayer,
    Interface,
    ClearLayer,
    CopyCanvas,
    ColourBalance,
    PerspectiveGuide,
    QuickMenu,
    GreyView,
}

impl KeyAction {
    /// Every action, in the list's order.
    pub const ALL: [KeyAction; 24] = [
        KeyAction::Undo,
        KeyAction::Redo,
        KeyAction::CopyLayer,
        KeyAction::CutLayer,
        KeyAction::Paste,
        KeyAction::Paint,
        KeyAction::Erase,
        KeyAction::Eyedropper,
        KeyAction::Selection,
        KeyAction::Transform,
        KeyAction::HueSaturationBrightness,
        KeyAction::Colours,
        KeyAction::Actions,
        KeyAction::Layers,
        KeyAction::SwapColours,
        KeyAction::Deselect,
        KeyAction::DuplicateLayer,
        KeyAction::Interface,
        KeyAction::ClearLayer,
        KeyAction::CopyCanvas,
        KeyAction::ColourBalance,
        KeyAction::PerspectiveGuide,
        KeyAction::QuickMenu,
        KeyAction::GreyView,
    ];

    /// The name the settings file knows it by (`key_undo`'s "undo").
    pub fn key(self) -> &'static str {
        match self {
            KeyAction::Undo => "undo",
            KeyAction::Redo => "redo",
            KeyAction::CopyLayer => "copy_layer",
            KeyAction::CutLayer => "cut_layer",
            KeyAction::Paste => "paste",
            KeyAction::Paint => "paint",
            KeyAction::Erase => "erase",
            KeyAction::Eyedropper => "eyedropper",
            KeyAction::Selection => "selection",
            KeyAction::Transform => "transform",
            KeyAction::HueSaturationBrightness => "hue_saturation_brightness",
            KeyAction::Colours => "colours",
            KeyAction::Actions => "actions",
            KeyAction::Layers => "layers",
            KeyAction::SwapColours => "swap_colours",
            KeyAction::Deselect => "deselect",
            KeyAction::DuplicateLayer => "duplicate_layer",
            KeyAction::Interface => "interface",
            KeyAction::ClearLayer => "clear_layer",
            KeyAction::CopyCanvas => "copy_canvas",
            KeyAction::ColourBalance => "colour_balance",
            KeyAction::PerspectiveGuide => "perspective_guide",
            KeyAction::QuickMenu => "quick_menu",
            KeyAction::GreyView => "grey_view",
        }
    }

    /// The action a settings file's name is, if it is one.
    pub fn from_key(name: &str) -> Option<KeyAction> {
        KeyAction::ALL.into_iter().find(|action| action.key() == name)
    }

    /// What pressing a key bound to it does.
    pub fn shortcut(self) -> KeyShortcut {
        match self {
            KeyAction::Undo => KeyShortcut::Undo,
            KeyAction::Redo => KeyShortcut::Redo,
            KeyAction::CopyLayer => KeyShortcut::CopyLayer,
            KeyAction::CutLayer => KeyShortcut::CutLayer,
            KeyAction::Paste => KeyShortcut::Paste,
            KeyAction::Paint => KeyShortcut::Paint,
            KeyAction::Erase => KeyShortcut::Erase,
            KeyAction::Eyedropper => KeyShortcut::Eyedropper,
            KeyAction::Selection => KeyShortcut::Selection,
            KeyAction::Transform => KeyShortcut::Transform,
            KeyAction::HueSaturationBrightness => KeyShortcut::HueSaturationBrightness,
            KeyAction::Colours => KeyShortcut::Colours,
            KeyAction::Actions => KeyShortcut::Actions,
            KeyAction::Layers => KeyShortcut::Layers,
            KeyAction::SwapColours => KeyShortcut::SwapColours,
            KeyAction::Deselect => KeyShortcut::Deselect,
            KeyAction::DuplicateLayer => KeyShortcut::DuplicateLayer,
            KeyAction::Interface => KeyShortcut::Interface,
            KeyAction::ClearLayer => KeyShortcut::ClearLayer,
            KeyAction::CopyCanvas => KeyShortcut::CopyCanvas,
            KeyAction::ColourBalance => KeyShortcut::ColourBalance,
            KeyAction::PerspectiveGuide => KeyShortcut::PerspectiveGuide,
            KeyAction::QuickMenu => KeyShortcut::QuickMenu,
            KeyAction::GreyView => KeyShortcut::GreyView,
        }
    }
}

/// Whether a key can be bound: a letter, a digit, or one of the keys the list names around them.
pub fn bindable_key(key: &str) -> bool {
    matches!(key, "space" | "delete" | "backspace" | "left" | "right" | "up" | "down" | "[" | "]" | ";" | "=" | "-")
        || (key.len() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// A press bound to an action: the key's name ([`KeyCombo`]) and the modifiers held with it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct KeyBinding {
    pub action: KeyAction,
    /// The presses that do it, any of them; none once it has been unbound.
    pub combos: Vec<KeyCombo>,
}

/// Every action's presses: what the shortcuts do now, as the list shows them and the keys do them.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct KeyBindings {
    pub bindings: Vec<KeyBinding>,
}

impl Default for KeyBindings {
    fn default() -> KeyBindings {
        KeyBindings::starting()
    }
}

impl KeyBindings {
    /// The presses the app starts with.
    pub fn starting() -> KeyBindings {
        let mut bindings: Vec<KeyBinding> = Vec::new();
        for entry in SHORTCUTS.iter().flat_map(|(_, entries)| *entries) {
            let Some(action) = entry.action else { continue };
            if bindings.iter().any(|b| b.action == action) {
                continue;
            }
            let combos = entry
                .presses
                .iter()
                .map(|&(command, shift, alt, key)| KeyCombo { key: key.to_lowercase(), command, shift, alt })
                .collect();
            bindings.push(KeyBinding { action, combos });
        }
        KeyBindings { bindings }
    }

    /// The action a press does, as the keys have always been read: a binding that names it
    /// exactly; with the command key, the same without Shift (so Ctrl+Shift+D deselects, as Ctrl+D
    /// does, while Ctrl+Shift+Z redoes, which is bound); otherwise the key alone (so Shift with D
    /// deselects too, and AltGr typing B paints). With the command key and no Alt, nothing falls
    /// back to the key alone: Ctrl+Space has never opened the QuickMenu.
    pub fn lookup(&self, key: &str, command: bool, shift: bool, alt: bool) -> Option<KeyAction> {
        let action = |command: bool, shift: bool, alt: bool| {
            self.bindings.iter().find(|b| b.combos.iter().any(|c| c.is(key, command, shift, alt))).map(|b| b.action)
        };
        action(command, shift, alt)
            .or_else(|| if command && !alt { action(true, false, alt) } else { action(false, false, false) })
    }

    /// The presses that do `action`.
    pub fn combos_for(&self, action: KeyAction) -> &[KeyCombo] {
        self.bindings.iter().find(|b| b.action == action).map_or(&[], |b| b.combos.as_slice())
    }

    /// Binds `combo` to `action`, taking it from whatever did it before.
    pub fn bind(&mut self, action: KeyAction, combo: KeyCombo) {
        for binding in &mut self.bindings {
            binding.combos.retain(|c| !c.same(&combo));
        }
        if let Some(binding) = self.bindings.iter_mut().find(|b| b.action == action) {
            binding.combos.push(combo);
        }
    }

    /// Takes every press that does `action` away from it.
    pub fn unbind(&mut self, action: KeyAction) {
        if let Some(binding) = self.bindings.iter_mut().find(|b| b.action == action) {
            binding.combos.clear();
        }
    }

    /// Back to the presses the app starts with.
    pub fn reset(&mut self) {
        *self = KeyBindings::starting();
    }

    /// A line of the settings file: "key_undo ctrl+z,ctrl+shift+z" (an action with no presses has
    /// an empty value). True if `name` was one.
    pub fn read_line(&mut self, name: &str, value: &str) -> bool {
        let Some(action) = name.strip_prefix("key_").and_then(KeyAction::from_key) else { return false };
        let combos: Vec<KeyCombo> =
            value.split(',').filter(|press| !press.is_empty()).filter_map(KeyCombo::parse).collect();
        if let Some(binding) = self.bindings.iter_mut().find(|b| b.action == action) {
            binding.combos = combos;
        }
        true
    }

    /// The settings file's lines, one an action.
    pub fn kept_lines(&self) -> Vec<String> {
        KeyAction::ALL
            .iter()
            .map(|&action| {
                let combos: Vec<String> = self.combos_for(action).iter().map(KeyCombo::text).collect();
                format!("key_{} {}", action.key(), combos.join(","))
            })
            .collect()
    }
}

/// How far off (window pixels) the point an arrow nudges towards is: far enough that the nudge
/// goes straight that way.
const FAR: f32 = 100_000.0;

/// What a mouse dragged on the canvas with a key held does, in place of painting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum KeyDrag {
    /// Space with the command key (Ctrl, ⌘): the canvas zooms in as the pointer goes right or up,
    /// and out as it goes left or down, about where it went down (e times every
    /// [`COMPANION_ZOOM_DISTANCE`] points, as the companion's Zoom).
    Zoom,
    /// R: the canvas turns about the window's middle as the pointer goes round it; with Shift it
    /// lands on the nearest 15° (the step R tapped turns it).
    Turn,
}

/// A key drag under way: what it does, where it went down and last was (window pixels), and for a
/// turn, the canvas's turn as it began and how far the pointer has gone round since (radians).
#[derive(Clone, Copy, Debug)]
pub(crate) struct KeyDragging {
    how: KeyDrag,
    from: (f32, f32),
    last: (f32, f32),
    start: f32,
    turned: f32,
}

/// `angle` (radians) the short way round: within half a turn either way.
fn short_way(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

#[uniffi::export]
impl CanvasSession {
    /// An arrow key while transforming: the content moves one canvas pixel the window's way
    /// (`dx`, `dy`). Returns the box.
    pub fn transform_arrow(&self, dx: f32, dy: f32) -> Option<TransformBox> {
        let (w, h) = self.shared.lock().window_size;
        self.transform_nudge(w as f32 / 2.0 + dx * FAR, h as f32 / 2.0 + dy * FAR)
    }

    /// R tapped: the canvas turns a step (15°) clockwise about the window's middle, or
    /// anticlockwise if `back` (Shift held).
    pub fn key_turn(&self, back: bool) {
        let (w, h) = self.shared.lock().window_size;
        let turn = if back { -TURN_STEP } else { TURN_STEP };
        self.view_gesture(w as f32 / 2.0, h as f32 / 2.0, 0.0, 0.0, 1.0, turn);
    }

    /// A mouse dragged with a key held goes down at (`x`, `y`) (window pixels): it does `how` until
    /// [`CanvasSession::key_drag_end`] (the platform sends it no touch, so it paints nothing).
    pub fn key_drag_begin(&self, how: KeyDrag, x: f32, y: f32) {
        if !(x.is_finite() && y.is_finite()) {
            return;
        }
        let mut st = self.shared.lock();
        let start = st.view.map_or(0.0, |v| v.rotation());
        st.key_drag = Some(KeyDragging { how, from: (x, y), last: (x, y), start, turned: 0.0 });
    }

    /// The key drag's pointer is at (`x`, `y`) (window pixels). A turn lands on the nearest 15°
    /// while `snap` (Shift held): asked again where the pointer is, Shift going down or up shows
    /// at once.
    pub fn key_drag_move(&self, x: f32, y: f32, snap: bool) {
        if !(x.is_finite() && y.is_finite()) {
            return;
        }
        let mut st = self.shared.lock();
        let Some(mut d) = st.key_drag else { return };
        let scale = st.display_scale;
        let (pivot, zoom, turn) = match d.how {
            KeyDrag::Zoom => {
                let along = (x - d.last.0) - (y - d.last.1);
                d.last = (x, y);
                (d.from, (along / (COMPANION_ZOOM_DISTANCE * scale)).exp(), 0.0)
            }
            KeyDrag::Turn => {
                let middle = (st.window_size.0 as f32 / 2.0, st.window_size.1 as f32 / 2.0);
                let (a, b) = ((d.last.0 - middle.0, d.last.1 - middle.1), (x - middle.0, y - middle.1));
                let still = TURN_STILL * scale;
                if b.0.hypot(b.1) >= still {
                    if a.0.hypot(a.1) >= still {
                        d.turned += short_way(b.1.atan2(b.0) - a.1.atan2(a.0));
                    }
                    d.last = (x, y);
                }
                let mut to = d.start + d.turned;
                if snap {
                    to = (to / TURN_STEP).round() * TURN_STEP;
                }
                let now = st.view.map_or(0.0, |v| v.rotation());
                (middle, 1.0, short_way(to - now))
            }
        };
        st.key_drag = Some(d);
        drop(st);
        self.view_gesture(pivot.0, pivot.1, 0.0, 0.0, zoom, turn);
    }

    /// The key drag ends (the mouse let go, or its key).
    pub fn key_drag_end(&self) {
        self.shared.lock().key_drag = None;
    }
}

/// A key pressed with modifiers, as [`key_shortcut`] names it: the key's name ("z", "[",
/// "delete", "left") and whether the command key (Ctrl, ⌘), Shift and Alt (⌥) are held.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct KeyCombo {
    pub key: String,
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
}

impl KeyCombo {
    /// Whether this is the press of `key` (either case) with exactly `command`, `shift` and `alt`.
    pub fn is(&self, key: &str, command: bool, shift: bool, alt: bool) -> bool {
        self.command == command && self.shift == shift && self.alt == alt && self.key.eq_ignore_ascii_case(key)
    }

    /// Whether this is the same press as `other`, in whatever case its key was written.
    pub fn same(&self, other: &KeyCombo) -> bool {
        self.is(&other.key, other.command, other.shift, other.alt)
    }

    /// The press for the settings file: "ctrl+shift+z", "space", "[", "ctrl+[".
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (on, name) in [(self.command, "ctrl"), (self.alt, "alt"), (self.shift, "shift")] {
            if on {
                out.push_str(name);
                out.push('+');
            }
        }
        out.push_str(&self.key);
        out
    }

    /// The press `text` names, if it names one: a key that can be bound, with "ctrl", "alt" and
    /// "shift" in front of it.
    pub fn parse(text: &str) -> Option<KeyCombo> {
        let mut parts: Vec<&str> = text.split('+').collect();
        let key = parts.pop()?.to_lowercase();
        if !bindable_key(&key) {
            return None;
        }
        let (mut command, mut shift, mut alt) = (false, false, false);
        for modifier in parts {
            match modifier {
                "ctrl" | "command" => command = true,
                "shift" => shift = true,
                "alt" => alt = true,
                _ => return None,
            }
        }
        Some(KeyCombo { key, command, shift, alt })
    }
}

/// A row of the keyboard shortcuts' list: its keys written out for the platform (`keys`), the
/// presses that do it (`combos`, none for a drag), what it does, and the action its keys can be
/// bound to again (`None` where the platform keeps them, as it does a drag).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ShortcutRow {
    pub keys: String,
    pub combos: Vec<KeyCombo>,
    pub does: String,
    pub action: Option<KeyAction>,
}

/// A group of the keyboard shortcuts' list, under its heading.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ShortcutGroup {
    pub heading: String,
    pub rows: Vec<ShortcutRow>,
}

/// One press in the list's table: modifiers (command, shift, alt) and the key's name.
type Press = (bool, bool, bool, &'static str);

const fn alone(key: &'static str) -> Press {
    (false, false, false, key)
}

const fn command(key: &'static str) -> Press {
    (true, false, false, key)
}

/// A row of the list's table: the presses it starts with (none for a drag, written as `drag`),
/// the action a press can be bound to (`None` where the platform keeps the presses), what it does,
/// and whether only the desktop has it.
struct Entry {
    action: Option<KeyAction>,
    presses: &'static [Press],
    drag: &'static str,
    does: &'static str,
    desktop: bool,
}

/// A row of a command a key can be bound to, with the presses it starts with.
const fn binding(action: KeyAction, presses: &'static [Press], does: &'static str) -> Entry {
    Entry { action: Some(action), presses, drag: "", does, desktop: false }
}

/// A row the platform keeps as it is.
const fn entry(presses: &'static [Press], does: &'static str) -> Entry {
    Entry { action: None, presses, drag: "", does, desktop: false }
}

const fn desktop(presses: &'static [Press], does: &'static str) -> Entry {
    Entry { action: None, presses, drag: "", does, desktop: true }
}

const fn drag(keys: &'static str, does: &'static str) -> Entry {
    Entry { action: None, presses: &[], drag: keys, does, desktop: true }
}

/// The list, by group ("{command}" in a drag's keys: the platform's command key). A row's presses
/// are the ones it starts with; a row with an action is the ones it has now.
const SHORTCUTS: &[(&str, &[Entry])] = &[
    (
        "Edit",
        &[
            binding(KeyAction::Undo, &[command("z")], "Undo"),
            binding(KeyAction::Redo, &[(true, true, false, "z")], "Redo"),
            binding(KeyAction::CopyLayer, &[command("c")], "Copy the layer"),
            binding(KeyAction::CutLayer, &[command("x")], "Cut the layer"),
            binding(KeyAction::Paste, &[command("v")], "Paste"),
            binding(KeyAction::CopyCanvas, &[command("a")], "Copy the canvas"),
            binding(KeyAction::DuplicateLayer, &[alone("j"), command("j")], "Duplicate the layer"),
            binding(KeyAction::ClearLayer, &[command("delete"), command("backspace")], "Clear the layer"),
            binding(KeyAction::Deselect, &[alone("d"), command("d")], "Deselect"),
        ],
    ),
    (
        "Tools",
        &[
            binding(KeyAction::Paint, &[alone("b")], "Paint"),
            binding(KeyAction::Erase, &[alone("e")], "Erase"),
            binding(KeyAction::Eyedropper, &[alone("m")], "Eyedropper: the next touch picks a colour"),
            binding(KeyAction::Selection, &[alone("s")], "Selection"),
            binding(KeyAction::Transform, &[alone("v")], "Transform"),
            binding(KeyAction::HueSaturationBrightness, &[alone("u"), command("u")], "Hue, Saturation, Brightness"),
            binding(KeyAction::ColourBalance, &[command("b")], "Color Balance"),
            binding(KeyAction::PerspectiveGuide, &[command(";")], "Perspective guide on or off"),
            binding(KeyAction::SwapColours, &[alone("x")], "Swap the colours"),
            entry(&[alone("left"), alone("right"), alone("up"), alone("down")], "Transforming: move it a pixel"),
        ],
    ),
    (
        "Panels",
        &[
            binding(KeyAction::Colours, &[alone("c")], "Colours"),
            binding(KeyAction::Actions, &[alone("k"), command("k")], "Actions"),
            binding(KeyAction::Layers, &[alone("l")], "Layers"),
            binding(KeyAction::QuickMenu, &[alone("space")], "QuickMenu"),
            binding(KeyAction::Interface, &[alone("0"), command("0")], "Hide or show the interface"),
        ],
    ),
    (
        "Brush size",
        &[
            entry(&[command("["), command("]")], "Smaller or bigger by 1%"),
            entry(&[alone("["), alone("]")], "By 5%"),
            entry(&[(false, true, false, "["), (false, true, false, "]")], "By 10%"),
        ],
    ),
    (
        "View",
        &[
            binding(KeyAction::GreyView, &[command("g")], "Grey view, to check values"),
            desktop(&[command("="), command("-")], "Zoom in or out"),
            desktop(&[command("0")], "Fit the canvas"),
            drag("{command}Space and drag", "Zoom"),
            desktop(&[alone("r"), (false, true, false, "r")], "Turn the canvas 15° (with Shift, back)"),
            drag("R and drag", "Turn the canvas (with Shift, in 15° steps)"),
            desktop(&[alone("escape")], "Close what is open, or set the canvas upright"),
        ],
    ),
    (
        "Files",
        &[
            desktop(&[command("n")], "New canvas"),
            desktop(&[command("o")], "Open or import"),
            desktop(&[command("s")], "Save"),
            desktop(&[(true, true, false, "s")], "Save as"),
        ],
    ),
];

/// How `key` (a name as [`key_shortcut`] takes it) is written: a Mac's symbols if `mac`.
fn key_label(key: &str, mac: bool) -> String {
    match (key, mac) {
        ("delete", false) => "Delete".into(),
        ("delete", true) => "⌦".into(),
        ("backspace", false) => "Backspace".into(),
        ("backspace", true) => "⌫".into(),
        ("escape", false) => "Esc".into(),
        ("escape", true) => "⎋".into(),
        ("left", _) => "←".into(),
        ("right", _) => "→".into(),
        ("up", _) => "↑".into(),
        ("down", _) => "↓".into(),
        ("space", _) => "Space".into(),
        ("=", _) => "+".into(),
        ("-", _) => "−".into(),
        _ => key.to_uppercase(),
    }
}

/// A press written out: "Ctrl+Shift+Z", or on a Mac "⇧⌘Z" (its modifiers in its own order).
fn press_label(command: bool, shift: bool, alt: bool, key: &str, mac: bool) -> String {
    let key = key_label(key, mac);
    if mac {
        let mods = [(alt, "⌥"), (shift, "⇧"), (command, "⌘")];
        let mut s: String = mods.iter().filter(|(on, _)| *on).map(|(_, m)| *m).collect();
        s.push_str(&key);
        s
    } else {
        let mods = [(command, "Ctrl"), (alt, "Alt"), (shift, "Shift")];
        let mut parts: Vec<&str> = mods.iter().filter(|(on, _)| *on).map(|(_, m)| *m).collect();
        parts.push(&key);
        parts.join("+")
    }
}

/// The keyboard shortcuts, grouped, as a list to show with the presses the app starts with.
#[uniffi::export]
pub fn key_shortcut_list(desktop: bool, mac: bool) -> Vec<ShortcutGroup> {
    key_shortcut_list_with(desktop, mac, KeyBindings::default())
}

/// The keyboard shortcuts, grouped, as a list to show: each row's presses as `bindings` has them
/// where a key can be bound to it (and "Unbound" where it has none); with the desktop's own too
/// (its files, zooming and turning the view) if `desktop`; the keys written as a Mac's (⌘, ⇧, ⌥)
/// if `mac`, else "Ctrl+", "Shift+", "Alt+".
#[uniffi::export]
pub fn key_shortcut_list_with(desktop: bool, mac: bool, bindings: KeyBindings) -> Vec<ShortcutGroup> {
    key_shortcut_list_bound(desktop, mac, &bindings)
}

/// [`key_shortcut_list_with`], with the bindings seen rather than taken.
pub fn key_shortcut_list_bound(desktop: bool, mac: bool, bindings: &KeyBindings) -> Vec<ShortcutGroup> {
    SHORTCUTS
        .iter()
        .filter_map(|(heading, entries)| {
            let rows: Vec<ShortcutRow> = entries
                .iter()
                .filter(|e| desktop || !e.desktop)
                .map(|e| {
                    let presses: Vec<(bool, bool, bool, &str)> = match e.action {
                        Some(action) => bindings
                            .combos_for(action)
                            .iter()
                            .map(|c| (c.command, c.shift, c.alt, c.key.as_str()))
                            .collect(),
                        None => e.presses.to_vec(),
                    };
                    let keys = if presses.is_empty() {
                        if e.drag.is_empty() {
                            "Unbound".to_string()
                        } else {
                            e.drag.replace("{command}", if mac { "⌘" } else { "Ctrl+" })
                        }
                    } else {
                        presses
                            .iter()
                            .map(|&(command, shift, alt, key)| press_label(command, shift, alt, key, mac))
                            .collect::<Vec<_>>()
                            .join("  ")
                    };
                    let combos = presses
                        .iter()
                        .map(|&(command, shift, alt, key)| KeyCombo { key: key.to_lowercase(), command, shift, alt })
                        .collect();
                    ShortcutRow { keys, combos, does: e.does.into(), action: e.action }
                })
                .collect();
            (!rows.is_empty()).then(|| ShortcutGroup { heading: (*heading).into(), rows })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::KeyShortcut::*;
    use super::*;

    fn key(k: &str, command: bool, shift: bool, alt: bool) -> Option<KeyShortcut> {
        key_shortcut(k.into(), command, shift, alt, false)
    }

    /// The press `text` names, as the settings file writes it.
    fn press(text: &str) -> KeyCombo {
        KeyCombo::parse(text).unwrap_or_else(|| panic!("{text} is no press"))
    }

    #[test]
    fn the_presses_bound_do_what_they_did_before_they_could_be_bound() {
        let bindings = KeyBindings::default();
        let at = |k: &str, command: bool, shift: bool, alt: bool| {
            key_shortcut_bound(&bindings, k, command, shift, alt, false)
        };
        // with the command key
        for (k, does) in [
            ("z", Undo),
            ("c", CopyLayer),
            ("x", CutLayer),
            ("v", Paste),
            ("a", CopyCanvas),
            ("b", ColourBalance),
            ("u", HueSaturationBrightness),
            ("d", Deselect),
            ("j", DuplicateLayer),
            ("k", Actions),
            ("0", Interface),
            (";", PerspectiveGuide),
            ("g", GreyView),
        ] {
            assert_eq!(at(k, true, false, false), Some(does), "{k}");
        }
        assert_eq!(at("z", true, true, false), Some(Redo));
        for k in ["delete", "backspace"] {
            assert_eq!(at(k, true, false, false), Some(ClearLayer), "{k}");
        }
        // alone
        for (k, does) in [
            ("b", Paint),
            ("e", Erase),
            ("m", Eyedropper),
            ("s", Selection),
            ("v", Transform),
            ("c", Colours),
            ("x", SwapColours),
            ("l", Layers),
            ("space", QuickMenu),
            ("0", Interface),
        ] {
            assert_eq!(at(k, false, false, false), Some(does), "{k}");
        }
        // a modifier no binding names narrows nothing: Shift with D still deselects, and AltGr
        // typing B still paints
        assert_eq!(at("d", false, true, false), Some(Deselect));
        assert_eq!(at("d", true, true, false), Some(Deselect));
        assert_eq!(at("b", true, false, true), Some(Paint));
        // and with the command key nothing falls back to the key alone
        assert_eq!(at("space", true, false, false), None);
        assert_eq!(at("e", true, false, false), None);
        assert_eq!(at("q", false, false, false), None);
    }

    #[test]
    fn a_key_bound_again_does_what_it_was_bound_to_and_the_list_says_so() {
        let mut bindings = KeyBindings::default();
        bindings.bind(KeyAction::Paint, press("p"));
        assert_eq!(key_shortcut_bound(&bindings, "p", false, false, false, false), Some(Paint));
        // P was nothing before: it is Paint's now, and B is still the one it started as
        assert_eq!(key_shortcut_bound(&bindings, "b", false, false, false, false), Some(Paint));
        let row = key_shortcut_list_with(true, false, bindings)
            .iter()
            .flat_map(|g| &g.rows)
            .find(|r| r.does == "Paint")
            .cloned()
            .unwrap();
        assert_eq!(row.keys, "B  P");
        assert_eq!(row.combos, vec![press("b"), press("p")]);
        assert_eq!(row.action, Some(KeyAction::Paint));
    }

    #[test]
    fn binding_a_press_takes_it_from_what_did_it_before() {
        let mut bindings = KeyBindings::default();
        bindings.bind(KeyAction::Erase, press("ctrl+z"));
        assert!(bindings.combos_for(KeyAction::Undo).is_empty(), "Ctrl+Z is Erase's now");
        assert_eq!(bindings.lookup("z", true, false, false), Some(KeyAction::Erase));
        // given back, Undo has it again and Erase has not
        bindings.reset();
        assert_eq!(bindings.lookup("z", true, false, false), Some(KeyAction::Undo));
        assert_eq!(bindings.lookup("e", false, false, false), Some(KeyAction::Erase));
    }

    #[test]
    fn a_command_unbound_has_no_keys_and_does_nothing() {
        let mut bindings = KeyBindings::default();
        bindings.unbind(KeyAction::Layers);
        assert_eq!(key_shortcut_bound(&bindings, "l", false, false, false, false), None);
        let row = key_shortcut_list_with(false, false, bindings)
            .iter()
            .flat_map(|g| &g.rows)
            .find(|r| r.does == "Layers")
            .cloned()
            .unwrap();
        assert_eq!(row.keys, "Unbound");
        assert!(row.combos.is_empty());
    }

    #[test]
    fn the_bindings_keep_and_come_back_as_they_were() {
        let mut bindings = KeyBindings::default();
        bindings.bind(KeyAction::Erase, press("ctrl+shift+e"));
        bindings.unbind(KeyAction::QuickMenu);
        let mut read = KeyBindings::default();
        for line in bindings.kept_lines() {
            let (name, value) = line.split_once(' ').unwrap();
            assert!(read.read_line(name, value), "{name}");
        }
        assert_eq!(read, bindings);
        // what is not a binding is left for the plugins, as before
        assert!(!read.read_line("something_else", "x"));
        assert!(read.read_line("key_undo", ""));
        assert!(read.combos_for(KeyAction::Undo).is_empty());
    }

    #[test]
    fn a_press_is_written_and_read_back_as_it_is() {
        for text in ["ctrl+shift+z", "space", "[", "ctrl+[", "alt+m", "ctrl+alt+0", "left"] {
            assert_eq!(press(text).text(), text, "{text}");
        }
        for text in ["", "ctrl+", "hyper+z", "ctrl+,", "z z"] {
            assert_eq!(KeyCombo::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn the_command_key_undoes_and_moves_layers_and_alone_the_letters_choose() {
        assert_eq!(key("Z", true, false, false), Some(Undo));
        assert_eq!(key("z", true, true, false), Some(Redo));
        assert_eq!(key("v", true, false, false), Some(Paste));
        assert_eq!(key("g", true, false, false), Some(GreyView));
        assert_eq!(key("y", true, false, false), None);
        assert_eq!(key("v", false, false, false), Some(Transform));
        assert_eq!(key("S", false, true, false), Some(Selection));
        assert_eq!(key("0", false, false, false), Some(Interface));
        assert_eq!(key("q", false, false, false), None);
    }

    #[test]
    fn the_command_key_reaches_what_alone_chooses_something_else() {
        assert_eq!(key("a", true, false, false), Some(CopyCanvas));
        assert_eq!(key("b", true, false, false), Some(ColourBalance));
        assert_eq!(key("b", false, false, false), Some(Paint));
        assert_eq!(key(";", true, false, false), Some(PerspectiveGuide));
        assert_eq!(key("space", false, false, false), Some(QuickMenu));
        for (k, shortcut) in [("k", Actions), ("u", HueSaturationBrightness), ("j", DuplicateLayer), ("0", Interface)] {
            assert_eq!(key(k, true, false, false), Some(shortcut));
            assert_eq!(key(k, false, false, false), Some(shortcut));
        }
    }

    #[test]
    fn clearing_a_layer_takes_the_command_key() {
        for k in ["delete", "backspace"] {
            assert_eq!(key(k, true, false, false), Some(ClearLayer));
            assert_eq!(key(k, false, false, false), None);
        }
    }

    #[test]
    fn the_list_shows_each_canvas_shortcut_as_the_keys_do_it() {
        let list = key_shortcut_list(false, false);
        let rows: Vec<&ShortcutRow> = list.iter().flat_map(|g| &g.rows).collect();
        // each press the list shows is one the canvas takes (the arrows while transforming)
        for row in &rows {
            assert!(!row.combos.is_empty(), "{}", row.does);
            for c in &row.combos {
                assert!(key_shortcut(c.key.clone(), c.command, c.shift, c.alt, true).is_some(), "{c:?}");
            }
        }
        let keys = |does: &str| rows.iter().find(|r| r.does == does).map(|r| r.keys.clone());
        assert_eq!(keys("Redo").as_deref(), Some("Ctrl+Shift+Z"));
        assert_eq!(keys("Clear the layer").as_deref(), Some("Ctrl+Delete  Ctrl+Backspace"));
        // the desktop's own only on the desktop, a Mac's keys as its symbols
        assert_eq!(keys("Save"), None);
        let mac = key_shortcut_list(true, true);
        let row = |does: &str| mac.iter().flat_map(|g| &g.rows).find(|r| r.does == does).cloned().unwrap();
        assert_eq!(row("Redo").keys, "⇧⌘Z");
        assert_eq!(row("By 10%").keys, "⇧[  ⇧]");
        assert_eq!(row("Save as").keys, "⇧⌘S");
        assert_eq!(row("Zoom").keys, "⌘Space and drag");
        assert!(row("Zoom").combos.is_empty());
    }

    #[test]
    fn d_deselects_alone_with_shift_and_with_the_command_key() {
        assert_eq!(key("d", false, false, false), Some(Deselect));
        assert_eq!(key("D", false, true, false), Some(Deselect));
        assert_eq!(key("d", true, false, false), Some(Deselect));
        assert_eq!(key("D", true, true, false), Some(Deselect));
    }

    #[test]
    fn brackets_step_the_size_and_arrows_nudge_only_a_transform() {
        assert_eq!(key("[", true, false, false), Some(BrushSize { by: -0.01 }));
        assert_eq!(key("]", false, false, false), Some(BrushSize { by: 0.05 }));
        assert_eq!(key("]", false, true, false), Some(BrushSize { by: 0.10 }));
        // AltGr typing a bracket is the bracket alone
        assert_eq!(key("[", true, false, true), Some(BrushSize { by: -0.05 }));
        assert_eq!(key("left", false, false, false), None);
        assert_eq!(key_shortcut("up".into(), false, false, false, true), Some(Nudge { dx: 0.0, dy: -1.0 }));
    }

    use procraft_engine::geom::Vec2;
    use procraft_engine::view::View;

    /// A session (no GPU) with a 400 square window, its 100 square canvas fitted upright (zoom 4,
    /// the window's middle at (200, 200)); two window pixels a point.
    fn viewed() -> std::sync::Arc<CanvasSession> {
        let s = CanvasSession::new(100, 100);
        {
            let mut st = s.shared.lock();
            st.window_size = (400, 400);
            st.view = Some(View::fit((100, 100), (400, 400)));
            st.display_scale = 2.0;
        }
        s
    }

    fn view(s: &CanvasSession) -> View {
        s.shared.lock().view.unwrap()
    }

    fn degrees(s: &CanvasSession) -> f32 {
        view(s).rotation().to_degrees()
    }

    /// The window point `r` from the middle at `angle` degrees (clockwise from the right).
    fn round_middle(r: f32, angle: f32) -> (f32, f32) {
        let (sin, cos) = angle.to_radians().sin_cos();
        (200.0 + r * cos, 200.0 + r * sin)
    }

    /// Where canvas point `c` shows in the window.
    fn shown(s: &CanvasSession, c: Vec2) -> Vec2 {
        view(s).to_screen().apply(c)
    }

    #[test]
    fn space_with_the_command_key_dragged_zooms_about_where_it_went_down() {
        let s = viewed();
        let (x, y) = (100.0, 300.0);
        let there = view(&s).screen_to_canvas(Vec2::new(x, y)).unwrap();
        s.key_drag_begin(KeyDrag::Zoom, x, y);
        // right zooms in, e times every COMPANION_ZOOM_DISTANCE points, however it gets there
        let far = COMPANION_ZOOM_DISTANCE * 2.0;
        s.key_drag_move(x + far / 3.0, y, false);
        s.key_drag_move(x + far, y, false);
        assert!((view(&s).zoom() - 4.0 * std::f32::consts::E).abs() < 1e-3, "{}", view(&s).zoom());
        // up as far again zooms in as much again, and what it went down on stays where it was
        s.key_drag_move(x + far, y - far, false);
        assert!((view(&s).zoom() - 4.0 * std::f32::consts::E.powi(2)).abs() < 1e-2, "{}", view(&s).zoom());
        let p = shown(&s, there);
        assert!((p.x - x).abs() < 1e-2 && (p.y - y).abs() < 1e-2, "{p:?}");
        // left and down past where it began zooms out; back there, it is as it was
        s.key_drag_move(x - far, y + far, false);
        assert!((view(&s).zoom() - 4.0 / std::f32::consts::E.powi(2)).abs() < 1e-3, "{}", view(&s).zoom());
        s.key_drag_move(x, y, false);
        assert!((view(&s).zoom() - 4.0).abs() < 1e-3);
        assert_eq!(degrees(&s), 0.0);
        // once it ends, moves do nothing
        s.key_drag_end();
        s.key_drag_move(x + far, y, false);
        assert!((view(&s).zoom() - 4.0).abs() < 1e-3);
    }

    #[test]
    fn r_dragged_turns_the_canvas_round_the_middle_as_the_pointer_goes_round() {
        let s = viewed();
        let middle = view(&s).screen_to_canvas(Vec2::new(200.0, 200.0)).unwrap();
        let (x, y) = round_middle(100.0, 0.0);
        s.key_drag_begin(KeyDrag::Turn, x, y);
        for angle in [10.0, 25.0, 40.0] {
            let (x, y) = round_middle(100.0, angle);
            s.key_drag_move(x, y, false);
        }
        assert!((degrees(&s) - 40.0).abs() < 1e-2, "{}", degrees(&s));
        // nearer the middle, or further out, only the angle counts; the middle stays put
        let (x, y) = round_middle(20.0, 50.0);
        s.key_drag_move(x, y, false);
        assert!((degrees(&s) - 50.0).abs() < 1e-2, "{}", degrees(&s));
        let p = shown(&s, middle);
        assert!((p.x - 200.0).abs() < 1e-2 && (p.y - 200.0).abs() < 1e-2, "{p:?}");
        assert!((view(&s).zoom() - 4.0).abs() < 1e-4);
        // right by the middle (within 8 points) the angle jumps about: it turns nothing
        s.key_drag_move(203.0, 205.0, false);
        assert!((degrees(&s) - 50.0).abs() < 1e-2, "{}", degrees(&s));
        // on round past half a turn, and back anticlockwise
        for angle in [120.0, 190.0, 230.0] {
            let (x, y) = round_middle(100.0, angle);
            s.key_drag_move(x, y, false);
        }
        assert!((degrees(&s) + 130.0).abs() < 1e-2, "{}", degrees(&s));
        for angle in [100.0, 20.0, -20.0] {
            let (x, y) = round_middle(100.0, angle);
            s.key_drag_move(x, y, false);
        }
        assert!((degrees(&s) + 20.0).abs() < 1e-2, "{}", degrees(&s));
        s.key_drag_end();
    }

    #[test]
    fn r_dragged_with_shift_lands_on_15_degree_steps() {
        let s = viewed();
        // turned 7° to begin with: Shift alone puts it on the nearest step, and let go back
        s.view_gesture(200.0, 200.0, 0.0, 0.0, 1.0, 7f32.to_radians());
        let (x, y) = round_middle(100.0, 0.0);
        s.key_drag_begin(KeyDrag::Turn, x, y);
        s.key_drag_move(x, y, true);
        assert!(degrees(&s).abs() < 1e-3, "{}", degrees(&s));
        s.key_drag_move(x, y, false);
        assert!((degrees(&s) - 7.0).abs() < 1e-3, "{}", degrees(&s));
        // the pointer 30° on: 37°, with Shift 30°; 40° on: 47°, with Shift 45°
        let (x, y) = round_middle(100.0, 30.0);
        s.key_drag_move(x, y, true);
        assert!((degrees(&s) - 30.0).abs() < 1e-3, "{}", degrees(&s));
        let (x, y) = round_middle(100.0, 40.0);
        s.key_drag_move(x, y, true);
        assert!((degrees(&s) - 45.0).abs() < 1e-3, "{}", degrees(&s));
        s.key_drag_move(x, y, false);
        assert!((degrees(&s) - 47.0).abs() < 1e-2, "{}", degrees(&s));
        s.key_drag_end();
    }

    #[test]
    fn r_tapped_turns_a_step_and_with_shift_back() {
        let s = viewed();
        s.key_turn(false);
        assert!((degrees(&s) - 15.0).abs() < 1e-3, "{}", degrees(&s));
        s.key_turn(true);
        s.key_turn(true);
        assert!((degrees(&s) + 15.0).abs() < 1e-3, "{}", degrees(&s));
        // about the middle, the zoom kept
        let p = shown(&s, Vec2::new(50.0, 50.0));
        assert!((p.x - 200.0).abs() < 1e-2 && (p.y - 200.0).abs() < 1e-2, "{p:?}");
        assert!((view(&s).zoom() - 4.0).abs() < 1e-4);
    }

    #[test]
    fn upright_again_keeps_the_zoom_and_what_is_in_the_middle() {
        let s = viewed();
        assert!(!s.settle_upright(), "upright already");
        s.view_gesture(120.0, 90.0, 30.0, -20.0, 1.5, 0.7);
        let middle = view(&s).screen_to_canvas(Vec2::new(200.0, 200.0)).unwrap();
        assert!(s.settle_upright());
        std::thread::sleep(std::time::Duration::from_secs_f32(s.view_settle_seconds()));
        s.shared.lock().step_view_motion();
        assert!(degrees(&s).abs() < 1e-3, "{}", degrees(&s));
        assert!((view(&s).zoom() - 6.0).abs() < 1e-3, "{}", view(&s).zoom());
        let p = shown(&s, middle);
        assert!((p.x - 200.0).abs() < 1e-2 && (p.y - 200.0).abs() < 1e-2, "{p:?}");
        assert!(!s.settle_upright());
    }
}
