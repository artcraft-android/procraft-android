//! The canvas's touches, the same for every app: what a pen, a finger or the mouse does on the
//! canvas, fed the platform's pointers and handing back what the app should do about them.
//!
//! A pen, a finger or the mouse paints (or does what Touch and Pen are set to); a finger held
//! still picks a colour (or does what Touch and hold is set to); a stroke held still snaps to a
//! shape, and a finger down meanwhile makes it perfect; two and three fingers tapped undo and redo,
//! and held still keep going; two fingers move, zoom and turn the canvas, and a quick pinch in fits
//! it; three fingers scrubbed clear the layer and swiped down do what Three-finger swipe is set to;
//! four fingers tapped do what Four-finger tap is. The modify button tapped makes the next touch
//! pick a colour; held, a touch does what □ + Touch (□ + Pen) is set to. A snapped shape being
//! edited has its handles, outline and body dragged.
//!
//! Positions are window pixels and times the platform's event clock (milliseconds).

use std::sync::Mutex;

use crate::adjust_menu::Adjustment;
use crate::adjusting::{CLONE_PUCK_RADIUS, CLONE_PUCK_SLOP};
use crate::{CanvasSession, PenSample, PointerKind, ToolKind};

/// A tap: down and up within this, moving no further than [`TAP_SLOP`] points.
const TAP_MS: f64 = 300.0;
/// A liquify brush lifted within this long of its last move carries on.
const LIQUIFY_STILL_MS: f64 = 60.0;
const TAP_SLOP: f32 = 24.0;
/// Two (three) fingers held still go on undoing (redoing) every this often.
const RAPID_REPEAT_MS: f64 = 120.0;
/// A pinch in this quick, to under this scale, fits the canvas.
const QUICK_PINCH_MS: f64 = 400.0;
const QUICK_PINCH_SCALE: f32 = 0.75;
/// Three fingers turning back and forth this often clear the layer, each leg this far (points).
const SCRUB_TURNS: i32 = 3;
const SCRUB_STEP: f32 = 24.0;
/// Three fingers swiped down this far (points), mostly straight.
const SWIPE_DOWN: f32 = 70.0;
/// A stroke counts as still within this many points.
const QUICKSHAPE_STILL: f32 = 6.0;
/// A snapped shape being edited: a handle within this takes a drag, and its outline within this.
const QUICKSHAPE_GRAB: f32 = 22.0;
/// A fill's touch slid past this (points) changes its threshold, this much more gently than a
/// selection's.
const POUR_SLOP: f32 = 15.0;
const POUR_SLIDE_GENTLER: f32 = 2.7;

/// A transform's touch moved this far (points) drags what it took (a selection's too).
const TRANSFORM_SLOP: f32 = 15.0;
/// A tap this near a freehand selection's first point closes it; its points are added this far
/// apart (points).
const FREEHAND_CLOSE: f32 = 22.0;
const FREEHAND_STEP: f32 = 2.0;

/// How the selection tool selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectKind {
    Automatic,
    Freehand,
    Rectangle,
    Ellipse,
}

/// The selection tool is open: how it selects, whether a new selection adds to the one there is
/// or takes away from it, and where a colour selection's threshold starts (0..1).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SelectTouch {
    pub kind: SelectKind,
    pub join: crate::SelectionJoin,
    pub threshold: f32,
}

/// A transform is open: touches work its box (a warp's control points shown as the advanced mesh
/// has them, or only its corners).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TransformTouch {
    pub advanced_mesh: bool,
}

/// What Continue filling fills with: a colour (sRGB, 0..1) and the threshold it starts at.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct Fill {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub threshold: f32,
}

/// What a way of touching is set to do (Gesture controls).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum GestureAction {
    Smudge,
    Erase,
    Assisted,
    Eyedropper,
    QuickShape,
    QuickMenu,
    FullScreen,
    ClearLayer,
    CopyPaste,
    LayerSelect,
}

/// The gesture settings the touches follow: what each way of touching does, and the delays.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TouchSettings {
    /// One finger paints as the pen does.
    pub finger_paint: bool,
    /// Two- and three-finger taps undo and redo.
    pub undo_taps: bool,
    /// Two fingers turn the canvas as they zoom it.
    pub rotate: bool,
    /// Pausing at the end of a stroke snaps it to a shape.
    pub draw_and_hold: bool,
    pub touch: Option<GestureAction>,
    pub pen: Option<GestureAction>,
    pub hold: Option<GestureAction>,
    pub modify_touch: Option<GestureAction>,
    pub modify_pen: Option<GestureAction>,
    pub four_tap: Option<GestureAction>,
    pub three_swipe: Option<GestureAction>,
    /// Three fingers rubbed back and forth clear the layer.
    pub scrub: bool,
    pub hold_ms: f64,
    pub quickshape_ms: f64,
    pub rapid_undo_ms: f64,
}

impl Default for TouchSettings {
    /// As the app ships.
    fn default() -> TouchSettings {
        TouchSettings {
            finger_paint: true,
            undo_taps: true,
            rotate: true,
            draw_and_hold: true,
            touch: None,
            pen: None,
            hold: Some(GestureAction::Eyedropper),
            modify_touch: Some(GestureAction::Eyedropper),
            modify_pen: Some(GestureAction::Eyedropper),
            four_tap: Some(GestureAction::FullScreen),
            three_swipe: Some(GestureAction::CopyPaste),
            scrub: true,
            hold_ms: 400.0,
            quickshape_ms: 650.0,
            rapid_undo_ms: 650.0,
        }
    }
}

