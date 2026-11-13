//! The pen's own gestures beside its touches, the same for every app: held over the canvas, its
//! double-tap (or side button) and its squeeze.
//!
//! Hovering, the brush's outline shows where the pen would paint ([`CanvasSession::pen_hover_cursor`]):
//! it fades in once the pen has hovered a moment, back out while it races across, and out as it
//! rises to the top of the height it is sensed at. Hover's touch works while it is there.
//!
//! Gesture controls say what a double-tap and a squeeze do. Left alone there, they do what the
//! platform's own pen settings ask ([`PenPreferred`]); a double-tap turned off there does nothing
//! at all.
//!
//! A pen's button can be held down, as a double-tap can't: set to erase (Erase's Pen button), the
//! eraser is borrowed while it is held and given back as it is let go.
//!
//! A squeeze is held. Set to the eyedropper, its loupe shows under the pen while the pen hovers
//! near and follows it, and letting go takes the colour there; the pen touching meanwhile picks
//! with that touch; a quick squeeze with the pen away opens the eyedropper for the next touch (or
//! closes it). Set to Layer Select, the layer drawn under the pen is chosen as it moves over the
//! canvas. Set to the quick menu, it opens where the pen is. Set to QuickShape, letting go turns
//! the last stroke into a shape. A squeeze while a stroke is painted does nothing.

use super::{CanvasSession, GestureAction, Mode, TAP_SLOP, ToolKind, TouchCommand, Touches, dist};

/// A squeeze let go within this long (milliseconds) is a quick one.
const QUICK_SQUEEZE_MS: f64 = 300.0;
/// Higher than this (a share of the height the pen is sensed at), or with no height, the pen is
/// too far to pick a colour or a layer where it hovers.
const NEAR: f32 = 0.99;
/// The hover cursor shows once the pen has hovered this long (seconds), and fully this long after.
const HOVER_SHOWS_S: f32 = 0.1;
const HOVER_FADES_IN_S: f32 = 0.125;
/// Hovering faster than this (points a second), the cursor fades back, the faster the quicker.
const HOVER_RACING: f32 = 3600.0;
/// The cursor fades out over the top third of the height the pen is sensed at.
const HOVER_HEIGHT_FADE: f32 = 3.0;
/// A step between hover events counts this long at most (seconds): the pen out of sight meanwhile.
const HOVER_STEP_MOST_S: f32 = 1.0 / 30.0;
/// Layer Select under a squeeze chooses again once the pen has moved this far (points).
const SELECT_STEP: f32 = TAP_SLOP;

/// What the platform's own pen settings ask a double-tap or a squeeze to do, followed when Gesture
/// controls leave the gesture alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PenPreferred {
    /// Turned off.
    Ignore,
    /// Between the current tool and the eraser.
    SwitchEraser,
    /// Between the current tool and the one used before it.
    SwitchPrevious,
    ShowColours,
    /// The brush's attributes: the brush library.
    ShowInkAttributes,
    /// What can be done where the pen is: the quick menu, there.
    ShowContextualPalette,
    /// The system runs a shortcut of its own: nothing for the app.
    RunSystemShortcut,
}

/// How far along a squeeze is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SqueezePhase {
    Began,
    Changed,
    Ended,
    Cancelled,
}

/// The pen over the canvas: where (window pixels), how high (a share of the height it is sensed
/// at, 0..1; 0 where the platform doesn't say), and when (the event clock, milliseconds).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct PenHover {
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub time_ms: f64,
}

impl PenHover {
    fn at(&self) -> (f32, f32) {
        (self.x, self.y)
    }

    fn sound(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.height.is_finite() && self.time_ms.is_finite()
    }

    /// Near enough the glass to pick where it is.
    fn near(&self) -> bool {
        self.height > 0.0 && self.height < NEAR
    }
}

/// The brush's outline under the hovering pen: where (window pixels), and how strongly it shows
/// (0..1, none at 0).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct HoverCursor {
    pub x: f32,
    pub y: f32,
    pub alpha: f32,
}

/// The pen's hover and squeeze, kept with the touches.
#[derive(Default)]
pub(super) struct PenState {
    hover: Option<PenHover>,
    /// How long the pen has hovered, seconds (0..1): less while it races.
    settled: f32,
    squeeze: Option<Squeeze>,
}

