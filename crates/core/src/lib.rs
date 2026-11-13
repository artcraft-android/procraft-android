//! The session both ProCraft apps drive: a [`CanvasSession`] takes input from the interface, builds
//! strokes with the engine and draws them with the GPU renderer. The Android app reaches it through
//! UniFFI's Kotlin bindings, and it draws on its own render thread straight into the app's
//! `SurfaceView`; the desktop app calls it directly and draws each frame into its window
//! (`host.rs`).
//!
//! On Android the window crosses over through two small JNI functions (`window.rs`, the crate's
//! only unsafe code with `layer.rs`): Kotlin hands its `Surface` to `NativeWindows.acquire` and
//! passes the id it returns to [`CanvasSession::attach_window`]. The iPad app (UniFFI's Swift
//! bindings) hands its canvas view's `CAMetalLayer` to `acquire_layer` (`layer.rs`) the same way,
//! and the same render thread draws into it.

uniffi::setup_scaffolding!();

mod adjust;
mod adjust_menu;
mod adjusting;
mod animation;
mod brush_files;
mod brush_libraries;
mod brush_library;
mod brush_pictures;
mod brushes;
mod bundled;
mod clipboard;
mod colour;
mod colour_blob;
mod colour_drag;
mod colour_models;
mod crop;
mod crop_edit;
mod device;
mod frame;
mod gallery;
mod gestures;
mod gradients;
mod grain_store;
mod guide;
mod history;
mod hooks;
mod host;
mod hues;
mod interface_size;
mod keys;
#[cfg(target_os = "ios")]
mod layer;
mod layer_ops;
mod layers;
mod liquify;
mod modes;
mod palettes;
mod pick;
mod pickers;
mod plugin_artwork;
mod plugin_canvas;
mod plugin_commands;
mod plugin_file;
mod plugin_filter;
mod plugin_layers;
mod plugin_panel;
mod plugin_run;
mod plugins;
mod pour;
mod presets;
mod project;
mod projection;
mod quick_menu;
mod quickshape;
mod recolor;
mod reference_view;
mod reference_window;
#[cfg(any(target_os = "android", target_os = "ios"))]
mod render;
mod reveal;
mod scene3d;
mod selection;
mod sharp;
mod sound_file;
mod stroke_sound;
mod studio;
#[cfg(test)]
mod tests;
mod text;
mod text_draw;
mod timelapse;
mod touch;
mod transform;
mod tween;
#[cfg(target_os = "android")]
mod window;
// the iPad app's window is its layer: taken and drawn into as Android's is
#[cfg(target_os = "ios")]
use layer as window;

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
use procraft_gpu::{Canvas, Gpu, SecondStroke, StrokeStyle, StrokeTool};

pub use adjust::{
    ChromaticKind, GlitchKind, HalftoneKind, LiquifyBrush, LiquifyKind, NoiseKind, PerspectiveKind, SwipeAdjustment,
};
pub use adjust_menu::{
    Adjustment, adjustment_groups, adjustment_label, adjustment_paintable, adjustment_pencil_amount, adjustment_swiped,
    quick_action_adjustment,
};
pub use adjusting::{AdjustShown, CLONE_PUCK_RADIUS};
pub use animation::{
    AnimationPlayback, AnimationSettings, FrameRate, PlayStep, animation_next_step, export_fps_at, export_fps_fraction,
    export_fps_label, frame_rate,
};
pub use brush_files::{BrushesRead, OTHER_BRUSH_EXTENSIONS, ReadBrushes, is_brush_zip, read_any_brushes, read_brushes};
pub use brush_libraries::BrushLibraryInfo;
pub use brush_library::prepare_brushes;
pub use brushes::{BrushInfo, BrushSetInfo, RecentBrush};
pub use colour::{
    cmyk_profiles, color_profiles, colours_in_p3, convert_color, import_profile, is_cmyk_profile, set_profile_folder,
};
pub use colour_blob::{BlobOutline, BlobShape, ColourBlob};
pub use colour_drag::{ColourDrag, DragStep};
pub use colour_models::{
    ColourModel, ColourPlace, ColourTrack, MODEL_SQUARE, OKLAB_AB, colour_model_along, colour_model_disc_key,
    colour_model_disc_pick, colour_model_disc_picture, colour_model_disc_point, colour_model_hsv, colour_model_key,
    colour_model_label, colour_model_letters, colour_model_of_key, colour_model_pure, colour_model_rgb,
    colour_model_ring, colour_model_ring_hue, colour_model_ring_turn, colour_model_slid, colour_model_square_picture,
    colour_model_square_side, colour_model_tracks, colour_models, oklab_of, oklab_of_hsv, oklab_slid, oklab_to_rgb,
    oklab_tracks, oklab_value_slid, oklab_value_tracks,
};
pub use crop_edit::CropShown;
pub use device::keep_device;
pub use frame::{configure, screen_profile};
pub use gallery::{ArtworkStore, GalleryArtwork, GalleryEntry, ZipKind, zip_file_kind};
pub use gestures::{
    GestureOption, GesturePurpose, GestureSettings, KeptGestures, MAX_DELAY_S, MIN_DELAY_S, Trigger, gesture_options,
    gesture_purpose_heading, gesture_purpose_title, gesture_purposes, gesture_trigger_label,
};
pub use gradients::{GradientInfo, GradientStop, Gradients};
pub use guide::{GuideInfo, GuideType, GuideUnit, SymmetryType};
pub use history::ColourHistory;
pub use hues::{HueControl, RING_HUES, SLIDER_HUES, hue_at, hue_position, hue_stops};
pub use keys::{KeyCombo, KeyDrag, KeyShortcut, ShortcutGroup, ShortcutRow, key_shortcut, key_shortcut_list};
pub use layers::{
    BlendModeInfo, Blocked, HiddenBy, LayerInfo, LayerOption, LayerOptionRow, LayerType, ThumbnailImage, blend_modes,
    brush_blend_modes, combine_blend_modes, grain_blend_modes, layers_for, set_layer_budget, set_resident_budget,
};
pub use liquify::liquify_reach;
pub use modes::{ModeButton, ModeSwitch, Modes, mode_switch};
pub use palettes::{PaletteInfo, Palettes, colour_name};
pub use pick::Pick;
pub use pickers::{
    disc_picture, disc_to_square, disc_to_square_point, harmony_wheel, p3_hsv_of, p3_hsv_to_rgb, rgb_to_p3_hsv,
    square_to_disc, square_to_disc_point, triangle_picture, triangle_point, triangle_sv,
};
pub use plugin_canvas::{BadgeLine, PluginBadge, PluginCanvas, PluginExport, PluginKind};
pub use plugin_file::read_plugin_file;
pub use plugin_filter::PluginFilterShown;
pub use plugin_panel::{PanelRow, PluginPanel};
pub use plugins::{
    PluginAction, PluginControl, PluginInfo, PluginOffer, PluginPart, PluginPlace, PluginSlider, PluginSource,
    PluginSwitch, Plugins, plugin_file_most_bytes, plugin_slider_along, plugin_slider_at, plugin_slider_shown,
    plugin_slider_whole,
};
pub use presets::{
    CanvasPreset, P3, SRGB, per_inch, preset_height, preset_label, preset_number, preset_width, presets_from_json,
    presets_json, profile_chip, standard_presets,
};
pub use project::{ArtworkInfo, export_file_name, read_signature, save_failure_said, write_photo, write_signature};
pub use project::{PackageTimeLapse, artwork_plays, package_time_lapse};
pub use project::{PagesOutcome, PsdOutcome, import_pages, import_psd};
pub use quick_menu::{
    QuickAction, QuickMenuSet, quick_action_adjusts, quick_action_label, quick_actions_listed,
    quick_adjustments_listed, quick_menu_active_after, quick_menu_added, quick_menu_saved, quick_menu_slots,
    quick_menus, quick_menus_saved,
};
pub use quickshape::{QuickShapeAnchor, QuickShapeInfo, QuickShapeKind};
pub use reference_view::{
    ReferenceSizes, ReferenceView, reference_picture_point, reference_picture_rect, reference_view_pinched,
};
pub use reference_window::{
    ReferencePlace, ReferenceShows, ReferenceSize, ReferenceWindowState, reference_window_fitted, reference_window_size,
};
pub use scene3d::{
    ModelFile, ModelInfo, ModelLight, ModelTextureFile, ModelWarning, SubmeshInfo, Turntable, model_warning_name,
    unpack_model_artwork, zip_model_name,
};
pub use selection::{SelectionDisplay, SelectionJoin};
pub use studio::{
    SliderExtrasInfo, SourceImage, SourceKind, StudioAbout, curve_samples, repeatable_source, slider_extras,
    small_source, source_from_copy, source_from_picture, source_library, source_size,
};
pub use text::{TextAlignment, TextDraw, TextDrawn, TextInfo, TextSide};
pub use text_draw::{
    TextPicture, TextTypeface, add_system_text_fonts, add_text_font, add_text_font_file, draw_chosen_text, draw_text,
    import_text_font, set_text_family_alias, set_text_font_folder, text_box, text_caret, text_families,
    text_index_from_utf16, text_index_to_utf16, text_styles, text_typefaces,
};
pub use timelapse::{
    SegmentPlan, TIME_LAPSE_FPS, TimeLapse, TimeLapseSettings, time_lapse_bits_per_pixel, time_lapse_picks,
    time_lapse_setting_names, time_lapse_side,
};
pub use touch::{
    COMPANION_ZOOM_DISTANCE, CompanionMode, Fill, GestureAction, HoverCursor, PenHover, PenPreferred, SelectKind,
    SelectTouch, SqueezePhase, TouchCommand, TouchSettings, TransformTouch,
};
pub use transform::{
    GuideSettings, Interpolation, ScreenBox, TransformAction, TransformBox, TransformHandle, TransformKind, handle_at,
    mesh_handle_at, screen_box, shown_mesh_points,
};

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
    /// Radians the pen's barrel is turned (a brush set to azimuth and barrel roll turns with it);
    /// 0 for a pen that doesn't say.
    #[uniffi(default = 0.0)]
    pub roll: f32,
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