/// What the touches ask of the app (the canvas itself they paint and move on their own).
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum TouchCommand {
    Undo,
    Redo,
    ClearLayer,
    ToggleBars,
    /// The canvas settles to fit the window.
    Fit,
    CopyPaste,
    QuickMenu {
        x: f32,
        y: f32,
    },
    LayerSelect {
        x: f32,
        y: f32,
    },
    /// The last stroke becomes a shape.
    QuickShapeLast,
    /// The eyedropper is under (x, y); then it lets go (its colour taken) or is called off.
    Pick {
        x: f32,
        y: f32,
    },
    PickDone,
    PickCancel,
    /// The stroke held still snapped to a shape; the shape changed (dragged, a finger down); the
    /// shape was let go of as it is.
    ShapeSnapped,
    ShapeChanged,
    ShapeEnded,
    /// A stroke finished.
    StrokeEnded,
    /// The view moved (two fingers).
    ViewMoved,
    /// The brush's outline is at (x, y), or goes.
    Cursor {
        x: f32,
        y: f32,
    },
    CursorGone,
    /// A stroke began on a text layer, which takes no paint until it is rasterized.
    TextLayer,
    /// Continue filling: a touch slid the fill's threshold to this (keep it for the next, and redo
    /// the fill with it, `pour_threshold`, when next drawing: it redoes the whole fill, so once a
    /// frame at most), and the fill was let go of (kept, at the threshold it was let go at).
    FillThreshold {
        threshold: f32,
    },
    Filled,
    /// A transform's touch came down (its popovers close); the box as it now is; the node held and
    /// where (window pixels), and let go of there (a rotate or box-turning node springs back); a
    /// node tapped, whose numbers are typed beside it (`angle` for the rotate node, else the size),
    /// at (`x`, `y`), on the box's `right` side or its left.
    TransformTouched,
    TransformMoved {
        transform: Option<crate::TransformBox>,
    },
    TransformHeld {
        handle: crate::TransformHandle,
        x: f32,
        y: f32,
    },
    TransformLetGo {
        handle: crate::TransformHandle,
        x: f32,
        y: f32,
    },
    TransformNumeric {
        angle: bool,
        x: f32,
        y: f32,
        right: bool,
    },
    /// The selection tool: a rectangle or ellipse being dragged out between two window points; a
    /// colour selection's threshold slid (shown with [`CanvasSession::show_colour_selection`]
    /// while not `done`, kept for the next when it is); the freehand path changed (see
    /// [`CanvasSession::freehand_path`]); a selection made.
    SelectionDrag {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
    },
    SelectionThreshold {
        x: f32,
        y: f32,
        threshold: f32,
        done: bool,
    },
    FreehandChanged,
    SelectionChanged,
    /// An adjustment open: its amount changed (see [`CanvasSession::adjustment_shown`]); a tap at
    /// window point (`x`, `y`) asks for its menu.
    AdjustChanged,
    AdjustMenu {
        x: f32,
        y: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    None,
    Pen,
    Finger,
    /// Down, doing nothing (a touch that only waits for a hold, or did its thing).
    Idle,
    Gesture,
    Pick,
    /// An edited shape's handle dragged; its outline dragged (turning and resizing it); itself moved.
    Shape(u32),
    ShapePin,
    ShapeMove,
    /// Continue filling: the touch filled where it landed, and slides the threshold.
    Pour,
    /// A transform's touch: it took a node (or the content), and drags it once past the slop.
    Transform,
    /// The selection tool's touch.
    Select,
    /// An adjustment open: the touch drags its amount (or a motion blur, or chromatic aberration's
    /// fringes) across the canvas; a tap opens its menu.
    Adjust,
    /// Cloning, the source dragged.
    Puck,
}

#[derive(Clone, Copy, Debug)]
struct Down {
    id: u64,
    at: (f32, f32),
}

/// The touches' state: kept by the session, apart from the canvas's (they call into it).
pub(crate) struct Touches {
    settings: TouchSettings,
    mode: Mode,
    down: Vec<Down>,
    stroke_pointer: u64,
    /// The tool chosen before this stroke used another (Touch set to erase), to put back after.
    tool_before: Option<ToolKind>,
    assisted: bool,
    down_at: (f32, f32),
    tap_start: f64,
    tap_pointers: usize,
    tap_moved: f32,
    gesture_start: f64,
    gesture_scale: f32,
    gesture_pair: Option<(u64, u64)>,
    last: (f32, f32, f32, f32),
    scrub_x: f32,
    scrub_direction: i32,
    scrub_travel: f32,
    scrub_turns: i32,
    swipe_start: (f32, f32),
    swiped: bool,
    hold_at: (f32, f32),
    hold_due: Option<f64>,
    quickshape_due: Option<f64>,
    rapid_due: Option<f64>,
    /// The modify button: tapped, the next touch picks a colour; held, touches do what it sets.
    pick_next: bool,
    modify_held: bool,
    modify_used: bool,
    /// The touch made while the modify button is held, and what it does.
    modify_pointer: Option<(u64, GestureAction)>,
    /// A snapped shape's handles show.
    shape_editing: bool,
    shape_dragged: bool,
    shape_last: (f32, f32),
    /// Continue filling is on: each touch fills; the fill's threshold as slid, and the touch's
    /// last place and time.
    filling: Option<Fill>,
    /// The selection tool is open; its touch has moved past the slop; a freehand path held still
    /// snapped to a shape; the colour selection's threshold as slid; the freehand path (canvas
    /// pixels).
    selecting: Option<SelectTouch>,
    select_moved: bool,
    lasso_snapped: bool,
    /// An adjustment's touch went past the slop (it is not a tap), and has begun displacing;
    /// liquify's brush, where it last was (window pixels), when, and how fast (pixels a
    /// millisecond).
    adjust_moved: bool,
    adjust_displacing: bool,
    liquify_at: (f32, f32),
    liquify_ms: f64,
    liquify_velocity: (f32, f32),
    liquify_pen: bool,
    /// Where on the clone source it was taken (window pixels from its centre).
    puck_grab: (f32, f32),
    select_threshold: f32,
    freehand: Vec<(f32, f32)>,
    /// A transform is open; what its touch took, and whether it has started dragging it.
    transforming: Option<TransformTouch>,
    transform_handle: crate::TransformHandle,
    transform_grabbed: bool,
    pour_moved: bool,
    pour_last: (f32, f64),
    /// Window pixels a point.
    scale: f32,
}

impl Default for Touches {
    fn default() -> Touches {
        Touches {
            settings: TouchSettings::default(),
            mode: Mode::None,
            down: Vec::new(),
            stroke_pointer: 0,
            tool_before: None,
            assisted: false,
            down_at: (0.0, 0.0),
            tap_start: 0.0,
            tap_pointers: 0,
            tap_moved: 0.0,
            gesture_start: 0.0,
            gesture_scale: 1.0,
            gesture_pair: None,
            last: (0.0, 0.0, 0.0, 0.0),
            scrub_x: 0.0,
            scrub_direction: 0,
            scrub_travel: 0.0,
            scrub_turns: 0,
            swipe_start: (0.0, 0.0),
            swiped: false,
            hold_at: (0.0, 0.0),
            hold_due: None,
            quickshape_due: None,
            rapid_due: None,
            pick_next: false,
            modify_held: false,
            modify_used: false,
            modify_pointer: None,
            shape_editing: false,
            shape_dragged: false,
            shape_last: (0.0, 0.0),
            filling: None,
            selecting: None,
            select_moved: false,
            lasso_snapped: false,
            adjust_moved: false,
            adjust_displacing: false,
            liquify_at: (0.0, 0.0),
            liquify_ms: 0.0,
            liquify_velocity: (0.0, 0.0),
            liquify_pen: false,
            puck_grab: (0.0, 0.0),
            select_threshold: 0.5,
            freehand: Vec::new(),
            transforming: None,
            transform_handle: crate::TransformHandle::Move,
            transform_grabbed: false,
            pour_moved: false,
            pour_last: (0.0, 0.0),
            scale: 1.0,
        }
    }
}

/// Where a pointer's samples end up, and when.
fn last_of(samples: &[PenSample]) -> Option<&PenSample> {
    samples.last()
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// Centre, distance and angle of two points.
fn pair(a: (f32, f32), b: (f32, f32)) -> (f32, f32, f32, f32) {
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0, dist(a, b), (b.1 - a.1).atan2(b.0 - a.0))
}

/// How much an amount dragged across the screen changes for a step of `dx` points over `ms`: more
/// for faster drags (the speed taken per 60 Hz frame), 200 points at a steady pace moving it most
/// of the way.
fn drag_step(dx: f32, ms: f64) -> f32 {
    let frames = (ms.max(1.0) / 1000.0 * 60.0).max(0.06) as f32;
    let speed = dx.abs() / frames;
    (speed / 30.0).powf(0.86) * dx / 200.0
}

/// A liquify stroke's points, (x, y, pressure) each: a finger's (or a mouse's) at full pressure.
fn liquify_points(pen: bool, samples: &[PenSample]) -> Vec<f32> {
    samples.iter().flat_map(|s| [s.x, s.y, if pen { s.pressure } else { 1.0 }]).collect()
}

fn is_pen(kind: PointerKind) -> bool {
    matches!(kind, PointerKind::Stylus | PointerKind::StylusEraser)
}

impl CanvasSession {
    fn touches(&self) -> std::sync::MutexGuard<'_, Touches> {
        self.shared.touches.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The gesture settings the touches follow from now on.
    pub fn set_touch_settings(&self, settings: TouchSettings) {
        self.touches().settings = settings;
    }

    /// The modify button tapped: whether the next touch picks a colour.
    pub fn set_pick_next(&self, on: bool) {
        self.touches().pick_next = on;
    }

    pub fn pick_next(&self) -> bool {
        self.touches().pick_next
    }

    /// The modify button is held (or let go). Let go, true if a touch used it meanwhile (so its
    /// tap does nothing).
    pub fn set_modify_held(&self, held: bool) -> bool {
        let mut t = self.touches();
        t.modify_held = held;
        let used = t.modify_used;
        if !held {
            t.modify_used = false;
        }
        used
    }

    /// The selection tool open (each touch selects, as `how` says) or not.
    pub fn set_touch_selecting(&self, how: Option<SelectTouch>) {
        let mut t = self.touches();
        t.selecting = how;
        if how.is_none() {
            t.freehand.clear();
        }
    }

    /// The colour selection being slid, shown at `threshold` from window point (`x`, `y`) where it
    /// went down: called once a frame with the last [`TouchCommand::SelectionThreshold`] not
    /// `done`. Once the touch has lifted it does nothing, as the lift made the selection.
    pub fn show_colour_selection(&self, x: f32, y: f32, threshold: f32) {
        let join = {
            let t = self.touches();
            if t.mode != Mode::Select {
                return;
            }
            match t.selecting {
                Some(how) if how.kind == SelectKind::Automatic => how.join,
                _ => return,
            }
        };
        self.select_colour_at((x, y), threshold, join, true);
    }

    /// The freehand selection's path drawn so far, in window pixels (x, y pairs).
    pub fn freehand_path(&self) -> Vec<f32> {
        let points = self.touches().freehand.clone();
        let st = self.shared.lock();
        let Some(view) = st.view else { return Vec::new() };
        points
            .iter()
            .flat_map(|&(x, y)| {
                let p = view.to_screen().apply(procraft_engine::geom::Vec2::new(x, y));
                [p.x, p.y]
            })
            .collect()
    }

    /// Closes the freehand path into a selection (fewer than three points are dropped).
    pub fn close_freehand(&self) {
        let (path, join) = {
            let mut t = self.touches();
            (std::mem::take(&mut t.freehand), t.selecting.map_or(crate::SelectionJoin::Add, |s| s.join))
        };
        if path.len() < 3 {
            return;
        }
        let window: Vec<f32> = {
            let st = self.shared.lock();
            let Some(view) = st.view else { return };
            path.iter()
                .flat_map(|&(x, y)| {
                    let p = view.to_screen().apply(procraft_engine::geom::Vec2::new(x, y));
                    [p.x, p.y]
                })
                .collect()
        };
        self.select_polygon(window, join);
    }

    /// A transform open (each touch works its box) or not.
    pub fn set_touch_transforming(&self, transforming: Option<TransformTouch>) {
        self.touches().transforming = transforming;
    }

    /// Continue filling on (each touch on the canvas fills with `fill`) or off.
    pub fn set_touch_filling(&self, fill: Option<Fill>) {
        self.touches().filling = fill;
    }

    /// The snapped shape's handles show (its bar tapped), or not.
    pub fn set_shape_editing(&self, editing: bool) {
        self.touches().shape_editing = editing;
    }

    /// Whether any pointer is down on the canvas.
    pub fn touch_active(&self) -> bool {
        !self.touches().down.is_empty()
    }

    /// When the touches next need [`CanvasSession::touch_tick`] (a hold, a repeat), if ever.
    pub fn touch_deadline(&self) -> Option<f64> {
        let t = self.touches();
        [t.hold_due, t.quickshape_due, t.rapid_due].into_iter().flatten().reduce(f64::min)
    }

    /// A pointer goes down on the canvas (`samples` its first input).
    pub fn touch_down(&self, id: u64, kind: PointerKind, samples: Vec<PenSample>) -> Vec<TouchCommand> {
        let Some(first) = last_of(&samples).copied() else { return Vec::new() };
        let mut out = Vec::new();
        let mut t = self.touches();
        t.scale = self.shared.lock().display_scale;
        let at = (first.x, first.y);
        t.down.retain(|d| d.id != id);
        t.down.push(Down { id, at });
        // a touch made while the modify button is held does what □ + Touch (□ + Pen) is set to
        if t.modify_held && t.modify_pointer.is_none() && (t.down.len() == 1 || t.mode == Mode::None) {
            let purpose = if is_pen(kind) { t.settings.modify_pen } else { t.settings.modify_touch };
            if let Some(purpose) = purpose {
                t.modify_used = true;
                t.modify_pointer = Some((id, purpose));
                drop(t);
                self.end_shape(&mut out);
                self.modify_begins(purpose, kind, &samples, &mut out);
                return out;
            }
        }
        if t.down.len() == 1 {
            self.first_down(&mut t, kind, first, &samples, &mut out);
        } else {
            self.another_down(&mut t, kind, first.time_ms, &mut out);
        }
        out
    }

    /// A pointer down moves (`samples` its input since the last).
    pub fn touch_move(&self, id: u64, samples: Vec<PenSample>) -> Vec<TouchCommand> {
        let Some(last) = last_of(&samples).copied() else { return Vec::new() };
        let mut out = Vec::new();
        let mut t = self.touches();
        let at = (last.x, last.y);
        let Some(d) = t.down.iter_mut().find(|d| d.id == id) else { return out };
        d.at = at;
        if let Some((pointer, purpose)) = t.modify_pointer {
            if pointer == id {
                match purpose {
                    GestureAction::Eyedropper => out.push(TouchCommand::Pick { x: at.0, y: at.1 }),
                    GestureAction::Smudge | GestureAction::Erase | GestureAction::Assisted => self.stroke_add(samples),
                    _ => {}
                }
            }
            return out;
        }
        let slop = TAP_SLOP * t.scale;
        match t.mode {
            Mode::Pen | Mode::Finger if id == t.stroke_pointer => {
                if t.mode == Mode::Finger && dist(at, t.down_at) > slop {
                    t.hold_due = None;
                }
                // moving on restarts the wait for a hold
                if dist(at, t.hold_at) > QUICKSHAPE_STILL * t.scale {
                    t.hold_at = at;
                    let snapped = self.shared.lock().shape.is_some();
                    t.quickshape_due =
                        (!snapped && t.settings.draw_and_hold).then_some(last.time_ms + t.settings.quickshape_ms);
                }
                drop(t);
                self.clone_stroke_at(at, false);
                self.stroke_add(samples);
                out.push(TouchCommand::Cursor { x: at.0, y: at.1 });
            }
            Mode::Puck if id == t.stroke_pointer => {
                let (gx, gy) = t.puck_grab;
                drop(t);
                self.adjustment_clone_move(at.0 - gx, at.1 - gy);
                out.push(TouchCommand::AdjustChanged);
            }
            Mode::Pick => out.push(TouchCommand::Pick { x: at.0, y: at.1 }),
            Mode::Select if id == t.stroke_pointer => {
                drop(t);
                self.select_step(at, last.time_ms, false, &mut out);
            }
            Mode::Adjust if id == t.stroke_pointer => {
                if dist(at, t.down_at) > TAP_SLOP * t.scale {
                    t.adjust_moved = true;
                }
                let (from, (last_x, last_ms)) = (t.down_at, t.pour_last);
                t.pour_last = (at.0, last.time_ms);
                let start = !std::mem::replace(&mut t.adjust_displacing, true);
                let step = drag_step((at.0 - last_x) / t.scale, last.time_ms - last_ms);
                drop(t);
                let (open, displacing) = {
                    let st = self.shared.lock();
                    (st.adjust.open().map(|o| o.0), st.adjust.displacing())
                };
                if open == Some(Adjustment::Liquify) {
                    // the brush liquifies along the drag, its speed followed for when it lifts
                    let mut t = self.touches();
                    let dt = (last.time_ms - t.liquify_ms) as f32;
                    if dt > 0.0 {
                        let (vx, vy) = t.liquify_velocity;
                        let (px, py) = t.liquify_at;
                        t.liquify_velocity = (vx * 0.5 + (at.0 - px) / dt * 0.5, vy * 0.5 + (at.1 - py) / dt * 0.5);
                        t.liquify_at = at;
                        t.liquify_ms = last.time_ms;
                    }
                    let pen = t.liquify_pen;
                    drop(t);
                    self.adjustment_liquify_stroke(liquify_points(pen, &samples), false);
                    out.push(TouchCommand::AdjustChanged);
                    return out;
                }
                match open {
                    // a motion blur follows the drag: along it, longer the further it goes
                    Some(Adjustment::MotionBlur) => self.adjustment_motion(at.0 - from.0, at.1 - from.1),
                    // displacing, the drag moves the fringes
                    Some(_) if displacing => self.adjustment_chroma_drag(at.0 - from.0, at.1 - from.1, start),
                    // across the canvas the amount follows the drag, faster the faster it goes
                    Some(a) if crate::adjust_menu::adjustment_swiped(a) => self.adjustment_amount_by(step),
                    _ => return out,
                }
                out.push(TouchCommand::AdjustChanged);
            }
            Mode::Transform if id == t.stroke_pointer => {
                let handle = t.transform_handle;
                if !t.transform_grabbed && dist(at, t.down_at) > TRANSFORM_SLOP * t.scale {
                    // the drag counts from where it was recognised, as the slop is used up
                    t.transform_grabbed = true;
                    self.transform_grab(handle, at.0, at.1);
                }
                if t.transform_grabbed {
                    drop(t);
                    let transform = self.transform_drag(at.0, at.1);
                    out.push(TouchCommand::TransformMoved { transform });
                    out.push(TouchCommand::TransformHeld { handle, x: at.0, y: at.1 });
                }
            }
            Mode::Pour if id == t.stroke_pointer => {
                if let Some(threshold) = Self::pour_slide(&mut t, at, last.time_ms) {
                    out.push(TouchCommand::FillThreshold { threshold });
                }
            }
            Mode::Idle => {
                if dist(at, t.down_at) > slop {
                    t.hold_due = None;
                }
            }
            Mode::Gesture => {
                if t.down.len() >= 3 {
                    Self::scrub(&mut t, &mut out);
                } else if self.move_gesture(&mut t) {
                    out.push(TouchCommand::ViewMoved);
                }
            }
            Mode::Shape(i) => {
                drop(t);
                self.quickshape_move_handle(i, at.0, at.1);
                out.push(TouchCommand::ShapeChanged);
            }
            Mode::ShapePin | Mode::ShapeMove if id == t.stroke_pointer => {
                // only a drag past a tap's slop moves it, then from where it began
                if !t.shape_dragged {
                    t.shape_dragged = dist(at, t.down_at) > slop;
                }
                if t.shape_dragged {
                    let (pin, from) = (t.mode == Mode::ShapePin, t.shape_last);
                    t.shape_last = at;
                    drop(t);
                    if pin {
                        self.quickshape_pin(at.0, at.1, false);
                    } else {
                        self.quickshape_move(from.0, from.1, at.0, at.1);
                    }
                    out.push(TouchCommand::ShapeChanged);
                }
            }
            _ => {}
        }
        out
    }

    /// A pointer lifts (`samples` its last input).
    pub fn touch_up(&self, id: u64, samples: Vec<PenSample>) -> Vec<TouchCommand> {
        self.lifted(id, samples, false)
    }

    /// The platform called the touches off: what they were doing is taken back.
    pub fn touch_cancel(&self) -> Vec<TouchCommand> {
        let ids: Vec<u64> = self.touches().down.iter().map(|d| d.id).collect();
        let mut out = Vec::new();
        for id in ids {
            out.extend(self.lifted(id, Vec::new(), true));
        }
        out
    }

    /// Runs the waits (a hold, a repeat) that have run out by `now_ms`.
    pub fn touch_tick(&self, now_ms: f64) -> Vec<TouchCommand> {
        let mut out = Vec::new();
        let mut t = self.touches();
        // a freehand selection held still snaps to a shape, which the lift then selects
        if t.mode == Mode::Select
            && t.quickshape_due.is_some_and(|due| due <= now_ms)
            && t.selecting.is_some_and(|s| s.kind == SelectKind::Freehand)
        {
            t.quickshape_due = None;
            if t.select_moved && t.settings.draw_and_hold {
                let flat: Vec<f32> = t.freehand.iter().flat_map(|&(x, y)| [x, y]).collect();
                if let Some(outline) = crate::quickshape::quickshape_outline(flat).filter(|o| o.len() >= 6) {
                    t.freehand = outline.as_chunks::<2>().0.iter().map(|&[x, y]| (x, y)).collect();
                    t.lasso_snapped = true;
                    out.push(TouchCommand::FreehandChanged);
                }
            }
        }
        if t.quickshape_due.is_some_and(|due| due <= now_ms) {
            t.quickshape_due = None;
            if matches!(t.mode, Mode::Pen | Mode::Finger) && t.settings.draw_and_hold {
                t.hold_due = None;
                drop(t);
                if self.quickshape_hold().is_some() {
                    out.push(TouchCommand::ShapeSnapped);
                }
                t = self.touches();
            }
        }
        if t.hold_due.is_some_and(|due| due <= now_ms) {
            t.hold_due = None;
            if matches!(t.mode, Mode::Finger | Mode::Idle)
                && let Some(purpose) = t.settings.hold
            {
                let painting = t.mode == Mode::Finger;
                let at = t.down_at;
                t.mode = if purpose == GestureAction::Eyedropper { Mode::Pick } else { Mode::Idle };
                let tool = t.tool_before.take();
                drop(t);
                if painting {
                    self.stroke_cancel();
                    out.push(TouchCommand::CursorGone);
                }
                if let Some(before) = tool {
                    self.set_tool(before);
                }
                Self::act(purpose, at, &mut out);
                t = self.touches();
            }
        }
        if t.rapid_due.is_some_and(|due| due <= now_ms) {
            t.rapid_due = None;
            // two fingers held still keep undoing, three redoing
            let still = t.tap_moved <= TAP_SLOP * t.scale;
            if t.mode == Mode::Gesture && still && t.down.len() == t.tap_pointers && t.settings.undo_taps {
                match t.down.len() {
                    2 => out.push(TouchCommand::Undo),
                    3 => out.push(TouchCommand::Redo),
                    _ => return out,
                }
                t.rapid_due = Some(now_ms + RAPID_REPEAT_MS);
            }
        }
        out
    }
}

impl CanvasSession {
    fn first_down(
        &self,
        t: &mut Touches,
        kind: PointerKind,
        first: PenSample,
        samples: &[PenSample],
        out: &mut Vec<TouchCommand>,
    ) {
        let at = (first.x, first.y);
        let now = first.time_ms;
        t.down_at = at;
        t.tap_start = now;
        t.tap_pointers = 1;
        t.tap_moved = 0.0;
        let id = t.down.first().map_or(0, |d| d.id);
        if t.pick_next {
            // the modify button was tapped: this touch picks a colour
            t.mode = Mode::Pick;
            out.push(TouchCommand::Pick { x: at.0, y: at.1 });
            return;
        }
        // a snapped shape's handle is dragged; while it is edited, a drag on its outline turns and
        // resizes it, one elsewhere moves it (a tap ends the editing); anything else keeps the
        // shape and draws on
        let grab = QUICKSHAPE_GRAB * t.scale;
        if t.shape_editing
            && let Some(info) = self.quickshape_info()
        {
            let handle = info
                .handles
                .as_chunks::<2>()
                .0
                .iter()
                .enumerate()
                .map(|(i, &[x, y])| (i, dist((x, y), at)))
                .filter(|(_, d)| *d <= grab)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, _)) = handle {
                t.mode = Mode::Shape(i as u32);
                return;
            }
            t.stroke_pointer = id;
            t.shape_dragged = false;
            t.shape_last = at;
            if self.quickshape_near_edge(at.0, at.1, grab) {
                t.mode = Mode::ShapePin;
                self.quickshape_pin(at.0, at.1, true);
                out.push(TouchCommand::ShapeChanged);
            } else {
                t.mode = Mode::ShapeMove;
            }
            return;
        }
        self.end_shape_with(t, out);
        let adjusting = self.shared.lock().adjust.open();
        let draws = is_pen(kind) || kind == PointerKind::Mouse;
        if let Some((open, pencil)) = adjusting
            && (open == Adjustment::Clone || (pencil && draws))
        {
            t.stroke_pointer = id;
            if let Some(puck) = self.clone_puck_at()
                && dist(puck, at) <= (CLONE_PUCK_RADIUS + CLONE_PUCK_SLOP) * t.scale
            {
                // the clone source is dragged
                t.puck_grab = (at.0 - puck.0, at.1 - puck.1);
                t.mode = Mode::Puck;
                return;
            }
            // cloning, and the pen (or a mouse, a desktop's drawing tool) painting an adjustment
            // on, paint (no eyedropper); the clone source follows the brush
            t.mode = if is_pen(kind) { Mode::Pen } else { Mode::Finger };
            t.hold_at = at;
            self.clone_stroke_at(at, true);
            self.stroke_begin(kind, samples.to_vec());
            return;
        }
        // an adjustment open otherwise: the touch drags it (or liquifies) and nothing paints
        if let Some((open, _)) = adjusting {
            t.stroke_pointer = id;
            t.adjust_moved = false;
            t.adjust_displacing = false;
            t.pour_last = (at.0, now);
            t.mode = Mode::Adjust;
            if open == Adjustment::Liquify {
                t.liquify_at = at;
                t.liquify_ms = now;
                t.liquify_velocity = (0.0, 0.0);
                t.liquify_pen = is_pen(kind);
                self.adjustment_liquify_stroke(liquify_points(t.liquify_pen, samples), true);
                out.push(TouchCommand::AdjustChanged);
            }
            return;
        }
        // the selection tool: the touch selects, nothing paints
        if t.selecting.is_some() {
            t.stroke_pointer = id;
            t.select_moved = false;
            t.lasso_snapped = false;
            t.hold_at = at;
            t.pour_last = (at.0, now);
            t.select_threshold = t.selecting.map_or(0.5, |s| s.threshold);
            t.mode = Mode::Select;
            return;
        }
        // a transform: the touch takes the node it lands on (or the content), nothing paints
        if let Some(how) = t.transforming {
            t.stroke_pointer = id;
            t.transform_grabbed = false;
            t.transform_handle = self.transform_handle_at(at, how);
            t.mode = Mode::Transform;
            out.push(TouchCommand::TransformTouched);
            return;
        }
        // Continue filling: the touch fills where it lands; slid, it changes the threshold
        if let Some(fill) = t.filling {
            t.stroke_pointer = id;
            t.pour_moved = false;
            t.pour_last = (at.0, now);
            if self.current_is_text() {
                out.push(TouchCommand::TextLayer);
                t.mode = Mode::Idle;
            } else {
                let poured = self.pour_begin(at.0, at.1, fill.r, fill.g, fill.b, fill.threshold);
                t.mode = if poured { Mode::Pour } else { Mode::Idle };
            }
            return;
        }
        let pen = is_pen(kind);
        // what this kind of touch is set to do
        let does = if pen { t.settings.pen } else { t.settings.touch };
        if !pen {
            match does {
                Some(GestureAction::Eyedropper) => {
                    t.mode = Mode::Pick;
                    out.push(TouchCommand::Pick { x: at.0, y: at.1 });
                    return;
                }
                Some(
                    a @ (GestureAction::QuickMenu
                    | GestureAction::CopyPaste
                    | GestureAction::FullScreen
                    | GestureAction::LayerSelect
                    | GestureAction::QuickShape),
                ) => {
                    t.mode = Mode::Idle;
                    Self::act(a, at, out);
                    return;
                }
                _ => {}
            }
        }
        if !pen && does.is_none() && !t.settings.finger_paint {
            t.mode = Mode::Idle;
            if t.settings.hold.is_some() {
                t.hold_due = Some(now + t.settings.hold_ms);
            }
            return;
        }
        t.stroke_pointer = id;
        t.mode = if pen { Mode::Pen } else { Mode::Finger };
        let other = match does {
            Some(GestureAction::Erase) => Some(ToolKind::Erase),
            Some(GestureAction::Smudge) => Some(ToolKind::Smudge),
            _ => None,
        };
        if let Some(tool) = other {
            t.tool_before = Some(self.swap_tool(tool));
        }
        t.assisted = does == Some(GestureAction::Assisted);
        if t.assisted {
            self.set_stroke_assist(true);
        }
        if self.current_is_text() {
            out.push(TouchCommand::TextLayer);
        }
        self.stroke_begin(kind, samples.to_vec());
        out.push(TouchCommand::Cursor { x: at.0, y: at.1 });
        if t.mode == Mode::Finger && t.settings.hold.is_some() {
            t.hold_due = Some(now + t.settings.hold_ms);
        }
        t.hold_at = at;
        if t.settings.draw_and_hold {
            t.quickshape_due = Some(now + t.settings.quickshape_ms);
        }
    }

    fn another_down(&self, t: &mut Touches, kind: PointerKind, now: f64, out: &mut Vec<TouchCommand>) {
        t.hold_due = None;
        t.quickshape_due = None;
        // a finger down while the stroke is held as a snapped shape snaps it, until lifted
        let snapped = self.shared.lock().shape.is_some();
        if matches!(t.mode, Mode::Pen | Mode::Finger | Mode::ShapePin) && snapped {
            if kind == PointerKind::Finger {
                self.quickshape_finger(true);
                out.push(TouchCommand::ShapeChanged);
            }
            return;
        }
        if matches!(t.mode, Mode::Pen | Mode::Pick) {
            return;
        }
        // a second finger takes back the fill the first one made, and a transform's drag
        if t.mode == Mode::Pour {
            self.pour_end(false, None);
        }
        // a colour selection under way is kept as it was slid; a shape being dragged out goes
        if t.mode == Mode::Select {
            let start = t.down_at;
            let (moved, how, threshold) = (t.select_moved, t.selecting, t.select_threshold);
            if let Some(how) = how {
                match how.kind {
                    SelectKind::Automatic if moved => {
                        self.select_colour_at(start, threshold, how.join, false);
                        out.push(TouchCommand::SelectionThreshold { x: start.0, y: start.1, threshold, done: true });
                        out.push(TouchCommand::SelectionChanged);
                    }
                    SelectKind::Rectangle | SelectKind::Ellipse => out.push(TouchCommand::SelectionChanged),
                    _ => {}
                }
            }
        }
        if t.mode == Mode::Transform && t.transform_grabbed {
            t.transform_grabbed = false;
            let transform = self.transform_cancel_drag();
            out.push(TouchCommand::TransformMoved { transform });
        }
        out.push(TouchCommand::CursorGone);
        if t.mode == Mode::Finger {
            self.stroke_cancel();
        }
        self.end_stroke_tools(t);
        if matches!(t.mode, Mode::Finger | Mode::Idle | Mode::Pour | Mode::Transform | Mode::Select) {
            t.gesture_start = now;
            t.gesture_scale = 1.0;
        }
        t.mode = Mode::Gesture;
        t.tap_pointers = t.tap_pointers.max(t.down.len());
        Self::follow_fingers(t);
        if t.down.len() == 3 {
            let c = Self::centre(t);
            t.swipe_start = c;
            t.swiped = false;
            t.scrub_x = c.0;
            t.scrub_direction = 0;
            t.scrub_travel = 0.0;
            t.scrub_turns = if t.settings.scrub { 0 } else { -1 };
        }
        t.rapid_due = Some(now + t.settings.rapid_undo_ms);
    }

    fn lifted(&self, id: u64, samples: Vec<PenSample>, cancelled: bool) -> Vec<TouchCommand> {
        let mut out = Vec::new();
        let mut t = self.touches();
        let Some(i) = t.down.iter().position(|d| d.id == id) else { return out };
        t.down.remove(i);
        let now = last_of(&samples).map_or(t.tap_start, |s| s.time_ms);
        // the touch made while the modify button is held ends
        if let Some((pointer, purpose)) = t.modify_pointer
            && pointer == id
        {
            t.modify_pointer = None;
            if t.down.is_empty() {
                t.mode = Mode::None;
            }
            drop(t);
            match purpose {
                GestureAction::Eyedropper => {
                    out.push(if cancelled { TouchCommand::PickCancel } else { TouchCommand::PickDone })
                }
                GestureAction::Smudge | GestureAction::Erase | GestureAction::Assisted => {
                    if cancelled {
                        self.stroke_cancel();
                    } else {
                        self.stroke_end(samples);
                    }
                    self.set_stroke_assist(false);
                    if let Some(before) = self.touches().tool_before.take() {
                        self.set_tool(before);
                    }
                    out.push(TouchCommand::StrokeEnded);
                }
                _ => {}
            }
            return out;
        }
        if !t.down.is_empty() {
            if t.mode == Mode::Gesture {
                Self::follow_fingers(&mut t);
            }
            // the last finger other than the stroke's lifted from a held snapped shape
            let stroke = t.stroke_pointer;
            if matches!(t.mode, Mode::Pen | Mode::Finger | Mode::ShapePin)
                && id != stroke
                && t.down.iter().all(|d| d.id == stroke)
                && self.shared.lock().shape.is_some()
            {
                drop(t);
                self.quickshape_finger(false);
                out.push(TouchCommand::ShapeChanged);
            }
            return out;
        }
        let mode = t.mode;
        t.mode = Mode::None;
        t.hold_due = None;
        t.quickshape_due = None;
        t.rapid_due = None;
        out.push(TouchCommand::CursorGone);
        match mode {
            Mode::Pen | Mode::Finger => {
                self.end_stroke_tools(&mut t);
                drop(t);
                if cancelled {
                    self.stroke_cancel();
                } else {
                    self.stroke_end(samples);
                    // a snapped shape the pen moved: its handles where it now is
                    out.push(TouchCommand::StrokeEnded);
                }
            }
            Mode::Gesture if !cancelled => {
                let (four, three) = (t.settings.four_tap, t.settings.undo_taps);
                if now - t.tap_start <= TAP_MS && t.tap_moved <= TAP_SLOP * t.scale {
                    match t.tap_pointers {
                        2 if three => out.push(TouchCommand::Undo),
                        3 if three => out.push(TouchCommand::Redo),
                        4 => {
                            if let Some(a) = four {
                                // in the middle of the screen, not under the fingers
                                Self::act(a, t.down_at, &mut out);
                            }
                        }
                        _ => {}
                    }
                } else if t.tap_pointers == 2
                    && now - t.gesture_start <= QUICK_PINCH_MS
                    && t.gesture_scale < QUICK_PINCH_SCALE
                {
                    // a quick pinch in fits the canvas to the screen, settling into place
                    out.push(TouchCommand::Fit);
                }
            }
            Mode::Pick => {
                t.pick_next = false;
                out.push(if cancelled { TouchCommand::PickCancel } else { TouchCommand::PickDone });
            }
            Mode::Adjust => {
                let at = last_of(&samples).map_or(t.down_at, |s| (s.x, s.y));
                let liquifying = self.shared.lock().adjust.open().is_some_and(|o| o.0 == Adjustment::Liquify);
                if liquifying {
                    // still moving when it lifts, liquify carries on
                    if !cancelled && now - t.liquify_ms <= LIQUIFY_STILL_MS {
                        let (vx, vy) = t.liquify_velocity;
                        drop(t);
                        self.adjustment_liquify_coast(vx, vy);
                    }
                } else if !cancelled && !t.adjust_moved && now - t.tap_start <= TAP_MS {
                    out.push(TouchCommand::AdjustMenu { x: at.0, y: at.1 });
                }
            }
            Mode::Select => {
                drop(t);
                if !cancelled {
                    let at = last_of(&samples).map(|s| ((s.x, s.y), s.time_ms));
                    if let Some((at, ms)) = at {
                        self.select_step(at, ms, true, &mut out);
                    }
                }
            }
            Mode::Transform => {
                let (handle, grabbed, tapped) = (t.transform_handle, t.transform_grabbed, now - t.tap_start <= TAP_MS);
                let at = last_of(&samples).map_or(t.down_at, |s| (s.x, s.y));
                let how = t.transforming;
                t.transform_grabbed = false;
                drop(t);
                if cancelled {
                    if grabbed {
                        out.push(TouchCommand::TransformMoved { transform: self.transform_cancel_drag() });
                    }
                } else if grabbed {
                    out.push(TouchCommand::TransformMoved { transform: self.transform_release() });
                    out.push(TouchCommand::TransformLetGo { handle, x: at.0, y: at.1 });
                } else if tapped && handle == crate::TransformHandle::Move {
                    // a tap off the nodes nudges the content a pixel towards it
                    out.push(TouchCommand::TransformMoved { transform: self.transform_nudge(at.0, at.1) });
                } else if tapped && how.is_some() {
                    // a tap on a node types its numbers beside it: the rotate node its angle, the
                    // others the size
                    if let Some(command) = self.numeric_at(handle) {
                        out.push(command);
                    }
                }
            }
            Mode::Pour => {
                let slid = last_of(&samples).and_then(|s| Self::pour_slide(&mut t, (s.x, s.y), s.time_ms));
                drop(t);
                if let Some(threshold) = slid.filter(|_| !cancelled) {
                    self.pour_threshold(threshold);
                    out.push(TouchCommand::FillThreshold { threshold });
                }
                self.pour_end(!cancelled, None);
                out.push(TouchCommand::Filled);
            }
            // a tap, on its outline or elsewhere, ends the editing; a drag only turned or moved it
            Mode::ShapePin | Mode::ShapeMove => {
                let tapped = !t.shape_dragged && now - t.tap_start <= TAP_MS;
                drop(t);
                if mode == Mode::ShapePin {
                    self.quickshape_unpin();
                    out.push(TouchCommand::ShapeChanged);
                }
                if tapped {
                    self.end_shape(&mut out);
                }
            }
            _ => {}
        }
        out
    }

    /// Lets a snapped shape go as it is (a touch elsewhere).
    fn end_shape(&self, out: &mut Vec<TouchCommand>) {
        let mut t = self.touches();
        self.end_shape_with(&mut t, out);
    }

    fn end_shape_with(&self, t: &mut Touches, out: &mut Vec<TouchCommand>) {
        if self.shared.lock().shape.is_some() {
            self.quickshape_done();
            t.shape_editing = false;
            out.push(TouchCommand::ShapeEnded);
        }
    }

    /// The touch made while the modify button is held begins.
    fn modify_begins(
        &self,
        purpose: GestureAction,
        kind: PointerKind,
        samples: &[PenSample],
        out: &mut Vec<TouchCommand>,
    ) {
        let Some(first) = samples.last() else { return };
        let at = (first.x, first.y);
        match purpose {
            GestureAction::Eyedropper => out.push(TouchCommand::Pick { x: at.0, y: at.1 }),
            GestureAction::Smudge | GestureAction::Erase | GestureAction::Assisted => {
                match purpose {
                    GestureAction::Erase => self.touches().tool_before = Some(self.swap_tool(ToolKind::Erase)),
                    GestureAction::Smudge => self.touches().tool_before = Some(self.swap_tool(ToolKind::Smudge)),
                    _ => self.set_stroke_assist(true),
                }
                if self.current_is_text() {
                    out.push(TouchCommand::TextLayer);
                }
                self.stroke_begin(kind, samples.to_vec());
            }
            a => Self::act(a, at, out),
        }
    }

    /// The tool and drawing assist as they were before a stroke that used others.
    fn end_stroke_tools(&self, t: &mut Touches) {
        if let Some(before) = t.tool_before.take() {
            self.set_tool(before);
        }
        if std::mem::take(&mut t.assisted) {
            self.set_stroke_assist(false);
        }
    }

    /// Paints with `tool` for a stroke; returns the tool chosen before, to put back after.
    fn swap_tool(&self, tool: ToolKind) -> ToolKind {
        let before = self.shared.lock().tool;
        self.set_tool(tool);
        before
    }

    /// A step of the selection tool's touch at `at` (`done` as it lifts): a rectangle or ellipse
    /// dragged out; a freehand path drawn (or tapped point by point, closed by a tap on its first
    /// point; held still, it snaps to a shape); a colour picked where it went down, its threshold
    /// slid left and right, faster the faster it goes.
    fn select_step(&self, at: (f32, f32), ms: f64, done: bool, out: &mut Vec<TouchCommand>) {
        let mut t = self.touches();
        let Some(how) = t.selecting else { return };
        let start = t.down_at;
        if dist(at, start) > TRANSFORM_SLOP * t.scale {
            t.select_moved = true;
        }
        let moved = t.select_moved;
        match how.kind {
            SelectKind::Rectangle | SelectKind::Ellipse => {
                drop(t);
                if done {
                    if dist(at, start) >= 1.0 {
                        let ellipse = how.kind == SelectKind::Ellipse;
                        self.select_shape(ellipse, start.0, start.1, at.0, at.1, how.join);
                    }
                    out.push(TouchCommand::SelectionChanged);
                } else if moved {
                    out.push(TouchCommand::SelectionDrag { x0: start.0, y0: start.1, x1: at.0, y1: at.1 });
                }
            }
            SelectKind::Freehand => {
                // snapped: the shape is kept as it is, and the lift selects it
                if t.lasso_snapped {
                    if done {
                        t.lasso_snapped = false;
                        drop(t);
                        self.close_freehand();
                        out.push(TouchCommand::FreehandChanged);
                        out.push(TouchCommand::SelectionChanged);
                    }
                    return;
                }
                if done {
                    t.quickshape_due = None;
                } else if moved && dist(at, t.hold_at) > QUICKSHAPE_STILL * t.scale {
                    // still for the QuickShape delay, it snaps
                    t.hold_at = at;
                    t.quickshape_due = t.settings.draw_and_hold.then_some(ms + t.settings.quickshape_ms);
                }
                let scale = t.scale;
                let last = t.freehand.last().copied();
                drop(t);
                let (to_canvas, to_window) = {
                    let st = self.shared.lock();
                    let view = st.view;
                    (
                        st.window_to_canvas(at.0, at.1).map(|p| (p.x, p.y)),
                        last.and_then(|(x, y)| {
                            view.map(|v| v.to_screen().apply(procraft_engine::geom::Vec2::new(x, y)))
                        }),
                    )
                };
                let first_canvas = self.shared.lock().window_to_canvas(start.0, start.1).map(|p| (p.x, p.y));
                if done && !moved {
                    // a tap: a point of the path, or, by its first point, the path closed
                    let mut t = self.touches();
                    let first = t.freehand.first().copied();
                    let near_first = first.zip(self.shared.lock().view).is_some_and(|((x, y), v)| {
                        let p = v.to_screen().apply(procraft_engine::geom::Vec2::new(x, y));
                        dist((p.x, p.y), at) <= FREEHAND_CLOSE * scale
                    });
                    if t.freehand.len() > 2 && near_first {
                        drop(t);
                        self.close_freehand();
                        out.push(TouchCommand::SelectionChanged);
                    } else if let Some(p) = to_canvas {
                        t.freehand.push(p);
                    }
                    out.push(TouchCommand::FreehandChanged);
                } else if moved && to_window.is_none_or(|w| dist((w.x, w.y), at) >= FREEHAND_STEP * scale) {
                    let mut t = self.touches();
                    if t.freehand.is_empty()
                        && let Some(first) = first_canvas
                    {
                        t.freehand.push(first);
                    }
                    if let Some(p) = to_canvas {
                        t.freehand.push(p);
                    }
                    out.push(TouchCommand::FreehandChanged);
                }
            }
            SelectKind::Automatic => {
                // across the screen the threshold follows the drag, faster the faster it goes
                let (last_x, last_ms) = t.pour_last;
                t.pour_last = (at.0, ms);
                let dx = (at.0 - last_x) / t.scale;
                t.select_threshold = (t.select_threshold + drag_step(dx, ms - last_ms)).clamp(0.0, 1.0);
                let threshold = t.select_threshold;
                drop(t);
                if done {
                    self.select_colour_at(start, threshold, how.join, false);
                    out.push(TouchCommand::SelectionThreshold { x: start.0, y: start.1, threshold, done });
                    out.push(TouchCommand::SelectionChanged);
                } else if moved {
                    // shown by the host once a frame: a selection by colour takes too long on a big
                    // canvas to redo at every move
                    out.push(TouchCommand::SelectionThreshold { x: start.0, y: start.1, threshold, done });
                }
            }
        }
    }

    /// Selects by colour at `at` with `threshold`: `live` while it is being slid (shown filled),
    /// else as it stays (tinted).
    fn select_colour_at(&self, at: (f32, f32), threshold: f32, join: crate::SelectionJoin, live: bool) {
        let display = if live { crate::SelectionDisplay::Fill } else { crate::SelectionDisplay::Tint };
        self.set_selection_display(display);
        self.select_by_colour(at.0, at.1, threshold, join, live);
    }

    /// What a transform's touch at `at` takes: a warp's shown control point, or the box's nearest
    /// node, else the content.
    fn transform_handle_at(&self, at: (f32, f32), how: TransformTouch) -> crate::TransformHandle {
        let (shown, view, density) = {
            let st = self.shared.lock();
            (st.transform_box(), st.view.map(|v| v.to_screen()), st.display_scale)
        };
        let (Some(b), Some(m)) = (shown, view) else { return crate::TransformHandle::Move };
        let view = vec![m.a, m.b, m.c, m.d, m.e, m.f];
        if !b.mesh.is_empty() {
            return crate::mesh_handle_at(b.mesh, view, at.0, at.1, density, how.advanced_mesh);
        }
        crate::screen_box(b.corners, view, density)
            .map_or(crate::TransformHandle::Move, |s| crate::handle_at(s, at.0, at.1, density))
    }

    /// Typing a tapped node's numbers beside it: where the node is, and which side of the box.
    fn numeric_at(&self, handle: crate::TransformHandle) -> Option<TouchCommand> {
        let (b, m, density) = {
            let st = self.shared.lock();
            (st.transform_box()?, st.view?.to_screen(), st.display_scale)
        };
        let s = crate::screen_box(b.corners, vec![m.a, m.b, m.c, m.d, m.e, m.f], density)?;
        let point = |v: &[f32], i: usize| Some((*v.get(2 * i)?, *v.get(2 * i + 1)?));
        let node = match handle {
            crate::TransformHandle::Rotate => point(&s.rotate, 0)?,
            crate::TransformHandle::Corner { index } => point(&s.corners, usize::from(index))?,
            crate::TransformHandle::Edge { index } => point(&s.edges, usize::from(index))?,
            _ => return None,
        };
        let middle = s.corners.as_chunks::<2>().0.iter().map(|[x, _]| x).sum::<f32>() / 4.0;
        Some(TouchCommand::TransformNumeric {
            angle: handle == crate::TransformHandle::Rotate,
            x: node.0,
            y: node.1,
            right: node.0 > middle + 1.0,
        })
    }

    /// Whether the current layer is a text layer (it takes no paint until rasterized).
    fn current_is_text(&self) -> bool {
        let st = self.shared.lock();
        st.doc.layer(st.doc.current()).is_some_and(|l| l.text.is_some())
    }

    /// What a gesture set to `action` does at `at`, for the app.
    fn act(action: GestureAction, at: (f32, f32), out: &mut Vec<TouchCommand>) {
        out.push(match action {
            GestureAction::Eyedropper => TouchCommand::Pick { x: at.0, y: at.1 },
            GestureAction::QuickShape => TouchCommand::QuickShapeLast,
            GestureAction::QuickMenu => TouchCommand::QuickMenu { x: at.0, y: at.1 },
            GestureAction::FullScreen => TouchCommand::ToggleBars,
            GestureAction::ClearLayer => TouchCommand::ClearLayer,
            GestureAction::CopyPaste => TouchCommand::CopyPaste,
            GestureAction::LayerSelect => TouchCommand::LayerSelect { x: at.0, y: at.1 },
            // strokes of another kind aren't things to do
            GestureAction::Smudge | GestureAction::Erase | GestureAction::Assisted => return,
        });
    }

    fn follow_fingers(t: &mut Touches) {
        match t.down.as_slice() {
            [a, b, ..] => {
                t.gesture_pair = Some((a.id, b.id));
                t.last = pair(a.at, b.at);
            }
            _ => t.gesture_pair = None,
        }
    }

    /// One step of two fingers: the view follows them. True if it moved.
    fn move_gesture(&self, t: &mut Touches) -> bool {
        let Some((a, b)) = t.gesture_pair else { return false };
        let at = |id: u64| t.down.iter().find(|d| d.id == id).map(|d| d.at);
        let (Some(a), Some(b)) = (at(a), at(b)) else { return false };
        let (x, y, d, angle) = pair(a, b);
        let (lx, ly, ld, la) = t.last;
        let scale = if ld > 1.0 && d > 1.0 { d / ld } else { 1.0 };
        let mut turn = angle - la;
        if turn > std::f32::consts::PI {
            turn -= std::f32::consts::TAU;
        }
        if turn < -std::f32::consts::PI {
            turn += std::f32::consts::TAU;
        }
        if !t.settings.rotate {
            turn = 0.0;
        }
        t.tap_moved += (x - lx).hypot(y - ly) + (d - ld).abs();
        t.gesture_scale *= scale;
        t.last = (x, y, d, angle);
        self.view_gesture(lx, ly, x - lx, y - ly, scale, turn);
        true
    }

    /// Three fingers rubbed back and forth (three turns) clear the current layer, once a gesture;
    /// swiped down, they do what Three-finger swipe is set to.
    fn scrub(t: &mut Touches, out: &mut Vec<TouchCommand>) {
        let c = Self::centre(t);
        if !t.swiped && t.down.len() == 3 {
            let (dx, dy) = (c.0 - t.swipe_start.0, c.1 - t.swipe_start.1);
            if dy >= SWIPE_DOWN * t.scale && dx.abs() <= dy / 2.0 {
                t.swiped = true;
                t.scrub_turns = -1;
                if let Some(a) = t.settings.three_swipe {
                    Self::act(a, c, out);
                }
            }
        }
        let dx = c.0 - t.scrub_x;
        t.scrub_x = c.0;
        t.tap_moved += dx.abs();
        if dx == 0.0 || t.scrub_turns < 0 {
            return;
        }
        let direction = if dx > 0.0 { 1 } else { -1 };
        if direction != t.scrub_direction {
            if t.scrub_direction != 0 && t.scrub_travel >= SCRUB_STEP * t.scale {
                t.scrub_turns += 1;
            }
            t.scrub_direction = direction;
            t.scrub_travel = 0.0;
        }
        t.scrub_travel += dx.abs();
        if t.scrub_turns >= SCRUB_TURNS {
            t.scrub_turns = -1;
            out.push(TouchCommand::ClearLayer);
        }
    }

    /// Continue filling's touch at `at`: slid past the slop, it moves the fill's threshold as a
    /// selection's follows a drag, only more gently. The threshold, when it moved.
    fn pour_slide(t: &mut Touches, at: (f32, f32), ms: f64) -> Option<f32> {
        if dist(at, t.down_at) > POUR_SLOP * t.scale {
            t.pour_moved = true;
        }
        let (last_x, last_ms) = t.pour_last;
        t.pour_last = (at.0, ms);
        let dx = (at.0 - last_x) / t.scale;
        let moved = t.pour_moved;
        let fill = t.filling.as_mut().filter(|_| moved && dx != 0.0)?;
        fill.threshold = (fill.threshold + drag_step(dx, ms - last_ms) / POUR_SLIDE_GENTLER).clamp(0.0, 1.0);
        Some(fill.threshold)
    }

    fn centre(t: &Touches) -> (f32, f32) {
        let n = t.down.len().max(1) as f32;
        let (x, y) = t.down.iter().fold((0.0, 0.0), |(x, y), d| (x + d.at.0, y + d.at.1));
        (x / n, y / n)
    }
}

