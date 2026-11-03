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
/// D deselects. Alone: B paints, E erases, M the eyedropper, S selection, V transform, U Hue, Saturation,
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
            "d" => Some(Deselect),
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
        assert_eq!(key("v", false, false, false), Some(Transform));
        assert_eq!(key("b", true, false, false), None);
        assert_eq!(key("S", false, true, false), Some(Selection));
        assert_eq!(key("0", false, false, false), Some(Interface));
        assert_eq!(key("backspace", false, false, false), Some(ClearLayer));
        assert_eq!(key("q", false, false, false), None);
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
        assert_eq!(key("[", false, false, false), Some(BrushSize { by: -0.01 }));
        assert_eq!(key("]", false, true, false), Some(BrushSize { by: 0.05 }));
        assert_eq!(key("]", false, true, true), Some(BrushSize { by: 0.10 }));
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