/// What a stroke paints with besides its brush: its colours, and whether its layer's alpha is locked.
#[derive(Clone, Copy, Debug)]
struct StrokePaint {
    colour: [f32; 3],
    secondary: [f32; 3],
    alpha_lock: bool,
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
    /// A mouse dragged with a key held, moving the view (keys.rs).
    pub(crate) key_drag: Option<keys::KeyDragging>,
    pub(crate) window_size: (u32, u32),
    pub(crate) reconfigure: bool,
    tool: ToolKind,
    /// The tool chosen before this one (not one a stroke only borrowed).
    previous_tool: Option<ToolKind>,
    /// The app's own smoothing and pressure, on top of every brush's.
    pub(crate) input: procraft_engine::stroke::InputSettings,
    /// Selection mask visibility, 0..1: how strongly what is not selected is banded.
    pub(crate) selection_visibility: f32,
    /// Brush scaling relative to the screen: the brush keeps its size on the screen as the canvas
    /// zooms (else it keeps its size on the canvas).
    pub(crate) brush_on_screen: bool,
    /// What samples go through on their way to a stroke, if anything.
    pub(crate) hooks: Option<Arc<hooks::Hooks>>,
    /// A plugin's own artwork shown here, if this session shows one: its strokes go to it.
    pub(crate) plugin_artwork: Option<plugin_artwork::PluginArtwork>,
    /// Opened from a folder holding a plugin's artwork: its kind and its data, until the app opens
    /// it with its plugin.
    pub(crate) plugin_opened: Option<plugin_artwork::PluginOpened>,
    /// The picker plugin's picture, compiled for this session's GPU: whose it is, and its code.
    pub(crate) picker_program: Option<(String, Arc<str>, Arc<procraft_gpu::PictureProgram>)>,
    /// When the stroke's input was last heard, for a sound plugin.
    pub(crate) sound_heard: Option<std::time::Instant>,
    /// Where the pen last hovered (window pixels) and when, for the spray can's rattle.
    pub(crate) sound_hover: Option<(procraft_engine::geom::Vec2, std::time::Instant)>,
    /// Crops kept for undo: the canvas and document from before; and one undone, for redo, with
    /// the steps the canvas then put back had made (redo only while that hasn't changed).
    crop_undo: Option<(Canvas, Document)>,
    crop_redo: Option<(Canvas, Document, u64)>,
    /// The text layer being edited, the document as the edit began, and the width changes made
    /// inside it.
    text_edit: Option<text::TextEdit>,
    /// Where the edit last drew its text into the layer (none: not yet, on a layer already there,
    /// so the next draw takes the whole layer).
    text_drawn: Option<procraft_engine::geom::IRect>,
    /// The steps finished text edits made (by [`Canvas::undo_step`]'s numbering) and their layers:
    /// undo and redo of one go back to editing that text.
    pub(crate) text_steps: Vec<(u64, procraft_engine::document::LayerId)>,
    /// The stroke being drawn on a plugin's layer, recorded as it goes.
    pub(crate) recording: Option<plugin_layers::Recording>,
    /// The changes strokes made to plugins' layers' data, by the step each made, for undo and redo.
    pub(crate) layer_steps: Vec<plugin_layers::LayerStep>,
    /// Plugins' layers as their items were last laid: where each item reached, so an edit lays
    /// again only what it changes.
    pub(crate) laid: std::collections::HashMap<procraft_engine::document::LayerId, plugin_layers::Laid>,
    /// How many times plugins' layers were laid again whole, and how many times only in part.
    pub(crate) lays: [u64; 2],
    /// Plugins' layers drawn sharp over the canvas when zoomed in.
    pub(crate) sharp: sharp::Sharp,
    /// While items are laid on the sharp picture's canvas: canvas pixels to its pixels.
    pub(crate) lay_to: Option<procraft_engine::geom::Affine>,
    /// Plugins' layers whose data couldn't be read (or their plugin couldn't take it): they take no
    /// strokes, and their data files are left as they are.
    pub(crate) layer_data_unread: std::collections::HashSet<procraft_engine::document::LayerId>,
    /// A colour dropped on the canvas whose threshold can still change.
    pub(crate) pour: Option<pour::Pour>,
    /// A dropped colour's fill spreading in, while it does.
    pub(crate) reveal: Option<reveal::Reveal>,
    /// The system asks for less motion: a dropped colour's fill shows at once.
    pub(crate) reduce_motion: bool,
    /// Recolor, while it is on.
    pub(crate) recolor: Option<recolor::Recolor>,
    /// The library in use as the user has it, and as it ships; the libraries.
    library: Vec<brush_library::LibrarySet>,
    pub(crate) libraries: brush_libraries::Libraries,
    shipped: Vec<BrushSet>,
    /// When the library is empty (never, but never panic).
    fallback: Brush,
    /// The brush each tool uses: (set, brush) in the library.
    paint_brush: (usize, usize),
    erase_brush: (usize, usize),
    smudge_brush: (usize, usize),
    /// Each set's last chosen brush, (set, brush) in the library, the latest choice last.
    set_choices: Vec<(usize, usize)>,
    /// The Recent set: the brushes last painted with (at most [`RECENT_BRUSHES`], the latest
    /// last) and those pinned there (in the order they were pinned), by place in the library.
    recent_brushes: Vec<(usize, usize)>,
    pinned_brushes: Vec<(usize, usize)>,
    /// The brushes QuickMenu buttons are set to, each by a key the button keeps (never 0).
    quick_brushes: Vec<(u64, (usize, usize))>,
    /// Each brush's sliders, per tool.
    sliders: HashMap<(ToolKind, usize, usize), Sliders>,
    /// Strokes started, to seed each one's randomness.
    strokes: u32,
    /// The small canvas brush previews are drawn on.
    preview: Option<(Canvas, Document)>,
    color: [f32; 3],
    /// The secondary colour, for secondary colour dynamics.
    secondary: [f32; 3],
    /// The app's colour history, which paint strokes add to.
    colour_history: Option<std::sync::Arc<ColourHistory>>,
    stroke: Option<StrokeBuilder>,
    /// The open stroke's second, a dual brush's (drawn along the same input).
    second: Option<StrokeBuilder>,
    #[cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]
    pub(crate) gpu: Option<Gpu>,
    pub(crate) canvas: Option<Canvas>,
    pub(crate) needs_frame: bool,
    /// A frame wanted at this time, not before: the selection's bands drift slowly, so a frame
    /// shows them moved only every so often (a pixel at a time).
    pub(crate) frame_due: Option<std::time::Instant>,
    /// Frames wait (the canvas is being redrawn a stroke at a time, as the brush studio's pad is
    /// when its brush changes), and one was asked for meanwhile.
    pub(crate) holding_frames: bool,
    pub(crate) frame_held: bool,
    pub(crate) stop: bool,
    /// Project canvas: the second display's size, and that its thread should end.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) projection_size: (u32, u32),
    pub(crate) projection_stop: bool,
    pub(crate) frames: u64,
    /// The drawing pad's strokes as drawn (pointer and input), drawn again when the brush changes;
    /// `None` on a canvas that keeps none.
    pub(crate) pad_strokes: Option<Vec<(PointerKind, Vec<PenSample>)>>,
    /// Brush files' grains at their own size.
    pub(crate) grain_store: grain_store::GrainStore,
    /// Where the library was last kept or read whole, and what it held then (see
    /// `brush_library::Images::Told`): kept again only once that changes.
    pub(crate) library_kept: Option<(std::path::PathBuf, u64)>,
    /// Plugins' changes in progress (an action's or a panel touch's): while any is, no stroke
    /// begins and undo and redo wait, or they would join its step.
    pub(crate) plugin_stepping: u32,
    /// The pad shows only its sample scribble: a stroke of one's own takes it away.
    pub(crate) pad_sample: bool,
    /// The pad drawn as a sphere, the flat pad's strokes set aside meanwhile.
    pub(crate) sphere: Option<crate::studio::SpherePad>,
    pub(crate) frame_interval_ms: f32,
    pub(crate) error: Option<String>,
    /// Window pixels per point at the screen's density: the canvas and gestures go by it.
    pub(crate) display_scale: f32,
    /// How big the interface shows beside that (1: as measured).
    pub(crate) interface_size: f32,
    /// The canvas shown in perceptual greys (the window's view only).
    pub(crate) grey_view: bool,
    /// The eyedropper picks from the current layer alone, not the canvas as composited.
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
    /// Since when time on the artwork counts, while it does (see [`CanvasSession::set_tracking`]):
    /// not yet in `info`.
    pub(crate) tracking_since: Option<std::time::Instant>,
    /// Saved layer pixels waiting for the canvas (an opened artwork before it is shown).
    pub(crate) pending_pixels: Vec<(procraft_engine::document::LayerId, project::Opened)>,
    /// The canvas couldn't be made from these pixels (a failure elsewhere doesn't stop the next try).
    pub(crate) canvas_failed: bool,
    /// Layers whose saved files couldn't be read (shown empty): a save leaves each file as it is
    /// until the layer is painted on, and then keeps the file aside before writing the new one.
    pub(crate) unreadable: std::collections::HashSet<procraft_engine::document::LayerId>,
    /// Those of them not told of yet (see [`CanvasSession::take_failure`]).
    pub(crate) unreadable_untold: Vec<procraft_engine::document::LayerId>,
    /// Let go of (see [`CanvasSession::release_canvas`]): no canvas is made again.
    pub(crate) closed: bool,
    /// A model open to paint on, in place of the flat canvas (the document is its current
    /// texture set's).
    pub(crate) scene: Option<scene3d::Scene>,
    /// A selection drawn on a model reaches every surface under it, not only the nearest.
    pub(crate) x_ray: bool,
    /// What fitting the canvas keeps clear of (left, top, right, bottom, window pixels).
    fit_insets: [f32; 4],
    /// Frosted panels open over the canvas: left, top, right, bottom and corner radius (window
    /// pixels).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) backdrops: Vec<[f32; 6]>,
    /// Where the glass over the canvas is thin (the side bar's slider tracks), the same way.
    pub(crate) backdrop_windows: Vec<[f32; 6]>,
    /// The view kept with the artwork, shown when the window first comes.
    pub(crate) saved_view: Option<project::SavedView>,
    /// The Reference window as the artwork keeps it, and its imported picture (a PNG, upright):
    /// apart from the document, so that undoing never moves it.
    pub(crate) reference_window: procraft_engine::reference_window::ReferenceWindow,
    pub(crate) reference_picture: Option<Arc<Vec<u8>>>,
    pub(crate) revision: u64,
    /// The open transform, how its handles act and how it resamples.
    transform: Option<procraft_engine::transform::Transform>,
    /// A plugin's layer being transformed: the layer and the ids of the items moving, which are
    /// moved as items as it ends; and whether its pixels were as its items were last laid as it
    /// began (the transform puts them back so before they move).
    transform_items: Option<(procraft_engine::document::LayerId, Vec<u32>, bool)>,
    transform_mode: procraft_engine::transform::TransformMode,
    interpolation: transform::Interpolation,
    transform_guides: transform::GuideSettings,
    /// An open transform's layers, while what its box snaps onto (the other layers' bounds) isn't
    /// found yet: snapping was off as it began; found when snapping comes on.
    snap_pending: Option<Vec<procraft_engine::document::LayerId>>,
    /// What is selected, if anything, and selections kept to load again.
    selection: Option<procraft_engine::selection::Mask>,
    saved_selections: Vec<procraft_engine::selection::Mask>,
    pub(crate) selection_display: selection::SelectionDisplay,
    /// The table showing a canvas with a profile of its own on the screen, and what it was made for.
    pub(crate) display_lut: Option<((usize, procraft_engine::profile::Profile), procraft_gpu::DisplayLut)>,
    /// While a colour selection's threshold is dragged: the selection before it, and the layer read once.
    selection_before: Option<Option<procraft_engine::selection::Mask>>,
    colour_source: Option<(Vec<u8>, procraft_engine::selection::ColourFlood)>,
    /// An adjustment is open on the current layer, and what it last drew.
    adjusting: bool,
    /// The open adjustment's values, as the apps show them (adjusting.rs).
    adjust: adjusting::Adjusting,
    filters: plugin_filter::Filters,
    /// Crop and Resize as it is edited.
    crop_edit: crop_edit::CropEdit,
    last_filter: Option<procraft_gpu::Filter>,
    /// The liquify brush as it goes.
    liquify: liquify::LiquifyRun,
    /// The brush studio is open on this (set, brush), and the brush as it opened.
    studio: Option<((usize, usize), Brush)>,
    /// It shows a dual brush's second, not its first.
    studio_second: bool,
    /// The adjustment is painted on (Pencil); cloning, from where, and by how far once it starts.
    pencil: bool,
    cloning: bool,
    /// Leaving an adjustment makes the steps made in it (Pencil and clone strokes) one.
    simplified_undos: bool,
    /// The clone brush's sliders (it is the soft airbrush).
    clone_sliders: Sliders,
    clone_source: Option<Vec2>,
    clone_offset: Option<Vec2>,
    /// The selection before the feather popover softened it, and the room its feathering works
    /// in (kept while the popover is open, so each step doesn't ask the system for it again).
    feather_base: Option<(procraft_engine::selection::Mask, Vec<f32>)>,
    /// The last drag step on the box (window point, when), for its speed.
    last_drag: Option<(Vec2, std::time::Instant)>,
    /// The stroke being drawn as input (canvas points), for QuickShape,
    /// and the shape it snapped to while that is shown.
    drawn: Vec<Vec2>,
    /// How long it was on the move, and when it was last heard from (milliseconds).
    drawn_moving: f64,
    drawn_at: f64,
    /// Its input as the stroke took it, for a shape to go back to.
    drawn_samples: Vec<Sample>,
    shape: Option<quickshape::ShapeEdit>,
    /// The open stroke as it began, and the last stroke kept, for QuickShape afterwards.
    began: Option<quickshape::LastStroke>,
    last_stroke: Option<quickshape::LastStroke>,
    /// The last shape kept, which undo gives back the stroke it was made of; and the shape whose
    /// stroke that gave back, which redo makes a shape again.
    kept_shape: Option<quickshape::KeptShape>,
    unkept_shape: Option<quickshape::KeptShape>,
    /// Strokes follow the drawing guide even where drawing assist is off (□ held down).
    force_assist: bool,
    /// Where the stroke began (canvas pixels), for assisted drawing; the guide and view as the
    /// guide editor opened.
    assist_from: Option<Vec2>,
    /// The direction (radians) an assisted stroke is kept to, once it has gone far enough to
    /// tell: chosen then and kept to its end, so a wavering hand doesn't switch it.
    assist_angle: Option<f32>,
    pub(crate) guide_before: Option<(procraft_engine::guide::Guide, Option<View>)>,
}

