//! A hardware keyboard's shortcuts on the canvas: which key does what. The platforms name the key
//! and do what it says (as they do the touches' commands); typing in a text field never comes
//! here.

use crate::{CanvasSession, TransformBox};

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

/// The shortcut on `key`, if any: a letter or digit (either case), "[", "]", "delete",
/// "backspace", or an arrow ("left", "right", "up", "down"); `command` is Ctrl (⌘), and the
/// arrows nudge only while `transforming`.
///
/// With the command key: Z undoes (with Shift redoes), C, X and V copy, cut and paste the layer,
/// G shows the canvas in perceptual greys (as Krita's soft proofing does).
/// Alone: B paints, E erases, M the eyedropper, S selection, V transform, U Hue, Saturation,
/// Brightness, C the colours, X swaps them, D deselects, J duplicates the layer, K Actions,
/// L Layers, 0 the interface, Delete (Backspace) clears the layer, [ and ] the brush's size by
/// 1% (with Shift 5%, with Alt 10%).
#[uniffi::export]
pub fn key_shortcut(key: String, command: bool, shift: bool, alt: bool, transforming: bool) -> Option<KeyShortcut> {
    use KeyShortcut::*;
    let key = key.to_lowercase();
    if command {
        return match key.as_str() {
            "z" if shift => Some(Redo),
            "z" => Some(Undo),
            "c" => Some(CopyLayer),
            "x" => Some(CutLayer),
            "v" => Some(Paste),
            "g" => Some(GreyView),
            _ => None,
        };
    }
    let step = if alt {
        0.10
    } else if shift {
        0.05
    } else {
        0.01
    };
    let nudge = |dx: f32, dy: f32| transforming.then_some(Nudge { dx, dy });
    match key.as_str() {
        "b" => Some(Paint),
        "e" => Some(Erase),
        "m" => Some(Eyedropper),
        "s" => Some(Selection),
        "v" => Some(Transform),
        "u" => Some(HueSaturationBrightness),
        "c" => Some(Colours),
        "x" => Some(SwapColours),
        "d" => Some(Deselect),
        "j" => Some(DuplicateLayer),
        "k" => Some(Actions),
        "l" => Some(Layers),
        "0" => Some(Interface),
        "delete" | "backspace" => Some(ClearLayer),
        "[" => Some(BrushSize { by: -step }),
        "]" => Some(BrushSize { by: step }),
        "left" => nudge(-1.0, 0.0),
        "right" => nudge(1.0, 0.0),
        "up" => nudge(0.0, -1.0),
        "down" => nudge(0.0, 1.0),
        _ => None,
    }
}

/// How far off (window pixels) the point an arrow nudges towards is: far enough that the nudge
/// goes straight that way.
const FAR: f32 = 100_000.0;

#[uniffi::export]
impl CanvasSession {
    /// An arrow key while transforming: the content moves one canvas pixel the window's way
    /// (`dx`, `dy`). Returns the box.
    pub fn transform_arrow(&self, dx: f32, dy: f32) -> Option<TransformBox> {
        let (w, h) = self.shared.lock().window_size;
        self.transform_nudge(w as f32 / 2.0 + dx * FAR, h as f32 / 2.0 + dy * FAR)
    }
}

#[cfg(test)]
mod tests {
    use super::KeyShortcut::*;
    use super::*;

    fn key(k: &str, command: bool, shift: bool, alt: bool) -> Option<KeyShortcut> {
        key_shortcut(k.into(), command, shift, alt, false)
    }

    #[test]
    fn the_command_key_undoes_and_moves_layers_and_alone_the_letters_choose() {
        assert_eq!(key("Z", true, false, false), Some(Undo));
        assert_eq!(key("z", true, true, false), Some(Redo));
        assert_eq!(key("v", true, false, false), Some(Paste));
        assert_eq!(key("g", true, false, false), Some(GreyView));
        assert_eq!(key("y", true, false, false), None);
        assert_eq!(key("v", false, false, false), Some(Transform));
        assert_eq!(key("b", true, false, false), None);
        assert_eq!(key("S", false, true, false), Some(Selection));
        assert_eq!(key("0", false, false, false), Some(Interface));
        assert_eq!(key("backspace", false, false, false), Some(ClearLayer));
        assert_eq!(key("q", false, false, false), None);
    }

    #[test]
    fn brackets_step_the_size_and_arrows_nudge_only_a_transform() {
        assert_eq!(key("[", false, false, false), Some(BrushSize { by: -0.01 }));
        assert_eq!(key("]", false, true, false), Some(BrushSize { by: 0.05 }));
        assert_eq!(key("]", false, true, true), Some(BrushSize { by: 0.10 }));
        assert_eq!(key("left", false, false, false), None);
        assert_eq!(key_shortcut("up".into(), false, false, false, true), Some(Nudge { dx: 0.0, dy: -1.0 }));
    }
}
