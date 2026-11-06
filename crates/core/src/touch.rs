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
//! edited has its handles, outline and body dragged. With a panel open over the canvas a touch waits:
//! a tap only closes the panel; a drag fades it and goes on from where it came down.
//!
//! Positions are window pixels and times the platform's event clock (milliseconds).

use std::sync::Mutex;

mod box_pinch;
mod pen;

pub use pen::{HoverCursor, PenHover, PenPreferred, SqueezePhase};

use crate::adjust::PerspectiveKind;
use crate::adjust_menu::Adjustment;
use crate::adjusting::{CLONE_PUCK_RADIUS, CLONE_PUCK_SLOP, DIRECTIONAL_INNER, DIRECTIONAL_REACH, POSITIONAL_GRAB};
use crate::{CanvasSession, PenSample, PointerKind, ToolKind};

/// A tap: down and up within this, moving no further than [`TAP_SLOP`] points.
const TAP_MS: f64 = 300.0;
/// Liquify held still works on from this long after it stops, as measured; moved less than this
/// (points) it counts as still. Time passes for it, and for what it set going, this often.
const LIQUIFY_HELD_WAIT_MS: f64 = 110.0;
const LIQUIFY_HELD_STEP_MS: f64 = 16.0;
const LIQUIFY_HELD_SLOP: f32 = 3.0;
const TAP_SLOP: f32 = 24.0;
/// Each of the fingers tapped together stays short of this (points) from where it landed: this
/// far, they are moving the canvas.
const FINGERS_TAP_SLOP: f32 = 15.0;
/// A touch on the canvas with a panel open moved this far (points) is a drag, not a tap.
const PANEL_SLOP: f32 = 10.0;
/// Fingers together on the canvas leave the view as it is until they have gone this far between
/// them (points) from where they landed, so a tap doesn't nudge it: measured over 8, under 12.
const VIEW_SLOP: f32 = 10.0;
/// The single touch companion's Zoom: a drag this far (points) up zooms the canvas in e times.
pub const COMPANION_ZOOM_DISTANCE: f32 = 180.0;

/// The single touch companion's mode: a lone press dragged on the canvas zooms it (up in, about
/// where it went down) or moves it, instead of painting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CompanionMode {
    Zoom,
    Move,
}
/// Two (three) fingers held still go on undoing (redoing) every this often.
const RAPID_REPEAT_MS: f64 = 120.0;
/// A pen held over the canvas and not heard from for this long (seconds) has gone.
const PEN_HOVER_STALE_S: f32 = 0.5;
/// Two fingers parting while they close faster than this (their spread, as a part of where it
/// started, a second) fit the canvas if it is pinched smaller than it fits; the speed is taken
/// over this long (milliseconds) before they part.
const QUICK_PINCH_SPEED: f32 = -2.5;
const QUICK_PINCH_WINDOW_MS: f64 = 40.0;
/// Three fingers turning back and forth this often clear the layer, each leg this far (points).
const SCRUB_TURNS: i32 = 3;
const SCRUB_STEP: f32 = 24.0;
/// Three fingers swiped down this far (points), mostly straight.
const SWIPE_DOWN: f32 = 70.0;
/// A stroke counts as still within this many points.
const QUICKSHAPE_STILL: f32 = 6.0;
/// A snapped shape being edited: a handle within this takes a drag, and its outline within this.
const QUICKSHAPE_GRAB: f32 = 22.0;
/// A shape's node stays where it is until the finger has gone this far (points), and then goes as
/// it does, that far behind it.
const NODE_SLOP: f32 = 10.0;
/// A fill's touch slid past this (points) changes its threshold, this much more gently than a
/// selection's.
const POUR_SLOP: f32 = 15.0;
pub(crate) const POUR_SLIDE_GENTLER: f32 = 2.7;

/// A transform's touch moved this far (points) drags what it took (a selection's too).
const TRANSFORM_SLOP: f32 = 15.0;
/// A touch on perspective blur's puck moved this far (points) drags it.
const PUCK_SLOP: f32 = 10.0;
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

/// A pointer's move within an input event: which, and its input since the last.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TouchMove {
    pub id: u64,
    pub samples: Vec<PenSample>,
}

/// The gesture settings the touches follow: what each way of touching does, and the delays.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TouchSettings {
    /// One finger paints as the pen does.
    pub finger_paint: bool,
    /// One finger, not painting, moves the canvas once past a tap's slop.
    pub one_finger_pan: bool,
    /// Two- and three-finger taps undo and redo (off, they still do while a transform, the
    /// selection tool or liquify is open).
    pub undo_taps: bool,
    /// Two fingers turn the canvas as they zoom it.
    pub rotate: bool,
    /// One finger paints a model, and turns it with the modify button held (else it turns it).
    pub model_finger_paint: bool,
    /// Pausing at the end of a stroke snaps it to a shape.
    pub draw_and_hold: bool,
    pub touch: Option<GestureAction>,
    pub pen: Option<GestureAction>,
    pub hold: Option<GestureAction>,
    pub modify_touch: Option<GestureAction>,
    pub modify_pen: Option<GestureAction>,
    pub four_tap: Option<GestureAction>,
    pub three_swipe: Option<GestureAction>,
    /// The pen's first and second buttons (the first, set to nothing, switches to the eraser and
    /// back).
    pub pen_button: Option<GestureAction>,
    pub pen_second_button: Option<GestureAction>,
    /// Three fingers rubbed back and forth clear the layer.
    pub scrub: bool,
    /// While the pen hovers over the canvas, a finger dragged across moves the brush's opacity and
    /// two pinched its size.
    pub hover_touch: bool,
    pub hold_ms: f64,
    pub quickshape_ms: f64,
    pub rapid_undo_ms: f64,
}