impl State {
    /// Window pixels per point of what the interface draws, its handles over the canvas too.
    pub(crate) fn interface_scale(&self) -> f32 {
        self.display_scale * self.interface_size
    }

    /// The surround's grid in window pixels, shifted by `by` (where the window's corner lands on
    /// the target).
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn surround_grid(&self, by: Vec2) -> procraft_gpu::SurroundGrid {
        let s = self.display_scale;
        let [x, y] = self.grid_origin.unwrap_or([0.5 * s, 0.5 * s]);
        procraft_gpu::SurroundGrid {
            spacing: GRID_SPACING_PT * s,
            origin: [x + by.x, y + by.y],
            top: GRID_TOP_PT * self.interface_scale() + by.y,
            width: GRID_WIDTH_PT * s,
            color: if self.light { GRID_LINE_LIGHT } else { GRID_LINE },
        }
    }

    /// The colour around the canvas.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(crate) fn surround(&self) -> [f32; 3] {
        if self.light { SURROUND_LIGHT } else { SURROUND }
    }

    /// Places the canvas in the window the first time: the view kept with the artwork, unless it
    /// would leave the canvas off the page, else fitted.
    pub(crate) fn place_view(&mut self) {
        if self.view.is_none() {
            let canvas = (self.doc.width(), self.doc.height());
            let kept = self
                .saved_view
                .take()
                .and_then(|v| v.view(self.window_size))
                .filter(|v| project::shows_canvas(v, self.window_size, canvas));
            self.view = Some(kept.unwrap_or_else(|| self.fitted_view()));
        }
    }

    /// The whole canvas fitted in the window, clear of the insets, turned by the right angle the
    /// view is nearest (fitting straightens a turned canvas, but keeps it on its side if it is).
    pub(crate) fn fitted_view(&self) -> View {
        let turns = self.view.map_or(0, |v| v.quarter_turns());
        View::fit_turned((self.doc.width(), self.doc.height()), self.window_size, self.fit_insets, turns)
    }

    /// Finishes an open stroke.
    fn finish_stroke(&mut self) {
        let shape = self.shape.take();
        // a stroke on a model ends there, as the pen lifting would end it
        if self.scene_shown() && self.scene.as_ref().is_some_and(|s| s.stroking) {
            self.scene_stroke_add(&[], true);
            return;
        }
        // one on a plugin's layer goes to the plugin as it ends (made a shape, as that: its geometry
        // kept, laid along its outline); undo takes it back whole, not to the stroke it was
        if let Some(mut recording) = self.recording.take()
            && let Some(open) = self.stroke.take()
        {
            if let Some(edit) = &shape {
                let (kind, nodes) = edit.geometry();
                recording.made_shape(&edit.samples(), kind, &nodes);
            }
            let second = self.second.take();
            self.end_recorded(recording, open, second);
            return;
        }
        if let (Some(open), Some(canvas)) = (self.stroke.take(), self.canvas.as_mut()) {
            close_stroke(canvas, open, self.second.take(), &self.doc.stroke_guide());
            self.bake_painted_on();
        }
        self.note_kept_shape(shape);
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
        let paint = StrokePaint {
            colour: self.color,
            secondary: self.secondary,
            alpha_lock: self.doc.layer(layer).is_some_and(|l| l.alpha_lock),
        };
        let preview = self.pad_strokes.is_some();
        self.stroke_brush_from(self.brush(tool).clone(), self.sliders(tool), tool, paint, preview)
    }

    /// [`State::stroke_brush`] for `brush` at `sliders`, painting as `paint` says (`preview`: on the
    /// brush studio's pad).
    pub(crate) fn stroke_brush_from(
        &mut self,
        brush: Brush,
        sliders: Sliders,
        tool: ToolKind,
        paint: StrokePaint,
        preview: bool,
    ) -> (Brush, StrokeStyle, f32) {
        let mut brush = brush;
        brush.touch_opacity = sliders.opacity.clamp(0.0, 1.0);
        // a blending brush's opacity lightens each stamp, as its flow does, so its strokes still
        // build up to full; a glaze's caps the stroke
        let opacity = if brush.blends() && matches!(tool.stroke_tool(), StrokeTool::Paint | StrokeTool::Erase) {
            brush.flow *= sliders.opacity.clamp(0.0, 1.0).powf(procraft_engine::brush::FLOW_POWER);
            1.0
        } else {
            brush.opacity(sliders.opacity)
        };
        let diameter = brush.diameter(sliders.size);
        let mut style = StrokeStyle::for_brush(&brush, tool.stroke_tool(), paint.colour, opacity, paint.alpha_lock);
        style.secondary = paint.secondary;
        style.preview = preview;
        style.wetness = StrokeStyle::wetness_at(sliders.opacity);
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.use_sources(gpu, &mut style, &brush);
        }
        // a brush file's grain at its own size, fixed to the canvas or moving with each stamp
        let full = brush.grain_file_now().and_then(|f| f.full.as_ref());
        if let Some(full) = full
            && let Some(pixels) = self.grain_store.pixels(full)
            && let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref())
        {
            canvas.use_full_grain(gpu, &mut style, full.id, full.side, &pixels);
        }
        self.use_full_shape(&mut style, &brush);
        (brush, style, diameter)
    }

    /// `style` reads `brush`'s shape from its file's image at its own size, if it has one.
    fn use_full_shape(&mut self, style: &mut StrokeStyle, brush: &Brush) {
        if let Some(full) = brush.shape_file_now().map(|f| &f.full)
            && let Some(pixels) = self.grain_store.pixels(full)
            && let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref())
        {
            canvas.use_full_shape(gpu, style, full.id, full.side, &pixels);
        }
    }

    /// A dual brush's second for a stroke of `tool` on `layer` with `first` (as
    /// [`State::stroke_brush`] gave it, with `first_style`): its brush, how it is drawn and
    /// combines, and its diameter at the size slider (before the brush scale). None for a brush
    /// without one, or a smudge or wet paint (drawn in place).
    pub(crate) fn second_brush(
        &mut self,
        tool: ToolKind,
        layer: procraft_engine::document::LayerId,
        first: &Brush,
        first_style: &StrokeStyle,
    ) -> Option<(Brush, SecondStroke, f32)> {
        let paint = StrokePaint {
            colour: self.color,
            secondary: self.secondary,
            alpha_lock: self.doc.layer(layer).is_some_and(|l| l.alpha_lock),
        };
        self.second_brush_from(tool, first, first_style, self.sliders(tool), paint)
    }

    /// [`State::second_brush`] at `sliders`, painting as `paint` says.
    pub(crate) fn second_brush_from(
        &mut self,
        tool: ToolKind,
        first: &Brush,
        first_style: &StrokeStyle,
        sliders: Sliders,
        paint: StrokePaint,
    ) -> Option<(Brush, SecondStroke, f32)> {
        let mut second = first.secondary.as_deref()?.clone().as_second();
        if !first_style.takes_second() || !matches!(tool.stroke_tool(), StrokeTool::Paint | StrokeTool::Erase) {
            return None;
        }
        let opacity = if second.blends() {
            second.flow *= sliders.opacity.clamp(0.0, 1.0).powf(procraft_engine::brush::FLOW_POWER);
            1.0
        } else {
            second.opacity(sliders.opacity)
        };
        let diameter = second.diameter(sliders.size);
        let mut style =
            StrokeStyle::for_brush(&second, tool.stroke_tool(), paint.colour, opacity, paint.alpha_lock).as_second();
        if style.in_place() {
            return None;
        }
        style.secondary = paint.secondary;
        style.preview = first_style.preview;
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.use_sources(gpu, &mut style, &second);
        }
        self.use_full_shape(&mut style, &second);
        let mode = first.setting("dual_blend").round().max(0.0) as u32;
        Some((second, SecondStroke { style, mode, threshold: style.alpha_threshold }, diameter))
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
        let brush = self.brush(tool);
        // where its file left them, else its preview size
        if let Some(&Some([size, opacity])) = brush.sliders.get(brushes::notch_tool(tool)) {
            return Sliders { size, opacity };
        }
        let curve = procraft_engine::brush::SIZE_CURVE;
        let size = brush.setting("preview_size").clamp(0.0, 1.0).powf(1.0 / curve);
        Sliders { size, ..DEFAULT_SLIDERS }
    }

    /// Brush `index` of `set` (`brush`) holding where its sliders stand for each tool it has been
    /// used with, to be written out.
    pub(crate) fn with_sliders(&self, brush: &Brush, set: usize, index: usize) -> Brush {
        let mut brush = brush.clone();
        for tool in [ToolKind::Paint, ToolKind::Smudge, ToolKind::Erase] {
            if let (Some(s), Some(kept)) =
                (self.sliders.get(&(tool, set, index)), brush.sliders.get_mut(brushes::notch_tool(tool)))
            {
                *kept = Some([s.size, s.opacity]);
            }
        }
        brush
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

    /// Layers `ids`' files couldn't be read: they show empty, saves leave their files, and the
    /// app is told.
    pub(crate) fn layers_unread(&mut self, ids: Vec<procraft_engine::document::LayerId>) {
        for id in ids {
            if self.unreadable.insert(id) {
                self.unreadable_untold.push(id);
            }
        }
    }

    /// Creates the canvas once there is a GPU.
    #[cfg_attr(not(any(target_os = "android", test)), allow(dead_code))]
    pub(crate) fn ensure_canvas(&mut self) {
        // one that couldn't be made isn't made again: what was opened stays, and a save writes it
        if self.canvas.is_some() || self.closed || self.canvas_failed {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else { return };
        let (w, h) = (self.doc.width(), self.doc.height());
        let (pixels, unread) = project::opened_pixels(std::mem::take(&mut self.pending_pixels), w, h);
        // (its fields, as the GPU is borrowed: see [`State::layers_unread`])
        for id in unread {
            if self.unreadable.insert(id) {
                self.unreadable_untold.push(id);
            }
        }
        let doc = &self.doc;
        // every layer up as it was read, or no canvas: one the GPU had no memory for would show
        // empty, and be saved so once painted on
        let made = gpu.checked(|| -> Result<Canvas, procraft_gpu::GpuError> {
            let mut c = Canvas::unfilled(gpu, doc)?;
            c.set_resident_budget(crate::layers::resident_budget());
            for (id, pixels) in &pixels {
                // past what fits, kept compressed until needed
                if c.has_room() { c.upload_layer(gpu, *id, pixels) } else { c.park_pixels(*id, pixels) }?;
            }
            c.fill_rest(gpu, doc);
            // as read: not changed
            c.take_changed();
            Ok(c)
        });
        match made.and_then(|made| made) {
            Ok(c) => self.canvas = Some(c),
            Err(e) => {
                log::error!("canvas: {e}");
                self.error = Some(e.to_string());
                self.canvas_failed = true;
                self.pending_pixels = pixels.into_iter().map(|(id, px)| (id, project::Opened::Pixels(px))).collect();
            }
        }
    }

    /// Samples in canvas pixels.
    fn to_canvas(&self, samples: &[PenSample]) -> Vec<Sample> {
        let Some(inv) = self.view.and_then(|v| v.to_screen().inverse()) else { return Vec::new() };
        samples
            .iter()
            .map(|s| {
                let (x, y) = self.hooks.as_ref().map_or((s.x, s.y), |hooks| hooks.position(s.x, s.y));
                Sample {
                    pos: inv.apply(Vec2::new(x, y)),
                    pressure: s.pressure,
                    tilt: s.tilt,
                    azimuth: s.orientation,
                    roll: s.roll,
                    time_ms: s.time_ms,
                }
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
    second: Option<&mut StrokeBuilder>,
    samples: &[Sample],
    guide: &procraft_engine::guide::Guide,
) -> bool {
    // a dual brush's second, as its stamps come (no tip of its own)
    if let Some(second) = second {
        canvas.add_second_stamps(&guide::mirrored(guide, &second.add(samples)));
    }
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
    show_ahead(canvas, builder, &[], guide);
    !stamps.is_empty()
}

/// Draws `samples`, the last of the open stroke's, as the pen lifts: their stamps, unless the stroke
/// shows its tip (it lays them with the rest of it as it ends); and a dual brush's second takes them.
fn lift_samples(
    canvas: &mut Canvas,
    builder: &mut StrokeBuilder,
    second: Option<&mut StrokeBuilder>,
    samples: &[Sample],
    guide: &procraft_engine::guide::Guide,
) {
    let stamps = builder.add(samples);
    if !(canvas.shows_tip() && builder.shows_tip()) {
        canvas.add_stamps(&guide::mirrored(guide, &stamps));
    }
    if let Some(second) = second {
        second.add(samples);
    }
}

/// Shows what is ahead of the open stroke (`samples` being predicted input) until the next input:
/// as its tip where the canvas takes one, on to the input itself, so it looks as the stroke will
/// (its grain, build-up and colour, under the layers above it and in its layer's blend); else
/// drawn over the canvas, and only as far as the path has got.
fn show_ahead(
    canvas: &mut Canvas,
    builder: &mut StrokeBuilder,
    samples: &[Sample],
    guide: &procraft_engine::guide::Guide,
) {
    if canvas.shows_tip() {
        canvas.set_tip(&guide::mirrored(guide, &builder.preview_to_input(samples)));
    } else {
        canvas.set_prediction(&guide::mirrored(guide, &builder.preview(samples)));
    }
}

/// Ends the canvas's open stroke with `builder`'s final stamps: a painted or erased stroke is
/// redrawn once in its final form (end taper); a smudge, which has already changed the layer, and
/// an adjustment painted on, which adds to its mask, only get the stamps not drawn yet.
fn close_stroke(
    canvas: &mut Canvas,
    builder: StrokeBuilder,
    second: Option<StrokeBuilder>,
    guide: &procraft_engine::guide::Guide,
) {
    // what undo calls it
    canvas.name_next_step(match canvas.stroke_tool() {
        Some(StrokeTool::Erase) => "Erase stroke",
        Some(StrokeTool::Smudge) => "Smudge stroke",
        _ if canvas.is_pencil() => "Filter stroke",
        _ => "Paint stroke",
    });
    lay_stroke(canvas, builder, second, guide);
}

/// [`close_stroke`] unnamed: the step it makes is part of one named already.
fn lay_stroke(
    canvas: &mut Canvas,
    mut builder: StrokeBuilder,
    second: Option<StrokeBuilder>,
    guide: &procraft_engine::guide::Guide,
) {
    if let Some(mut second) = second {
        canvas.replace_second_stamps(&guide::mirrored(guide, &second.finish()));
    }
    let drawn = builder.stamp_count();
    let for_good = if canvas.shows_tip() { builder.drawn_for_good() } else { None };
    let all = builder.finish();
    if canvas.stroke_tool() == Some(StrokeTool::Smudge) || canvas.is_pencil() {
        canvas.add_stamps(&guide::mirrored(guide, all.get(drawn..).unwrap_or(&[])));
    } else if let Some(rest) = for_good.and_then(|stamps| all.strip_prefix(stamps.as_slice())) {
        // nothing laid changes: a long stroke's end needn't lay it all again
        canvas.add_stamps(&guide::mirrored(guide, rest));
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
    /// The artwork's edits as the last save that worked found them, [`project::NOT_SAVED`]
    /// before one (read without waiting on a save).
    pub(crate) edits_saved: std::sync::atomic::AtomicU64,
    /// How far the export under way has come (0..1, an f32's bits), read without waiting on it.
    pub(crate) export_progress: std::sync::atomic::AtomicU32,
    /// Why the autosave failed, not yet asked for, and whether it is failing (told once until a
    /// save works again); a lock of its own, never held through a save.
    pub(crate) save_failure: Mutex<(Option<String>, bool)>,
    /// The canvas's touches (they call into the session, so they keep their own lock).
    pub(crate) touches: touch::TouchState,
    /// The brush studio's pad being drawn again off the caller's thread (see
    /// [`CanvasSession::sync_pad`]).
    pub(crate) pad_sync: Mutex<studio::PadSync>,
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
        if st.holding_frames {
            st.frame_held = true;
            return;
        }
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
            /// Where it is painted (only that is sent), with its compressed copy when layers may
            /// soon be paged.
            Upload(Vec<u8>, Option<procraft_engine::geom::IRect>, Option<procraft_gpu::ReadyToPark>),
            Park(procraft_gpu::ReadyToPark),
        }
        // every layer up as it was read, or no canvas (what was opened kept): one the GPU had no
        // memory for would show empty, and be saved so once painted on
        let made = gpu.checked(|| -> Result<Canvas, procraft_gpu::GpuError> {
            let budget = crate::layers::resident_budget();
            let (w, h) = (doc.width(), doc.height());
            // read on a few threads (each file decoded and its compressed copy made: those below
            // the top layers that fit are parked, those that fit keep it, so letting them go
            // later needs no reading back), put on the canvas here in order
            let first_resident = ids.len().saturating_sub(procraft_gpu::most_resident(budget, w, h));
            // the copies of layers that go up take an eighth of the textures' budget at most, and
            // only those that pack to a quarter or less are kept
            let copy_room = budget / 8;
            let mut copies = 0u64;
            // and only when the layers take half the textures' budget or more: far under it,
            // none is likely to be let go (one that is is read back then)
            let layers_bytes = ids.len() as u64 * u64::from(w) * u64::from(h) * 4;
            let copying = budget > 0 && layers_bytes.saturating_mul(2) >= budget;
            let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).saturating_sub(1).clamp(1, 6);
            let next = std::sync::atomic::AtomicUsize::new(0);
            std::thread::scope(|scope| {
                let (send, receive) = std::sync::mpsc::sync_channel(workers);
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
                                        let copy = copying.then(|| procraft_gpu::ReadyToPark::of(&px, w, h));
                                        let painted = crate::layer_ops::painted_rect(&px, w);
                                        (Read::Upload(px, painted, copy), hash)
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
                // the canvas made while the first layers are read (its textures take a while); if
                // it can't be, the readers stop at their next layer, nothing taking what they read
                let mut c = Canvas::unfilled(&gpu, &doc)?;
                c.set_resident_budget(budget);
                let mut waiting = std::collections::BTreeMap::new();
                let mut at = 0;
                let mut unread = Vec::new();
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
                                match layer {
                                    Read::Upload(px, painted, Some(copy))
                                        if copy.bytes() * 4 <= px.len()
                                            && copies + copy.bytes() as u64 <= copy_room =>
                                    {
                                        copies += copy.bytes() as u64;
                                        c.upload_layer_kept(&gpu, *id, &px, copy, painted)
                                    }
                                    Read::Upload(px, painted, _) => c.upload_layer_painted(&gpu, *id, &px, painted),
                                    Read::Park(ready) => {
                                        c.park_ready(*id, ready);
                                        Ok(())
                                    }
                                }?;
                            }
                            Some(Err(e)) => {
                                log::error!("layer {id:?}: {e}");
                                unread.push(*id);
                            }
                        }
                    }
                }
                c.fill_rest(&gpu, &doc);
                // as read: not changed
                c.take_changed();
                self.lock().layers_unread(unread);
                log::info!(
                    "canvas made in {} ms: {} layers, {} kept compressed, {} KB of copies of the rest",
                    started.elapsed().as_millis(),
                    ids.len(),
                    c.parked_layers(),
                    copies / 1024
                );
                Ok(c)
            })
        });
        let mut st = self.lock();
        if st.canvas.is_some() {
            return;
        }
        let same = st.doc == doc && st.pending_pixels.iter().map(|(id, _)| *id).eq(ids.iter().copied());
        match made.and_then(|made| made) {
            Ok(c) if same => {
                st.pending_pixels.clear();
                st.canvas = Some(c);
            }
            Ok(_) => st.ensure_canvas(),
            Err(e) => {
                log::error!("canvas: {e}");
                st.error = Some(e.to_string());
                st.canvas_failed = true;
            }
        }
    }
}

