//! ProCraft's native library for Android: a [`CanvasSession`] (UniFFI) that takes input from the
//! Kotlin UI, builds strokes with the engine and draws them with the GPU renderer on its own
//! render thread, straight into the app's `SurfaceView`.
//!
//! The window crosses over through two small JNI functions (`window.rs`, the crate's only unsafe
//! code): Kotlin hands its `Surface` to `NativeWindows.acquire` and passes the id it returns to
//! [`CanvasSession::attach_window`].

uniffi::setup_scaffolding!();

mod adjust;
mod animation;
mod brush_files;
mod brush_library;
mod brushes;
mod clipboard;
mod colour;
mod crop;
mod device;
mod guide;
mod layer_ops;
mod layers;
mod pick;
mod pour;
mod project;
mod projection;
mod quickshape;
#[cfg(target_os = "android")]
mod render;
mod scene3d;
mod selection;
mod studio;
#[cfg(test)]
mod tests;
mod text;
mod transform;
#[cfg(target_os = "android")]
mod window;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use procraft_engine::brush::Brush;
use procraft_engine::document::Document;
use procraft_engine::geom::Vec2;
use procraft_engine::input::{Pointer, Sample};
use procraft_engine::library::{BrushSet, library};
use procraft_engine::stroke::StrokeBuilder;
use procraft_engine::view::View;
use procraft_gpu::{Canvas, Gpu, StrokeStyle, StrokeTool};

/// Colour around the canvas, sRGB; and with the light interface.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const SURROUND: [f32; 3] = [37.0 / 255.0, 37.0 / 255.0, 37.0 / 255.0];
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const SURROUND_LIGHT: [f32; 3] = [206.0 / 255.0, 209.0 / 255.0, 210.0 / 255.0];

/// The grid over the surround: its line colour (sRGB), and its spacing, line width (a point, its
/// edges smoothed a third of a pixel out on a 2x screen) and top (the bar's height, none under
/// it), in interface points. It is fixed to the glass: see [`CanvasSession::set_grid_origin`].
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const GRID_LINE: [f32; 3] = [46.0 / 255.0, 46.0 / 255.0, 46.0 / 255.0];
/// The grid's lines with the light interface: a little darker than its surround.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const GRID_LINE_LIGHT: [f32; 3] = [202.0 / 255.0, 205.0 / 255.0, 206.0 / 255.0];
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const GRID_SPACING_PT: f32 = 16.0;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const GRID_WIDTH_PT: f32 = 4.0 / 3.0;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const GRID_TOP_PT: f32 = 44.0;

/// The bands over what is not selected: their period across the window and drift (points, points
/// a second, so a period a second), and their coverage.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const STRIPE_PERIOD_PT: f32 = 10.0;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const STRIPE_SPEED_PT: f32 = 10.0;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
/// How far what is behind a frosted panel is blurred (Gaussian sigma, points).
pub(crate) const BACKDROP_SIGMA_PT: f32 = 23.5;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
/// How dark the bands over what is not selected are, at full selection mask visibility.
pub(crate) const STRIPE_COVERAGE: f32 = 0.25;
/// What is selected is multiplied by this while selecting by colour, and covered by the other
/// while the threshold is dragged (sRGB).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const SELECTION_TINT: [f32; 3] = [1.0 / 255.0, 120.0 / 255.0, 1.0];
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const SELECTION_FILL: [f32; 3] = [50.0 / 255.0, 117.0 / 255.0, 247.0 / 255.0];

