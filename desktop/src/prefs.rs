//! The Prefs tab's settings, Pressure and Smoothing's and Advanced settings', kept between runs in
//! the app's folder (`interface`, a setting a line), as the Android app keeps them in its interface
//! preferences.

use procraft_core::CanvasSession;

/// When the brush cursor shows: while the pointer hovers, while painting, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShows {
    Hovering,
    Painting,
    Both,
}

/// How the brush cursor is drawn: light and dark rings, in the colour painted with, or as the
/// brush says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyle {
    Contrast,
    Colour,
    Brush,
}

/// What an artwork dragged out of the gallery becomes, and a picture's format when one is wanted.
pub const DRAG_FORMATS: [&str; 5] = ["ProCraft", "PSD", "JPEG", "PNG", "TIFF"];
pub const DRAG_IMAGE_FORMATS: [&str; 3] = ["JPEG", "PNG", "TIFF"];

pub const RAPID_UNDO_MIN_S: f32 = 0.1;
pub const RAPID_UNDO_MAX_S: f32 = 1.5;

#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    /// Panels and bars light grey with dark text.
    pub light_interface: bool,
    /// The side bar on the right edge instead of the left.
    pub right_handed: bool,
    /// Brush sizes relative to the screen (else to the canvas).
    pub brush_on_screen: bool,
    pub brush_cursor: bool,
    pub cursor_shows: CursorShows,
    pub cursor_style: CursorStyle,
    /// The colour model the Colors panel's disc and Value sliders work in.
    pub colour_model: crate::colour_models::ColourModel,
    /// How long two fingers are held before undo repeats, seconds.
    pub rapid_undo_delay: f32,
    /// How strongly what isn't selected is banded, 0..1.
    pub selection_visibility: f32,
    /// ColorDrop keeps out of gaps up to this many canvas pixels wide in its lines (0: none).
    pub fill_gap: u32,
    /// The app's own smoothing for every stroke, each 0..1, and its pressure curve.
    pub stabilization: f32,
    pub motion_filtering: f32,
    pub motion_expression: f32,
    pub pressure_smoothing: f32,
    pub curve: Vec<[f32; 2]>,
    /// The Actions panel's tab open.
    pub actions_tab: usize,
    /// The brush studio's pad a sphere on every page.
    pub pad_3d: bool,
    /// The canvas shown on a second display too.
    pub project_canvas: bool,
    /// The eyedropper picks from the current layer alone, not the canvas as composited.
    pub eyedropper_layer: bool,
    /// Advanced settings: an artwork opens as it was left; fitting the canvas keeps it clear of
    /// the top bar and the side bar; new artworks don't record their time-lapse; strokes made in
    /// an adjustment undo together; how far the pen moves before it counts as fast (0.25..1); the
    /// colour named in a message as it changes.
    pub orientation_memory: bool,
    pub fit_inside: bool,
    pub no_time_lapse: bool,
    pub simplified_undos: bool,
    pub speed_distance: f32,
    pub colour_names: bool,
    /// A floating bar of undo, redo, zoom, move and fit for one finger; a tap landing where the
    /// finger first touched, even if it drifts before lifting.
    pub touch_companion: bool,
    pub tap_assistance: bool,
    /// Undo, redo, tools and panels click as they are used.
    pub feedback_sounds: bool,
    /// The short colour panel shows the history, not the active palette.
    pub colour_history_shown: bool,
    /// Drag and Drop export: what an artwork dragged out of the gallery becomes (an index into
    /// [`DRAG_FORMATS`]), and a picture's format when one is wanted (into [`DRAG_IMAGE_FORMATS`]).
    pub drag_format: usize,
    pub drag_image_format: usize,
    /// Recolor's Flood, kept from one time to the next.
    pub recolor_threshold: f32,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            light_interface: false,
            right_handed: false,
            brush_on_screen: false,
            brush_cursor: true,
            cursor_shows: CursorShows::Hovering,
            cursor_style: CursorStyle::Brush,
            colour_model: crate::colour_models::ColourModel::Hsb,
            rapid_undo_delay: 0.65,
            selection_visibility: 0.25,
            fill_gap: 0,
            stabilization: 0.0,
            motion_filtering: 0.0,
            motion_expression: 0.0,
            pressure_smoothing: 0.0,
            curve: straight(),
            actions_tab: 0,
            pad_3d: false,
            project_canvas: false,
            eyedropper_layer: false,
            orientation_memory: true,
            fit_inside: false,
            no_time_lapse: false,
            simplified_undos: true,
            speed_distance: 1.0,
            colour_names: false,
            touch_companion: false,
            tap_assistance: false,
            feedback_sounds: false,
            colour_history_shown: false,
            drag_format: 0,
            drag_image_format: 0,
            recolor_threshold: 0.0,
        }
    }
}