/// A canvas being painted: the document, the brush, the input and the renderer.
#[derive(uniffi::Object)]
pub struct CanvasSession {
    pub(crate) shared: Arc<Shared>,
    /// Itself, for what it hands plugins told of an event.
    me: std::sync::Weak<CanvasSession>,
}

impl CanvasSession {
    /// Tells the plugins on `hook` (an event's) that it happened: each acts at once, as one step
    /// of its own. Not while a plugin is changing the artwork itself: its changes tell nothing.
    pub(crate) fn tell_plugins(&self, hook: plugin_run::Hook) {
        let hooks = {
            let st = self.shared.lock();
            if st.plugin_stepping > 0 {
                return;
            }
            st.hooks.clone()
        };
        let (Some(hooks), Some(me)) = (hooks, self.me.upgrade()) else { return };
        if !hooks.listens(hook) {
            return;
        }
        let _step = self.plugin_step();
        hooks.tell(hook, me);
    }
}

#[uniffi::export]
impl CanvasSession {
    /// A new artwork of `width` × `height` pixels.
    #[uniffi::constructor]
    pub fn new(width: u32, height: u32) -> Arc<CanvasSession> {
        let most = procraft_engine::document::canvas_edge_most();
        CanvasSession::with_document(
            Document::new(width.min(most), height.min(most)),
            DEFAULT_NAME.to_owned(),
            Vec::new(),
        )
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
        device::start_log();
        let shipped = library();
        let shelf = brush_library::LibrarySet::shipped(&shipped);
        // the round pen to paint, the soft airbrush to erase, the round brush to smudge
        let [paint_brush, erase_brush, smudge_brush] =
            [(1, 0), (3, 0), (2, 0)].map(|origin| brush_library::place_of(&shelf, origin));
        let state = State {
            doc,
            view: None,
            view_motion: None,
            key_drag: None,
            window_size: (1, 1),
            reconfigure: false,
            tool: ToolKind::Paint,
            previous_tool: None,
            text_edit: None,
            text_drawn: None,
            text_steps: Vec::new(),
            recording: None,
            layer_steps: Vec::new(),
            laid: std::collections::HashMap::new(),
            lays: [0; 2],
            sharp: sharp::Sharp::default(),
            lay_to: None,
            layer_data_unread: std::collections::HashSet::new(),
            pour: None,
            reveal: None,
            reduce_motion: false,
            recolor: None,
            crop_undo: None,
            selection_visibility: 0.25,
            brush_on_screen: false,
            hooks: None,
            picker_program: None,
            plugin_artwork: None,
            plugin_opened: None,
            sound_heard: None,
            sound_hover: None,
            input: procraft_engine::stroke::InputSettings::default(),
            crop_redo: None,
            library: shelf,
            libraries: brush_libraries::Libraries::default(),
            shipped,
            fallback: Brush::round_pen(),
            paint_brush,
            erase_brush,
            smudge_brush,
            set_choices: Vec::new(),
            recent_brushes: Vec::new(),
            pinned_brushes: Vec::new(),
            quick_brushes: Vec::new(),
            sliders: HashMap::new(),
            strokes: 0,
            preview: None,
            color: [0.0, 0.0, 0.0],
            secondary: [1.0, 1.0, 1.0],
            colour_history: None,
            stroke: None,
            second: None,
            gpu: None,
            canvas: None,
            scene: None,
            x_ray: false,
            needs_frame: false,
            frame_due: None,
            holding_frames: false,
            frame_held: false,
            stop: false,
            projection_size: (1, 1),
            projection_stop: false,
            frames: 0,
            pad_strokes: None,
            grain_store: grain_store::GrainStore::default(),
            library_kept: None,
            plugin_stepping: 0,
            pad_sample: false,
            sphere: None,
            frame_interval_ms: 0.0,
            error: None,
            display_scale: 2.0,
            interface_size: 1.0,
            grey_view: false,
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
            tracking_since: None,
            revision: 0,
            transform: None,
            transform_items: None,
            transform_mode: procraft_engine::transform::TransformMode::Freeform,
            interpolation: transform::Interpolation::Bilinear,
            transform_guides: transform::GuideSettings::default(),
            snap_pending: None,
            selection: None,
            saved_selections: Vec::new(),
            selection_display: selection::SelectionDisplay::Stripes,
            display_lut: None,
            selection_before: None,
            colour_source: None,
            feather_base: None,
            adjusting: false,
            adjust: adjusting::Adjusting::default(),
            filters: plugin_filter::Filters::default(),
            crop_edit: crop_edit::CropEdit::default(),
            last_filter: None,
            liquify: liquify::LiquifyRun::default(),
            studio: None,
            studio_second: false,
            pencil: false,
            cloning: false,
            simplified_undos: true,
            clone_sliders: DEFAULT_SLIDERS,
            clone_source: None,
            clone_offset: None,
            last_drag: None,
            drawn: Vec::new(),
            drawn_moving: 0.0,
            drawn_samples: Vec::new(),
            drawn_at: 0.0,
            shape: None,
            began: None,
            last_stroke: None,
            kept_shape: None,
            unkept_shape: None,
            force_assist: false,
            assist_from: None,
            assist_angle: None,
            guide_before: None,
            name,
            pending_pixels: project::opened(pixels),
            canvas_failed: false,
            unreadable: std::collections::HashSet::new(),
            unreadable_untold: Vec::new(),
            closed: false,
            fit_insets: [0.0; 4],
            backdrops: Vec::new(),
            backdrop_windows: Vec::new(),
            saved_view: None,
            reference_window: Default::default(),
            reference_picture: None,
        };
        Arc::new_cyclic(|me| CanvasSession {
            me: me.clone(),
            shared: Arc::new(Shared {
                state: parking_lot::Mutex::new(state),
                wake: parking_lot::Condvar::new(),
                thread: Mutex::new(None),
                projection: Mutex::new(None),
                saving: Mutex::new(project::Saved::default()),
                edits_saved: std::sync::atomic::AtomicU64::new(project::NOT_SAVED),
                export_progress: std::sync::atomic::AtomicU32::new(0),
                save_failure: Mutex::new((None, false)),
                touches: Mutex::new(touch::Touches::default()),
                pad_sync: Mutex::new(studio::PadSync::default()),
            }),
        })
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Starts drawing into the window `window` (an id from `NativeWindows.acquire`, or on iPadOS
    /// from `acquire_layer`), `width` × `height` pixels. The session owns the window from
    /// here on.
    pub fn attach_window(&self, window: u64, width: u32, height: u32) -> Result<(), MobileError> {
        self.detach_window();
        #[cfg(any(target_os = "android", target_os = "ios"))]
        {
            let native =
                window::take(window).ok_or_else(|| MobileError::Window { reason: format!("no window {window}") })?;
            {
                let mut st = self.shared.lock();
                st.window_size = (width.max(1), height.max(1));
                st.place_view();
                st.stop = false;
                st.reconfigure = true;
                self.shared.frame(&mut st);
            }
            let shared = Arc::clone(&self.shared);
            let handle = std::thread::Builder::new()
                .name("procraft-render".to_owned())
                .spawn(move || {
                    // a panic (in a driver's wrapper, say) leaves the canvas saying it can't draw
                    // rather than waiting on a thread that is gone
                    let run = Arc::clone(&shared);
                    if let Err(panic) =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || render::run(run, native)))
                    {
                        let reason = format!("the renderer stopped: {}", procraft_gpu::panic_message(panic.as_ref()));
                        log::error!("render thread: {reason}");
                        shared.lock().error = Some(reason);
                    }
                })
                .map_err(|e| MobileError::Window { reason: e.to_string() })?;
            *self.shared.thread.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
            Ok(())
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let _ = (width, height);
            Err(MobileError::Window { reason: format!("windows are the tablet apps' only (asked for {window})") })
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
        // a plugin's own artwork takes its strokes itself
        if st.plugin_touch(plugin_artwork::Phase::Down, &samples) {
            self.shared.frame(st);
            return;
        }
        // a plugin changing the artwork: this stroke would join its step
        if st.plugin_stepping > 0 {
            return;
        }
        // radial symmetry in as many parts as a plugin says, for this stroke
        st.doc.guide.radial = st.hooks.as_ref().and_then(|hooks| hooks.radial_segments());
        // a fill still spreading in shows all of itself before anything is drawn over it
        st.end_reveal();
        // the graphics stopped (perhaps while the app was away, its notice long gone): nothing
        // drawn will show, so each stroke tells it again
        if st.gpu.as_ref().is_some_and(Gpu::is_lost) {
            st.crop_edit.lost_told = false;
        }
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
        // locked layers, those that don't show (hidden themselves or by a group, or left out
        // while another is shown alone), groups, text layers and plugins' layers their plugins
        // can't take strokes on take no paint
        if st.doc.is_locked(current)
            || st.plugin_layer_off(current)
            || st.doc.unseen(current).is_some()
            || st
                .doc
                .layer(current)
                .is_none_or(|l| l.kind == procraft_engine::document::LayerKind::Group || l.text.is_some())
        {
            return;
        }
        let tool = if pointer == PointerKind::StylusEraser { ToolKind::Erase } else { st.tool };
        let (brush, style, diameter) = st.stroke_brush(tool, current);
        let second = st.second_brush(tool, current, &brush, &style);
        let diameter = diameter * st.brush_scale();
        st.strokes = st.strokes.wrapping_add(1);
        // the tool's brush is one of the Recent set's now (unless pinned there)
        st.note_recent(st.brush_ref(tool));
        st.info.strokes = st.info.strokes.saturating_add(1);
        let seed = st.strokes.wrapping_mul(0x9e37_79b9);
        let make = |st: &State, brush: Brush, diameter: f32, seed: u32| {
            let preview_tilt = brush.setting("preview_tilt") >= 0.5;
            let mut builder = StrokeBuilder::with_seed(brush, diameter, pointer.into(), seed)
                .scaled(st.brush_scale())
                .with_input(st.input.clone())
                .on_view_turned(st.view.map_or(0.0, |v| v.rotation()))
                .seen_at(st.view.map_or(1.0, |v| v.zoom()) / st.display_scale);
            if st.pad_strokes.is_some() {
                // on the studio's pad the lean moves no stamps, and the tilt switches act only
                // when the brush previews tilt
                builder = builder.on_pad(preview_tilt);
            }
            if st.pad_sample {
                // the pad's sample scribble is drawn as given: its path whatever the brush's
                // streamline, its pressure as it comes (its pace still counts)
                builder = builder.exact_path();
            }
            builder
        };
        let mut builder = make(st, brush.clone(), diameter, seed);
        let mut second_builder =
            second.as_ref().map(|(b, _, d)| make(st, b.clone(), d * st.brush_scale(), seed ^ 0x5eed_0002));
        let canvas_samples = st.to_canvas(&samples);
        st.assist_from = canvas_samples.first().map(|s| s.pos);
        st.assist_angle = None;
        let canvas_samples = st.assisted(canvas_samples);
        // on a plugin's layer it is recorded as it goes (not on the brush studio's pad)
        st.recording = None;
        if st.pad_strokes.is_none() {
            st.begin_recording(current, tool, pointer, seed, &canvas_samples);
        }
        st.drawn.clear();
        st.drawn_moving = 0.0;
        st.drawn_samples.clear();
        st.remember_input(&canvas_samples);
        // painted and erased strokes can be made shapes once they end
        st.last_stroke = None;
        st.kept_shape = None;
        st.unkept_shape = None;
        st.began =
            matches!(tool.stroke_tool(), StrokeTool::Paint | StrokeTool::Erase).then(|| quickshape::LastStroke {
                layer: current,
                style,
                brush: brush.clone(),
                diameter,
                pointer,
                drawn: Vec::new(),
                pace: quickshape::SAMPLE_PACE,
                steps: 0,
                fresh: second.is_none().then(|| builder.clone()),
                samples: Vec::new(),
                line: builder.clone(),
            });
        let Some(canvas) = st.canvas.as_mut() else { return };
        match second {
            Some((_, second, _)) => canvas.begin_dual_stroke(current, style, second),
            None => canvas.begin_stroke(current, style),
        }
        draw_samples(canvas, &mut builder, second_builder.as_mut(), &canvas_samples, &st.doc.stroke_guide());
        st.stroke = Some(builder);
        st.second = second_builder;
        self.shared.frame(st);
    }

    /// More input for the stroke (including the samples batched in between).
    pub fn stroke_add(&self, samples: Vec<PenSample>) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.plugin_touch(plugin_artwork::Phase::Moved, &samples) {
            self.shared.frame(st);
            return;
        }
        if st.plugin_stepping > 0 {
            return;
        }
        if st.scene_shown() {
            st.scene_stroke_add(&samples, false);
            self.shared.frame(st);
            return;
        }
        st.keep_pad_input(&samples);
        let canvas_samples = st.assisted(st.to_canvas(&samples));
        // a snapped shape turns and grows with the pen still holding it
        if st.shape.is_some() {
            if let Some(to) = canvas_samples.iter().rev().find(|s| s.is_finite()).map(|s| s.pos)
                && st.follow_shape(to)
            {
                self.shared.frame(st);
            }
            return;
        }
        st.remember_input(&canvas_samples);
        if let Some(recording) = st.recording.as_mut().filter(|_| st.stroke.is_some()) {
            recording.add(&canvas_samples);
        }
        let (Some(builder), Some(canvas)) = (st.stroke.as_mut(), st.canvas.as_mut()) else { return };
        if draw_samples(canvas, builder, st.second.as_mut(), &canvas_samples, &st.doc.stroke_guide()) {
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
        // a snapped shape is drawn whole: nothing runs ahead of it
        if st.shape.is_some() {
            return;
        }
        let canvas_samples = st.to_canvas(&samples);
        let (Some(builder), Some(canvas)) = (st.stroke.as_mut(), st.canvas.as_mut()) else { return };
        let guide = st.doc.stroke_guide();
        // a stroke shown with its tip ends it where the input is predicted to go
        if canvas.shows_tip() && builder.shows_tip() {
            canvas.set_tip(&guide::mirrored(&guide, &builder.preview_tip(&canvas_samples)));
        } else {
            show_ahead(canvas, builder, &canvas_samples, &guide);
        }
        self.shared.frame(st);
    }

    /// The stroke ends (the pen or finger lifted) after `samples`.
    pub fn stroke_end(&self, samples: Vec<PenSample>) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.plugin_touch(plugin_artwork::Phase::Up, &samples) {
            self.shared.frame(st);
            return;
        }
        if st.plugin_stepping > 0 {
            return;
        }
        // a paint stroke's colour goes into the history (painting an adjustment on uses none)
        if st.tool == ToolKind::Paint
            && !st.adjusting
            && let Some(history) = &st.colour_history
        {
            history.painted(history::hex(st.color));
        }
        if st.scene_shown() {
            st.scene_stroke_add(&samples, true);
            self.shared.frame(st);
            return;
        }
        st.keep_pad_input(&samples);
        let canvas_samples = st.assisted(st.to_canvas(&samples));
        // a snapped shape stays open to be edited, until the next thing done keeps it
        if st.shape.is_some() {
            if let Some(to) = canvas_samples.iter().rev().find(|s| s.is_finite()).map(|s| s.pos) {
                st.follow_shape(to);
            }
            st.let_go_shape();
            self.shared.frame(st);
            return;
        }
        // (painted into an open adjustment, it tells no plugin: what one did would end it)
        let tells = !st.adjusting;
        let (Some(mut builder), Some(canvas)) = (st.stroke.take(), st.canvas.as_mut()) else { return };
        let mut second = st.second.take();
        lift_samples(canvas, &mut builder, second.as_mut(), &canvas_samples, &st.doc.stroke_guide());
        match st.recording.take() {
            // on a plugin's layer, handed to the plugin
            Some(mut recording) => {
                recording.lift(&canvas_samples);
                st.end_recorded(recording, builder, second);
            }
            None => close_stroke(canvas, builder, second, &st.doc.stroke_guide()),
        }
        st.redraw_painted_on();
        st.bake_painted_on();
        st.remember_last_stroke();
        self.shared.frame(st);
        drop(guard);
        // plugins told it was kept, before the next touch is taken
        if tells {
            self.tell_plugins(plugin_run::Hook::StrokeKept);
        }
    }