/// One input sample in window pixels.
#[derive(Clone, Copy, Debug, uniffi::Record)]
pub struct PenSample {
    pub x: f32,
    pub y: f32,
    /// 0..1 (ignored for fingers).
    pub pressure: f32,
    /// Radians from upright.
    pub tilt: f32,
    /// Radians, the direction the pen leans.
    pub orientation: f32,
    /// Milliseconds, the platform's event clock.
    pub time_ms: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PointerKind {
    Finger,
    Stylus,
    StylusEraser,
    Mouse,
}

impl From<PointerKind> for Pointer {
    fn from(p: PointerKind) -> Pointer {
        match p {
            PointerKind::Finger => Pointer::Finger,
            PointerKind::Stylus => Pointer::Stylus,
            PointerKind::StylusEraser => Pointer::StylusEraser,
            PointerKind::Mouse => Pointer::Mouse,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ToolKind {
    Paint,
    Erase,
    Smudge,
}

impl ToolKind {
    fn stroke_tool(self) -> StrokeTool {
        match self {
            ToolKind::Paint => StrokeTool::Paint,
            ToolKind::Erase => StrokeTool::Erase,
            ToolKind::Smudge => StrokeTool::Smudge,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CanvasInfo {
    pub width: u32,
    pub height: u32,
    /// Dots per inch.
    pub dpi: f32,
    pub tool: ToolKind,
    /// The current tool's sliders, 0..1.
    pub size: f32,
    pub opacity: f32,
    /// The current tool's brush across at that size, canvas pixels.
    pub brush_diameter: f32,
    pub can_undo: bool,
    pub can_redo: bool,
    pub zoom: f32,
    /// Frames shown with the canvas drawn on them.
    pub frames: u64,
    /// Time between the last two frames, milliseconds.
    pub frame_interval_ms: f32,
    /// Why the canvas can't draw, if it can't.
    pub error: Option<String>,
    /// Changes whenever the layers or their settings change (also by undo and redo).
    pub revision: u64,
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileError {
    #[error("window: {reason}")]
    Window { reason: String },
    #[error("file: {reason}")]
    File { reason: String },
}

#[derive(Clone, Copy, Debug)]
struct Sliders {
    size: f32,
    opacity: f32,
}

/// A brush's sliders before they are first moved.
const DEFAULT_SLIDERS: Sliders = Sliders { size: 0.3, opacity: 1.0 };

/// How long the view takes to settle into place, and how much it gives on the way.
const VIEW_SETTLE_SECONDS: f32 = 0.5;
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
const VIEW_SETTLE_DAMPING: f32 = 0.8;
/// How long the view takes to grow back to fit, slowing to a stop.
const VIEW_EASE_SECONDS: f32 = 0.3;

/// The view settling from `from` to `to`, begun at `start`, along `curve`.
#[derive(Clone, Copy)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) struct ViewMotion {
    pub(crate) from: View,
    pub(crate) to: View,
    pub(crate) start: std::time::Instant,
    pub(crate) curve: ViewCurve,
}

/// How a view moves into place: settling on a spring (fitting), or slowing to a stop (crop).
#[derive(Clone, Copy)]
pub(crate) enum ViewCurve {
    Settle,
    EaseOut(f32),
}

impl ViewCurve {
    /// How far along it is `t` seconds in, and how long it takes.
    fn at(self, t: f32) -> (f32, f32) {
        match self {
            ViewCurve::Settle => {
                let spring = procraft_engine::motion::Spring::new(VIEW_SETTLE_SECONDS, VIEW_SETTLE_DAMPING);
                (spring.at(t), spring.seconds())
            }
            ViewCurve::EaseOut(seconds) => (procraft_engine::motion::Bezier::ease_out(seconds).at(t), seconds),
        }
    }
}

impl State {
    /// Moves the view on to where its settling has got by now; true while it is still moving.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn step_view_motion(&mut self) -> bool {
        let Some(m) = self.view_motion else { return false };
        let t = m.start.elapsed().as_secs_f32();
        let (along, seconds) = m.curve.at(t);
        let centre = Vec2::new(self.doc.width() as f32 / 2.0, self.doc.height() as f32 / 2.0);
        self.view = Some(View::between(&m.from, &m.to, centre, along));
        if t >= seconds {
            self.view_motion = None;
            return false;
        }
        true
    }
}

pub(crate) struct State {
    pub(crate) doc: Document,
    /// Fitted once the window's size is known.
    pub(crate) view: Option<View>,
    /// The view settling from one place to another (fitting the canvas), stepped each frame.
    pub(crate) view_motion: Option<ViewMotion>,
    pub(crate) window_size: (u32, u32),
    pub(crate) reconfigure: bool,
    tool: ToolKind,
    /// The app's own smoothing and pressure, on top of every brush's.
    pub(crate) input: procraft_engine::stroke::InputSettings,
    /// Selection mask visibility, 0..1: how strongly what is not selected is banded.
    pub(crate) selection_visibility: f32,
    /// Brush scaling relative to the screen: the brush keeps its size on the screen as the canvas
    /// zooms (else it keeps its size on the canvas).
    pub(crate) brush_on_screen: bool,
    /// Crops kept for undo: the canvas and document from before; and one undone, for redo, with
    /// the steps the canvas then put back had made (redo only while that hasn't changed).
    crop_undo: Option<(Canvas, Document)>,
    crop_redo: Option<(Canvas, Document, u64)>,
    /// The text layer being edited, and the document as the edit began.
    text_edit: Option<(procraft_engine::document::LayerId, Document)>,
    /// The steps finished text edits made (by [`Canvas::undo_step`]'s numbering) and their layers:
    /// undo and redo of one go back to editing that text.
    pub(crate) text_steps: Vec<(u64, procraft_engine::document::LayerId)>,
    /// A colour dropped on the canvas whose threshold can still change.
    pub(crate) pour: Option<pour::Pour>,
    /// The library as the user has it, and as it ships.
    library: Vec<brush_library::LibrarySet>,
    shipped: Vec<BrushSet>,
    /// When the library is empty (never, but never panic).
    fallback: Brush,
    /// The brush each tool uses: (set, brush) in the library.
    paint_brush: (usize, usize),
    erase_brush: (usize, usize),
    smudge_brush: (usize, usize),
    /// Each set's last chosen brush, (set, brush) in the library, the latest choice last.
    set_choices: Vec<(usize, usize)>,
    /// Each brush's sliders, per tool.
    sliders: HashMap<(ToolKind, usize, usize), Sliders>,
    /// Strokes started, to seed each one's randomness.
    strokes: u32,
    /// The small canvas brush previews are drawn on.
    preview: Option<(Canvas, Document)>,
    color: [f32; 3],
    /// The secondary colour, for secondary colour dynamics.
    secondary: [f32; 3],
    stroke: Option<StrokeBuilder>,
    #[cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]
    pub(crate) gpu: Option<Gpu>,
    pub(crate) canvas: Option<Canvas>,
    pub(crate) needs_frame: bool,
    pub(crate) stop: bool,
    /// Project canvas: the second display's size, and that its thread should end.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) projection_size: (u32, u32),
    pub(crate) projection_stop: bool,
    pub(crate) frames: u64,
    /// The drawing pad's strokes as drawn (pointer and input), drawn again when the brush changes;
    /// `None` on a canvas that keeps none.
    pub(crate) pad_strokes: Option<Vec<(PointerKind, Vec<PenSample>)>>,
    /// The pad shows only its sample scribble: a stroke of one's own takes it away.
    pub(crate) pad_sample: bool,
    /// The pad drawn as a sphere, the flat pad's strokes set aside meanwhile.
    pub(crate) sphere: Option<crate::studio::SpherePad>,
    pub(crate) frame_interval_ms: f32,
    pub(crate) error: Option<String>,
    /// Window pixels per interface point.
    pub(crate) display_scale: f32,
    /// Where the middle of one of the surround grid's lines is, across and down (window pixels);
    /// none given, half a point in from the top-left corner.
    grid_origin: Option<[f32; 2]>,
    /// The light interface: a light surround and grid.
    light: bool,
    /// The document before a continuous change (a slider drag) began.
    pending_doc: Option<Document>,
    /// What undo calls that change.
    pending_name: Option<String>,
    /// The artwork's title.
    pub(crate) name: String,
    /// What the artwork keeps about itself: when it was made, strokes, time spent, its author.
    pub(crate) info: project::ArtworkInfo,
    /// Saved layer pixels waiting for the canvas (an opened artwork before it is shown).
    pub(crate) pending_pixels: Vec<(procraft_engine::document::LayerId, project::Opened)>,
    /// Let go of (see [`CanvasSession::release_canvas`]): no canvas is made again.
    pub(crate) closed: bool,
    /// A model open to paint on, in place of the flat canvas (the document is its current
    /// texture set's).
    pub(crate) scene: Option<scene3d::Scene>,
    /// What fitting the canvas keeps clear of (left, top, right, bottom, window pixels).
    fit_insets: [f32; 4],
    /// Frosted panels open over the canvas: left, top, right, bottom and corner radius (window
    /// pixels).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) backdrops: Vec<[f32; 6]>,
    /// The view kept with the artwork, shown when the window first comes.
    pub(crate) saved_view: Option<project::SavedView>,
    pub(crate) revision: u64,
    /// The open transform, how its handles act and how it resamples.
    transform: Option<procraft_engine::transform::Transform>,
    transform_mode: procraft_engine::transform::TransformMode,
    interpolation: transform::Interpolation,
    transform_guides: transform::GuideSettings,
    /// What is selected, if anything, and selections kept to load again.
    selection: Option<procraft_engine::selection::Mask>,
    saved_selections: Vec<procraft_engine::selection::Mask>,
    pub(crate) selection_display: selection::SelectionDisplay,
    /// The table showing a canvas with a profile of its own on the screen, and what it was made for.
    #[cfg(target_os = "android")]
    pub(crate) display_lut: Option<((usize, procraft_engine::profile::Profile), procraft_gpu::DisplayLut)>,
    /// While a colour selection's threshold is dragged: the selection before it, and the layer read once.
    selection_before: Option<Option<procraft_engine::selection::Mask>>,
    colour_source: Option<Vec<u8>>,
    /// An adjustment is open on the current layer, and what it last drew.
    adjusting: bool,
    last_filter: Option<procraft_gpu::Filter>,
    /// The liquify brush's last stamp (canvas pixels).
    liquify_last: Option<Vec2>,
    /// The brush studio is open on this (set, brush), and the brush as it opened.
    studio: Option<((usize, usize), Brush)>,
    /// The adjustment is painted on (Pencil); cloning, from where, and by how far once it starts.
    pencil: bool,
    cloning: bool,
    /// Leaving an adjustment makes the steps made in it (Pencil and clone strokes) one.
    simplified_undos: bool,
    /// The clone brush's sliders (it is the soft airbrush).
    clone_sliders: Sliders,
    clone_source: Option<Vec2>,
    clone_offset: Option<Vec2>,
    /// The selection before the feather popover softened it.
    feather_base: Option<procraft_engine::selection::Mask>,
    /// The last drag step on the box (window point, when), for its speed.
    last_drag: Option<(Vec2, std::time::Instant)>,
    /// The stroke being drawn as input (canvas points) and its pressures summed, for QuickShape,
    /// and the shape it snapped to while that is shown.
    drawn: Vec<Vec2>,
    drawn_pressure: f32,
    shape: Option<quickshape::ShapeEdit>,
    /// The open stroke as it began, and the last stroke kept, for QuickShape afterwards.
    began: Option<quickshape::LastStroke>,
    last_stroke: Option<quickshape::LastStroke>,
    /// Strokes follow the drawing guide even where drawing assist is off (□ held down).
    force_assist: bool,
    /// Where the stroke began (canvas pixels), for assisted drawing; the guide and view as the
    /// guide editor opened.
    assist_from: Option<Vec2>,
    pub(crate) guide_before: Option<(procraft_engine::guide::Guide, Option<View>)>,
}

impl State {
    /// The surround's grid in window pixels, shifted by `by` (where the window's corner lands on
    /// the target).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn surround_grid(&self, by: Vec2) -> procraft_gpu::SurroundGrid {
        let s = self.display_scale;
        let [x, y] = self.grid_origin.unwrap_or([0.5 * s, 0.5 * s]);
        procraft_gpu::SurroundGrid {
            spacing: GRID_SPACING_PT * s,
            origin: [x + by.x, y + by.y],
            top: GRID_TOP_PT * s + by.y,
            width: GRID_WIDTH_PT * s,
            color: if self.light { GRID_LINE_LIGHT } else { GRID_LINE },
        }
    }

    /// The colour around the canvas.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn surround(&self) -> [f32; 3] {
        if self.light { SURROUND_LIGHT } else { SURROUND }
    }

    /// The whole canvas fitted in the window, clear of the insets, turned by the right angle the
    /// view is nearest (fitting straightens a turned canvas, but keeps it on its side if it is).
    pub(crate) fn fitted_view(&self) -> View {
        let turns = self.view.map_or(0, |v| v.quarter_turns());
        View::fit_turned((self.doc.width(), self.doc.height()), self.window_size, self.fit_insets, turns)
    }

    /// Finishes an open stroke.
    fn finish_stroke(&mut self) {
        self.shape = None;
        if let (Some(open), Some(canvas)) = (self.stroke.take(), self.canvas.as_mut()) {
            close_stroke(canvas, open, &self.doc.stroke_guide());
            self.bake_painted_on();
        }
    }

    /// Ends a continuous change still open (a slider drag, a transform): it becomes one undo step.
    fn settle(&mut self) {
        self.end_transform(true);
        self.end_adjustment(true);
        let name = self.pending_name.take();
        if let (Some(before), Some(canvas)) = (self.pending_doc.take(), self.canvas.as_mut()) {
            if let Some(name) = name {
                canvas.name_next_step(&name);
            }
            canvas.record_document(before);
        }
    }

    /// Runs the queued canvas work now (without drawing), so the document already reflects an
    /// undo or redo when the call returns.
    fn flush(&mut self) {
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.render(gpu, &mut self.doc, None);
        }
    }

    /// The (set, brush) `tool` uses.
    fn brush_ref(&self, tool: ToolKind) -> (usize, usize) {
        match tool {
            ToolKind::Paint => self.paint_brush,
            ToolKind::Erase => self.erase_brush,
            ToolKind::Smudge => self.smudge_brush,
        }
    }

    /// What brush sizes are multiplied by: 1 relative to the canvas; relative to the screen, the
    /// canvas fitted to the window over the zoom now, so a brush looks the same size at any zoom.
    fn brush_scale(&self) -> f32 {
        if !self.brush_on_screen {
            return 1.0;
        }
        let fit = View::fit((self.doc.width(), self.doc.height()), self.window_size).zoom();
        let zoom = self.view.map_or(fit, |v| v.zoom());
        if zoom > 0.0 && fit.is_finite() { (fit / zoom).clamp(0.01, 100.0) } else { 1.0 }
    }

    fn brush(&self, tool: ToolKind) -> &Brush {
        let (set, index) = self.brush_ref(tool);
        self.library.get(set).and_then(|s| s.brushes.get(index)).unwrap_or(&self.fallback)
    }

    /// The brush a stroke of `tool` on `layer` uses, its style, and its diameter at the size
    /// slider (canvas pixels, before the brush scale).
    pub(crate) fn stroke_brush(
        &mut self,
        tool: ToolKind,
        layer: procraft_engine::document::LayerId,
    ) -> (Brush, StrokeStyle, f32) {
        let sliders = self.sliders(tool);
        let mut brush = self.brush(tool).clone();
        // a blending brush's opacity lightens each stamp, as its flow does, so its strokes still
        // build up to full; a glaze's caps the stroke
        let opacity = if brush.blends() && matches!(tool.stroke_tool(), StrokeTool::Paint | StrokeTool::Erase) {
            brush.flow *= sliders.opacity.clamp(0.0, 1.0).powf(procraft_engine::brush::FLOW_POWER);
            1.0
        } else {
            brush.opacity(sliders.opacity)
        };
        let diameter = brush.diameter(sliders.size);
        let mut style = StrokeStyle::for_brush(
            &brush,
            tool.stroke_tool(),
            self.color,
            opacity,
            self.doc.layer(layer).is_some_and(|l| l.alpha_lock),
        );
        style.secondary = self.secondary;
        style.preview = self.pad_strokes.is_some();
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.use_sources(gpu, &mut style, &brush);
        }
        (brush, style, diameter)
    }

    /// More input for the pad's last kept stroke, if it keeps them.
    fn keep_pad_input(&mut self, samples: &[PenSample]) {
        if let Some((_, input)) = self.pad_strokes.as_mut().and_then(|k| k.last_mut()) {
            input.extend_from_slice(samples);
        }
    }

    /// The tool's brush in the library, to change it (none if the tool has fallen back).
    fn brush_mut(&mut self, tool: ToolKind) -> Option<&mut Brush> {
        let (set, index) = self.brush_ref(tool);
        self.library.get_mut(set).and_then(|s| s.brushes.get_mut(index))
    }

    fn sliders(&self, tool: ToolKind) -> Sliders {
        let (set, index) = self.brush_ref(tool);
        self.sliders.get(&(tool, set, index)).copied().unwrap_or_else(|| self.starting_sliders(tool))
    }

    /// Where a brush's sliders start: at the size its preview shows it, full opacity.
    fn starting_sliders(&self, tool: ToolKind) -> Sliders {
        let curve = procraft_engine::brush::SIZE_CURVE;
        let size = self.brush(tool).setting("preview_size").clamp(0.0, 1.0).powf(1.0 / curve);
        Sliders { size, ..DEFAULT_SLIDERS }
    }

    /// The sliders the side bar shows: the clone brush's while cloning, the paint brush's while
    /// an adjustment is painted on, else the tool's.
    fn shown_sliders(&self) -> Sliders {
        if self.cloning {
            self.clone_sliders
        } else {
            self.sliders(if self.pencil { ToolKind::Paint } else { self.tool })
        }
    }

    fn sliders_mut(&mut self) -> &mut Sliders {
        if self.cloning {
            return &mut self.clone_sliders;
        }
        let tool = if self.pencil { ToolKind::Paint } else { self.tool };
        let (set, index) = self.brush_ref(tool);
        let start = self.starting_sliders(tool);
        self.sliders.entry((tool, set, index)).or_insert(start)
    }

    /// Creates the canvas once there is a GPU.
    #[cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]
    pub(crate) fn ensure_canvas(&mut self) {
        if self.canvas.is_some() || self.closed {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        match Canvas::new(gpu, &self.doc) {
            Ok(mut c) => {
                c.set_resident_budget(crate::layers::resident_budget());
                let (w, h) = (self.doc.width(), self.doc.height());
                for (id, pixels) in project::opened_pixels(std::mem::take(&mut self.pending_pixels), w, h) {
                    // past what fits, kept compressed until needed
                    let opened =
                        if c.has_room() { c.upload_layer(gpu, id, &pixels) } else { c.park_pixels(id, &pixels) };
                    if let Err(e) = opened {
                        log::error!("opening a layer: {e}");
                    }
                }
                // as read: not changed
                c.take_changed();
                self.canvas = Some(c);
            }
            Err(e) => {
                log::error!("canvas: {e}");
                self.error = Some(e.to_string());
            }
        }
    }

    /// Samples in canvas pixels.
    fn to_canvas(&self, samples: &[PenSample]) -> Vec<Sample> {
        let Some(inv) = self.view.and_then(|v| v.to_screen().inverse()) else { return Vec::new() };
        samples
            .iter()
            .map(|s| Sample {
                pos: inv.apply(Vec2::new(s.x, s.y)),
                pressure: s.pressure,
                tilt: s.tilt,
                azimuth: s.orientation,
                time_ms: s.time_ms,
            })
            .collect()
    }
}

/// Draws `samples` into the canvas's open stroke: their stamps as they come or, for a stroke shown
/// with its tip, those the end taper no longer reaches for good and the rest as the tip. Whether
/// anything was drawn.
fn draw_samples(
    canvas: &mut Canvas,
    builder: &mut StrokeBuilder,
    samples: &[Sample],
    guide: &procraft_engine::guide::Guide,
) -> bool {
    if canvas.shows_tip() && builder.shows_tip() {
        let (kept, tip) = builder.add_tipped(samples);
        canvas.add_stamps(&guide::mirrored(guide, &kept));
        canvas.set_tip(&guide::mirrored(guide, &tip));
        return !kept.is_empty() || !tip.is_empty();
    }
    let stamps = guide::mirrored(guide, &builder.add(samples));
    canvas.add_stamps(&stamps);
    // the stretch to the newest point waits for the point after it to curve by: shown meanwhile
    // (as predicted input is, until the next)
    canvas.set_prediction(&guide::mirrored(guide, &builder.preview(&[])));
    !stamps.is_empty()
}

/// Ends the canvas's open stroke with `builder`'s final stamps: a painted or erased stroke is
/// redrawn once in its final form (end taper); a smudge, which has already changed the layer, and
/// an adjustment painted on, which adds to its mask, only get the stamps not drawn yet.
fn close_stroke(canvas: &mut Canvas, mut builder: StrokeBuilder, guide: &procraft_engine::guide::Guide) {
    let drawn = builder.stamp_count();
    let all = builder.finish();
    // what undo calls it
    canvas.name_next_step(match canvas.stroke_tool() {
        Some(StrokeTool::Erase) => "Erase stroke",
        Some(StrokeTool::Smudge) => "Smudge stroke",
        _ if canvas.is_pencil() => "Filter stroke",
        _ => "Paint stroke",
    });
    if canvas.stroke_tool() == Some(StrokeTool::Smudge) || canvas.is_pencil() {
        canvas.add_stamps(&guide::mirrored(guide, all.get(drawn..).unwrap_or(&[])));
    } else {
        canvas.replace_stamps(&guide::mirrored(guide, &all));
    }
    canvas.end_stroke();
}

pub(crate) struct Shared {
    /// Fair when it matters: the render thread hands it on to whoever waits.
    state: parking_lot::Mutex<State>,
    pub(crate) wake: parking_lot::Condvar,
    thread: Mutex<Option<JoinHandle<()>>>,
    /// Project canvas's render thread.
    projection: Mutex<Option<JoinHandle<()>>>,
    /// Held while the artwork is saved: one save at a time, so two never write the same files.
    pub(crate) saving: Mutex<project::Saved>,
}

impl Shared {
    pub(crate) fn lock(&self) -> parking_lot::MutexGuard<'_, State> {
        self.state.lock()
    }

    /// Lets the canvas finish remaking its caches a render at a time, the lock let go between,
    /// so a save or export after reads the picture as it is without holding everything up.
    fn finish_caches(&self) {
        // each render remakes at least one item, so this ends well before the bound
        for _ in 0..100_000 {
            {
                let mut guard = self.lock();
                let st = &mut *guard;
                let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return };
                canvas.render(gpu, &mut st.doc, None);
                if !canvas.busy() {
                    return;
                }
                // whoever waits on the lock (a touch, the render thread) has it next
                parking_lot::MutexGuard::unlock_fair(guard);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// The lock, taken once the canvas has finished remaking its caches (made a render at a
    /// time, the lock let go between): what is read under it is the picture as it is, and
    /// nothing waits on a whole remake.
    pub(crate) fn lock_finished(&self) -> parking_lot::MutexGuard<'_, State> {
        for _ in 0..1000 {
            self.finish_caches();
            let guard = self.lock();
            if guard.canvas.as_ref().is_none_or(|c| !c.busy()) {
                return guard;
            }
        }
        self.lock()
    }

    fn frame(&self, st: &mut State) {
        st.needs_frame = true;
        self.wake.notify_all();
    }

    /// Creates the canvas once there is a GPU, as [`State::ensure_canvas`] does, but with the
    /// opened layers sent to the GPU between holds of the state: an artwork of many layers takes
    /// seconds, and the app's own thread would wait that long on any call. Each layer is copied
    /// out (and decoded) as it goes, so what was opened stays in the state (a save meanwhile still
    /// has it); if
    /// the document or what was opened changed meanwhile, the canvas is made again under the hold.
    #[cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]
    pub(crate) fn make_canvas(&self) {
        let (gpu, doc, ids) = {
            let st = self.lock();
            let Some(gpu) = st.gpu.as_ref().filter(|_| st.canvas.is_none() && !st.closed) else { return };
            (gpu.clone(), st.doc.clone(), st.pending_pixels.iter().map(|(id, _)| *id).collect::<Vec<_>>())
        };
        let started = std::time::Instant::now();
        /// A layer read, for the GPU or (past what fits) to keep compressed.
        enum Read {
            Upload(Vec<u8>),
            Park(procraft_gpu::ReadyToPark),
        }
        let made = Canvas::new(&gpu, &doc).map(|mut c| {
            c.set_resident_budget(crate::layers::resident_budget());
            let (w, h) = (doc.width(), doc.height());
            // read on a few threads (each file decoded and, below the top layers that fit, its
            // compressed copy made), put on the canvas here in order
            let first_resident = ids.len().saturating_sub(c.most_resident());
            let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).clamp(1, 6);
            let next = std::sync::atomic::AtomicUsize::new(0);
            let (send, receive) = std::sync::mpsc::sync_channel(workers);
            std::thread::scope(|scope| {
                for _ in 0..workers {
                    let (send, next, ids) = (send.clone(), &next, &ids);
                    scope.spawn(move || {
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(id) = ids.get(i) else { break };
                            // none if the artwork changed meanwhile: what is made is let go
                            let layer =
                                self.lock().pending_pixels.get(i).filter(|(at, _)| at == id).map(|(_, p)| p.clone());
                            let read = layer.map(|l| {
                                l.pixels(w, h).map(|px| {
                                    let hash = project::pixels_hash(&px);
                                    if i >= first_resident {
                                        (Read::Upload(px), hash)
                                    } else {
                                        (Read::Park(procraft_gpu::ReadyToPark::of(&px, w, h)), hash)
                                    }
                                })
                            });
                            if send.send((i, read)).is_err() {
                                break;
                            }
                        }
                    });
                }
                drop(send);
                let mut waiting = std::collections::BTreeMap::new();
                let mut at = 0;
                for (i, read) in receive {
                    waiting.insert(i, read);
                    while let Some(read) = waiting.remove(&at) {
                        let Some(id) = ids.get(at) else { break };
                        at += 1;
                        match read {
                            None => {}
                            Some(Ok((layer, hash))) => {
                                if let Ok(mut saved) = self.saving.lock() {
                                    saved.read(*id, hash);
                                }
                                let opened = match layer {
                                    Read::Upload(px) => c.upload_layer(&gpu, *id, &px),
                                    Read::Park(ready) => {
                                        c.park_ready(*id, ready);
                                        Ok(())
                                    }
                                };
                                if let Err(e) = opened {
                                    log::error!("opening a layer: {e}");
                                }
                            }
                            Some(Err(e)) => log::error!("layer {id:?}: {e}"),
                        }
                    }
                }
            });
            // as read: not changed
            c.take_changed();
            log::info!(
                "canvas made in {} ms: {} layers, {} kept compressed",
                started.elapsed().as_millis(),
                ids.len(),
                c.parked_layers()
            );
            c
        });
        let mut st = self.lock();
        if st.canvas.is_some() {
            return;
        }
        let same = st.doc == doc && st.pending_pixels.iter().map(|(id, _)| *id).eq(ids.iter().copied());
        match made {
            Ok(c) if same => {
                st.pending_pixels.clear();
                st.canvas = Some(c);
            }
            Ok(_) => st.ensure_canvas(),
            Err(e) => {
                log::error!("canvas: {e}");
                st.error = Some(e.to_string());
            }
        }
    }
}

/// A canvas being painted: the document, the brush, the input and the renderer.
#[derive(uniffi::Object)]
pub struct CanvasSession {
    pub(crate) shared: Arc<Shared>,
}

#[uniffi::export]
impl CanvasSession {
    /// A new artwork of `width` × `height` pixels.
    #[uniffi::constructor]
    pub fn new(width: u32, height: u32) -> Arc<CanvasSession> {
        CanvasSession::with_document(Document::new(width, height), DEFAULT_NAME.to_owned(), Vec::new())
    }
}

/// What a new artwork is called.
pub(crate) const DEFAULT_NAME: &str = "Untitled Artwork";

impl CanvasSession {
    /// A session on `doc`, with saved pixels for its layers.
    pub(crate) fn with_document(
        doc: Document,
        name: String,
        pixels: Vec<(procraft_engine::document::LayerId, Vec<u8>)>,
    ) -> Arc<CanvasSession> {
        #[cfg(target_os = "android")]
        android_logger::init_once(
            android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("procraft"),
        );
        let shipped = library();
        let shelf = brush_library::LibrarySet::shipped(&shipped);
        // the round pen to paint, the soft airbrush to erase, the round brush to smudge
        let [paint_brush, erase_brush, smudge_brush] =
            [(1, 0), (3, 0), (2, 0)].map(|origin| brush_library::place_of(&shelf, origin));
        let state = State {
            doc,
            view: None,
            view_motion: None,
            window_size: (1, 1),
            reconfigure: false,
            tool: ToolKind::Paint,
            text_edit: None,
            text_steps: Vec::new(),
            pour: None,
            crop_undo: None,
            selection_visibility: 0.25,
            brush_on_screen: false,
            input: procraft_engine::stroke::InputSettings::default(),
            crop_redo: None,
            library: shelf,
            shipped,
            fallback: Brush::round_pen(),
            paint_brush,
            erase_brush,
            smudge_brush,
            set_choices: Vec::new(),
            sliders: HashMap::new(),
            strokes: 0,
            preview: None,
            color: [0.0, 0.0, 0.0],
            secondary: [1.0, 1.0, 1.0],
            stroke: None,
            gpu: None,
            canvas: None,
            scene: None,
            needs_frame: false,
            stop: false,
            projection_size: (1, 1),
            projection_stop: false,
            frames: 0,
            pad_strokes: None,
            pad_sample: false,
            sphere: None,
            frame_interval_ms: 0.0,
            error: None,
            display_scale: 2.0,
            grid_origin: None,
            light: false,
            pending_doc: None,
            pending_name: None,
            info: project::ArtworkInfo {
                created_ms: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0)),
                ..project::ArtworkInfo::default()
            },
            revision: 0,
            transform: None,
            transform_mode: procraft_engine::transform::TransformMode::Freeform,
            interpolation: transform::Interpolation::Bilinear,
            transform_guides: transform::GuideSettings::default(),
            selection: None,
            saved_selections: Vec::new(),
            selection_display: selection::SelectionDisplay::Stripes,
            #[cfg(target_os = "android")]
            display_lut: None,
            selection_before: None,
            colour_source: None,
            feather_base: None,
            adjusting: false,
            last_filter: None,
            liquify_last: None,
            studio: None,
            pencil: false,
            cloning: false,
            simplified_undos: true,
            clone_sliders: DEFAULT_SLIDERS,
            clone_source: None,
            clone_offset: None,
            last_drag: None,
            drawn: Vec::new(),
            drawn_pressure: 0.0,
            shape: None,
            began: None,
            last_stroke: None,
            force_assist: false,
            assist_from: None,
            guide_before: None,
            name,
            pending_pixels: project::opened(pixels),
            closed: false,
            fit_insets: [0.0; 4],
            backdrops: Vec::new(),
            saved_view: None,
        };
        Arc::new(CanvasSession {
            shared: Arc::new(Shared {
                state: parking_lot::Mutex::new(state),
                wake: parking_lot::Condvar::new(),
                thread: Mutex::new(None),
                projection: Mutex::new(None),
                saving: Mutex::new(project::Saved::default()),
            }),
        })
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Starts drawing into the window `window` (an id from `NativeWindows.acquire`), `width` ×
    /// `height` pixels. The session owns the window from here on.
    pub fn attach_window(&self, window: u64, width: u32, height: u32) -> Result<(), MobileError> {
        self.detach_window();
        #[cfg(target_os = "android")]
        {
            let native =
                window::take(window).ok_or_else(|| MobileError::Window { reason: format!("no window {window}") })?;
            {
                let mut st = self.shared.lock();
                st.window_size = (width.max(1), height.max(1));
                if st.view.is_none() {
                    // the view kept with the artwork, unless it would leave the canvas off the page
                    let canvas = (st.doc.width(), st.doc.height());
                    let kept = st
                        .saved_view
                        .take()
                        .and_then(|v| v.view(st.window_size))
                        .filter(|v| project::shows_canvas(v, st.window_size, canvas));
                    st.view = Some(kept.unwrap_or_else(|| st.fitted_view()));
                }
                st.stop = false;
                st.reconfigure = true;
                self.shared.frame(&mut st);
            }
            let shared = Arc::clone(&self.shared);
            let handle = std::thread::Builder::new()
                .name("procraft-render".to_owned())
                .spawn(move || render::run(shared, native))
                .map_err(|e| MobileError::Window { reason: e.to_string() })?;
            *self.shared.thread.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
            Ok(())
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (width, height);
            Err(MobileError::Window { reason: format!("windows are Android-only (asked for {window})") })
        }
    }