impl Default for TouchSettings {
    /// As the app ships.
    fn default() -> TouchSettings {
        TouchSettings {
            finger_paint: true,
            one_finger_pan: false,
            undo_taps: true,
            rotate: true,
            model_finger_paint: false,
            draw_and_hold: true,
            touch: None,
            pen: None,
            hold: Some(GestureAction::Eyedropper),
            modify_touch: Some(GestureAction::Eyedropper),
            modify_pen: Some(GestureAction::Eyedropper),
            four_tap: Some(GestureAction::FullScreen),
            three_swipe: Some(GestureAction::CopyPaste),
            pen_button: None,
            pen_second_button: None,
            scrub: true,
            hover_touch: false,
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
    /// The brush's size or opacity changed (Hover's touch): its sliders show it again.
    BrushChanged,
    /// Recolor's reticle dragged to window pixel (`x`, `y`)
    /// ([`CanvasSession::recolor_move_to`]), or a tap there starting it again
    /// ([`CanvasSession::recolor_tap`]): the app moves it, at once or on its fill thread.
    RecolorMoved {
        x: f32,
        y: f32,
    },
    RecolorTapped {
        x: f32,
        y: f32,
    },
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
    /// The colour panel opens (or closes); the brush library: what the pen's own settings on the
    /// platform ask its double-tap or squeeze to show ([`crate::PenPreferred`]).
    ShowColours,
    ShowBrushes,
    /// A touch held for an open panel ([`CanvasSession::set_touch_panel_open`]) became a drag: the
    /// panel fades away, and the touch goes on. It was a tap: the panel closes, and nothing else
    /// happens. (Held still, it does what a hold does, the panel staying.)
    PanelFades,
    PanelTapped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    None,
    /// A picture being placed on a model follows the touch.
    Picture,
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
    /// Layer Opacity: the touch drags the layer's opacity across the canvas.
    Opacity,
    /// Recolor is on: the touch drags its reticle about, or (a tap) starts it again where it lands.
    Recolor,
    /// The companion's Zoom or Move: the press zooms or moves the canvas; others meanwhile do
    /// nothing.
    Companion,
    /// A finger on a shown model: it turns the model.
    Turn,
    /// Hover's touch: a finger dragging the opacity, two pinching the size.
    Hover,
    /// A panel is open: the touch waits to be a tap or a drag, doing nothing yet.
    Held,
}

/// A drag on perspective blur's puck: it moves the puck (held this far from its centre, window
/// pixels) or turns its arrow.
#[derive(Clone, Copy, Debug, PartialEq)]
enum PuckDrag {
    Move(f32, f32),
    Turn,
}

#[derive(Clone, Copy, Debug)]
struct Down {
    id: u64,
    at: (f32, f32),
    /// Where it landed.
    from: (f32, f32),
}

/// A touch waiting for an open panel: its kind, its first input, and its input since.
struct Held {
    kind: PointerKind,
    first: Vec<PenSample>,
    moves: Vec<PenSample>,
}

/// The touches' state: kept by the session, apart from the canvas's (they call into it).
pub(crate) struct Touches {
    settings: TouchSettings,
    mode: Mode,
    down: Vec<Down>,
    stroke_pointer: u64,
    /// The tool chosen before this stroke used another (Touch set to erase), to put back after.
    tool_before: Option<ToolKind>,
    /// The tool the pen's button left for the eraser, to go back to.
    tool_before_eraser: Option<ToolKind>,
    assisted: bool,
    down_at: (f32, f32),
    /// When the last finger of a tap landed (the only one, until another does), when the first
    /// of them lifted, how many there were, and how far they have gone (see
    /// [`CanvasSession::note_fingers_moved`]).
    tap_start: f64,
    tap_lifted: Option<f64>,
    tap_pointers: usize,
    tap_moved: f32,
    /// The fingers down have tapped, or held still until they repeat: nothing more until all
    /// are up.
    tap_done: bool,
    /// The two fingers' spread as a part of where it started, and its recent values with their
    /// times (the first at or before the speed's window).
    gesture_scale: f32,
    /// The fingers have gone far enough to move the view ([`VIEW_SLOP`]).
    view_moving: bool,
    /// Others of the fingers moving in this event are still to be told of.
    more_to_come: bool,
    pinch: Vec<(f64, f32)>,
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
    /// liquify's brush, when it last moved and when time last passed for it, and whether a pen
    /// holds it.
    adjust_moved: bool,
    adjust_displacing: bool,
    liquify_ms: f64,
    liquify_tick_ms: f64,
    liquify_pen: bool,
    /// Where liquify's brush came to rest and when, how long (seconds) it has worked on there, and
    /// when it next does.
    liquify_rest: (f32, f32),
    liquify_rest_ms: f64,
    liquify_held: f32,
    liquify_due: Option<f64>,
    /// Where on the clone source it was taken (window pixels from its centre).
    puck_grab: (f32, f32),
    /// What the touch does to perspective blur's puck, and whether it has started.
    puck_drag: Option<PuckDrag>,
    puck_dragging: bool,
    select_threshold: f32,
    freehand: Vec<(f32, f32)>,
    /// A transform is open; what its touch took, and whether it has started dragging it.
    transforming: Option<TransformTouch>,
    transform_handle: crate::TransformHandle,
    transform_grabbed: bool,
    /// Two fingers came down with one on the box: what it holds is theirs, once they've gone a
    /// little way (until then they may yet be a tap); then where they took it.
    box_pinch: Option<Option<[(f32, f32); 2]>>,
    pour_moved: bool,
    pour_last: (f32, f64),
    /// Layer Opacity is on: the layer a drag changes and its opacity as dragged; whether the
    /// touch has changed it yet.
    layer_opacity: Option<(u64, f32)>,
    opacity_moved: bool,
    /// Recolor's touch: past a tap's slop, and where the reticle was (window pixels) as it went down.
    recolor_dragged: bool,
    recolor_from: (f32, f32),
    /// The companion's Zoom or Move, while on, and where its press last was.
    companion: Option<CompanionMode>,
    companion_last: (f32, f32),
    /// Where the finger turning a model last was.
    turn_last: (f32, f32),
    /// The lone finger set to move the canvas: where it last was, and whether it has moved past
    /// a tap's slop (it moves the canvas from then on).
    pan_last: Option<(f32, f32)>,
    panned: bool,
    /// When the pen was last told held over the canvas (none: it has left), and Hover's touch:
    /// where its finger last was (window x) and how far apart two fingers last were.
    pen_hovering: Option<std::time::Instant>,
    hover_x: f32,
    hover_spread: f32,
    /// The pen's own gestures: where it hovers, and a squeeze held.
    pen: pen::PenState,
    /// A panel is open over the canvas, and the touch waiting for it to go.
    panel_open: bool,
    held: Option<Held>,
    /// Window pixels a point: how far a touch has to go.
    scale: f32,
    /// Window pixels a point of the interface: how near a touch has to be to a handle it draws.
    handle_scale: f32,
}

impl Default for Touches {
    fn default() -> Touches {
        Touches {
            settings: TouchSettings::default(),
            mode: Mode::None,
            down: Vec::new(),
            stroke_pointer: 0,
            tool_before: None,
            tool_before_eraser: None,
            assisted: false,
            down_at: (0.0, 0.0),
            tap_start: 0.0,
            tap_lifted: None,
            tap_pointers: 0,
            tap_moved: 0.0,
            tap_done: false,
            gesture_scale: 1.0,
            view_moving: false,
            more_to_come: false,
            pinch: Vec::new(),
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
            liquify_ms: 0.0,
            liquify_tick_ms: 0.0,
            liquify_pen: false,
            liquify_rest: (0.0, 0.0),
            liquify_rest_ms: 0.0,
            liquify_held: 0.0,
            liquify_due: None,
            puck_grab: (0.0, 0.0),
            puck_drag: None,
            puck_dragging: false,
            select_threshold: 0.5,
            freehand: Vec::new(),
            transforming: None,
            transform_handle: crate::TransformHandle::Move,
            transform_grabbed: false,
            box_pinch: None,
            pour_moved: false,
            pour_last: (0.0, 0.0),
            layer_opacity: None,
            opacity_moved: false,
            recolor_dragged: false,
            recolor_from: (0.0, 0.0),
            companion: None,
            companion_last: (0.0, 0.0),
            turn_last: (0.0, 0.0),
            pan_last: None,
            panned: false,
            pen_hovering: None,
            hover_x: 0.0,
            hover_spread: 0.0,
            pen: pen::PenState::default(),
            panel_open: false,
            held: None,
            scale: 1.0,
            handle_scale: 1.0,
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
pub(crate) fn drag_step(dx: f32, ms: f64) -> f32 {
    let frames = (ms.max(1.0) / 1000.0 * 60.0).max(0.06) as f32;
    let speed = dx.abs() / frames;
    (speed / 30.0).powf(0.86) * dx / 200.0
}

/// A liquify stroke's points, (x, y, pressure, milliseconds since the last) each, the first since
/// `since`: a finger's (or a mouse's) at full pressure.
fn liquify_points(pen: bool, samples: &[PenSample], mut since: f64) -> Vec<f32> {
    let mut points = Vec::with_capacity(samples.len() * 4);
    for s in samples {
        points.extend([s.x, s.y, if pen { s.pressure } else { 1.0 }, (s.time_ms - since) as f32]);
        since = s.time_ms;
    }
    points
}

fn is_pen(kind: PointerKind) -> bool {
    matches!(kind, PointerKind::Stylus | PointerKind::StylusEraser)
}

impl CanvasSession {
    fn touches(&self) -> std::sync::MutexGuard<'_, Touches> {
        self.shared.touches.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The tool a stroke borrows (or the one it gives back): not counted as one chosen.
    fn put_tool(&self, tool: ToolKind) {
        self.shared.lock().tool = tool;
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

    /// Layer Opacity on for layer `id` (a drag across the canvas changes its opacity, each drag
    /// one undo step) or off.
    pub fn set_touch_layer_opacity(&self, id: Option<u64>) {
        let start = id.and_then(|id| self.layers().into_iter().find(|l| l.id == id)).map(|l| (l.id, l.opacity));
        self.touches().layer_opacity = start;
    }

    /// The pen is held over the canvas (or not): with Hover's touch on, a finger then sets the
    /// brush rather than painting. Told again with each of the pen's moves: a pen not heard from
    /// for [`PEN_HOVER_STALE_S`] has gone (a system may not say when it leaves).
    pub fn set_touch_pen_hovering(&self, hovering: bool) {
        self.touches().pen_hovering = hovering.then(std::time::Instant::now);
    }

    /// The single touch companion's Zoom or Move on (a lone press dragged on the canvas zooms or
    /// moves it), or off.
    pub fn set_touch_companion(&self, mode: Option<CompanionMode>) {
        self.touches().companion = mode;
    }

    /// A panel is open over the canvas (or not): a touch landing on the canvas waits, until a tap
    /// only closes the panel ([`TouchCommand::PanelTapped`]) or a drag fades it
    /// ([`TouchCommand::PanelFades`]) and does what it would have from where it came down. Held
    /// still, it does what a hold does; more fingers landing move the canvas: both leave the
    /// panel up.
    pub fn set_touch_panel_open(&self, open: bool) {
        self.touches().panel_open = open;
    }

    /// Layer Opacity's layer's opacity as dragged, while it is on.
    pub fn touch_layer_opacity(&self) -> Option<f32> {
        self.touches().layer_opacity.map(|(_, opacity)| opacity)
    }

    /// The snapped shape's handles show (its bar tapped), or not.
    pub fn set_shape_editing(&self, editing: bool) {
        self.touches().shape_editing = editing;
    }

    /// When the touches next need [`CanvasSession::touch_tick`] (a hold, a repeat), if ever.
    pub fn touch_deadline(&self) -> Option<f64> {
        let t = self.touches();
        [t.hold_due, t.quickshape_due, t.rapid_due, t.liquify_due].into_iter().flatten().reduce(f64::min)
    }

    /// A pointer goes down on the canvas (`samples` its first input).
    pub fn touch_down(&self, id: u64, kind: PointerKind, samples: Vec<PenSample>) -> Vec<TouchCommand> {
        let Some(first) = last_of(&samples).copied() else { return Vec::new() };
        let mut out = Vec::new();
        let mut t = self.touches();
        (t.scale, t.handle_scale) = {
            let st = self.shared.lock();
            (st.display_scale, st.interface_scale())
        };
        let at = (first.x, first.y);
        t.down.retain(|d| d.id != id);
        t.down.push(Down { id, at, from: at });
        // the companion's Zoom or Move takes a lone press, and others while it is held do nothing
        if t.mode == Mode::Companion {
            return out;
        }
        if t.companion.is_some() && t.down.len() == 1 {
            t.stroke_pointer = id;
            t.down_at = at;
            t.companion_last = at;
            t.mode = Mode::Companion;
            return out;
        }
        // a touch made while the modify button is held does what □ + Touch (□ + Pen) is set to
        if t.modify_held && t.modify_pointer.is_none() && (t.down.len() == 1 || t.mode == Mode::None) {
            // on a model painted with a finger, the button held makes a finger turn it
            if kind == PointerKind::Finger && t.settings.model_finger_paint && self.shared.lock().scene_shown() {
                t.modify_used = true;
                t.stroke_pointer = id;
                t.turn_last = (first.x, first.y);
                t.down_at = t.turn_last;
                t.mode = Mode::Turn;
                return out;
            }
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
        // the pen landing while only fingers are down (a hand resting on the screen) draws as it
        // would alone: what the fingers were doing is taken back, and they are left alone until
        // lifted
        if is_pen(kind)
            && t.down.len() > 1
            && t.modify_pointer.is_none()
            && matches!(t.mode, Mode::Finger | Mode::Idle | Mode::Gesture | Mode::Pick | Mode::Held)
        {
            let mode = std::mem::replace(&mut t.mode, Mode::None);
            t.held = None;
            t.down.retain(|d| d.id == id);
            t.hold_due = None;
            t.quickshape_due = None;
            t.rapid_due = None;
            match mode {
                Mode::Finger => {
                    self.end_stroke_tools(&mut t);
                    drop(t);
                    self.stroke_cancel();
                    t = self.touches();
                }
                Mode::Pick => {
                    t.pick_next = false;
                    out.push(TouchCommand::PickCancel);
                }
                _ => {}
            }
            out.push(TouchCommand::CursorGone);
        }
        if t.down.len() == 1 && t.panel_open {
            Self::wait_for_panel(&mut t, kind, first, samples);
        } else if t.down.len() == 1 {
            self.first_down(&mut t, kind, first, &samples, &mut out);
        } else {
            self.another_down(&mut t, kind, first.time_ms, &mut out);
        }
        out
    }

    /// Pointers down move together, as one input event has them: the view goes by all of them at
    /// once, never by a spread that only some of them have got to.
    pub fn touch_moves(&self, moves: Vec<TouchMove>) -> Vec<TouchCommand> {
        let mut out = Vec::new();
        // the last that counts: a resting finger the pen put aside moves in the event too
        let last = {
            let t = self.touches();
            moves.iter().rposition(|m| t.down.iter().any(|d| d.id == m.id)).unwrap_or(0)
        };
        for (i, step) in moves.into_iter().enumerate() {
            self.touches().more_to_come = i < last;
            out.extend(self.touch_move(step.id, step.samples));
        }
        self.touches().more_to_come = false;
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
            Mode::Held if id == t.stroke_pointer => {
                if let Some(held) = t.held.as_mut() {
                    held.moves.extend(samples);
                }
                if dist(at, t.down_at) > PANEL_SLOP * t.scale {
                    self.held_goes_on(t, true, &mut out);
                }
            }
            Mode::Puck if id == t.stroke_pointer => {
                let (gx, gy) = t.puck_grab;
                drop(t);
                self.adjustment_clone_move(at.0 - gx, at.1 - gy);
                out.push(TouchCommand::AdjustChanged);
            }
            Mode::Pick => out.push(TouchCommand::Pick { x: at.0, y: at.1 }),
            Mode::Companion if id == t.stroke_pointer => {
                let last = std::mem::replace(&mut t.companion_last, at);
                let (dx, dy) = (at.0 - last.0, at.1 - last.1);
                let (from, zoom, scale) = (t.down_at, t.companion == Some(CompanionMode::Zoom), t.scale);
                drop(t);
                if zoom {
                    let k = (-dy / (COMPANION_ZOOM_DISTANCE * scale)).exp();
                    self.view_gesture(from.0, from.1, 0.0, 0.0, k, 0.0);
                } else {
                    self.view_gesture(at.0, at.1, dx, dy, 1.0, 0.0);
                }
                out.push(TouchCommand::ViewMoved);
            }
            Mode::Hover => {
                if let (Some(a), Some(b)) = (t.down.first(), t.down.get(1)) {
                    // pinched: the size grows and shrinks with how far apart they are (the first
                    // finger followed, so it drags the opacity on from where it is once alone)
                    let spread = dist(a.at, b.at);
                    if id == t.stroke_pointer {
                        t.hover_x = at.0;
                    }
                    let last = std::mem::replace(&mut t.hover_spread, spread);
                    if last > 1.0 && spread > 1.0 {
                        drop(t);
                        let mut st = self.shared.lock();
                        let sliders = st.sliders_mut();
                        sliders.size = (sliders.size * spread / last).clamp(0.0, 1.0);
                        out.push(TouchCommand::BrushChanged);
                    }
                } else if id == t.stroke_pointer {
                    // across the window, from none to full opacity
                    let dx = at.0 - std::mem::replace(&mut t.hover_x, at.0);
                    drop(t);
                    let mut st = self.shared.lock();
                    let width = st.window_size.0.max(1) as f32;
                    let sliders = st.sliders_mut();
                    sliders.opacity = (sliders.opacity + dx / width).clamp(0.0, 1.0);
                    out.push(TouchCommand::BrushChanged);
                }
            }
            Mode::Picture if id == t.stroke_pointer => {
                let last = std::mem::replace(&mut t.turn_last, at);
                drop(t);
                self.model_picture_move(at.0 - last.0, at.1 - last.1, 1.0, 0.0);
            }
            Mode::Turn if id == t.stroke_pointer => {
                let last = std::mem::replace(&mut t.turn_last, at);
                if dist(at, t.down_at) > TAP_SLOP * t.scale {
                    t.hold_due = None;
                }
                drop(t);
                self.turn_model(at.0 - last.0, at.1 - last.1);
            }
            Mode::Recolor if id == t.stroke_pointer => {
                // past a tap's slop, the reticle follows the touch
                if !t.recolor_dragged && dist(at, t.down_at) > TAP_SLOP * t.scale {
                    t.recolor_dragged = true;
                }
                if t.recolor_dragged {
                    // as far from where it was as the touch went from where it came down
                    let (from, down) = (t.recolor_from, t.down_at);
                    out.push(TouchCommand::RecolorMoved { x: from.0 + at.0 - down.0, y: from.1 + at.1 - down.1 });
                }
            }
            Mode::Opacity if id == t.stroke_pointer => {
                // the opacity follows the drag across, faster the faster it goes
                let (last_x, last_ms) = t.pour_last;
                t.pour_last = (at.0, last.time_ms);
                let step = drag_step((at.0 - last_x) / t.scale, last.time_ms - last_ms);
                if let Some((layer, opacity)) = t.layer_opacity.filter(|_| step != 0.0) {
                    let opacity = (opacity + step).clamp(0.0, 1.0);
                    t.layer_opacity = Some((layer, opacity));
                    t.opacity_moved = true;
                    drop(t);
                    self.set_layer_opacity(layer, opacity, false);
                }
            }
            Mode::Select if id == t.stroke_pointer => {
                drop(t);
                self.select_step(at, last.time_ms, false, &mut out);
            }
            Mode::Adjust if id == t.stroke_pointer => {
                if dist(at, t.down_at) > TAP_SLOP * t.scale {
                    t.adjust_moved = true;
                }
                // a drag on perspective blur's puck moves it or turns its arrow, once past the slop
                if let Some(drag) = t.puck_drag {
                    let begins = !t.puck_dragging && dist(at, t.down_at) > PUCK_SLOP * t.scale;
                    if begins {
                        t.puck_dragging = true;
                        t.adjust_moved = true;
                    }
                    let dragging = t.puck_dragging;
                    drop(t);
                    if begins {
                        self.perspective_grabbed();
                    }
                    if dragging {
                        match drag {
                            PuckDrag::Move(gx, gy) => self.perspective_move(at.0 - gx, at.1 - gy),
                            PuckDrag::Turn => self.perspective_turn(at.0, at.1),
                        }
                        out.push(TouchCommand::AdjustChanged);
                    }
                    return out;
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
                    // the brush liquifies along the drag
                    let mut t = self.touches();
                    if dist(at, t.liquify_rest) > LIQUIFY_HELD_SLOP * t.scale {
                        t.liquify_rest = at;
                        t.liquify_rest_ms = last.time_ms;
                        t.liquify_held = 0.0;
                    }
                    let pen = t.liquify_pen;
                    let points = liquify_points(pen, &samples, t.liquify_ms);
                    t.liquify_ms = last.time_ms;
                    drop(t);
                    self.adjustment_liquify_stroke(points, false, pen);
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
                // the lone finger set to move the canvas moves it, from where it went down
                if let Some(last) = t.pan_last.filter(|_| id == t.stroke_pointer && t.down.len() == 1)
                    && (t.panned || dist(at, t.down_at) > slop)
                {
                    t.panned = true;
                    t.pan_last = Some(at);
                    drop(t);
                    self.view_gesture(at.0, at.1, at.0 - last.0, at.1 - last.1, 1.0, 0.0);
                    out.push(TouchCommand::ViewMoved);
                }
            }
            Mode::Gesture if t.more_to_come => {}
            Mode::Gesture => {
                Self::note_fingers_moved(&mut t);
                if t.down.len() >= 3 {
                    Self::scrub(&mut t, &mut out);
                } else if t.box_pinch.is_some() {
                    self.pinch_box(&mut t, &mut out);
                } else if self.move_gesture(&mut t, last.time_ms) {
                    out.push(TouchCommand::ViewMoved);
                }
            }
            Mode::Shape(i) => {
                let began = !t.shape_dragged && dist(at, t.down_at) > NODE_SLOP * t.scale;
                if began {
                    // from here on, the node's way from the finger
                    t.shape_dragged = true;
                    t.shape_last = (t.shape_last.0 - at.0, t.shape_last.1 - at.1);
                }
                if t.shape_dragged {
                    let grip = t.shape_last;
                    drop(t);
                    if began {
                        self.quickshape_step();
                    }
                    self.quickshape_move_handle(i, at.0 + grip.0, at.1 + grip.1);
                    out.push(TouchCommand::ShapeChanged);
                }
            }
            Mode::ShapePin | Mode::ShapeMove if id == t.stroke_pointer => {
                // only a drag past a tap's slop moves it, then from where it began
                let began = !t.shape_dragged && dist(at, t.down_at) > slop;
                t.shape_dragged |= began;
                if t.shape_dragged {
                    let (pin, from) = (t.mode == Mode::ShapePin, t.shape_last);
                    t.shape_last = at;
                    drop(t);
                    if began {
                        self.quickshape_step();
                    }
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

    /// The platform took back one pointer (it was a hand resting on the screen): what it alone
    /// was doing is taken back, and the touch it was part of is no tap.
    pub fn touch_cancel_one(&self, id: u64) -> Vec<TouchCommand> {
        self.lifted(id, Vec::new(), true)
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
        // a finger waiting for a panel held still: the hold goes on below, the panel staying
        if t.mode == Mode::Held && t.hold_due.is_some_and(|due| due <= now_ms) {
            self.held_goes_on(t, false, &mut out);
            t = self.touches();
        }
        // liquify held still works on as the time passes, and what it set going goes on
        if t.liquify_due.is_some_and(|due| due <= now_ms) {
            t.liquify_due = None;
            let passed = ((now_ms - t.liquify_tick_ms) / 1000.0) as f32;
            t.liquify_tick_ms = now_ms;
            let held = (t.mode == Mode::Adjust).then(|| {
                let to = ((now_ms - t.liquify_rest_ms - LIQUIFY_HELD_WAIT_MS) / 1000.0).max(0.0) as f32;
                (t.liquify_rest, std::mem::replace(&mut t.liquify_held, to), to)
            });
            drop(t);
            let worked =
                held.is_some_and(|(at, from, to)| to > from && self.adjustment_liquify_held(at.0, at.1, from, to));
            let going = self.adjustment_liquify_tick(passed);
            t = self.touches();
            if going || held.is_some() {
                t.liquify_due = Some(now_ms + LIQUIFY_HELD_STEP_MS);
            }
            if worked || going {
                out.push(TouchCommand::AdjustChanged);
            }
        }
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
            if matches!(t.mode, Mode::Finger | Mode::Idle | Mode::Turn)
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
                    self.put_tool(before);
                }
                Self::act(purpose, at, &mut out);
                t = self.touches();
            }
        }
        if t.rapid_due.is_some_and(|due| due <= now_ms) {
            t.rapid_due = None;
            // two fingers held still keep undoing, three redoing; held so, they are no tap
            let still = t.tap_moved < FINGERS_TAP_SLOP * t.scale;
            if t.mode == Mode::Gesture && still && t.down.len() == t.tap_pointers && self.undo_taps_allowed(&t) {
                match t.down.len() {
                    2 => out.push(TouchCommand::Undo),
                    3 => out.push(TouchCommand::Redo),
                    _ => return out,
                }
                t.tap_done = true;
                t.rapid_due = Some(now_ms + RAPID_REPEAT_MS);
            }
        }
        out
    }
}

impl CanvasSession {
    /// What a drag from window point `at` does to perspective blur's puck, if it takes it at all
    /// (`scale`: window pixels a point).
    fn perspective_drag_at(&self, at: (f32, f32), scale: f32) -> Option<PuckDrag> {
        let (kind, puck, arrow) = self.perspective_puck_at()?;
        let (dx, dy) = (at.0 - puck.0, at.1 - puck.1);
        let far = dx.hypot(dy) / scale.max(1e-6);
        let moves = Some(PuckDrag::Move(dx, dy));
        match kind {
            PerspectiveKind::Positional => moves.filter(|_| far < POSITIONAL_GRAB),
            PerspectiveKind::Directional if far <= DIRECTIONAL_INNER => moves,
            // the arrow's side (within a right angle of it) turns it
            PerspectiveKind::Directional if far <= DIRECTIONAL_REACH => {
                if dx * arrow.cos() + dy * arrow.sin() > 0.0 {
                    Some(PuckDrag::Turn)
                } else {
                    moves
                }
            }
            PerspectiveKind::Directional => None,
        }
    }

    /// Layer Opacity's touch lifted (or another came down): what it dragged is one undo step.
    fn opacity_lifted(&self, t: &mut Touches) {
        if let Some((layer, opacity)) = t.layer_opacity.filter(|_| std::mem::take(&mut t.opacity_moved)) {
            self.set_layer_opacity(layer, opacity, true);
        }
    }

    /// The first pointer down with a panel open: it waits to be a tap or a drag.
    fn wait_for_panel(t: &mut Touches, kind: PointerKind, first: PenSample, samples: Vec<PenSample>) {
        t.down_at = (first.x, first.y);
        t.pan_last = None;
        t.tap_start = first.time_ms;
        t.tap_lifted = None;
        t.tap_pointers = 1;
        t.tap_moved = 0.0;
        t.tap_done = false;
        t.stroke_pointer = t.down.first().map_or(0, |d| d.id);
        t.held = Some(Held { kind, first: samples, moves: Vec::new() });
        t.mode = Mode::Held;
        if !is_pen(kind) && t.settings.hold.is_some() {
            t.hold_due = Some(first.time_ms + t.settings.hold_ms);
        }
    }

    /// The touch waiting for a panel is no tap: it does what it would have done from where it came
    /// down, then its input since; a drag first fades the panel (`fades`), a hold leaves it up.
    fn held_goes_on(&self, mut t: std::sync::MutexGuard<'_, Touches>, fades: bool, out: &mut Vec<TouchCommand>) {
        let Some(held) = t.held.take() else { return };
        let Some(first) = last_of(&held.first).copied() else { return };
        let id = t.stroke_pointer;
        t.mode = Mode::None;
        t.hold_due = None;
        if fades {
            out.push(TouchCommand::PanelFades);
        }
        self.first_down(&mut t, held.kind, first, &held.first, out);
        drop(t);
        if !held.moves.is_empty() {
            out.extend(self.touch_move(id, held.moves));
        }
    }

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
        t.pan_last = None;
        t.tap_start = now;
        t.tap_lifted = None;
        t.tap_pointers = 1;
        t.tap_moved = 0.0;
        t.tap_done = false;
        let id = t.down.first().map_or(0, |d| d.id);
        // the modify button was tapped (or the pen is squeezed for the eyedropper, and touches):
        // this touch picks a colour
        if t.pick_next || (is_pen(kind) && t.pen.squeeze_takes_touch()) {
            t.mode = Mode::Pick;
            out.push(TouchCommand::Pick { x: at.0, y: at.1 });
            return;
        }
        // Hover: with the pen held over the canvas, a finger drags the opacity and a pinch the size
        let hovering = t.pen_hovering.is_some_and(|at| at.elapsed().as_secs_f32() < PEN_HOVER_STALE_S);
        if kind == PointerKind::Finger && hovering && t.settings.hover_touch && self.shared.lock().shape.is_none() {
            t.stroke_pointer = id;
            t.hover_x = at.0;
            t.hover_spread = 0.0;
            t.mode = Mode::Hover;
            return;
        }
        if t.layer_opacity.is_some() {
            // Layer Opacity: the touch drags the layer's opacity
            t.stroke_pointer = id;
            t.pour_last = (at.0, now);
            t.opacity_moved = false;
            t.mode = Mode::Opacity;
            return;
        }
        if self.shared.lock().recolor.is_some() {
            // Recolor: the touch moves its reticle, or a tap starts it again
            t.stroke_pointer = id;
            t.recolor_dragged = false;
            t.recolor_from = self.recolor_point().and_then(|p| Some((*p.first()?, *p.get(1)?))).unwrap_or(at);
            t.mode = Mode::Recolor;
            return;
        }
        // a snapped shape's handle is dragged; while it is edited, a drag on its outline turns and
        // resizes it, one elsewhere moves it (a tap ends the editing); anything else keeps the
        // shape and draws on
        let grab = QUICKSHAPE_GRAB * t.handle_scale;
        if t.shape_editing
            && let Some(info) = self.quickshape_info()
        {
            let handle = info
                .handles
                .as_chunks::<2>()
                .0
                .iter()
                .enumerate()
                .map(|(i, &[x, y])| (i, (x, y), dist((x, y), at)))
                .filter(|(.., d)| *d <= grab)
                .min_by(|a, b| a.2.total_cmp(&b.2));
            if let Some((i, node, _)) = handle {
                t.mode = Mode::Shape(i as u32);
                t.shape_dragged = false;
                t.shape_last = node;
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
                && dist(puck, at) <= (CLONE_PUCK_RADIUS + CLONE_PUCK_SLOP) * t.handle_scale
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
            // perspective blur: a touch on its puck moves the puck or turns its arrow
            t.puck_drag = self.perspective_drag_at(at, t.handle_scale);
            t.puck_dragging = false;
            if open == Adjustment::Liquify {
                t.liquify_ms = now;
                t.liquify_tick_ms = now;
                t.liquify_pen = is_pen(kind);
                t.liquify_rest = at;
                t.liquify_rest_ms = now;
                t.liquify_held = 0.0;
                t.liquify_due = Some(now + LIQUIFY_HELD_STEP_MS);
                self.adjustment_liquify_stroke(liquify_points(t.liquify_pen, samples, now), true, t.liquify_pen);
                out.push(TouchCommand::AdjustChanged);
            }
            return;
        }
        // a picture being placed on a model is moved by what lands on it (a pen or mouse, anywhere);
        // otherwise a finger turns the model under it
        let placing = self.shared.lock().scene_picture_under(at);
        if placing == Some(true) || (placing.is_some() && kind != PointerKind::Finger) {
            t.stroke_pointer = id;
            t.turn_last = at;
            t.mode = Mode::Picture;
            return;
        }
        // on a shown model a finger still turns it while a selection is drawn with the pen
        if t.selecting.is_some() && !self.finger_turns_model(kind, t) {
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
        // on a shown model a finger turns it, unless set to paint; the pen paints. Held still, it
        // does what a hold does anywhere
        let on_model = kind == PointerKind::Finger && self.shared.lock().scene_shown();
        if on_model && !t.settings.model_finger_paint {
            t.stroke_pointer = id;
            t.turn_last = at;
            t.mode = Mode::Turn;
            if t.settings.hold.is_some() {
                t.hold_due = Some(now + t.settings.hold_ms);
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
        // a finger set not to paint (a mouse, a desktop's drawing tool, paints as the pen does)
        if kind == PointerKind::Finger && does.is_none() && !t.settings.finger_paint && !on_model {
            t.mode = Mode::Idle;
            // set to move the canvas: it does once it moves past a tap's slop
            if t.settings.one_finger_pan {
                t.stroke_pointer = id;
                t.pan_last = Some(at);
                t.panned = false;
            }
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

    /// Whether a touch of `kind` on the screen turns a shown model.
    fn finger_turns_model(&self, kind: PointerKind, t: &Touches) -> bool {
        kind == PointerKind::Finger && !t.settings.model_finger_paint && self.shared.lock().scene_shown()
    }

    fn another_down(&self, t: &mut Touches, kind: PointerKind, now: f64, out: &mut Vec<TouchCommand>) {
        // Hover's second finger: the two pinch the brush's size from how far apart they now are
        if t.mode == Mode::Hover {
            if let (Some(a), Some(b)) = (t.down.first(), t.down.get(1)) {
                t.hover_spread = dist(a.at, b.at);
            }
            return;
        }
        // fingers landing after the others tapped or repeated do nothing until all are up
        if t.tap_done {
            t.mode = Mode::Idle;
            t.rapid_due = None;
            return;
        }
        t.hold_due = None;
        t.quickshape_due = None;
        t.pan_last = None;
        // a finger down while the stroke is held as a snapped shape snaps it, until lifted
        let snapped = self.shared.lock().shape.is_some();
        if matches!(t.mode, Mode::Pen | Mode::Finger | Mode::ShapePin) && snapped {
            if kind == PointerKind::Finger {
                self.quickshape_finger(true);
                out.push(TouchCommand::ShapeChanged);
            }
            return;
        }
        // (a pen waiting for a panel is a pen: fingers landing leave it to draw)
        let pen_held = t.held.as_ref().is_some_and(|h| is_pen(h.kind));
        if matches!(t.mode, Mode::Pen | Mode::Pick) || pen_held {
            return;
        }
        t.held = None;
        // a second finger takes back the fill the first one made, and a transform's drag (a move
        // of what the box holds is kept when the fingers go on to take it); it keeps the opacity
        // dragged so far
        if t.mode == Mode::Pour {
            self.pour_end(false, None);
        }
        if t.mode == Mode::Opacity {
            self.opacity_lifted(t);
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
        self.second_finger_on_box(t, out);
        out.push(TouchCommand::CursorGone);
        if t.mode == Mode::Finger {
            self.stroke_cancel();
        }
        self.end_stroke_tools(t);
        if matches!(
            t.mode,
            Mode::Finger
                | Mode::Idle
                | Mode::Pour
                | Mode::Transform
                | Mode::Select
                | Mode::Recolor
                | Mode::Turn
                | Mode::Picture
                | Mode::Held
        ) {
            t.gesture_scale = 1.0;
            t.pinch.clear();
            t.pinch.push((now, 1.0));
        }
        if t.mode != Mode::Gesture {
            t.view_moving = false;
        }
        t.mode = Mode::Gesture;
        // a tap's time counts from when its last finger landed; those down already have gone as
        // far as they have
        if t.down.len() > t.tap_pointers {
            t.tap_start = now;
            t.tap_pointers = t.down.len();
        }
        Self::note_fingers_moved(t);
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
                        self.put_tool(before);
                    }
                    out.push(TouchCommand::StrokeEnded);
                }
                _ => {}
            }
            return out;
        }
        if !t.down.is_empty() {
            // one pointer taken back: a stroke of its goes, and what is left is no tap
            if cancelled {
                t.tap_moved = f32::INFINITY;
                if t.mode == Mode::Finger && id == t.stroke_pointer {
                    t.mode = Mode::Idle;
                    t.hold_due = None;
                    t.quickshape_due = None;
                    self.end_stroke_tools(&mut t);
                    drop(t);
                    self.stroke_cancel();
                    out.push(TouchCommand::CursorGone);
                    return out;
                }
            }
            if t.mode == Mode::Gesture {
                let first = t.tap_lifted.is_none();
                t.tap_lifted.get_or_insert(now);
                let parted = t.gesture_pair.is_some_and(|(a, b)| a == id || b == id);
                let pinched = parted && self.end_box_pinch(&mut t, cancelled, &mut out);
                Self::follow_fingers(&mut t);
                // two fingers parting as they close fast fit a canvas pinched smaller than it
                // fits, settling into place (a tap's fingers aren't a pinch)
                let tap = Self::fingers_tapped(&t, now);
                if parted && !pinched && !cancelled && !tap && t.tap_pointers == 2 && t.gesture_pair.is_none() {
                    let flicked = Self::pinch_speed(&t, now) < QUICK_PINCH_SPEED;
                    if flicked && self.pinched_smaller_than_fit() {
                        out.push(TouchCommand::Fit);
                    }
                }
                // the first lift decides a tap, and it does what it does there: the fingers left
                // down, wherever they go, do nothing more
                if first && tap && !cancelled {
                    self.fingers_did_tap(&mut t, &mut out);
                }
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
            Mode::Pick => {
                t.pick_next = false;
                out.push(if cancelled { TouchCommand::PickCancel } else { TouchCommand::PickDone });
            }
            Mode::Adjust => {
                let at = last_of(&samples).map_or(t.down_at, |s| (s.x, s.y));
                let liquifying = self.shared.lock().adjust.open().is_some_and(|o| o.0 == Adjustment::Liquify);
                if liquifying {
                    // still moving when it lifts, liquify carries on
                    drop(t);
                    let going = !cancelled && self.adjustment_liquify_lift();
                    self.touches().liquify_due = going.then_some(now + LIQUIFY_HELD_STEP_MS);
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
            Mode::Opacity => self.opacity_lifted(&mut t),
            Mode::Held => {
                t.held = None;
                if !cancelled {
                    out.push(TouchCommand::PanelTapped);
                }
            }
            // a tap starts the reticle again where it lands
            Mode::Recolor if !t.recolor_dragged && !cancelled => {
                let at = last_of(&samples).map_or(t.down_at, |s| (s.x, s.y));
                out.push(TouchCommand::RecolorTapped { x: at.0, y: at.1 });
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
            // a tap on a handle rather than a drag of it: a curve's anchor steps to its next state
            Mode::Shape(i) => {
                let tapped = !t.shape_dragged && now - t.tap_start <= TAP_MS;
                drop(t);
                if tapped {
                    self.quickshape_cycle_anchor(i);
                    out.push(TouchCommand::ShapeChanged);
                }
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
            self.put_tool(before);
        }
        if std::mem::take(&mut t.assisted) {
            self.set_stroke_assist(false);
        }
    }

    /// Paints with `tool` for a stroke; returns the tool chosen before, to put back after (as for
    /// the stroke only, neither counts as the tool chosen last).
    fn swap_tool(&self, tool: ToolKind) -> ToolKind {
        let before = self.shared.lock().tool;
        self.put_tool(tool);
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
        let (shown, view, density, warping) = {
            let st = self.shared.lock();
            let warping = st.transform_mode == procraft_engine::transform::TransformMode::Warp;
            (st.transform_box(), st.view.map(|v| v.to_screen()), st.interface_scale(), warping)
        };
        let (Some(b), Some(m)) = (shown, view) else { return crate::TransformHandle::Move };
        let view = vec![m.a, m.b, m.c, m.d, m.e, m.f];
        // a warp's mesh stays with the content after it, but only a warp shows its points: the
        // other kinds take the box's nodes
        if warping && !b.mesh.is_empty() {
            return crate::mesh_handle_at(b.mesh, view, at.0, at.1, density, how.advanced_mesh);
        }
        crate::screen_box(b.corners, view, density)
            .map_or(crate::TransformHandle::Move, |s| crate::handle_at(s, at.0, at.1, density))
    }

    /// Typing a tapped node's numbers beside it: where the node is, and which side of the box.
    fn numeric_at(&self, handle: crate::TransformHandle) -> Option<TouchCommand> {
        let (b, m, density) = {
            let st = self.shared.lock();
            (st.transform_box()?, st.view?.to_screen(), st.interface_scale())
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

    /// Keeps how far any finger down has gone from where it landed, each by itself: fingers
    /// pinched or turned a little are still a tap, and a finger's wobble, which a touch screen
    /// reports move by move, takes it nowhere.
    fn note_fingers_moved(t: &mut Touches) {
        t.tap_moved = t.down.iter().map(|d| dist(d.at, d.from)).fold(t.tap_moved, f32::max);
    }

    /// Whether fingers lifting at `now` (the first of them) were a tap: down together for no
    /// longer than a tap takes, none gone away, and not held until they repeated. It goes by the
    /// first lift: a finger left down after it counts for nothing, as measured.
    fn fingers_tapped(t: &Touches, now: f64) -> bool {
        let lifted = t.tap_lifted.unwrap_or(now);
        lifted - t.tap_start <= TAP_MS && t.tap_moved < FINGERS_TAP_SLOP * t.scale && !t.tap_done
    }

    /// The fingers were a tap: two undo, three redo, four do what Four-finger tap is set to. Those
    /// left down do nothing more until all are up.
    fn fingers_did_tap(&self, t: &mut Touches, out: &mut Vec<TouchCommand>) {
        let undo_taps = self.undo_taps_allowed(t);
        match t.tap_pointers {
            2 if undo_taps => out.push(TouchCommand::Undo),
            3 if undo_taps => out.push(TouchCommand::Redo),
            4 => {
                if let Some(a) = t.settings.four_tap {
                    // in the middle of the screen, not under the fingers
                    Self::act(a, t.down_at, out);
                }
            }
            _ => {}
        }
        // two on a transform's box let go of what it holds
        self.end_box_pinch(t, false, out);
        t.tap_done = true;
        t.mode = Mode::Idle;
        t.rapid_due = None;
    }

    /// Whether two and three fingers undo and redo: unless turned off, and even then while a
    /// transform, the selection tool or liquify is open.
    fn undo_taps_allowed(&self, t: &Touches) -> bool {
        t.settings.undo_taps
            || t.transforming.is_some()
            || t.selecting.is_some()
            || self.shared.lock().adjust.open().is_some_and(|(open, _)| open == Adjustment::Liquify)
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

    /// How fast two fingers' spread changed (a part of where it started, a second) over the
    /// [`QUICK_PINCH_WINDOW_MS`] before `now`: nothing heard meanwhile counts as still.
    fn pinch_speed(t: &Touches, now: f64) -> f32 {
        let Some(&(_, scale)) = t.pinch.last() else { return 0.0 };
        let from = now - QUICK_PINCH_WINDOW_MS;
        let before = t.pinch.iter().rev().find(|(ms, _)| *ms <= from).or(t.pinch.first());
        match before {
            Some(&(ms, was)) if now - ms > 0.5 => (scale - was) / ((now - ms) / 1000.0) as f32,
            _ => 0.0,
        }
    }

    /// The canvas as pinched is smaller on the screen than it is fitted.
    fn pinched_smaller_than_fit(&self) -> bool {
        let st = self.shared.lock();
        st.view.is_some_and(|v| v.zoom() < st.fitted_view().zoom())
    }

    /// One step of two fingers at `ms`: the view follows them. True if there were two to follow.
    fn move_gesture(&self, t: &mut Touches, ms: f64) -> bool {
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
        t.gesture_scale *= scale;
        // the spread's recent values, one a moment, and the last from before the speed's window
        let spread = t.gesture_scale;
        match t.pinch.last_mut() {
            Some(last) if last.0 >= ms => last.1 = spread,
            _ => t.pinch.push((ms, spread)),
        }
        let from = ms - QUICK_PINCH_WINDOW_MS;
        let old = t.pinch.iter().skip(1).take_while(|(at, _)| *at <= from).count();
        t.pinch.drain(..old);
        t.last = (x, y, d, angle);
        let gone: f32 = t.down.iter().map(|d| dist(d.at, d.from)).sum();
        t.view_moving |= gone >= VIEW_SLOP * t.scale;
        // from where they are when it starts, so it doesn't jump
        if t.view_moving {
            self.view_gesture(lx, ly, x - lx, y - ly, scale, turn);
        }
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
    use procraft_engine::geom::Vec2;

    fn at(x: f32, y: f32, ms: f64) -> Vec<PenSample> {
        vec![PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: ms, roll: 0.0 }]
    }

    fn session() -> std::sync::Arc<CanvasSession> {
        CanvasSession::new(64, 64)
    }

    /// What the touches cost a pen stroke's events, the router and the stroke's own work (no GPU):
    /// run on a device with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn touch_cost() {
        let s = CanvasSession::new(2048, 2048);
        let sample = |i: usize, ms: f64| {
            let u = i as f32 * 0.5;
            PenSample {
                x: 100.0 + u % 1800.0,
                y: 600.0 + (u * 0.02).sin() * 400.0,
                pressure: 0.3 + 0.6 * (u * 0.01).sin().abs(),
                tilt: 0.4,
                orientation: 1.0,
                time_ms: ms,
                roll: 0.0,
            }
        };
        s.touch_down(1, PointerKind::Stylus, vec![sample(0, 0.0)]);
        // a 240 Hz pen read at 120 frames a second: two samples an event
        let mut took: Vec<f64> = (1..=4000)
            .map(|k| {
                let samples = vec![sample(2 * k - 1, k as f64 * 8.33 - 4.0), sample(2 * k, k as f64 * 8.33)];
                let start = std::time::Instant::now();
                s.touch_move(1, samples);
                start.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        let start = std::time::Instant::now();
        s.touch_up(1, vec![sample(8001, 4000.0 * 8.33 + 4.0)]);
        let up = start.elapsed().as_secs_f64() * 1000.0;
        let (first, last) = (took[..400].iter().sum::<f64>() / 400.0, took[3600..].iter().sum::<f64>() / 400.0);
        took.sort_by(f64::total_cmp);
        let at = |q: f64| took[((took.len() - 1) as f64 * q) as usize];
        println!(
            "pen event ms: median {:.4}, 90th {:.4}, 99th {:.4}, most {:.4}; first 400 {first:.4}, last 400 {last:.4}; up {up:.4}",
            at(0.5),
            at(0.9),
            at(0.99),
            at(1.0)
        );
    }

    #[test]
    fn two_fingers_tapped_undo_and_three_redo() {
        let s = session();
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(300.0, 100.0, 10.0));
        // as the first lifts
        let out = s.touch_up(2, at(300.0, 100.0, 100.0));
        assert!(out.contains(&TouchCommand::Undo), "{out:?}");
        assert!(!s.touch_up(1, at(100.0, 100.0, 110.0)).contains(&TouchCommand::Undo));

        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 1000.0));
        s.touch_down(2, PointerKind::Finger, at(300.0, 100.0, 1005.0));
        s.touch_down(3, PointerKind::Finger, at(500.0, 100.0, 1010.0));
        let mut out = s.touch_up(3, at(500.0, 100.0, 1100.0));
        assert!(out.contains(&TouchCommand::Redo), "{out:?}");
        out.extend(s.touch_up(2, at(300.0, 100.0, 1100.0)));
        out.extend(s.touch_up(1, at(100.0, 100.0, 1110.0)));
        assert!(redid_once(&out), "{out:?}");
    }

    #[test]
    fn four_fingers_tapped_do_what_four_finger_tap_is_set_to() {
        let s = session();
        let mut out = Vec::new();
        for i in 0..4u8 {
            out.extend(s.touch_down(
                u64::from(i),
                PointerKind::Finger,
                at(100.0 + 80.0 * f32::from(i), 300.0, 10.0 * f64::from(i)),
            ));
        }
        for i in 0..4u8 {
            out.extend(s.touch_up(u64::from(i), at(100.0 + 80.0 * f32::from(i), 300.0, 120.0 + 10.0 * f64::from(i))));
        }
        assert_eq!(out.iter().filter(|c| **c == TouchCommand::ToggleBars).count(), 1, "{out:?}");
        assert!(!out.iter().any(|c| matches!(c, TouchCommand::Undo | TouchCommand::Redo)), "{out:?}");
    }

    #[test]
    fn the_first_lift_decides_and_the_fingers_left_do_nothing_more() {
        let s = shown(1.0);
        // the first lifts in time: the redo is done there, though the others then wander off,
        // and a fourth landing meanwhile changes nothing
        s.touch_down(1, PointerKind::Finger, at(100.0, 300.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(200.0, 300.0, 20.0));
        s.touch_down(3, PointerKind::Finger, at(300.0, 300.0, 40.0));
        let view = s.view_matrix();
        let mut out = s.touch_up(1, at(100.0, 300.0, 140.0));
        assert!(redid_once(&out), "{out:?}");
        for k in 1..=10 {
            let d = 12.0 * k as f32;
            out.extend(s.touch_moves(vec![
                TouchMove { id: 2, samples: at(200.0 - d, 300.0, 140.0 + 8.0 * f64::from(k)) },
                TouchMove { id: 3, samples: at(300.0 + d, 300.0 + d, 140.0 + 8.0 * f64::from(k)) },
            ]));
        }
        out.extend(s.touch_down(4, PointerKind::Finger, at(500.0, 300.0, 240.0)));
        for id in [2, 4, 3] {
            out.extend(s.touch_up(id, at(500.0, 300.0, 300.0)));
        }
        assert!(redid_once(&out), "{out:?}");
        assert!(!out.contains(&TouchCommand::ViewMoved), "{out:?}");
        assert_eq!(s.view_matrix(), view);
        // the next touch is a touch again
        s.touch_down(1, PointerKind::Finger, at(100.0, 300.0, 1000.0));
        s.touch_down(2, PointerKind::Finger, at(200.0, 300.0, 1010.0));
        assert!(s.touch_up(1, at(100.0, 300.0, 1100.0)).contains(&TouchCommand::Undo));
    }

    #[test]
    fn fingers_held_until_they_repeat_are_no_tap_as_they_lift() {
        let s = session();
        // repeating from 100 ms on, then lifted well within a tap's time: the repeat's undo only
        s.set_touch_settings(TouchSettings { rapid_undo_ms: 100.0, ..TouchSettings::default() });
        let mut out = s.touch_down(1, PointerKind::Finger, at(100.0, 300.0, 0.0));
        out.extend(s.touch_down(2, PointerKind::Finger, at(200.0, 300.0, 10.0)));
        out.extend(s.touch_tick(110.0));
        out.extend(s.touch_up(1, at(100.0, 300.0, 150.0)));
        out.extend(s.touch_up(2, at(200.0, 300.0, 160.0)));
        assert_eq!(out.iter().filter(|c| **c == TouchCommand::Undo).count(), 1, "{out:?}");
    }

    #[test]
    fn with_undo_taps_off_they_still_undo_in_a_transform_or_the_selection_tool() {
        let s = session();
        s.set_touch_settings(TouchSettings { undo_taps: false, ..TouchSettings::default() });
        let tap = |s: &CanvasSession, ms: f64| {
            let mut out = s.touch_down(1, PointerKind::Finger, at(100.0, 300.0, ms));
            out.extend(s.touch_down(2, PointerKind::Finger, at(200.0, 300.0, ms + 10.0)));
            out.extend(s.touch_up(1, at(100.0, 300.0, ms + 100.0)));
            out.extend(s.touch_up(2, at(200.0, 300.0, ms + 110.0)));
            out.contains(&TouchCommand::Undo)
        };
        assert!(!tap(&s, 0.0));
        let how = SelectTouch { kind: SelectKind::Freehand, join: crate::SelectionJoin::Add, threshold: 0.5 };
        s.set_touch_selecting(Some(how));
        assert!(tap(&s, 1000.0));
        s.set_touch_selecting(None);
        s.set_touch_transforming(Some(TransformTouch { advanced_mesh: false }));
        assert!(tap(&s, 2000.0));
        s.set_touch_transforming(None);
        assert!(!tap(&s, 3000.0));
    }

    /// Two fingers tapped on a screen that reports their wobble: each comes down, wanders `wobble`
    /// pixels either way about where it landed at `every_ms`, and lifts. Times: the second lands
    /// at `landed`, the first lifts at `lifted` and the second at `gone`.
    fn wobbling_tap(s: &CanvasSession, wobble: f32, every_ms: f64, landed: f64, lifted: f64, gone: f64) -> bool {
        let (a, b) = ((200.0, 300.0), (260.0, 310.0));
        let shaken = |p: (f32, f32), k: usize| {
            let (u, v) = ((k as f32 * 2.4).sin(), (k as f32 * 3.7).cos());
            (p.0 + wobble * u, p.1 + wobble * v)
        };
        let mut out = s.touch_down(1, PointerKind::Finger, at(a.0, a.1, 0.0));
        let mut ms = every_ms;
        let mut k = 0;
        while ms < gone {
            k += 1;
            if ms >= landed && !s.touches().down.iter().any(|d| d.id == 2) {
                out.extend(s.touch_down(2, PointerKind::Finger, at(b.0, b.1, ms)));
            }
            if ms >= lifted && s.touches().down.iter().any(|d| d.id == 1) {
                out.extend(s.touch_up(1, at(a.0, a.1, ms)));
            }
            let p = shaken(a, k);
            out.extend(s.touch_move(1, at(p.0, p.1, ms)));
            let p = shaken(b, k + 1);
            out.extend(s.touch_move(2, at(p.0, p.1, ms + every_ms / 2.0)));
            ms += every_ms;
        }
        out.extend(s.touch_up(2, at(b.0, b.1, gone)));
        out.contains(&TouchCommand::Undo)
    }

    #[test]
    fn two_fingers_tapped_on_a_screen_that_wobbles_undo() {
        let s = session();
        // a laptop's touch screen: a point a pixel, every wobble of the fingers reported 120 times
        // a second; none has gone more than a pixel or two from where it landed
        s.set_display_scale(1.0);
        for wobble in [0.5, 1.0, 2.0] {
            assert!(wobbling_tap(&s, wobble, 8.0, 30.0, 150.0, 190.0), "{wobble} px");
        }
    }

    #[test]
    fn two_fingers_tapped_loosely_undo_as_long_as_they_were_together_a_tap_s_time() {
        let s = session();
        // down over 80 ms and up over 110: 340 ms in all, 150 of them together
        assert!(wobbling_tap(&s, 0.0, 8.0, 80.0, 230.0, 340.0));
        // together longer than a tap is a hold, not a tap
        assert!(!wobbling_tap(&s, 0.0, 8.0, 30.0, 380.0, 400.0));
        // the first lift decides: one finger left down long after the other is a tap still
        assert!(wobbling_tap(&s, 0.0, 8.0, 30.0, 150.0, 1000.0));
        // nor does it matter how long one was down before the other, as measured
        assert!(wobbling_tap(&s, 0.0, 8.0, 5000.0, 5100.0, 5110.0));
    }

    #[test]
    fn fingers_that_went_somewhere_are_no_tap() {
        let s = session();
        s.set_display_scale(1.0);
        // two fingers slid together past the tap's reach, quickly: the canvas moved, no undo
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(200.0, 100.0, 10.0));
        for k in 1..=5 {
            let d = k as f32 * 5.0;
            s.touch_move(1, at(100.0 + d, 100.0, 10.0 + k as f64 * 8.0));
            s.touch_move(2, at(200.0 + d, 100.0, 14.0 + k as f64 * 8.0));
        }
        let mut out = s.touch_up(1, at(125.0, 100.0, 60.0));
        out.extend(s.touch_up(2, at(225.0, 100.0, 70.0)));
        assert!(!out.contains(&TouchCommand::Undo), "{out:?}");
        // nor fingers gone exactly the tap's reach, where they begin to move the canvas, together
        // or pinched
        for dx in [15.0, -15.0] {
            s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 700.0));
            s.touch_down(2, PointerKind::Finger, at(200.0, 100.0, 710.0));
            s.touch_move(1, at(100.0 + dx, 100.0, 730.0));
            s.touch_move(2, at(200.0 + dx.abs(), 100.0, 734.0));
            let mut out = s.touch_up(1, at(100.0 + dx, 100.0, 760.0));
            out.extend(s.touch_up(2, at(200.0 + dx.abs(), 100.0, 770.0)));
            assert!(!out.contains(&TouchCommand::Undo), "{dx}: {out:?}");
        }
        // nor one that had moved before the other landed: a stroke, taken back by a second finger
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 1000.0));
        s.touch_move(1, at(140.0, 100.0, 1050.0));
        s.touch_down(2, PointerKind::Finger, at(200.0, 100.0, 1060.0));
        let mut out = s.touch_up(2, at(200.0, 100.0, 1100.0));
        out.extend(s.touch_up(1, at(140.0, 100.0, 1110.0)));
        assert!(!out.contains(&TouchCommand::Undo), "{out:?}");
    }

    #[test]
    fn fingers_pinched_or_turned_a_little_are_still_a_tap() {
        let s = session();
        s.set_display_scale(1.0);
        // each short of the tap's reach by itself, though they part by more: a tap, as each
        // finger alone decides
        for (dx, dy) in [(-8.0, 0.0), (0.0, 8.0), (10.0, 0.0)] {
            s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 500.0));
            s.touch_down(2, PointerKind::Finger, at(200.0, 100.0, 510.0));
            s.touch_move(1, at(100.0 + dx, 100.0 + dy, 530.0));
            s.touch_move(2, at(200.0 - dx, 100.0 - dy, 534.0));
            let mut out = s.touch_up(1, at(100.0 + dx, 100.0 + dy, 560.0));
            out.extend(s.touch_up(2, at(200.0 - dx, 100.0 - dy, 570.0)));
            assert!(out.contains(&TouchCommand::Undo), "{dx} {dy}: {out:?}");
        }
        // three, spreading a little each its own way
        let fingers: Vec<Finger> = (0..3)
            .map(|i| Finger { lands: 20.0 * f64::from(i), lifts: 200.0, drift: 9.0, towards: 2.1 * i as f32 })
            .collect();
        assert!(redid_once(&real_tap(&s, &fingers, 1.5, 8.0, true)));
    }

    #[test]
    fn two_fingers_held_on_a_screen_that_wobbles_keep_undoing() {
        let s = session();
        s.set_display_scale(1.0);
        s.touch_down(1, PointerKind::Finger, at(200.0, 300.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(260.0, 310.0, 20.0));
        for k in 0..80 {
            let (u, v) = ((k as f32 * 2.4).sin(), (k as f32 * 3.7).cos());
            s.touch_move(1, at(200.0 + u, 300.0 + v, 20.0 + k as f64 * 8.0));
            s.touch_move(2, at(260.0 + v, 310.0 + u, 24.0 + k as f64 * 8.0));
        }
        let due = s.touch_deadline().unwrap_or(f64::MAX);
        assert!(s.touch_tick(due).contains(&TouchCommand::Undo));
    }

    #[test]
    fn the_pen_landing_on_a_resting_hand_draws_and_the_hand_does_nothing() {
        let s = session();
        // the hand touches first: a finger stroke begins (fingers paint as the pen does)
        s.touch_down(1, PointerKind::Finger, at(100.0, 300.0, 0.0));
        s.touch_move(1, at(104.0, 302.0, 20.0));
        // the pen lands: it draws, the hand's stroke taken back
        let out = s.touch_down(2, PointerKind::Stylus, at(300.0, 100.0, 40.0));
        assert!(out.contains(&TouchCommand::Cursor { x: 300.0, y: 100.0 }), "{out:?}");
        assert!(s.touch_move(1, at(140.0, 320.0, 60.0)).is_empty());
        s.touch_move(2, at(340.0, 120.0, 80.0));
        let out = s.touch_up(2, at(360.0, 130.0, 100.0));
        assert!(out.contains(&TouchCommand::StrokeEnded), "{out:?}");
        assert!(s.touch_up(1, at(140.0, 320.0, 120.0)).is_empty());
        // two fingers resting, then the pen tapped: a dot, no undo for the fingers lifted quickly
        s.touch_down(1, PointerKind::Finger, at(100.0, 300.0, 1000.0));
        s.touch_down(3, PointerKind::Finger, at(160.0, 300.0, 1005.0));
        s.touch_down(2, PointerKind::Stylus, at(300.0, 100.0, 1010.0));
        let out = s.touch_up(2, at(300.0, 100.0, 1040.0));
        assert!(out.contains(&TouchCommand::StrokeEnded), "{out:?}");
        let mut out = s.touch_up(3, at(160.0, 300.0, 1050.0));
        out.extend(s.touch_up(1, at(100.0, 300.0, 1060.0)));
        assert!(!out.contains(&TouchCommand::Undo), "{out:?}");
    }

    /// Whether a touch is painting (no GPU here, so no stroke to look at).
    fn painting(s: &CanvasSession) -> bool {
        matches!(s.touches().mode, Mode::Pen | Mode::Finger)
    }

    #[test]
    fn a_tap_with_a_panel_open_only_closes_it() {
        let s = session();
        s.set_touch_panel_open(true);
        for kind in [PointerKind::Finger, PointerKind::Stylus, PointerKind::Mouse] {
            assert!(s.touch_down(1, kind, at(100.0, 100.0, 0.0)).is_empty());
            assert!(!painting(&s));
            // moving a little is still a tap
            assert!(s.touch_move(1, at(105.0, 100.0, 20.0)).is_empty());
            let out = s.touch_up(1, at(105.0, 100.0, 40.0));
            assert!(out.contains(&TouchCommand::PanelTapped), "{out:?}");
            assert!(!out.contains(&TouchCommand::StrokeEnded), "{out:?}");
            assert!(!painting(&s));
        }
        // the platform calling it off closes nothing
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 100.0));
        assert!(!s.touch_cancel().contains(&TouchCommand::PanelTapped));
    }

    #[test]
    fn a_drag_with_a_panel_open_fades_it_and_paints_from_where_it_came_down() {
        let s = session();
        s.set_touch_panel_open(true);
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        s.touch_move(1, at(115.0, 100.0, 10.0));
        assert!(!painting(&s));
        let out = s.touch_move(1, at(125.0, 100.0, 20.0));
        assert_eq!(out.first(), Some(&TouchCommand::PanelFades), "{out:?}");
        assert!(out.contains(&TouchCommand::Cursor { x: 125.0, y: 100.0 }), "{out:?}");
        assert!(painting(&s));
        // the panel goes only once
        assert!(!s.touch_move(1, at(130.0, 100.0, 30.0)).contains(&TouchCommand::PanelFades));
        let out = s.touch_up(1, at(140.0, 100.0, 40.0));
        assert!(out.contains(&TouchCommand::StrokeEnded), "{out:?}");
        assert!(!out.contains(&TouchCommand::PanelTapped), "{out:?}");
    }

    #[test]
    fn a_finger_held_still_with_a_panel_open_picks_and_leaves_the_panel() {
        let s = session();
        s.set_touch_panel_open(true);
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        assert_eq!(s.touch_deadline(), Some(400.0));
        let out = s.touch_tick(400.0);
        assert!(out.contains(&TouchCommand::Pick { x: 100.0, y: 100.0 }), "{out:?}");
        assert!(!out.contains(&TouchCommand::PanelFades), "{out:?}");
        assert!(!painting(&s));
        let out = s.touch_up(1, at(100.0, 100.0, 500.0));
        assert!(out.contains(&TouchCommand::PickDone), "{out:?}");
        assert!(!out.contains(&TouchCommand::PanelTapped), "{out:?}");
    }

    #[test]
    fn fingers_with_a_panel_open_move_the_canvas_and_leave_the_panel() {
        let s = session();
        s.set_touch_panel_open(true);
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(300.0, 100.0, 10.0));
        let mut out = s.touch_move(2, at(340.0, 100.0, 30.0));
        assert!(out.contains(&TouchCommand::ViewMoved), "{out:?}");
        out.extend(s.touch_up(2, at(340.0, 100.0, 60.0)));
        out.extend(s.touch_up(1, at(100.0, 100.0, 70.0)));
        assert!(!out.contains(&TouchCommand::PanelFades) && !out.contains(&TouchCommand::PanelTapped), "{out:?}");
        assert!(!painting(&s));
    }

    #[test]
    fn a_pen_waiting_for_a_panel_draws_on_past_a_finger_landing() {
        let s = session();
        s.set_touch_panel_open(true);
        s.touch_down(1, PointerKind::Stylus, at(100.0, 100.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(300.0, 100.0, 10.0));
        let out = s.touch_move(1, at(130.0, 100.0, 20.0));
        assert_eq!(out.first(), Some(&TouchCommand::PanelFades), "{out:?}");
        assert!(painting(&s));
        s.touch_cancel();
        // and a pen landing on a finger waiting takes over from it, waiting in its turn
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 100.0));
        s.touch_down(2, PointerKind::Stylus, at(300.0, 100.0, 110.0));
        let out = s.touch_up(2, at(300.0, 100.0, 130.0));
        assert!(out.contains(&TouchCommand::PanelTapped), "{out:?}");
    }

    #[test]
    fn with_finger_painting_off_fingers_only_move_the_canvas() {
        let s = session();
        let settings = TouchSettings { finger_paint: false, one_finger_pan: true, ..TouchSettings::default() };
        s.set_touch_settings(TouchSettings { undo_taps: false, ..settings });
        // one finger
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        assert!(!painting(&s));
        assert!(s.touch_move(1, at(200.0, 100.0, 20.0)).contains(&TouchCommand::ViewMoved));
        assert!(!painting(&s));
        assert!(!s.touch_up(1, at(200.0, 100.0, 40.0)).contains(&TouchCommand::StrokeEnded));
        // two, one after the other and both at once
        for (gap, ms) in [(30.0, 1000.0), (0.0, 2000.0)] {
            s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, ms));
            s.touch_down(2, PointerKind::Finger, at(300.0, 100.0, ms + gap));
            assert!(!painting(&s));
            let moved = s.touch_move(2, at(340.0, 100.0, ms + 60.0));
            assert!(moved.contains(&TouchCommand::ViewMoved), "{moved:?}");
            assert!(!painting(&s));
            let mut out = s.touch_up(2, at(340.0, 100.0, ms + 80.0));
            out.extend(s.touch_up(1, at(100.0, 100.0, ms + 90.0)));
            assert!(!out.contains(&TouchCommand::StrokeEnded), "{out:?}");
        }
    }

    #[test]
    fn a_pointer_the_platform_takes_back_makes_no_tap_and_no_stroke() {
        let s = session();
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(300.0, 100.0, 10.0));
        s.touch_cancel_one(2);
        let out = s.touch_up(1, at(100.0, 100.0, 110.0));
        assert!(!out.contains(&TouchCommand::Undo), "{out:?}");
        // a finger stroke taken back ends without being kept
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 1000.0));
        s.touch_move(1, at(140.0, 100.0, 1020.0));
        let out = s.touch_cancel_one(1);
        assert!(!out.contains(&TouchCommand::StrokeEnded), "{out:?}");
    }

    #[test]
    fn the_pen_button_switches_to_the_eraser_and_back_unless_set_otherwise() {
        let s = session();
        s.set_tool(ToolKind::Smudge);
        assert!(s.touch_pen_button(false, 10.0, 10.0).is_empty());
        assert_eq!(s.info().tool, ToolKind::Erase);
        s.touch_pen_button(false, 10.0, 10.0);
        assert_eq!(s.info().tool, ToolKind::Smudge);
        // the second button, set to nothing, does nothing
        assert!(s.touch_pen_button(true, 10.0, 10.0).is_empty());
        assert_eq!(s.info().tool, ToolKind::Smudge);
        // set to the quick menu, the first opens it where the pen is, the tool left alone
        s.set_touch_settings(TouchSettings { pen_button: Some(GestureAction::QuickMenu), ..TouchSettings::default() });
        assert_eq!(s.touch_pen_button(false, 30.0, 40.0), vec![TouchCommand::QuickMenu { x: 30.0, y: 40.0 }]);
        assert_eq!(s.info().tool, ToolKind::Smudge);
        // set to the eyedropper, the next touch picks
        s.set_touch_settings(TouchSettings {
            pen_second_button: Some(GestureAction::Eyedropper),
            ..TouchSettings::default()
        });
        s.touch_pen_button(true, 0.0, 0.0);
        assert!(s.pick_next());
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

    /// A 400 x 300 canvas in an 800 x 600 window, shown `zoom` times as big as it fits.
    fn shown(zoom: f32) -> std::sync::Arc<CanvasSession> {
        let s = CanvasSession::new(400, 300);
        {
            let mut st = s.shared.lock();
            st.window_size = (800, 600);
            let mut view = st.fitted_view();
            view.gesture(Vec2::new(400.0, 300.0), Vec2::new(0.0, 0.0), zoom, 0.0);
            st.view = Some(view);
        }
        s
    }

    /// Two fingers 600 apart pinched at `speed` (their spread, a part of where it began, a
    /// second) for `ms` at 200 Hz, then held still for `still` before they lift: what the lifts
    /// gave.
    fn pinch(s: &CanvasSession, speed: f32, ms: f64, still: f64) -> Vec<TouchCommand> {
        s.touch_down(1, PointerKind::Finger, at(100.0, 300.0, 0.0));
        s.touch_down(2, PointerKind::Finger, at(700.0, 300.0, 0.0));
        let (mut t, mut half) = (0.0, 300.0);
        while t + 5.0 <= ms {
            t += 5.0;
            half = 300.0 * (1.0 + speed * t as f32 / 1000.0);
            s.touch_move(1, at(400.0 - half, 300.0, t));
            s.touch_move(2, at(400.0 + half, 300.0, t));
        }
        let mut out = s.touch_up(2, at(400.0 + half, 300.0, t + still));
        out.extend(s.touch_up(1, at(400.0 - half, 300.0, t + still + 8.0)));
        out
    }

    fn zoom(s: &CanvasSession) -> f32 {
        s.shared.lock().view.map_or(0.0, |v| v.zoom())
    }

    #[test]
    fn fingers_moving_in_one_event_never_show_a_spread_they_didn_t_have() {
        let s = shown(1.0);
        let before = zoom(&s);
        let (a, b) = ((200.0, 300.0), (400.0, 300.0));
        s.touch_down(1, PointerKind::Finger, at(a.0, a.1, 0.0));
        s.touch_down(2, PointerKind::Finger, at(b.0, b.1, 5.0));
        // panned: both go the same way, so the zoom never moves
        for i in 1..=20 {
            let (x, ms) = (i as f32 * 6.0, f64::from(i) * 8.0 + 400.0);
            let step = |id: u64, p: (f32, f32)| TouchMove { id, samples: at(p.0 + x, p.1, ms) };
            s.touch_moves(vec![step(1, a), step(2, b)]);
            assert!((zoom(&s) / before - 1.0).abs() < 1e-5, "{} at {i}", zoom(&s));
        }
        assert!(!s.touches().more_to_come);
        // an event's last pointer may be one no longer followed: the view still goes by the others
        let zoomed = zoom(&s);
        let step = |id: u64, x: f32, ms: f64| TouchMove { id, samples: at(x, 300.0, ms) };
        s.touch_moves(vec![step(1, 200.0, 700.0), step(2, 520.0, 700.0), step(9, 0.0, 700.0)]);
        assert!(zoom(&s) > zoomed * 1.1, "{} from {zoomed}", zoom(&s));
    }

    #[test]
    fn fingers_leave_the_view_as_it_is_until_they_have_gone_a_little_way() {
        let s = shown(1.0);
        let before = zoom(&s);
        let (a, b) = ((200.0, 300.0), (400.0, 300.0));
        s.touch_down(1, PointerKind::Finger, at(a.0, a.1, 0.0));
        s.touch_down(2, PointerKind::Finger, at(b.0, b.1, 5.0));
        // as measured: each 4 out leaves it, each 6 moves it
        s.touch_move(1, at(a.0 - 4.0, a.1, 400.0));
        s.touch_move(2, at(b.0 + 4.0, b.1, 405.0));
        assert_eq!(zoom(&s), before);
        s.touch_move(1, at(a.0 - 6.0, a.1, 450.0));
        s.touch_move(2, at(b.0 + 6.0, b.1, 455.0));
        // on from there, so with no jump: only what came after the first step past it
        let started = zoom(&s);
        assert!(started >= before && started < before * 1.02, "{started} from {before}");
        s.touch_move(1, at(a.0 - 56.0, a.1, 500.0));
        s.touch_move(2, at(b.0 + 56.0, b.1, 505.0));
        assert!(zoom(&s) > started * 1.3, "{}", zoom(&s));
    }

    #[test]
    fn a_quick_pinch_in_fits_the_canvas() {
        let s = shown(1.0);
        let fitted = zoom(&s);
        let out = pinch(&s, -5.0, 120.0, 0.0);
        assert!(zoom(&s) < fitted * 0.5);
        assert!(out.contains(&TouchCommand::Fit), "{out:?}");
        // from fitted, a small flick in fits it again
        let s = shown(1.0);
        let out = pinch(&s, -6.9, 32.0, 0.0);
        assert!(out.contains(&TouchCommand::Fit), "{out:?}");
        // closing a little faster than 2.5 a second fits, a little slower doesn't
        assert!(pinch(&shown(1.0), -2.8, 150.0, 0.0).contains(&TouchCommand::Fit));
        assert!(!pinch(&shown(1.0), -2.2, 150.0, 0.0).contains(&TouchCommand::Fit));
    }

    #[test]
    fn a_pinch_that_stops_before_lifting_doesn_t_fit() {
        // however quick and far it went, fingers still as they lift aren't a flick
        for (speed, ms, still) in [(-5.0, 100.0, 60.0), (-3.3, 150.0, 45.0)] {
            let out = pinch(&shown(1.0), speed, ms, still);
            assert!(!out.contains(&TouchCommand::Fit), "{speed}: {out:?}");
        }
        // and a slow zoom out to well under the fit never is
        for (speed, ms) in [(-1.5, 300.0), (-1.0, 450.0), (-0.6, 800.0)] {
            let out = pinch(&shown(1.0), speed, ms, 0.0);
            assert!(!out.contains(&TouchCommand::Fit), "{speed}: {out:?}");
        }
    }

    #[test]
    fn a_flick_that_leaves_the_canvas_bigger_than_it_fits_doesn_t_fit() {
        let s = shown(3.0);
        let fitted = zoom(&shown(1.0));
        let out = pinch(&s, -6.9, 70.0, 0.0);
        assert!(zoom(&s) > fitted * 1.3, "{} {fitted}", zoom(&s));
        assert!(!out.contains(&TouchCommand::Fit), "{out:?}");
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

    /// As a hand does them on a tablet at twice the pixels a point: the fingers landing and moving
    /// one after another, a swipe down wobbling sideways, a scrub drifting up and down.
    #[test]
    fn a_hand_s_swipe_down_never_clears_and_its_scrub_does() {
        let fingers = [(1, 400.0f32), (2, 600.0), (3, 800.0)];
        let s = session();
        s.set_display_scale(2.0);
        for (k, (id, x)) in fingers.into_iter().enumerate() {
            s.touch_down(id, PointerKind::Finger, at(x, 300.0, 5.0 * k as f64));
        }
        let (mut cleared, mut copy_paste) = (false, false);
        for step in 1..=25 {
            for (k, (id, x)) in fingers.into_iter().enumerate() {
                // 12 down a step, each finger swaying 15 to the sides out of step with the others
                let sway = 15.0 * (step as f32 * 0.9 + k as f32 * 2.1).sin();
                let out = s.touch_move(id, at(x + sway, 300.0 + 12.0 * step as f32, 10.0 + 8.0 * f64::from(step)));
                cleared |= out.contains(&TouchCommand::ClearLayer);
                copy_paste |= out.contains(&TouchCommand::CopyPaste);
            }
        }
        for (id, x) in fingers {
            s.touch_up(id, at(x, 600.0, 300.0));
        }
        assert!(copy_paste && !cleared, "swipe: copy and paste {copy_paste}, cleared {cleared}");

        let s = session();
        s.set_display_scale(2.0);
        for (k, (id, x)) in fingers.into_iter().enumerate() {
            s.touch_down(id, PointerKind::Finger, at(x, 300.0, 5.0 * k as f64));
        }
        let (mut cleared, mut copy_paste) = (false, false);
        // four legs 90 across (45 points), 6 moves each, drifting 20 up and down
        for step in 1..=24 {
            let leg = (step - 1) / 6;
            let along = ((step - 1) % 6 + 1) as f32 * 15.0;
            let dx = if leg % 2 == 0 { along } else { 90.0 - along };
            let drift = 20.0 * (step as f32 * 0.5).sin();
            for (id, x) in fingers {
                let out = s.touch_move(id, at(x + dx, 300.0 + drift, 10.0 + 12.0 * f64::from(step)));
                cleared |= out.contains(&TouchCommand::ClearLayer);
                copy_paste |= out.contains(&TouchCommand::CopyPaste);
            }
        }
        assert!(cleared && !copy_paste, "scrub: cleared {cleared}, copy and paste {copy_paste}");
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

    /// A session with perspective blur open on a 1000 × 800 canvas seen one to one, its puck in
    /// the middle (two window pixels a point).
    fn perspective_session(kind: PerspectiveKind) -> std::sync::Arc<CanvasSession> {
        let s = CanvasSession::new(1000, 800);
        {
            let mut st = s.shared.lock();
            st.view = Some(procraft_engine::view::View::from_affine(procraft_engine::geom::Affine::IDENTITY));
            st.adjust = crate::adjusting::Adjusting::opened(Adjustment::PerspectiveBlur);
        }
        s.adjustment_perspective_kind(kind);
        s
    }

    /// Perspective blur's puck and the point along its arrow, in the window.
    fn puck(s: &CanvasSession) -> Vec<f32> {
        s.adjustment_shown().perspective_puck.unwrap_or_default().iter().map(|v| (v * 100.0).round() / 100.0).collect()
    }

    #[test]
    fn perspective_blurs_puck_is_dragged_about_and_undo_steps_it_back() {
        let s = perspective_session(PerspectiveKind::Positional);
        assert_eq!(puck(&s), [500.0, 400.0, 500.0, 300.0]);
        // a touch on the puck: it waits for the slop, then the puck follows where it was held
        s.touch_down(1, PointerKind::Finger, at(520.0, 400.0, 0.0));
        let out = s.touch_move(1, at(525.0, 402.0, 10.0));
        assert!(!out.contains(&TouchCommand::AdjustChanged), "{out:?}");
        let out = s.touch_move(1, at(620.0, 450.0, 40.0));
        assert!(out.contains(&TouchCommand::AdjustChanged), "{out:?}");
        let out = s.touch_up(1, at(620.0, 450.0, 60.0));
        assert!(!out.iter().any(|c| matches!(c, TouchCommand::AdjustMenu { .. })), "{out:?}");
        assert_eq!(puck(&s), [600.0, 450.0, 600.0, 350.0]);
        assert_eq!(s.adjustment_shown().amount, 0.0);
        // away from it the amount is swiped, the puck staying
        s.touch_down(1, PointerKind::Finger, at(900.0, 400.0, 100.0));
        s.touch_move(1, at(980.0, 400.0, 130.0));
        s.touch_up(1, at(980.0, 400.0, 140.0));
        let amount = s.adjustment_shown().amount;
        assert!(amount > 0.0);
        assert_eq!(puck(&s), [600.0, 450.0, 600.0, 350.0]);
        // undo puts the puck back, the amount staying; redo moves it again
        assert_eq!(s.undo().as_deref(), Some("Perspective Blur"));
        assert_eq!(puck(&s), [500.0, 400.0, 500.0, 300.0]);
        assert_eq!(s.adjustment_shown().amount, amount);
        s.redo();
        assert_eq!(puck(&s), [600.0, 450.0, 600.0, 350.0]);
    }

    #[test]
    fn directional_perspective_blurs_arrow_turns_from_its_side_and_the_puck_moves_from_the_other() {
        let s = perspective_session(PerspectiveKind::Directional);
        assert_eq!(s.adjustment_shown().perspective_kind, PerspectiveKind::Directional);
        // a tap on it asks for the menu
        s.touch_down(1, PointerKind::Finger, at(505.0, 400.0, 0.0));
        let out = s.touch_up(1, at(505.0, 400.0, 50.0));
        assert!(out.contains(&TouchCommand::AdjustMenu { x: 505.0, y: 400.0 }), "{out:?}");
        // from the arrow's side (60 points up) a drag turns the arrow to point at it
        s.touch_down(1, PointerKind::Finger, at(500.0, 280.0, 100.0));
        s.touch_move(1, at(650.0, 400.0, 130.0));
        s.touch_up(1, at(650.0, 400.0, 140.0));
        assert_eq!(puck(&s), [500.0, 400.0, 600.0, 400.0]);
        // from behind it a drag moves the puck, the arrow keeping its way
        s.touch_down(1, PointerKind::Finger, at(380.0, 400.0, 200.0));
        s.touch_move(1, at(410.0, 440.0, 230.0));
        s.touch_up(1, at(410.0, 440.0, 240.0));
        assert_eq!(puck(&s), [530.0, 440.0, 630.0, 440.0]);
        // beyond its reach (150 points) the amount is swiped
        s.touch_down(1, PointerKind::Finger, at(530.0, 140.0, 300.0));
        s.touch_move(1, at(600.0, 140.0, 330.0));
        s.touch_up(1, at(600.0, 140.0, 340.0));
        assert!(s.adjustment_shown().amount > 0.0);
        assert_eq!(puck(&s), [530.0, 440.0, 630.0, 440.0]);
        // undo steps back the move, then the turn
        s.undo();
        assert_eq!(puck(&s), [500.0, 400.0, 600.0, 400.0]);
        s.undo();
        assert_eq!(puck(&s), [500.0, 400.0, 500.0, 300.0]);
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
    fn after_a_warp_the_other_kinds_drag_the_box_s_nodes() {
        use procraft_engine::transform::{Transform, TransformMode};
        let s = session();
        {
            let mut st = s.shared.lock();
            st.view = Some(procraft_engine::view::View::from_affine(procraft_engine::geom::Affine::IDENTITY));
            let mut t = Transform::new(100.0, 100.0, 200.0, 100.0, Vec::new());
            t.start_warp();
            st.transform = Some(t);
            st.transform_mode = TransformMode::Distort;
        }
        s.set_touch_transforming(Some(TransformTouch { advanced_mesh: false }));
        let corner = |s: &CanvasSession| {
            let b = s.shared.lock().transform_box().map(|b| b.corners).unwrap_or_default();
            (b.first().copied(), b.get(1).copied())
        };
        // the top left corner dragged up and left goes with the touch from where the drag began
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        s.touch_move(1, at(70.0, 80.0, 16.0));
        s.touch_move(1, at(50.0, 60.0, 32.0));
        s.touch_up(1, at(50.0, 60.0, 48.0));
        assert_eq!(corner(&s), (Some(80.0), Some(80.0)));
        s.set_touch_transforming(None);
    }

    #[test]
    fn layer_opacity_follows_a_drag_across_and_paints_nothing() {
        let s = session();
        let layer = s.layers().into_iter().find(|l| l.current).unwrap();
        s.set_touch_layer_opacity(Some(layer.id));
        assert_eq!(s.touch_layer_opacity(), Some(layer.opacity));
        let opacity = |s: &CanvasSession| s.layers().into_iter().find(|l| l.id == layer.id).unwrap().opacity;
        // a tap changes nothing
        s.touch_down(1, PointerKind::Finger, at(400.0, 100.0, 0.0));
        let out = s.touch_up(1, at(400.0, 100.0, 50.0));
        assert!(!out.contains(&TouchCommand::StrokeEnded), "{out:?}");
        assert_eq!(opacity(&s), layer.opacity);
        // a drag to the left lowers it, as it goes
        s.touch_down(1, PointerKind::Finger, at(400.0, 100.0, 100.0));
        for k in 1..=10 {
            s.touch_move(1, at(400.0 - 20.0 * k as f32, 100.0, 100.0 + 16.0 * f64::from(k)));
        }
        let out = s.touch_up(1, at(200.0, 100.0, 300.0));
        assert!(!out.contains(&TouchCommand::StrokeEnded), "{out:?}");
        let dragged = s.touch_layer_opacity().unwrap();
        assert!(dragged < layer.opacity, "{dragged}");
        assert_eq!(opacity(&s), dragged);
        s.set_touch_layer_opacity(None);
        assert_eq!(s.touch_layer_opacity(), None);
    }

    #[test]
    fn hover_s_finger_drags_the_opacity_and_two_pinch_the_size() {
        let s = session();
        s.shared.lock().window_size = (1000, 800);
        s.set_brush_size(0.5);
        s.set_brush_opacity(0.5);
        s.set_touch_settings(TouchSettings { hover_touch: true, ..TouchSettings::default() });
        // the pen held over the canvas: a finger dragged a fifth of the window across adds a fifth
        s.set_touch_pen_hovering(true);
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        let out = s.touch_move(1, at(300.0, 100.0, 16.0));
        assert_eq!(out, vec![TouchCommand::BrushChanged]);
        assert!((s.info().opacity - 0.7).abs() < 1e-4, "{}", s.info().opacity);
        // a second finger: twice as far apart, twice the size
        s.touch_down(2, PointerKind::Finger, at(400.0, 100.0, 20.0));
        s.touch_move(2, at(500.0, 100.0, 30.0));
        assert!((s.info().size - 1.0).abs() < 1e-4, "{}", s.info().size);
        // the first finger moved in the pinch drags on from where it went
        s.touch_move(1, at(200.0, 100.0, 32.0));
        s.touch_up(2, at(500.0, 100.0, 40.0));
        s.touch_move(1, at(250.0, 100.0, 45.0));
        assert!((s.info().opacity - 0.75).abs() < 1e-4, "{}", s.info().opacity);
        let out = s.touch_up(1, at(250.0, 100.0, 50.0));
        assert!(!out.contains(&TouchCommand::StrokeEnded), "{out:?}");
        // the pen gone, a finger paints again
        s.set_touch_pen_hovering(false);
        s.touch_down(1, PointerKind::Finger, at(10.0, 10.0, 100.0));
        s.touch_move(1, at(60.0, 10.0, 120.0));
        assert!(s.touch_up(1, at(60.0, 10.0, 140.0)).contains(&TouchCommand::StrokeEnded));
        // and when it went without a word (not heard from for a while)
        s.set_touch_pen_hovering(true);
        s.touches().pen_hovering = std::time::Instant::now().checked_sub(std::time::Duration::from_secs(1));
        s.touch_down(1, PointerKind::Finger, at(10.0, 30.0, 200.0));
        s.touch_move(1, at(60.0, 30.0, 220.0));
        assert!(s.touch_up(1, at(60.0, 30.0, 240.0)).contains(&TouchCommand::StrokeEnded));
    }

    #[test]
    fn recolor_s_touch_drags_its_reticle_or_taps_it_elsewhere_and_paints_nothing() {
        let s = session();
        s.shared.lock().view = Some(procraft_engine::view::View::from_affine(procraft_engine::geom::Affine::IDENTITY));
        s.recolor_start(32.0, 32.0, 0.0, 0.0, 1.0, 0.0);
        // the app moves the reticle where the touches say
        let obey = |out: Vec<TouchCommand>| {
            for c in &out {
                match *c {
                    TouchCommand::RecolorMoved { x, y } => s.recolor_move_to(x, y),
                    TouchCommand::RecolorTapped { x, y } => s.recolor_tap(x, y),
                    _ => {}
                }
            }
            out
        };
        // a drag past the slop moves it as far as the touch went from where it came down
        obey(s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0)));
        obey(s.touch_move(1, at(200.0, 100.0, 16.0)));
        let out = obey(s.touch_move(1, at(210.0, 110.0, 32.0)));
        assert_eq!(out, vec![TouchCommand::RecolorMoved { x: 142.0, y: 42.0 }]);
        let out = obey(s.touch_up(1, at(210.0, 110.0, 48.0)));
        assert!(!out.contains(&TouchCommand::StrokeEnded), "{out:?}");
        assert_eq!(s.recolor_point(), Some(vec![142.0, 42.0]));
        // a tap puts it where it lands
        obey(s.touch_down(1, PointerKind::Finger, at(10.0, 12.0, 100.0)));
        obey(s.touch_up(1, at(11.0, 12.0, 150.0)));
        assert_eq!(s.recolor_point(), Some(vec![11.0, 12.0]));
        s.recolor_end();
    }

    #[test]
    fn the_companion_s_move_takes_a_lone_press_and_moves_the_view_without_painting() {
        let s = session();
        s.shared.lock().view = Some(procraft_engine::view::View::from_affine(procraft_engine::geom::Affine::IDENTITY));
        s.set_touch_companion(Some(CompanionMode::Move));
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        let out = s.touch_move(1, at(130.0, 120.0, 16.0));
        assert!(out.contains(&TouchCommand::ViewMoved), "{out:?}");
        // a second finger meanwhile does nothing
        assert!(s.touch_down(2, PointerKind::Finger, at(300.0, 300.0, 20.0)).is_empty());
        s.touch_up(2, at(300.0, 300.0, 30.0));
        let out = s.touch_up(1, at(130.0, 120.0, 40.0));
        assert!(!out.contains(&TouchCommand::StrokeEnded), "{out:?}");
        let m = s.view_matrix();
        assert_eq!((m.get(4).copied(), m.get(5).copied()), (Some(30.0), Some(20.0)));
        // off, a press paints again
        s.set_touch_companion(None);
        s.touch_down(1, PointerKind::Finger, at(10.0, 10.0, 100.0));
        s.touch_move(1, at(60.0, 10.0, 120.0));
        let out = s.touch_up(1, at(60.0, 10.0, 140.0));
        assert!(out.contains(&TouchCommand::StrokeEnded), "{out:?}");
    }

    #[test]
    fn one_finger_set_to_move_the_canvas_moves_it_and_the_pen_still_paints() {
        let s = session();
        s.shared.lock().view = Some(procraft_engine::view::View::from_affine(procraft_engine::geom::Affine::IDENTITY));
        let g = crate::gestures::GestureSettings::new(crate::gestures::KeptGestures::default());
        let changed = g.set(crate::gestures::GesturePurpose::General, "ONE_FINGER_PAN".into(), true);
        // painting with a finger goes off with it, and back on turns it off again
        assert_eq!(changed.get("GENERAL.FINGER_PAINT"), Some(&false));
        let settings = g.touch_settings(0.0);
        assert!(settings.one_finger_pan && !settings.finger_paint && settings.touch.is_none());
        s.set_touch_settings(settings);
        // a drag moves the canvas with the finger, painting nothing
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 0.0));
        let out = s.touch_move(1, at(120.0, 110.0, 8.0));
        assert!(!out.contains(&TouchCommand::ViewMoved), "within a tap's slop: {out:?}");
        let out = s.touch_move(1, at(160.0, 140.0, 16.0));
        assert!(out.contains(&TouchCommand::ViewMoved), "{out:?}");
        s.touch_move(1, at(170.0, 145.0, 24.0));
        let out = s.touch_up(1, at(170.0, 145.0, 40.0));
        assert!(!out.contains(&TouchCommand::StrokeEnded), "{out:?}");
        assert!(!s.info().can_undo);
        let m = s.view_matrix();
        assert_eq!((m.get(4).copied(), m.get(5).copied()), (Some(70.0), Some(45.0)));
        // held still, it still picks a colour
        s.touch_down(1, PointerKind::Finger, at(200.0, 200.0, 1000.0));
        assert!(s.touch_tick(1450.0).contains(&TouchCommand::Pick { x: 200.0, y: 200.0 }));
        s.touch_up(1, at(200.0, 200.0, 1500.0));
        // two fingers tapped still undo
        s.touch_down(1, PointerKind::Finger, at(100.0, 100.0, 2000.0));
        s.touch_down(2, PointerKind::Finger, at(200.0, 100.0, 2010.0));
        assert!(s.touch_up(1, at(100.0, 100.0, 2050.0)).contains(&TouchCommand::Undo));
        s.touch_up(2, at(200.0, 100.0, 2060.0));
        // the pen paints, and so does a mouse (a desktop's drawing tool)
        for kind in [PointerKind::Stylus, PointerKind::Mouse] {
            s.touch_down(3, kind, at(10.0, 10.0, 3000.0));
            s.touch_move(3, at(60.0, 10.0, 3020.0));
            assert!(s.touch_up(3, at(60.0, 10.0, 3040.0)).contains(&TouchCommand::StrokeEnded), "{kind:?}");
        }
        // painting with a finger back on: the finger paints again
        assert_eq!(
            g.set(crate::gestures::GesturePurpose::General, "FINGER_PAINT".into(), true).get("GENERAL.ONE_FINGER_PAN"),
            Some(&false)
        );
        s.set_touch_settings(g.touch_settings(0.0));
        s.touch_down(1, PointerKind::Finger, at(10.0, 30.0, 4000.0));
        s.touch_move(1, at(60.0, 30.0, 4020.0));
        assert!(s.touch_up(1, at(60.0, 30.0, 4040.0)).contains(&TouchCommand::StrokeEnded));
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

    /// One finger of a tap on a real screen: when it lands and lifts (ms), and how far (points)
    /// it drifts from where it landed by the time it lifts, which way (radians).
    #[derive(Clone, Copy, Debug)]
    struct Finger {
        lands: f64,
        lifts: f64,
        drift: f32,
        towards: f32,
    }

    /// Fingers tapped as a real screen reports them: each wobbles `wobble` points about its
    /// drifting path, all the fingers' moves reported every `every` ms, in one event (as Android
    /// gives them) or each its own (as the desktop does). The touches' waits run as they come due,
    /// as the apps run them. What the touches asked for.
    fn real_tap(s: &CanvasSession, fingers: &[Finger], wobble: f32, every: f64, batched: bool) -> Vec<TouchCommand> {
        let scale = s.shared.lock().display_scale;
        let at_time = |i: usize, ms: f64, k: usize| {
            let f = fingers[i];
            let gone = ((ms - f.lands) / (f.lifts - f.lands).max(1.0)).clamp(0.0, 1.0) as f32;
            let base = (200.0 + 140.0 * i as f32, 300.0 + 15.0 * (i * i) as f32);
            let shake = if ms <= f.lands {
                (0.0, 0.0)
            } else {
                ((k as f32 * 2.4 + i as f32).sin(), (k as f32 * 3.7 + i as f32).cos())
            };
            let x = base.0 + scale * (f.drift * gone * f.towards.cos() + wobble * shake.0);
            let y = base.1 + scale * (f.drift * gone * f.towards.sin() + wobble * shake.1);
            at(x, y, ms)
        };
        // downs, then moves, then lifts, at a time
        let mut events: Vec<(f64, u8, usize)> = Vec::new();
        for (i, f) in fingers.iter().enumerate() {
            events.push((f.lands, 0, i));
            events.push((f.lifts, 2, i));
        }
        let start = fingers.iter().map(|f| f.lands).fold(f64::MAX, f64::min);
        let end = fingers.iter().map(|f| f.lifts).fold(0.0, f64::max);
        let mut ms = start + every;
        while ms < end {
            events.push((ms, 1, 0));
            ms += every;
        }
        events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut out = Vec::new();
        let mut down = vec![false; fingers.len()];
        for (k, (ms, what, i)) in events.into_iter().enumerate() {
            while let Some(due) = s.touch_deadline().filter(|due| *due <= ms) {
                out.extend(s.touch_tick(due));
            }
            match what {
                0 => {
                    down[i] = true;
                    out.extend(s.touch_down(i as u64 + 1, PointerKind::Finger, at_time(i, ms, k)));
                }
                1 => {
                    let moves: Vec<TouchMove> = (0..fingers.len())
                        .filter(|j| down[*j])
                        .map(|j| TouchMove { id: j as u64 + 1, samples: at_time(j, ms, k) })
                        .collect();
                    if batched {
                        out.extend(s.touch_moves(moves));
                    } else {
                        for m in moves {
                            out.extend(s.touch_move(m.id, m.samples));
                        }
                    }
                }
                _ => {
                    down[i] = false;
                    out.extend(s.touch_up(i as u64 + 1, at_time(i, ms, k)));
                }
            }
        }
        out
    }

    /// Three fingers tapped as hands tap them (ms): landing up to 100 apart and lifting up to 100
    /// apart, in any order, each drifting a few points from where it landed as it presses and
    /// lifts (the first, painting until the second lands, too).
    fn hands_three_finger_taps() -> Vec<Vec<Finger>> {
        let finger = |lands, lifts, drift, towards| Finger { lands, lifts, drift, towards };
        vec![
            // nearly together
            vec![finger(0.0, 100.0, 0.0, 0.0), finger(8.0, 100.0, 1.0, 2.0), finger(16.0, 108.0, 1.0, 4.0)],
            // one after another, lifting in the order they landed
            vec![finger(0.0, 150.0, 3.0, 0.0), finger(30.0, 170.0, 3.0, 2.1), finger(60.0, 200.0, 3.0, 4.2)],
            // 100 apart down and up, the first lifting last
            vec![finger(0.0, 290.0, 5.0, 1.0), finger(50.0, 190.0, 4.0, 3.0), finger(100.0, 230.0, 5.0, 5.0)],
            // the last landing just before the first lifts
            vec![finger(0.0, 180.0, 2.0, 0.5), finger(90.0, 260.0, 4.0, 2.5), finger(100.0, 280.0, 2.0, 4.5)],
            // slow: all three down most of a tap's time
            vec![finger(0.0, 380.0, 6.0, 0.0), finger(60.0, 420.0, 6.0, 2.1), finger(100.0, 400.0, 6.0, 4.2)],
        ]
    }

    /// Whether what the touches asked for is one redo and nothing else a tap could do.
    fn redid_once(out: &[TouchCommand]) -> bool {
        let redone = out.iter().filter(|c| **c == TouchCommand::Redo).count();
        redone == 1
            && !out
                .iter()
                .any(|c| matches!(c, TouchCommand::Undo | TouchCommand::StrokeEnded | TouchCommand::ToggleBars))
    }

    #[test]
    fn three_fingers_tapped_as_hands_tap_them_redo_once() {
        // a desktop's screen at a point a pixel, a tablet's at two
        for scale in [1.0, 2.0] {
            for finger_paint in [true, false] {
                // Android hands every finger's move in one event; the desktop each its own
                for batched in [true, false] {
                    for (k, fingers) in hands_three_finger_taps().iter().enumerate() {
                        let s = session();
                        s.set_display_scale(scale);
                        s.set_touch_settings(TouchSettings { finger_paint, ..TouchSettings::default() });
                        let out = real_tap(&s, fingers, 1.0, 8.0, batched);
                        assert!(
                            redid_once(&out),
                            "tap {k} at {scale}, painting {finger_paint}, batched {batched}: {out:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn three_fingers_tapped_with_the_pen_near_or_a_panel_open_redo_once() {
        for (k, fingers) in hands_three_finger_taps().iter().enumerate() {
            // the pen held over the screen
            let s = session();
            s.set_touch_pen_hovering(true);
            assert!(redid_once(&real_tap(&s, fingers, 1.0, 8.0, true)), "tap {k}, the pen near");
            // a panel open: the first finger waits to be a tap or a drag, then the others land
            let s = session();
            s.set_touch_panel_open(true);
            let out = real_tap(&s, fingers, 1.0, 8.0, true);
            assert!(redid_once(&out) && !out.contains(&TouchCommand::PanelTapped), "tap {k}, a panel open: {out:?}");
        }
    }

    #[test]
    fn three_fingers_that_went_somewhere_or_stayed_are_no_redo() {
        let finger = |lands, lifts, drift, towards| Finger { lands, lifts, drift, towards };
        // each gone past a tap's reach
        let s = session();
        let slid: Vec<Finger> = (0..3).map(|i| finger(30.0 * f64::from(i), 200.0, 20.0, 0.0)).collect();
        assert!(!real_tap(&s, &slid, 0.0, 8.0, true).contains(&TouchCommand::Redo));
        // all three down longer than a tap
        let s = session();
        let held: Vec<Finger> =
            (0..3).map(|i| finger(30.0 * f64::from(i), 400.0 + 10.0 * f64::from(i), 0.0, 0.0)).collect();
        assert!(!real_tap(&s, &held, 0.0, 8.0, true).contains(&TouchCommand::Redo));
        // turned off
        let s = session();
        s.set_touch_settings(TouchSettings { undo_taps: false, ..TouchSettings::default() });
        assert!(!real_tap(&s, &hands_three_finger_taps()[1], 1.0, 8.0, true).contains(&TouchCommand::Redo));
    }
}