    /// Drops the stroke (a second finger turned it into a gesture).
    pub fn stroke_cancel(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.plugin_touch(plugin_artwork::Phase::Off, &[]) {
            self.shared.frame(st);
            return;
        }
        if st.scene_shown() {
            st.scene_stroke_cancel();
            self.shared.frame(st);
            return;
        }
        if let Some(kept) = st.pad_strokes.as_mut() {
            kept.pop();
        }
        st.stroke = None;
        st.second = None;
        st.shape = None;
        st.began = None;
        st.recording = None;
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
        if st.scene_picture_under((pivot_x, pivot_y)) == Some(true) {
            drop(st);
            self.model_picture_move(pan_x, pan_y, scale, rotation);
            return;
        }
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

    /// Shows the canvas in perceptual greys (`on`), to judge its values, or in colour: only as
    /// the window shows it, not as it is exported, recorded or picked from.
    pub fn set_grey_view(&self, on: bool) {
        let mut st = self.shared.lock();
        if st.grey_view != on {
            st.grey_view = on;
            self.shared.frame(&mut st);
        }
    }

    pub fn grey_view(&self) -> bool {
        self.shared.lock().grey_view
    }

    /// Whether the system asks for less motion (Reduce Motion, animations off): a dropped colour's
    /// fill then shows at once instead of spreading in.
    pub fn set_reduce_motion(&self, reduce: bool) {
        self.shared.lock().reduce_motion = reduce;
    }

    /// How many window pixels make one interface point (the screen's density).
    pub fn set_display_scale(&self, pixels_per_point: f32) {
        if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
            let mut st = self.shared.lock();
            st.display_scale = pixels_per_point;
            self.shared.frame(&mut st);
        }
    }