impl PenState {
    /// The pen touches while squeezed for the eyedropper: the touch picks, in place of the squeeze.
    pub(super) fn squeeze_takes_touch(&mut self) -> bool {
        match self.squeeze.as_mut() {
            Some(s) if s.action == Some(GestureAction::Eyedropper) && !s.taken => {
                s.taken = true;
                s.picking = false;
                true
            }
            _ => false,
        }
    }
}

/// The pen's button held down to erase: which (its `second`), and the tool it borrowed the eraser
/// from, given back as it is let go. Borrowed, the eraser doesn't count as a tool chosen.
#[derive(Clone, Copy, Debug)]
pub(super) struct HeldEraser {
    second: bool,
    before: ToolKind,
}

/// A squeeze held.
#[derive(Clone, Copy, Debug)]
struct Squeeze {
    /// What it does as it goes (none once the platform's choice was done as it began).
    action: Option<GestureAction>,
    began_ms: f64,
    /// The eyedropper's loupe shows under the pen.
    picking: bool,
    /// The pen touched and picks with that touch.
    taken: bool,
    /// Where Layer Select last chose.
    selected: Option<(f32, f32)>,
}

#[uniffi::export]
impl CanvasSession {
    /// The pen held over the canvas (`hover`), or gone from over it (`None`): the brush's outline
    /// follows it ([`CanvasSession::pen_hover_cursor`]), Hover's touch works while it is there,
    /// and a squeeze held meanwhile follows it (the eyedropper's loupe, Layer Select).
    pub fn touch_pen_hover(&self, hover: Option<PenHover>) -> Vec<TouchCommand> {
        let mut out = Vec::new();
        let hover = hover.filter(PenHover::sound);
        let points = self.shared.lock().display_scale.max(1e-3);
        let mut t = self.touches();
        t.pen_hovering = hover.is_some().then(std::time::Instant::now);
        match (hover, t.pen.hover) {
            (Some(h), Some(last)) => {
                let seconds = ((h.time_ms - last.time_ms) / 1000.0).max(0.0) as f32;
                let step = seconds.min(HOVER_STEP_MOST_S);
                let speed = if seconds > 0.0 { dist(h.at(), last.at()) / points / seconds } else { 0.0 };
                let gained = if speed > HOVER_RACING { -step * speed / HOVER_RACING } else { step };
                t.pen.settled = (t.pen.settled + gained).clamp(0.0, 1.0);
            }
            _ => t.pen.settled = 0.0,
        }
        t.pen.hover = hover;
        self.squeeze_follows(&mut t, hover, &mut out);
        out
    }

    /// The brush's outline while the pen hovers, if it does and a stroke there would paint (no
    /// adjustment, selection or transform open; the eyedropper's loupe under a squeeze stands in
    /// for it). The app shows it as Brush cursor is set to (while hovering).
    pub fn pen_hover_cursor(&self) -> Option<HoverCursor> {
        let (h, settled) = {
            let t = self.touches();
            let picking = t.pen.squeeze.is_some_and(|s| s.picking);
            if picking || t.selecting.is_some() || t.transforming.is_some() {
                return None;
            }
            (t.pen.hover?, t.pen.settled)
        };
        if self.shared.lock().adjust.open().is_some() {
            return None;
        }
        let shown = ((settled - HOVER_SHOWS_S) / HOVER_FADES_IN_S).clamp(0.0, 1.0);
        let low = if h.height > 0.0 { ((1.0 - h.height) * HOVER_HEIGHT_FADE).clamp(0.0, 1.0) } else { 1.0 };
        Some(HoverCursor { x: h.x, y: h.y, alpha: shown * low })
    }

    /// The pen's button pressed (its `second`), hovering or touching at (`x`, `y`): what Gesture
    /// controls set it to do. Set to nothing, the first switches between the current tool and the
    /// eraser, and back, as a pen's double-tap does by default; the second does nothing. Set to
    /// erase, the eraser is borrowed until the button is let go
    /// ([`CanvasSession::touch_pen_button_up`]).
    pub fn touch_pen_button(&self, second: bool, x: f32, y: f32) -> Vec<TouchCommand> {
        let action = {
            let t = self.touches();
            if second { t.settings.pen_second_button } else { t.settings.pen_button }
        };
        let mut out = Vec::new();
        if action == Some(GestureAction::Erase) {
            self.hold_eraser(second);
            return out;
        }
        let preferred = if second { PenPreferred::Ignore } else { PenPreferred::SwitchEraser };
        self.pen_does(action, preferred, (x, y), &mut out);
        out
    }