/// The touches' state, kept with the session's.
pub(crate) type TouchState = Mutex<Touches>;

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32, ms: f64) -> Vec<PenSample> {
        vec![PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: ms }]
    }

    fn session() -> std::sync::Arc<CanvasSession> {
        CanvasSession::new(64, 64)
    }

    #[test]
    fn two_fingers_tapped_undo_and_three_redo() {
        let s = session();
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(300.0, 100.0, 10.0));
        s.touch_up(2, at(300.0, 100.0, 100.0));
        let out = s.touch_up(1, at(100.0, 100.0, 110.0));
        assert!(out.contains(&TouchCommand::Undo), "{out:?}");

        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 1000.0));
        s.touch_down(2, PointerKind::Finger, at(300.0, 100.0, 1005.0));
        s.touch_down(3, PointerKind::Finger, at(500.0, 100.0, 1010.0));
        s.touch_up(3, at(500.0, 100.0, 1100.0));
        s.touch_up(2, at(300.0, 100.0, 1100.0));
        let out = s.touch_up(1, at(100.0, 100.0, 1110.0));
        assert!(out.contains(&TouchCommand::Redo), "{out:?}");
    }

    #[test]
    fn two_fingers_held_still_keep_undoing() {
        let s = session();
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(300.0, 100.0, 10.0));
        assert_eq!(s.touch_deadline(), Some(660.0));
        assert_eq!(s.touch_tick(660.0), vec![TouchCommand::Undo]);
        assert_eq!(s.touch_tick(780.0), vec![TouchCommand::Undo]);
    }

    #[test]
    fn a_quick_pinch_in_fits_the_canvas() {
        let s = session();
        s.touch_down(1, PointerKind::Finger, at(100.0, 300.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(700.0, 300.0, 5.0));
        for k in 1..=10 {
            let d = 30.0 * k as f32;
            s.touch_move(1, at(100.0 + d, 300.0, 5.0 + 20.0 * f64::from(k)));
            s.touch_move(2, at(700.0 - d, 300.0, 5.0 + 20.0 * f64::from(k)));
        }
        s.touch_up(2, at(400.0, 300.0, 260.0));
        let out = s.touch_up(1, at(400.0, 300.0, 270.0));
        assert!(out.contains(&TouchCommand::Fit), "{out:?}");
    }

    #[test]
    fn a_finger_held_still_picks_a_colour() {
        let s = session();
        s.touch_down(1, PointerKind::Finger, at(200.0, 200.0, 0.0));
        let out = s.touch_tick(399.0);
        assert!(!out.iter().any(|c| matches!(c, TouchCommand::Pick { .. })));
        let out = s.touch_tick(400.0);
        assert!(out.contains(&TouchCommand::Pick { x: 200.0, y: 200.0 }), "{out:?}");
        let out = s.touch_move(1, at(210.0, 200.0, 450.0));
        assert!(out.contains(&TouchCommand::Pick { x: 210.0, y: 200.0 }));
        let out = s.touch_up(1, at(210.0, 200.0, 500.0));
        assert!(out.contains(&TouchCommand::PickDone));
    }

    #[test]
    fn three_fingers_scrubbed_clear_and_swiped_down_open_copy_and_paste() {
        let s = session();
        for (id, x) in [(1, 100.0), (2, 200.0), (3, 300.0)] {
            s.touch_down(id, PointerKind::Finger, at(x, 300.0, 0.0));
        }
        let mut cleared = false;
        for (leg, dx) in [100.0f32, -100.0, 100.0, -100.0, 100.0].into_iter().enumerate() {
            for step in 1..=5 {
                let shift = if leg % 2 == 0 { 0.0 } else { 100.0 } + dx * step as f32 / 5.0;
                for (id, x) in [(1, 100.0), (2, 200.0), (3, 300.0)] {
                    let out = s.touch_move(id, at(x + shift, 300.0, 10.0 * (leg * 5 + step) as f64));
                    cleared |= out.contains(&TouchCommand::ClearLayer);
                }
            }
        }
        assert!(cleared);
        for id in 1..=3 {
            s.touch_up(id, at(0.0, 0.0, 1000.0));
        }

        for (id, x) in [(1, 100.0), (2, 200.0), (3, 300.0)] {
            s.touch_down(id, PointerKind::Finger, at(x, 100.0, 2000.0));
        }
        let mut copy_paste = false;
        for step in 1..=10 {
            for (id, x) in [(1, 100.0), (2, 200.0), (3, 300.0)] {
                let out = s.touch_move(id, at(x, 100.0 + 20.0 * step as f32, 2000.0 + 10.0 * f64::from(step)));
                copy_paste |= out.contains(&TouchCommand::CopyPaste);
            }
        }
        assert!(copy_paste);
    }

    #[test]
    fn the_modify_button_picks_with_the_next_touch_or_while_held() {
        let s = session();
        s.set_pick_next(true);
        let out = s.touch_down(1, PointerKind::Stylus, at(50.0, 60.0, 0.0));
        assert!(out.contains(&TouchCommand::Pick { x: 50.0, y: 60.0 }));
        let out = s.touch_up(1, at(50.0, 60.0, 50.0));
        assert!(out.contains(&TouchCommand::PickDone));
        assert!(!s.pick_next());

        s.set_modify_held(true);
        let out = s.touch_down(1, PointerKind::Finger, at(70.0, 80.0, 100.0));
        assert!(out.contains(&TouchCommand::Pick { x: 70.0, y: 80.0 }));
        let out = s.touch_up(1, at(70.0, 80.0, 150.0));
        assert!(out.contains(&TouchCommand::PickDone));
        // a touch used it: its tap does nothing
        assert!(s.set_modify_held(false));
        assert!(!s.set_modify_held(false));
    }

    #[test]
    fn an_adjustments_amount_is_swiped_and_a_tap_asks_for_its_menu() {
        let s = session();
        s.shared.lock().adjust = crate::adjusting::Adjusting::opened(Adjustment::GaussianBlur);
        let out = s.touch_down(1, PointerKind::Finger, at(10.0, 30.0, 0.0));
        assert!(!out.iter().any(|c| matches!(c, TouchCommand::Cursor { .. })), "{out:?}");
        let out = s.touch_move(1, at(60.0, 30.0, 30.0));
        assert!(out.contains(&TouchCommand::AdjustChanged), "{out:?}");
        let swiped = s.adjustment_shown().amount;
        assert!(swiped > 0.0, "{swiped}");
        let out = s.touch_up(1, at(60.0, 30.0, 40.0));
        assert!(!out.iter().any(|c| matches!(c, TouchCommand::AdjustMenu { .. })), "{out:?}");
        // back the other way, the amount goes down
        s.touch_down(1, PointerKind::Finger, at(60.0, 30.0, 100.0));
        s.touch_move(1, at(20.0, 30.0, 130.0));
        s.touch_up(1, at(20.0, 30.0, 140.0));
        assert!(s.adjustment_shown().amount < swiped);
        // a tap asks for the menu where it was
        s.touch_down(1, PointerKind::Finger, at(30.0, 30.0, 300.0));
        let out = s.touch_up(1, at(31.0, 30.0, 350.0));
        assert!(out.contains(&TouchCommand::AdjustMenu { x: 31.0, y: 30.0 }), "{out:?}");
    }

    #[test]
    fn the_selection_tool_drags_out_shapes_and_slides_a_colour_selections_threshold() {
        let s = session();
        let tool = |kind| Some(SelectTouch { kind, join: crate::SelectionJoin::Add, threshold: 0.5 });
        s.set_touch_selecting(tool(SelectKind::Rectangle));
        s.touch_down(1, PointerKind::Stylus, at(10.0, 10.0, 0.0));
        let out = s.touch_move(1, at(120.0, 90.0, 20.0));
        assert!(out.contains(&TouchCommand::SelectionDrag { x0: 10.0, y0: 10.0, x1: 120.0, y1: 90.0 }), "{out:?}");
        let out = s.touch_up(1, at(120.0, 90.0, 40.0));
        assert!(out.contains(&TouchCommand::SelectionChanged), "{out:?}");
        assert!(!out.contains(&TouchCommand::StrokeEnded));

        s.set_touch_selecting(tool(SelectKind::Automatic));
        s.touch_down(1, PointerKind::Finger, at(50.0, 50.0, 100.0));
        let out = s.touch_move(1, at(120.0, 50.0, 120.0));
        let slid = out.iter().find_map(|c| match c {
            TouchCommand::SelectionThreshold { threshold, done: false, .. } => Some(*threshold),
            _ => None,
        });
        assert!(slid.is_some_and(|t| t > 0.5), "{out:?}");
        let out = s.touch_up(1, at(120.0, 50.0, 140.0));
        assert!(out.iter().any(|c| matches!(c, TouchCommand::SelectionThreshold { done: true, .. })), "{out:?}");
        s.set_touch_selecting(None);
    }

    #[test]
    fn a_transform_takes_the_touches_and_paints_nothing() {
        let s = session();
        s.set_touch_transforming(Some(TransformTouch { advanced_mesh: false }));
        let out = s.touch_down(1, PointerKind::Stylus, at(20.0, 20.0, 0.0));
        assert!(out.contains(&TouchCommand::TransformTouched), "{out:?}");
        // a tap off the nodes nudges (no box here: it says so)
        let out = s.touch_up(1, at(20.0, 20.0, 50.0));
        assert!(out.contains(&TouchCommand::TransformMoved { transform: None }), "{out:?}");
        assert!(!out.contains(&TouchCommand::StrokeEnded));
        s.set_touch_transforming(None);
        let out = s.touch_down(1, PointerKind::Stylus, at(20.0, 20.0, 100.0));
        assert!(!out.contains(&TouchCommand::TransformTouched));
    }

    #[test]
    fn continue_filling_fills_each_touch_and_a_slide_moves_its_threshold() {
        let s = session();
        s.set_touch_filling(Some(Fill { r: 1.0, g: 0.0, b: 0.0, threshold: 0.5 }));
        // off the canvas (no view yet) nothing fills, and the touch does nothing else
        let out = s.touch_down(1, PointerKind::Finger, at(10.0, 10.0, 0.0));
        assert!(!out.iter().any(|c| matches!(c, TouchCommand::Pick { .. })), "{out:?}");
        let out = s.touch_up(1, at(10.0, 10.0, 50.0));
        assert!(!out.contains(&TouchCommand::Filled), "{out:?}");

        // a slide's threshold, as the touch's own
        let mut t = s.touches();
        t.mode = Mode::Pour;
        t.down_at = (100.0, 100.0);
        t.pour_last = (100.0, 0.0);
        t.pour_moved = false;
        assert_eq!(CanvasSession::pour_slide(&mut t, (110.0, 100.0), 16.0), None, "within the slop");
        let up = CanvasSession::pour_slide(&mut t, (160.0, 100.0), 32.0).unwrap();
        assert!(up > 0.5 && up < 1.0, "{up}");
        let down = CanvasSession::pour_slide(&mut t, (100.0, 100.0), 48.0).unwrap();
        assert!(down < up, "{down} < {up}");
        t.mode = Mode::None;
        drop(t);
        s.set_touch_filling(None);
        let out = s.touch_down(1, PointerKind::Finger, at(10.0, 10.0, 100.0));
        s.touch_up(1, at(10.0, 10.0, 150.0));
        assert!(!out.contains(&TouchCommand::Filled));
    }
}