    /// How big the interface shows beside the screen's density (1: as measured). The canvas, its
    /// brushes and how far a gesture goes stay as they are: only the handles the interface draws
    /// over the canvas take touches where they show.
    pub fn set_interface_size(&self, size: f32) {
        if size.is_finite() && size > 0.0 {
            let mut st = self.shared.lock();
            if st.interface_size != size {
                st.interface_size = size;
                self.shared.frame(&mut st);
            }
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

    /// Turns the canvas back upright about the middle of the window, keeping its zoom and what is
    /// in the middle, settling into place as [`CanvasSession::settle_to_fit`] does. False if it
    /// already was upright.
    pub fn settle_upright(&self) -> bool {
        let mut st = self.shared.lock();
        let Some(from) = st.view.filter(|v| v.rotation().abs() > 1e-5) else { return false };
        let middle = Vec2::new(st.window_size.0 as f32 / 2.0, st.window_size.1 as f32 / 2.0);
        let Some(to) = from.screen_to_canvas(middle).and_then(|c| View::centred(c, from.zoom(), 0.0, st.window_size))
        else {
            return false;
        };
        st.view_motion = Some(ViewMotion { from, to, start: std::time::Instant::now(), curve: ViewCurve::Settle });
        self.shared.frame(&mut st);
        true
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

    /// Whether the canvas is made (once the artwork is shown): until then nothing can read its
    /// pictures (an animation's frames).
    pub fn canvas_made(&self) -> bool {
        self.shared.lock().canvas.is_some()
    }

    /// The frosted panels open over the canvas, five numbers each (left, top, right, bottom and
    /// corner radius, window pixels): what is behind them is drawn blurred.
    pub fn set_backdrops(&self, regions: Vec<f32>) {
        self.set_glass(regions, Vec::new());
    }

    /// The same, with `windows` drawn over them, where the glass is thin: what is behind only
    /// softens there.
    pub fn set_glass(&self, regions: Vec<f32>, windows: Vec<f32>) {
        let ((regions, _), (windows, _)) = (regions.as_chunks::<6>(), windows.as_chunks::<6>());
        let mut st = self.shared.lock();
        if st.backdrops != regions || st.backdrop_windows != windows {
            st.backdrops = regions.to_vec();
            st.backdrop_windows = windows.to_vec();
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

    /// The tool chosen; the one it replaces is kept as the last used (a pen's "switch to the
    /// previous tool" goes back to it).
    pub fn set_tool(&self, tool: ToolKind) {
        let mut st = self.shared.lock();
        if st.tool != tool {
            st.previous_tool = Some(st.tool);
        }
        st.tool = tool;
    }

    /// The paint colour, sRGB 0..1 (red, green, blue).
    pub fn color(&self) -> Vec<f32> {
        self.shared.lock().color.to_vec()
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

    /// The app's colour history: each paint stroke's colour is added to it as the stroke ends.
    pub fn use_colour_history(&self, history: std::sync::Arc<ColourHistory>) {
        self.shared.lock().colour_history = Some(history);
    }

    /// The secondary colour, which brushes with secondary colour dynamics mix in.
    pub fn set_secondary_color(&self, r: f32, g: f32, b: f32) {
        let c = [r, g, b].map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 });
        self.shared.lock().secondary = c;
    }

    /// Undoes the last step; returns what it was called, if it was named.
    pub fn undo(&self) -> Option<String> {
        // (nothing while a plugin changes the artwork: its step isn't one yet)
        if self.shared.lock().plugin_stepping > 0 {
            return None;
        }
        // a plugin's own artwork goes back through what it kept; the canvas's own steps (an action's,
        // say) once it has none
        if self.plugin_undo(true) == Some(true) {
            return None;
        }
        self.await_pour();
        self.shared.lock().end_reveal();
        // transforming, the transform's own last change goes back: the canvas's steps wait until
        // it ends (one taken back under it would leave the box over what is no longer there)
        if self.shared.lock().transform.is_some() {
            self.transform_apply(crate::transform::TransformAction::Undo);
            return None;
        }
        // a picture being placed on a model goes first
        if self.model_picture_box().is_some() {
            self.model_picture_end(false);
            return Some("new layer".to_owned());
        }
        // editing text, a change to its box's width goes back first, the text still edited
        if let Some(name) = self.text_box_undo() {
            return Some(name);
        }
        if self.swap_crop(true) {
            return Some("Crop canvas".to_owned());
        }
        // recoloring, the fill open under the reticle is taken back first
        if self.recolor_step_aside() {
            return Some(recolor::STEP.to_owned());
        }
        // an adjustment open on the layer: its changes taken back, or the step before it
        if let Some(name) = self.step_adjusting(Self::undo_step, true) {
            return name;
        }
        if self.quickshape_undo() {
            return None;
        }
        self.undo_step()
    }

    /// Undoes the last step (cloning, the source going back too); returns its name.
    fn undo_step(&self) -> Option<String> {
        // cloning, the source goes back to where it was before the stroke
        self.shared.lock().adjust.puck_step(true);
        self.edit(Canvas::undo);
        self.shared.lock().canvas.as_ref().and_then(Canvas::redo_name)
    }

    /// Redoes the last step undone; returns what it was called, if it was named.
    pub fn redo(&self) -> Option<String> {
        if self.shared.lock().plugin_stepping > 0 {
            return None;
        }
        if self.plugin_undo(false) == Some(true) {
            return None;
        }
        self.await_pour();
        self.shared.lock().end_reveal();
        // transforming, nothing is redone until it ends
        if self.shared.lock().transform.is_some() {
            return None;
        }
        if let Some(name) = self.text_box_redo() {
            return Some(name);
        }
        if self.swap_crop(false) {
            return Some("Crop canvas".to_owned());
        }
        // an adjustment open on the layer, with nothing changed yet: the step after it
        if let Some(name) = self.step_adjusting(Self::redo_step, false) {
            return name;
        }
        if self.quickshape_redo() {
            return None;
        }
        // recoloring, the open fill steps aside and is made again over what comes back
        let recoloring = self.recolor_step_aside();
        let name = self.redo_step();
        if recoloring {
            self.recolor_again();
        }
        name
    }

    /// Redoes the last step undone (cloning, the source going forward too); returns its name.
    fn redo_step(&self) -> Option<String> {
        self.shared.lock().adjust.puck_step(false);
        self.edit(Canvas::redo);
        self.shared.lock().canvas.as_ref().and_then(Canvas::undo_name)
    }

    pub fn clear_layer(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let current = st.doc.current();
        st.finish_stroke();
        st.settle();
        // a plugin's layer: the strokes the selection takes (every one, with nothing selected) go
        if st.plugin_layer_kind(current).is_some() {
            if st.delete_taken_items(current) {
                self.shared.frame(st);
            }
            return;
        }
        if st.doc.is_locked(current) || st.doc.layer(current).is_none_or(|l| l.drawn_from_data()) {
            return;
        }
        // with a selection, only what is selected is cleared
        if let Some(mask) = st.reach_selection() {
            let Some(rect) = layer_ops::mask_rect(&mask) else { return };
            let pixels = st.layer_part(current, rect);
            if let (Some(mut pixels), Some(canvas)) = (pixels, st.canvas.as_mut()) {
                for (px, &m) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(&layer_ops::mask_part(&mask, rect)) {
                    let keep = 255 - u16::from(m);
                    px.iter_mut().for_each(|c| *c = ((u16::from(*c) * keep + 127) / 255) as u8);
                }
                canvas.write_layer_part(current, rect, pixels);
                self.shared.frame(st);
            }
            return;
        }
        // the undo step keeps only what was painted, where the GPU can tell
        st.flush();
        let painted = match (st.canvas.as_ref(), st.gpu.as_ref()) {
            (Some(canvas), Some(gpu)) => canvas.painted_boxes(gpu, &[current]).pop().flatten(),
            _ => None,
        };
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("clear layer");
            match painted {
                Some(painted) => canvas.clear_layer_within(current, painted),
                None => canvas.clear_layer(current),
            }
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
            // an open shape is kept, then undone; a plugin's artwork goes by its own steps
            can_undo: st.plugin_steps().is_some_and(|(undo, _)| undo)
                || st.canvas.as_ref().is_some_and(Canvas::can_undo)
                || st.crop_undo.is_some()
                || st.shape.is_some()
                || st.adjust.touched()
                || st.text_edit.as_ref().is_some_and(|e| e.has_step(true)),
            // an open shape has only its own drags to redo
            can_redo: st.shape.as_ref().map_or_else(
                || st.canvas.as_ref().is_some_and(Canvas::can_redo) || st.crop_redo_ready() || st.shape_to_redo(),
                quickshape::ShapeEdit::can_redo,
            ) || st.text_edit.as_ref().is_some_and(|e| e.has_step(false))
                || st.plugin_steps().is_some_and(|(_, redo)| redo),
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
        st.flush();
        let before = st.canvas.as_ref().map(|c| (c.undo_step(), c.redo_step()));
        if let Some(canvas) = st.canvas.as_mut() {
            f(canvas);
            st.flush();
            // a stroke on a model's colour painted its roughness and metalness too; a tile's choice
            // chooses again what was before (or after) it
            if let Some(before) = before {
                st.follow_joint_step(before);
                st.follow_tile_step(before);
                st.follow_look_step(before);
                st.follow_layer_step(before);
            }
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