    /// The pen's button let go (its `second`): held to erase, it gives the eraser back to the tool
    /// it was borrowed from (a stroke going on erases to its end, as it began), and says so (the
    /// tool may have changed). Anything else a button does was done as it was pressed. A tool
    /// chosen meanwhile stays.
    pub fn touch_pen_button_up(&self, second: bool) -> bool {
        let mut t = self.touches();
        let Some(held) = t.held_eraser.filter(|h| h.second == second) else { return false };
        t.held_eraser = None;
        // a stroke borrowing a tool of its own from the eraser gives back the one chosen
        if t.tool_before == Some(ToolKind::Erase) {
            t.tool_before = Some(held.before);
        } else if self.shared.lock().tool == ToolKind::Erase {
            self.put_tool(held.before);
        }
        true
    }

    /// The pen double-tapped where it is (`x`, `y`, window pixels), the platform's own setting for
    /// it `preferred`: what Gesture controls set the double-tap to do, else what the platform
    /// asks. Turned off there, it does nothing.
    pub fn touch_pen_double_tap(&self, preferred: PenPreferred, x: f32, y: f32) -> Vec<TouchCommand> {
        let mut out = Vec::new();
        if preferred == PenPreferred::Ignore || !(x.is_finite() && y.is_finite()) {
            return out;
        }
        // a double-tap can't be held: set to erase while held, it does what the platform asks
        let action = self.touches().settings.pen_button.filter(|&a| a != GestureAction::Erase);
        self.pen_does(action, preferred, (x, y), &mut out);
        out
    }

    /// The pen squeezed (`phase`), the platform's own setting for a squeeze `preferred`, the pen
    /// over the canvas as `hover` says (if it says), at `time_ms`: what Gesture controls set the
    /// squeeze to do, else what the platform asks, as it begins. Held, the eyedropper's loupe and
    /// Layer Select follow the pen and letting go picks the colour; the quick menu opens as it
    /// begins, QuickShape as it is let go.
    pub fn touch_pen_squeeze(
        &self,
        phase: SqueezePhase,
        preferred: PenPreferred,
        hover: Option<PenHover>,
        time_ms: f64,
    ) -> Vec<TouchCommand> {
        let mut out = Vec::new();
        let hover = hover.filter(PenHover::sound);
        let mut t = self.touches();
        match phase {
            SqueezePhase::Began => {
                t.pen.squeeze = None;
                // painting, a squeeze does nothing, nor what follows of it
                if matches!(t.mode, Mode::Pen | Mode::Finger) {
                    return out;
                }
                let at = match hover.or(t.pen.hover) {
                    Some(h) => h.at(),
                    None => {
                        let (w, h) = self.shared.lock().window_size;
                        (w as f32 / 2.0, h as f32 / 2.0)
                    }
                };
                let set = t.settings.pen_second_button;
                // left to the platform, its palette for where the pen is is the quick menu
                let action =
                    set.or((preferred == PenPreferred::ShowContextualPalette).then_some(GestureAction::QuickMenu));
                t.pen.squeeze =
                    Some(Squeeze { action, began_ms: time_ms, picking: false, taken: false, selected: None });
                match action {
                    None => {
                        drop(t);
                        self.platform_does(preferred, at, &mut out);
                    }
                    Some(GestureAction::Eyedropper | GestureAction::LayerSelect) => {
                        self.squeeze_follows(&mut t, hover, &mut out);
                    }
                    // as it is let go
                    Some(GestureAction::QuickShape) => {}
                    Some(a) => {
                        drop(t);
                        self.pen_does(Some(a), PenPreferred::Ignore, at, &mut out);
                    }
                }
            }
            SqueezePhase::Changed => self.squeeze_follows(&mut t, hover, &mut out),
            SqueezePhase::Ended | SqueezePhase::Cancelled => {
                let Some(s) = t.pen.squeeze.take() else { return out };
                let cancelled = phase == SqueezePhase::Cancelled;
                let near = hover.filter(PenHover::near);
                match s.action {
                    Some(GestureAction::Eyedropper) if !s.taken => {
                        let quick = time_ms - s.began_ms > 0.0 && time_ms - s.began_ms < QUICK_SQUEEZE_MS;
                        match near {
                            // the colour under the pen is taken
                            Some(h) if !cancelled => {
                                out.push(TouchCommand::Pick { x: h.x, y: h.y });
                                out.push(TouchCommand::PickDone);
                            }
                            _ => {
                                if s.picking {
                                    out.push(TouchCommand::PickCancel);
                                }
                                // a quick squeeze away from the glass: the next touch picks
                                if quick && !cancelled {
                                    t.pick_next = !t.pick_next;
                                }
                            }
                        }
                    }
                    Some(GestureAction::QuickShape) if !cancelled => out.push(TouchCommand::QuickShapeLast),
                    _ => {}
                }
            }
        }
        out
    }
}