    pub fn resize_window(&self, width: u32, height: u32) {
        let mut st = self.shared.lock();
        let size = (width.max(1), height.max(1));
        if st.window_size != size {
            st.window_size = size;
            st.reconfigure = true;
            self.shared.frame(&mut st);
        }
    }

    /// Stops drawing and lets go of the window (call before the platform destroys it). Returns
    /// once the render thread has finished.
    pub fn detach_window(&self) {
        let handle = self.shared.thread.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(handle) = handle {
            {
                let mut st = self.shared.lock();
                st.stop = true;
                self.shared.wake.notify_all();
            }
            let _ = handle.join();
            self.shared.lock().stop = false;
        }
    }

    /// A stroke starts: `samples` are its first input.
    pub fn stroke_begin(&self, pointer: PointerKind, samples: Vec<PenSample>) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(kept) = st.pad_strokes.as_mut() {
            kept.push((pointer, samples.clone()));
        }
        // a layer being transformed or adjusted takes no paint, unless the adjustment is painted on
        if st.transform.is_some() || (st.adjusting && !st.pencil) {
            return;
        }
        if st.adjusting {
            st.pencil_stroke(pointer, &samples);
            self.shared.frame(st);
            return;
        }
        // on a model the stroke is drawn on the screen and carried onto it
        if st.scene_shown() {
            st.scene_stroke_begin(pointer, &samples);
            self.shared.frame(st);
            return;
        }
        let current = st.doc.current();
        st.finish_stroke();
        st.settle();
        // locked and hidden layers, groups and text layers take no paint
        if st.doc.layer(current).is_none_or(|l| {
            l.locked || !l.visible || l.kind == procraft_engine::document::LayerKind::Group || l.text.is_some()
        }) {
            return;
        }
        let tool = if pointer == PointerKind::StylusEraser { ToolKind::Erase } else { st.tool };
        let (brush, style, diameter) = st.stroke_brush(tool, current);
        let diameter = diameter * st.brush_scale();
        st.strokes = st.strokes.wrapping_add(1);
        st.info.strokes = st.info.strokes.saturating_add(1);
        let seed = st.strokes.wrapping_mul(0x9e37_79b9);
        let mut builder = StrokeBuilder::with_seed(brush.clone(), diameter, pointer.into(), seed)
            .scaled(st.brush_scale())
            .with_input(st.input.clone())
            .on_view_turned(st.view.map_or(0.0, |v| v.rotation()))
            .seen_at(st.view.map_or(1.0, |v| v.zoom()) / st.display_scale);
        if st.pad_strokes.is_some() {
            // on the studio's pad the lean moves no stamps, and the tilt switches act only when
            // the brush previews tilt
            builder = builder.on_pad(brush.setting("preview_tilt") >= 0.5);
        }
        if st.pad_sample {
            // the pad's sample scribble is drawn as given: its path whatever the brush's
            // streamline, its pressure as it comes (its pace still counts)
            builder = builder.exact_path();
        }
        let canvas_samples = st.to_canvas(&samples);
        st.assist_from = canvas_samples.first().map(|s| s.pos);
        let canvas_samples = st.assisted(canvas_samples);
        st.drawn.clear();
        st.drawn_pressure = 0.0;
        st.remember_input(&canvas_samples);
        // painted and erased strokes can be made shapes once they end
        st.last_stroke = None;
        st.began =
            matches!(tool.stroke_tool(), StrokeTool::Paint | StrokeTool::Erase).then(|| quickshape::LastStroke {
                layer: current,
                style,
                brush: brush.clone(),
                diameter,
                pointer,
                drawn: Vec::new(),
                pressure: 1.0,
                steps: 0,
            });
        let Some(canvas) = st.canvas.as_mut() else { return };
        canvas.begin_stroke(current, style);
        draw_samples(canvas, &mut builder, &canvas_samples, &st.doc.stroke_guide());
        st.stroke = Some(builder);
        self.shared.frame(st);
    }

    /// More input for the stroke (including the samples batched in between).
    pub fn stroke_add(&self, samples: Vec<PenSample>) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.scene_shown() {
            st.scene_stroke_add(&samples, false);
            self.shared.frame(st);
            return;
        }
        st.keep_pad_input(&samples);
        let canvas_samples = st.assisted(st.to_canvas(&samples));
        // a snapped shape follows the held finger no further
        if st.shape.is_some() {
            return;
        }
        st.remember_input(&canvas_samples);
        let (Some(builder), Some(canvas)) = (st.stroke.as_mut(), st.canvas.as_mut()) else { return };
        if draw_samples(canvas, builder, &canvas_samples, &st.doc.stroke_guide()) {
            st.redraw_painted_on();
            self.shared.frame(st);
        }
    }

    /// Where the input is predicted to go next: drawn ahead of the stroke until the next input,
    /// never kept.
    pub fn stroke_predict(&self, samples: Vec<PenSample>) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.scene_shown() {
            return;
        }
        let canvas_samples = st.to_canvas(&samples);
        let (Some(builder), Some(canvas)) = (st.stroke.as_ref(), st.canvas.as_mut()) else { return };
        let guide = st.doc.stroke_guide();
        // a stroke shown with its tip ends it where the input is predicted to go
        if canvas.shows_tip() && builder.shows_tip() {
            canvas.set_tip(&guide::mirrored(&guide, &builder.preview_tip(&canvas_samples)));
        } else {
            canvas.set_prediction(&guide::mirrored(&guide, &builder.preview(&canvas_samples)));
        }
        self.shared.frame(st);
    }

    /// The stroke ends (the pen or finger lifted) after `samples`.
    pub fn stroke_end(&self, samples: Vec<PenSample>) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.scene_shown() {
            st.scene_stroke_add(&samples, true);
            self.shared.frame(st);
            return;
        }
        st.keep_pad_input(&samples);
        let canvas_samples = st.assisted(st.to_canvas(&samples));
        // a snapped shape stays open to be edited, until the next thing done keeps it
        if st.shape.is_some() {
            self.shared.frame(st);
            return;
        }
        let (Some(mut builder), Some(canvas)) = (st.stroke.take(), st.canvas.as_mut()) else { return };
        let stamps = builder.add(&canvas_samples);
        canvas.add_stamps(&guide::mirrored(&st.doc.stroke_guide(), &stamps));
        close_stroke(canvas, builder, &st.doc.stroke_guide());
        st.redraw_painted_on();
        st.bake_painted_on();
        st.remember_last_stroke();
        self.shared.frame(st);
    }

    /// Drops the stroke (a second finger turned it into a gesture).
    pub fn stroke_cancel(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.scene_shown() {
            st.scene_stroke_cancel();
            self.shared.frame(st);
            return;
        }
        if let Some(kept) = st.pad_strokes.as_mut() {
            kept.pop();
        }
        st.stroke = None;
        st.shape = None;
        st.began = None;
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.cancel_stroke();
            // what it painted on is in the adjustment's mask: start that again
            st.restart_painted_on(None);
            self.shared.frame(st);
        }
    }

    /// A step of a two-finger gesture (see `View::gesture`).
    pub fn view_gesture(&self, pivot_x: f32, pivot_y: f32, pan_x: f32, pan_y: f32, scale: f32, rotation: f32) {
        let mut st = self.shared.lock();
        // the fingers take the canvas from wherever it was settling to
        st.view_motion = None;
        if st.scene_shown() {
            st.scene_gesture(Vec2::new(pan_x, pan_y), scale);
            self.shared.frame(&mut st);
            return;
        }
        if let Some(view) = st.view.as_mut() {
            view.gesture(Vec2::new(pivot_x, pivot_y), Vec2::new(pan_x, pan_y), scale, rotation);
            self.shared.frame(&mut st);
        }
    }

    /// How many window pixels make one interface point (the screen's density).
    pub fn set_display_scale(&self, pixels_per_point: f32) {
        if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
            let mut st = self.shared.lock();
            st.display_scale = pixels_per_point;
            self.shared.frame(&mut st);
        }
    }

    /// The surround's grid is fixed to the glass as the canvas is: its lines run from the panel's
    /// own corner, so the window gives where the middle of one of them lands (window pixels) as
    /// the display turns.
    pub fn set_grid_origin(&self, x: f32, y: f32) {
        if x.is_finite() && y.is_finite() {
            let mut st = self.shared.lock();
            st.grid_origin = Some([x, y]);
            self.shared.frame(&mut st);
        }
    }

    /// The light interface: the surround and its grid go light.
    pub fn set_light_interface(&self, on: bool) {
        let mut st = self.shared.lock();
        st.light = on;
        self.shared.frame(&mut st);
    }

    /// Fits the whole canvas in the window again, at once.
    pub fn fit_view(&self) {
        let mut st = self.shared.lock();
        st.view_motion = None;
        st.view = Some(st.fitted_view());
        self.shared.frame(&mut st);
    }

    /// Fits the whole canvas in the window again, settling into place on a spring over
    /// [`CanvasSession::view_settle_seconds`] (the view read meanwhile is where it has got to).
    pub fn settle_to_fit(&self) {
        let mut st = self.shared.lock();
        let to = st.fitted_view();
        st.view_motion =
            st.view.map(|from| ViewMotion { from, to, start: std::time::Instant::now(), curve: ViewCurve::Settle });
        if st.view_motion.is_none() {
            st.view = Some(to);
        }
        self.shared.frame(&mut st);
    }

    /// Zooms to the canvas's actual size (a canvas pixel to a window pixel) about the middle of
    /// the window, keeping its turn, settling into place as [`CanvasSession::settle_to_fit`] does.
    pub fn settle_to_actual_size(&self) {
        let mut st = self.shared.lock();
        let Some(from) = st.view else { return };
        let middle = Vec2::new(st.window_size.0 as f32 / 2.0, st.window_size.1 as f32 / 2.0);
        let Some(to) =
            from.screen_to_canvas(middle).and_then(|c| View::centred(c, 1.0, from.rotation(), st.window_size))
        else {
            return;
        };
        st.view_motion = Some(ViewMotion { from, to, start: std::time::Instant::now(), curve: ViewCurve::Settle });
        self.shared.frame(&mut st);
    }

    /// Fits the whole canvas in the window again, growing back into place over 0.3 s, slowing to
    /// a stop (as leaving crop without cropping does).
    pub fn ease_to_fit(&self) {
        let mut st = self.shared.lock();
        let to = st.fitted_view();
        st.view_motion = st.view.map(|from| ViewMotion {
            from,
            to,
            start: std::time::Instant::now(),
            curve: ViewCurve::EaseOut(VIEW_EASE_SECONDS),
        });
        if st.view_motion.is_none() {
            st.view = Some(to);
        }
        self.shared.frame(&mut st);
    }

    /// How long the view takes to settle into place.
    pub fn view_settle_seconds(&self) -> f32 {
        VIEW_SETTLE_SECONDS
    }

    /// Whether the view is still moving into place (what is drawn over the canvas follows it).
    pub fn view_moving(&self) -> bool {
        self.shared.lock().view_motion.is_some()
    }

    /// The frosted panels open over the canvas, five numbers each (left, top, right, bottom and
    /// corner radius, window pixels): what is behind them is drawn blurred.
    pub fn set_backdrops(&self, regions: Vec<f32>) {
        let (regions, _) = regions.as_chunks::<6>();
        let mut st = self.shared.lock();
        if st.backdrops != regions {
            st.backdrops = regions.to_vec();
            self.shared.frame(&mut st);
        }
    }

    /// What fitting the canvas keeps clear of from now on (left, top, right, bottom, window
    /// pixels): the interface, with Fit Canvas Inside Interface on.
    pub fn set_fit_insets(&self, left: f32, top: f32, right: f32, bottom: f32) {
        self.shared.lock().fit_insets = [left, top, right, bottom];
    }

    /// Lets go of the canvas and everything it holds, on the GPU and off it (its layers, parked
    /// or not, and a model's), once the artwork is saved and left: a large artwork's memory is
    /// free at once rather than whenever this object is collected. Nothing is drawn after.
    pub fn release_canvas(&self) {
        let mut st = self.shared.lock();
        st.closed = true;
        st.canvas = None;
        st.pending_pixels.clear();
        st.scene = None;
    }

    /// The artwork opens fitted, not as it was left (Canvas Orientation Memory off).
    pub fn forget_saved_view(&self) {
        self.shared.lock().saved_view = None;
    }

    pub fn set_tool(&self, tool: ToolKind) {
        self.shared.lock().tool = tool;
    }

    /// The current tool's size slider, 0..1.
    pub fn set_brush_size(&self, slider: f32) {
        if slider.is_finite() {
            self.shared.lock().sliders_mut().size = slider.clamp(0.0, 1.0);
        }
    }

    /// The current tool's opacity slider, 0..1.
    pub fn set_brush_opacity(&self, slider: f32) {
        if slider.is_finite() {
            self.shared.lock().sliders_mut().opacity = slider.clamp(0.0, 1.0);
        }
    }

    /// The paint colour, sRGB 0..1.
    pub fn set_color(&self, r: f32, g: f32, b: f32) {
        let c = [r, g, b].map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 });
        self.shared.lock().color = c;
    }

    /// The secondary colour, which brushes with secondary colour dynamics mix in.
    pub fn set_secondary_color(&self, r: f32, g: f32, b: f32) {
        let c = [r, g, b].map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 });
        self.shared.lock().secondary = c;
    }

    /// Undoes the last step; returns what it was called, if it was named.
    pub fn undo(&self) -> Option<String> {
        if self.swap_crop(true) {
            return Some("Crop canvas".to_owned());
        }
        self.edit(Canvas::undo);
        self.shared.lock().canvas.as_ref().and_then(Canvas::redo_name)
    }

    /// Redoes the last step undone; returns what it was called, if it was named.
    pub fn redo(&self) -> Option<String> {
        if self.swap_crop(false) {
            return Some("Crop canvas".to_owned());
        }
        self.edit(Canvas::redo);
        self.shared.lock().canvas.as_ref().and_then(Canvas::undo_name)
    }

    pub fn clear_layer(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let current = st.doc.current();
        st.finish_stroke();
        st.settle();
        if st.doc.layer(current).is_none_or(|l| l.locked || l.text.is_some()) {
            return;
        }
        // with a selection, only what is selected is cleared
        if let Some(mask) = st.selection.clone() {
            st.flush();
            let pixels = match (st.canvas.as_mut(), st.gpu.as_ref()) {
                (Some(canvas), Some(gpu)) => canvas.read_layer(gpu, current).ok(),
                _ => None,
            };
            if let (Some(mut pixels), Some(canvas)) = (pixels, st.canvas.as_mut()) {
                for (px, &m) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(&mask.data) {
                    let keep = 255 - u16::from(m);
                    px.iter_mut().for_each(|c| *c = ((u16::from(*c) * keep + 127) / 255) as u8);
                }
                canvas.write_layer(current, pixels);
                self.shared.frame(st);
            }
            return;
        }
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("clear layer");
            canvas.clear_layer(current);
            self.shared.frame(st);
        }
    }

    pub fn info(&self) -> CanvasInfo {
        let st = self.shared.lock();
        let sliders = st.shown_sliders();
        CanvasInfo {
            width: st.doc.width(),
            dpi: st.doc.dpi,
            height: st.doc.height(),
            tool: st.tool,
            size: sliders.size,
            opacity: sliders.opacity,
            brush_diameter: st.brush(st.tool).diameter(sliders.size) * st.brush_scale(),
            // an open shape is kept, then undone
            can_undo: st.canvas.as_ref().is_some_and(Canvas::can_undo) || st.crop_undo.is_some() || st.shape.is_some(),
            // keeping an open shape drops what could be redone
            can_redo: st.shape.is_none() && (st.canvas.as_ref().is_some_and(Canvas::can_redo) || st.crop_redo_ready()),
            zoom: st.view.map_or(1.0, |v| v.zoom()),
            frames: st.frames,
            frame_interval_ms: st.frame_interval_ms,
            error: st.error.clone(),
            revision: st.revision,
        }
    }
}

impl CanvasSession {
    /// Undo or redo, after finishing any open stroke; done at once, so the document is current
    /// when this returns.
    fn edit(&self, f: fn(&mut Canvas)) {
        // text being edited is kept first: it is the step undone
        self.text_end(true);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        // painting an adjustment on, undo and redo step through the strokes and it stays open
        if st.adjusting && st.pencil {
            st.restart_painted_on(Some(f));
            st.flush();
            st.revision += 1;
            self.shared.frame(st);
            return;
        }
        st.settle();
        if let Some(canvas) = st.canvas.as_mut() {
            f(canvas);
            st.flush();
            st.sync_selection();
            st.revision += 1;
            self.shared.frame(st);
        }
    }
}

impl Drop for CanvasSession {
    fn drop(&mut self) {
        self.detach_window();
    }
}