/// The pressure curve as it starts: straight.
pub fn straight() -> Vec<[f32; 2]> {
    vec![[0.0, 0.0], [1.0, 1.0]]
}

const SHOWS: [CursorShows; 3] = [CursorShows::Hovering, CursorShows::Painting, CursorShows::Both];
const STYLES: [CursorStyle; 3] = [CursorStyle::Contrast, CursorStyle::Colour, CursorStyle::Brush];

impl Prefs {
    /// As kept (anything missing or unreadable as it starts).
    pub fn kept() -> Prefs {
        let mut p = Prefs::default();
        let text = std::fs::read_to_string(crate::files::app_dir().join("interface")).unwrap_or_default();
        for line in text.lines() {
            let Some((name, value)) = line.split_once(' ') else { continue };
            let on = value == "true";
            let number = value.parse::<f32>().ok().filter(|v| v.is_finite());
            let unit = number.map(|v| v.clamp(0.0, 1.0));
            match name {
                "light_interface" => p.light_interface = on,
                "right_handed" => p.right_handed = on,
                "brush_on_screen" => p.brush_on_screen = on,
                "brush_cursor" => p.brush_cursor = on,
                "cursor_shows" => p.cursor_shows = pick(&SHOWS, value).unwrap_or(p.cursor_shows),
                "cursor_style" => p.cursor_style = pick(&STYLES, value).unwrap_or(p.cursor_style),
                "colour_model" => {
                    p.colour_model = crate::colour_models::ColourModel::of_key(value).unwrap_or(p.colour_model);
                }
                "rapid_undo_delay" => {
                    if let Some(v) = number {
                        p.rapid_undo_delay = v.clamp(RAPID_UNDO_MIN_S, RAPID_UNDO_MAX_S);
                    }
                }
                "selection_visibility" => p.selection_visibility = unit.unwrap_or(p.selection_visibility),
                "fill_gap" => {
                    if let Some(v) = number {
                        p.fill_gap = (v.max(0.0).round() as u32).min(procraft_engine::fill::MOST_GAP);
                    }
                }
                "stabilization" => p.stabilization = unit.unwrap_or(0.0),
                "motion_filtering" => p.motion_filtering = unit.unwrap_or(0.0),
                "motion_expression" => p.motion_expression = unit.unwrap_or(0.0),
                "pressure_smoothing" => p.pressure_smoothing = unit.unwrap_or(0.0),
                "pad_3d" => p.pad_3d = on,
                "project_canvas" => p.project_canvas = on,
                "eyedropper_layer" => p.eyedropper_layer = on,
                "orientation_memory" => p.orientation_memory = on,
                "fit_inside" => p.fit_inside = on,
                "no_time_lapse" => p.no_time_lapse = on,
                "simplified_undos" => p.simplified_undos = on,
                "speed_distance" => p.speed_distance = number.map_or(1.0, |v| v.clamp(0.25, 1.0)),
                "color_names" => p.colour_names = on,
                "single_touch_companion" => p.touch_companion = on,
                "tap_assistance" => p.tap_assistance = on,
                "feedback_sounds" => p.feedback_sounds = on,
                "color_history_shown" => p.colour_history_shown = on,
                "drag_format" => p.drag_format = value.parse::<usize>().map_or(0, |t| t.min(DRAG_FORMATS.len() - 1)),
                "drag_image_format" => {
                    p.drag_image_format = value.parse::<usize>().map_or(0, |t| t.min(DRAG_IMAGE_FORMATS.len() - 1))
                }
                "recolor_threshold" => p.recolor_threshold = unit.unwrap_or(0.0),
                "actions_tab" => p.actions_tab = value.parse::<usize>().map_or(0, |t| t.min(5)),
                "curve" => {
                    let numbers: Vec<f32> = value.split(',').filter_map(|v| v.parse().ok()).collect();
                    let points: Vec<[f32; 2]> = numbers.as_chunks::<2>().0.to_vec();
                    if points.len() >= 2 {
                        p.curve = points;
                    }
                }
                _ => {}
            }
        }
        // the interface shows as it was left
        crate::look::set_light(p.light_interface);
        p
    }