impl CanvasSession {
    /// The pen's button (its `second`) pressed to erase while it is held: the eraser is borrowed
    /// from the tool chosen (from a stroke going on, the one it borrowed its own from), until the
    /// button is let go.
    fn hold_eraser(&self, second: bool) {
        let mut t = self.touches();
        if t.held_eraser.is_some() {
            return;
        }
        let chosen = t.tool_before.take();
        let tool = self.swap_tool(ToolKind::Erase);
        t.held_eraser = Some(HeldEraser { second, before: chosen.unwrap_or(tool) });
    }

    /// A double-tap, a pen's button or a squeeze set to `action` does it at `at`; set to nothing,
    /// it does what the platform asks (`preferred`).
    fn pen_does(
        &self,
        action: Option<GestureAction>,
        preferred: PenPreferred,
        at: (f32, f32),
        out: &mut Vec<TouchCommand>,
    ) {
        match action {
            // the next touch picks a colour
            Some(GestureAction::Eyedropper) => self.touches().pick_next = true,
            Some(GestureAction::Assisted) => {
                let on = self.guide().assisted;
                self.set_guide_assisted(!on);
            }
            Some(a) => Self::act(a, at, out),
            None => self.platform_does(preferred, at, out),
        }
    }

    /// What the platform's own pen settings ask a gesture to do, at `at`.
    fn platform_does(&self, preferred: PenPreferred, at: (f32, f32), out: &mut Vec<TouchCommand>) {
        match preferred {
            PenPreferred::Ignore | PenPreferred::RunSystemShortcut => {}
            PenPreferred::SwitchEraser => {
                let mut t = self.touches();
                let tool = self.shared.lock().tool;
                let next = if tool == ToolKind::Erase {
                    t.tool_before_eraser.take().unwrap_or(ToolKind::Paint)
                } else {
                    t.tool_before_eraser = Some(tool);
                    ToolKind::Erase
                };
                drop(t);
                self.set_tool(next);
            }
            PenPreferred::SwitchPrevious => {
                let previous = self.shared.lock().previous_tool;
                if let Some(tool) = previous {
                    self.set_tool(tool);
                }
            }
            PenPreferred::ShowColours => out.push(TouchCommand::ShowColours),
            PenPreferred::ShowInkAttributes => out.push(TouchCommand::ShowBrushes),
            PenPreferred::ShowContextualPalette => Self::act(GestureAction::QuickMenu, at, out),
        }
    }

    /// A squeeze held follows the pen to `hover`: the eyedropper's loupe shows under it while it
    /// is near (and goes when it isn't), and Layer Select chooses the layer under it as it moves.
    fn squeeze_follows(&self, t: &mut Touches, hover: Option<PenHover>, out: &mut Vec<TouchCommand>) {
        let points = self.shared.lock().display_scale.max(1e-3);
        let Some(s) = t.pen.squeeze.as_mut().filter(|s| !s.taken) else { return };
        let near = hover.filter(PenHover::near);
        match (s.action, near) {
            (Some(GestureAction::Eyedropper), Some(h)) => {
                s.picking = true;
                out.push(TouchCommand::Pick { x: h.x, y: h.y });
            }
            (Some(GestureAction::Eyedropper), None) if s.picking => {
                s.picking = false;
                out.push(TouchCommand::PickCancel);
            }
            // once it has moved on
            (Some(GestureAction::LayerSelect), Some(h))
                if s.selected.is_none_or(|p| dist(p, h.at()) > SELECT_STEP * points) =>
            {
                s.selected = Some(h.at());
                out.push(TouchCommand::LayerSelect { x: h.x, y: h.y });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{CanvasSession, GestureAction, PenSample, PointerKind, ToolKind, TouchSettings};

    use super::*;

    fn session() -> std::sync::Arc<CanvasSession> {
        CanvasSession::new(64, 64)
    }

    fn over(x: f32, y: f32, height: f32, ms: f64) -> Option<PenHover> {
        Some(PenHover { x, y, height, time_ms: ms })
    }

    fn sample(x: f32, y: f32, ms: f64) -> Vec<PenSample> {
        vec![PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: ms, roll: 0.0 }]
    }

    fn squeeze_set(s: &CanvasSession, action: Option<GestureAction>) {
        s.set_touch_settings(TouchSettings { pen_second_button: action, ..TouchSettings::default() });
    }

    fn held_to_erase(s: &CanvasSession) {
        s.set_touch_settings(TouchSettings { pen_button: Some(GestureAction::Erase), ..TouchSettings::default() });
    }

    #[test]
    fn a_pen_button_set_to_erase_borrows_the_eraser_while_it_is_held() {
        let s = session();
        s.set_tool(ToolKind::Smudge);
        held_to_erase(&s);
        assert!(s.touch_pen_button(false, 10.0, 10.0).is_empty());
        assert_eq!(s.info().tool, ToolKind::Erase);
        // a stroke while held erases, and the eraser stays after it while the button is held
        s.touch_down(1, PointerKind::Stylus, sample(10.0, 10.0, 0.0));
        assert_eq!(s.info().tool, ToolKind::Erase);
        s.touch_up(1, sample(40.0, 10.0, 100.0));
        assert_eq!(s.info().tool, ToolKind::Erase);
        // pressed again while held (a driver saying it twice), or the other button let go: nothing
        s.touch_pen_button(false, 10.0, 10.0);
        assert!(!s.touch_pen_button_up(true));
        assert_eq!(s.info().tool, ToolKind::Erase);
        // let go, the tool comes back; let go again, nothing
        assert!(s.touch_pen_button_up(false));
        assert_eq!(s.info().tool, ToolKind::Smudge);
        assert!(!s.touch_pen_button_up(false));
        assert_eq!(s.info().tool, ToolKind::Smudge);
        // borrowed, the eraser was never a tool chosen: the one before the smudge tool is the brush
        s.touch_pen_double_tap(PenPreferred::SwitchPrevious, 0.0, 0.0);
        assert_eq!(s.info().tool, ToolKind::Paint);
    }

    #[test]
    fn a_pen_button_held_to_erase_let_go_mid_stroke_gives_the_tool_back_for_the_next() {
        let s = session();
        held_to_erase(&s);
        s.touch_pen_button(false, 10.0, 10.0);
        s.touch_down(1, PointerKind::Stylus, sample(10.0, 10.0, 0.0));
        // the stroke erases to its end, as it began; the tool is back for the next
        s.touch_pen_button_up(false);
        assert_eq!(s.info().tool, ToolKind::Paint);
        s.touch_up(1, sample(40.0, 10.0, 100.0));
        assert_eq!(s.info().tool, ToolKind::Paint);
        // a tool chosen while the button was held stays when it is let go
        s.touch_pen_button(false, 10.0, 10.0);
        s.set_tool(ToolKind::Smudge);
        s.touch_pen_button_up(false);
        assert_eq!(s.info().tool, ToolKind::Smudge);
    }

    #[test]
    fn a_pen_button_held_to_erase_wins_over_what_the_pen_s_strokes_borrow() {
        let s = session();
        s.set_touch_settings(TouchSettings {
            pen: Some(GestureAction::Smudge),
            pen_button: Some(GestureAction::Erase),
            ..TouchSettings::default()
        });
        // pressed mid-stroke: the smudge stroke goes on, and the eraser stays after it
        s.touch_down(1, PointerKind::Stylus, sample(10.0, 10.0, 0.0));
        assert_eq!(s.info().tool, ToolKind::Smudge);
        s.touch_pen_button(false, 10.0, 10.0);
        s.touch_up(1, sample(40.0, 10.0, 100.0));
        assert_eq!(s.info().tool, ToolKind::Erase);
        // held, the next stroke erases rather than smudging
        s.touch_down(2, PointerKind::Stylus, sample(10.0, 10.0, 200.0));
        assert_eq!(s.info().tool, ToolKind::Erase);
        s.touch_up(2, sample(40.0, 10.0, 300.0));
        // let go, the tool chosen comes back and the pen smudges again
        s.touch_pen_button_up(false);
        assert_eq!(s.info().tool, ToolKind::Paint);
        s.touch_down(3, PointerKind::Stylus, sample(10.0, 10.0, 400.0));
        assert_eq!(s.info().tool, ToolKind::Smudge);
        s.touch_up(3, sample(40.0, 10.0, 500.0));
        assert_eq!(s.info().tool, ToolKind::Paint);
    }

    #[test]
    fn a_double_tap_set_to_erase_while_held_switches_as_the_platform_asks() {
        let s = session();
        held_to_erase(&s);
        s.touch_pen_double_tap(PenPreferred::SwitchEraser, 0.0, 0.0);
        assert_eq!(s.info().tool, ToolKind::Erase);
        s.touch_pen_double_tap(PenPreferred::SwitchEraser, 0.0, 0.0);
        assert_eq!(s.info().tool, ToolKind::Paint);
    }

    #[test]
    fn a_double_tap_turned_off_on_the_platform_does_nothing_whatever_it_is_set_to() {
        let s = session();
        s.set_touch_settings(TouchSettings { pen_button: Some(GestureAction::QuickMenu), ..TouchSettings::default() });
        assert!(s.touch_pen_double_tap(PenPreferred::Ignore, 10.0, 10.0).is_empty());
        // set to the quick menu, it opens there whatever the platform would rather
        assert_eq!(
            s.touch_pen_double_tap(PenPreferred::ShowColours, 10.0, 20.0),
            vec![TouchCommand::QuickMenu { x: 10.0, y: 20.0 }]
        );
    }

    #[test]
    fn a_double_tap_left_alone_does_what_the_platform_asks() {
        let s = session();
        assert!(s.touch_pen_double_tap(PenPreferred::SwitchEraser, 0.0, 0.0).is_empty());
        assert_eq!(s.info().tool, ToolKind::Erase);
        s.touch_pen_double_tap(PenPreferred::SwitchEraser, 0.0, 0.0);
        assert_eq!(s.info().tool, ToolKind::Paint);
        assert_eq!(s.touch_pen_double_tap(PenPreferred::ShowColours, 0.0, 0.0), vec![TouchCommand::ShowColours]);
        assert_eq!(s.touch_pen_double_tap(PenPreferred::ShowInkAttributes, 0.0, 0.0), vec![TouchCommand::ShowBrushes]);
        assert_eq!(
            s.touch_pen_double_tap(PenPreferred::ShowContextualPalette, 5.0, 6.0),
            vec![TouchCommand::QuickMenu { x: 5.0, y: 6.0 }]
        );
        assert!(s.touch_pen_double_tap(PenPreferred::RunSystemShortcut, 0.0, 0.0).is_empty());
        assert_eq!(s.info().tool, ToolKind::Paint);
    }

    #[test]
    fn switching_to_the_previous_tool_goes_back_and_forth_and_a_borrowed_tool_is_not_one() {
        let s = session();
        // none chosen before: nothing to go back to
        s.touch_pen_double_tap(PenPreferred::SwitchPrevious, 0.0, 0.0);
        assert_eq!(s.info().tool, ToolKind::Paint);
        s.set_tool(ToolKind::Smudge);
        s.touch_pen_double_tap(PenPreferred::SwitchPrevious, 0.0, 0.0);
        assert_eq!(s.info().tool, ToolKind::Paint);
        s.touch_pen_double_tap(PenPreferred::SwitchPrevious, 0.0, 0.0);
        assert_eq!(s.info().tool, ToolKind::Smudge);
        // a finger set to erase borrows the eraser for its stroke only
        s.set_touch_settings(TouchSettings { touch: Some(GestureAction::Erase), ..TouchSettings::default() });
        s.touch_down(1, PointerKind::Finger, sample(10.0, 10.0, 0.0));
        s.touch_up(1, sample(40.0, 10.0, 100.0));
        assert_eq!(s.info().tool, ToolKind::Smudge);
        s.touch_pen_double_tap(PenPreferred::SwitchPrevious, 0.0, 0.0);
        assert_eq!(s.info().tool, ToolKind::Paint);
    }

    #[test]
    fn the_hover_cursor_fades_in_as_the_pen_settles_and_out_as_it_rises() {
        let s = session();
        assert_eq!(s.pen_hover_cursor(), None);
        s.touch_pen_hover(over(20.0, 20.0, 0.2, 0.0));
        assert_eq!(s.pen_hover_cursor().map(|c| c.alpha), Some(0.0));
        // held still at 120 events a second: nothing for a tenth of a second, then in over 1/8 s
        let mut ms = 0.0;
        for _ in 0..12 {
            ms += 1000.0 / 120.0;
            s.touch_pen_hover(over(20.0, 20.0, 0.2, ms));
        }
        assert!(s.pen_hover_cursor().is_some_and(|c| c.alpha < 0.01), "{:?}", s.pen_hover_cursor());
        for _ in 0..16 {
            ms += 1000.0 / 120.0;
            s.touch_pen_hover(over(21.0, 20.0, 0.2, ms));
        }
        let shown = s.pen_hover_cursor().map(|c| (c.x, c.alpha));
        assert_eq!(shown, Some((21.0, 1.0)));
        // the top third of the height it is sensed at fades it out
        s.touch_pen_hover(over(21.0, 20.0, 0.9, ms + 8.0));
        let alpha = s.pen_hover_cursor().map_or(0.0, |c| c.alpha);
        assert!((alpha - 0.3).abs() < 0.01, "{alpha}");
        // racing across, it fades back
        let mut x = 21.0;
        for _ in 0..120 {
            ms += 1000.0 / 120.0;
            x += 120.0;
            s.touch_pen_hover(over(x, 20.0, 0.2, ms));
        }
        assert_eq!(s.pen_hover_cursor().map(|c| c.alpha), Some(0.0));
        // gone from over the canvas, gone; back, it starts again
        s.touch_pen_hover(None);
        assert_eq!(s.pen_hover_cursor(), None);
    }

    #[test]
    fn hovering_lets_hover_s_touch_set_the_brush() {
        let s = session();
        s.set_touch_settings(TouchSettings { hover_touch: true, ..TouchSettings::default() });
        s.touch_pen_hover(over(20.0, 20.0, 0.3, 0.0));
        assert!(s.touches().pen_hovering.is_some());
        s.touch_pen_hover(None);
        assert!(s.touches().pen_hovering.is_none());
    }

    #[test]
    fn a_squeeze_for_the_eyedropper_follows_the_pen_and_takes_the_colour_where_it_lets_go() {
        let s = session();
        squeeze_set(&s, Some(GestureAction::Eyedropper));
        let squeeze = |phase, hover, ms| s.touch_pen_squeeze(phase, PenPreferred::Ignore, hover, ms);
        assert_eq!(
            squeeze(SqueezePhase::Began, over(10.0, 10.0, 0.4, 0.0), 0.0),
            vec![TouchCommand::Pick { x: 10.0, y: 10.0 }]
        );
        // the loupe stands in for the brush's outline
        s.touch_pen_hover(over(10.0, 10.0, 0.4, 5.0));
        assert_eq!(s.pen_hover_cursor(), None);
        assert_eq!(s.touch_pen_hover(over(30.0, 12.0, 0.4, 10.0)), vec![TouchCommand::Pick { x: 30.0, y: 12.0 }]);
        assert_eq!(
            squeeze(SqueezePhase::Ended, over(31.0, 12.0, 0.4, 900.0), 900.0),
            vec![TouchCommand::Pick { x: 31.0, y: 12.0 }, TouchCommand::PickDone]
        );
        // lifted away while squeezed, the loupe goes, and letting go takes nothing
        squeeze(SqueezePhase::Began, over(10.0, 10.0, 0.4, 1000.0), 1000.0);
        assert_eq!(
            squeeze(SqueezePhase::Changed, over(10.0, 10.0, 1.0, 1100.0), 1100.0),
            vec![TouchCommand::PickCancel]
        );
        assert!(squeeze(SqueezePhase::Ended, None, 1900.0).is_empty());
        assert!(!s.pick_next());
    }

    #[test]
    fn a_quick_squeeze_away_from_the_glass_opens_the_eyedropper_for_the_next_touch() {
        let s = session();
        squeeze_set(&s, Some(GestureAction::Eyedropper));
        assert!(s.touch_pen_squeeze(SqueezePhase::Began, PenPreferred::Ignore, None, 0.0).is_empty());
        assert!(s.touch_pen_squeeze(SqueezePhase::Ended, PenPreferred::Ignore, None, 150.0).is_empty());
        assert!(s.pick_next());
        // again, closed
        s.touch_pen_squeeze(SqueezePhase::Began, PenPreferred::Ignore, None, 1000.0);
        s.touch_pen_squeeze(SqueezePhase::Ended, PenPreferred::Ignore, None, 1100.0);
        assert!(!s.pick_next());
        // held long, nothing
        s.touch_pen_squeeze(SqueezePhase::Began, PenPreferred::Ignore, None, 2000.0);
        s.touch_pen_squeeze(SqueezePhase::Ended, PenPreferred::Ignore, None, 2600.0);
        assert!(!s.pick_next());
    }

    #[test]
    fn the_pen_touching_while_squeezed_for_the_eyedropper_picks_with_its_touch() {
        let s = session();
        squeeze_set(&s, Some(GestureAction::Eyedropper));
        s.touch_pen_squeeze(SqueezePhase::Began, PenPreferred::Ignore, over(10.0, 10.0, 0.3, 0.0), 0.0);
        assert_eq!(
            s.touch_down(1, PointerKind::Stylus, sample(12.0, 10.0, 50.0)),
            vec![TouchCommand::Pick { x: 12.0, y: 10.0 }]
        );
        assert!(s.touch_up(1, sample(14.0, 10.0, 200.0)).contains(&TouchCommand::PickDone));
        // the touch took it: letting go adds nothing
        assert!(s.touch_pen_squeeze(SqueezePhase::Ended, PenPreferred::Ignore, None, 300.0).is_empty());
    }

    #[test]
    fn a_squeeze_while_painting_does_nothing() {
        let s = session();
        squeeze_set(&s, Some(GestureAction::QuickShape));
        s.touch_down(1, PointerKind::Stylus, sample(10.0, 10.0, 0.0));
        assert!(s.touch_pen_squeeze(SqueezePhase::Began, PenPreferred::Ignore, None, 10.0).is_empty());
        s.touch_up(1, sample(40.0, 10.0, 100.0));
        assert!(s.touch_pen_squeeze(SqueezePhase::Ended, PenPreferred::Ignore, None, 200.0).is_empty());
        // not painting, letting go makes the last stroke a shape
        s.touch_pen_squeeze(SqueezePhase::Began, PenPreferred::Ignore, None, 300.0);
        assert_eq!(
            s.touch_pen_squeeze(SqueezePhase::Ended, PenPreferred::Ignore, None, 400.0),
            vec![TouchCommand::QuickShapeLast]
        );
    }

    #[test]
    fn a_squeeze_for_layer_select_chooses_under_the_pen_as_it_moves() {
        let s = session();
        squeeze_set(&s, Some(GestureAction::LayerSelect));
        let began = s.touch_pen_squeeze(SqueezePhase::Began, PenPreferred::Ignore, over(10.0, 10.0, 0.3, 0.0), 0.0);
        assert_eq!(began, vec![TouchCommand::LayerSelect { x: 10.0, y: 10.0 }]);
        // a little way on, nothing new; further, chosen again
        assert!(s.touch_pen_hover(over(15.0, 10.0, 0.3, 10.0)).is_empty());
        assert_eq!(
            s.touch_pen_hover(over(60.0, 10.0, 0.3, 20.0)),
            vec![TouchCommand::LayerSelect { x: 60.0, y: 10.0 }]
        );
        assert!(s.touch_pen_squeeze(SqueezePhase::Ended, PenPreferred::Ignore, None, 500.0).is_empty());
        assert!(s.touch_pen_hover(over(10.0, 10.0, 0.3, 600.0)).is_empty());
    }

    #[test]
    fn a_squeeze_left_alone_does_what_the_platform_asks_as_it_begins() {
        let s = session();
        let began = s.touch_pen_squeeze(SqueezePhase::Began, PenPreferred::ShowColours, None, 0.0);
        assert_eq!(began, vec![TouchCommand::ShowColours]);
        assert!(s.touch_pen_squeeze(SqueezePhase::Ended, PenPreferred::ShowColours, None, 100.0).is_empty());
        // its palette for where the pen is: the quick menu, there (else mid-window)
        let began = s.touch_pen_squeeze(
            SqueezePhase::Began,
            PenPreferred::ShowContextualPalette,
            over(9.0, 8.0, 0.2, 0.0),
            0.0,
        );
        assert_eq!(began, vec![TouchCommand::QuickMenu { x: 9.0, y: 8.0 }]);
        s.touch_pen_squeeze(SqueezePhase::Ended, PenPreferred::ShowContextualPalette, None, 100.0);
        s.touch_pen_squeeze(SqueezePhase::Began, PenPreferred::SwitchEraser, None, 200.0);
        assert_eq!(s.info().tool, ToolKind::Erase);
        assert!(s.touch_pen_squeeze(SqueezePhase::Began, PenPreferred::Ignore, None, 300.0).is_empty());
    }
}