    pub fn keep(&self) {
        let curve: Vec<String> = self.curve.iter().map(|[x, y]| format!("{x},{y}")).collect();
        let lines = [
            format!("light_interface {}", self.light_interface),
            format!("right_handed {}", self.right_handed),
            format!("brush_on_screen {}", self.brush_on_screen),
            format!("brush_cursor {}", self.brush_cursor),
            format!("cursor_shows {}", place(&SHOWS, self.cursor_shows)),
            format!("cursor_style {}", place(&STYLES, self.cursor_style)),
            format!("colour_model {}", self.colour_model.key()),
            format!("rapid_undo_delay {}", self.rapid_undo_delay),
            format!("selection_visibility {}", self.selection_visibility),
            format!("fill_gap {}", self.fill_gap),
            format!("stabilization {}", self.stabilization),
            format!("motion_filtering {}", self.motion_filtering),
            format!("motion_expression {}", self.motion_expression),
            format!("pressure_smoothing {}", self.pressure_smoothing),
            format!("curve {}", curve.join(",")),
            format!("actions_tab {}", self.actions_tab),
            format!("pad_3d {}", self.pad_3d),
            format!("project_canvas {}", self.project_canvas),
            format!("eyedropper_layer {}", self.eyedropper_layer),
            format!("orientation_memory {}", self.orientation_memory),
            format!("fit_inside {}", self.fit_inside),
            format!("no_time_lapse {}", self.no_time_lapse),
            format!("simplified_undos {}", self.simplified_undos),
            format!("speed_distance {}", self.speed_distance),
            format!("color_names {}", self.colour_names),
            format!("single_touch_companion {}", self.touch_companion),
            format!("tap_assistance {}", self.tap_assistance),
            format!("feedback_sounds {}", self.feedback_sounds),
            format!("color_history_shown {}", self.colour_history_shown),
            format!("drag_format {}", self.drag_format),
            format!("drag_image_format {}", self.drag_image_format),
            format!("recolor_threshold {}", self.recolor_threshold),
        ];
        crate::files::keep_text("interface", &(lines.join("\n") + "\n"));
    }

    /// The artwork follows them.
    pub fn apply(&self, session: &CanvasSession) {
        crate::look::set_light(self.light_interface);
        session.set_light_interface(self.light_interface);
        session.set_brush_scaling(self.brush_on_screen);
        session.set_selection_visibility(self.selection_visibility);
        session.set_fill_gap(self.fill_gap);
        session.set_simplified_undos(self.simplified_undos);
        session.set_speed_distance(self.speed_distance);
        session.set_input_settings(
            self.stabilization,
            self.motion_filtering,
            self.motion_expression,
            self.pressure_smoothing,
            self.curve.iter().flatten().copied().collect(),
        );
    }

    pub fn rapid_undo_ms(&self) -> f64 {
        f64::from(self.rapid_undo_delay) * 1000.0
    }

    /// Whether the cursor shows while the pointer hovers (else while painting).
    pub fn cursor_shown(&self, hovering: bool) -> bool {
        self.brush_cursor
            && if hovering {
                self.cursor_shows != CursorShows::Painting
            } else {
                self.cursor_shows != CursorShows::Hovering
            }
    }

    /// Pressure and Smoothing as they start.
    pub fn reset_input(&mut self) {
        self.stabilization = 0.0;
        self.motion_filtering = 0.0;
        self.motion_expression = 0.0;
        self.pressure_smoothing = 0.0;
        self.curve = straight();
    }
}

fn place<T: PartialEq>(list: &[T], v: T) -> usize {
    list.iter().position(|x| *x == v).unwrap_or(0)
}

fn pick<T: Copy>(list: &[T], value: &str) -> Option<T> {
    list.get(value.parse::<usize>().ok()?).copied()
}
