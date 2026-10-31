//! The Prefs tab's settings and Pressure and Smoothing's, kept between runs in the app's folder
//! (`interface`, a setting a line), as the Android app keeps them in its interface preferences.

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

pub const RAPID_UNDO_MIN_S: f32 = 0.1;
pub const RAPID_UNDO_MAX_S: f32 = 1.5;

#[derive(Clone, Debug, PartialEq)]
pub struct Prefs {
    /// The side bar on the right edge instead of the left.
    pub right_handed: bool,
    /// Brush sizes relative to the screen (else to the canvas).
    pub brush_on_screen: bool,
    pub brush_cursor: bool,
    pub cursor_shows: CursorShows,
    pub cursor_style: CursorStyle,
    /// How long two fingers are held before undo repeats, seconds.
    pub rapid_undo_delay: f32,
    /// How strongly what isn't selected is banded, 0..1.
    pub selection_visibility: f32,
    /// The app's own smoothing for every stroke, each 0..1, and its pressure curve.
    pub stabilization: f32,
    pub motion_filtering: f32,
    pub motion_expression: f32,
    pub pressure_smoothing: f32,
    pub curve: Vec<[f32; 2]>,
    /// The Actions panel's tab open.
    pub actions_tab: usize,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            right_handed: false,
            brush_on_screen: false,
            brush_cursor: true,
            cursor_shows: CursorShows::Hovering,
            cursor_style: CursorStyle::Brush,
            rapid_undo_delay: 0.65,
            selection_visibility: 0.25,
            stabilization: 0.0,
            motion_filtering: 0.0,
            motion_expression: 0.0,
            pressure_smoothing: 0.0,
            curve: straight(),
            actions_tab: 0,
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
                "right_handed" => p.right_handed = on,
                "brush_on_screen" => p.brush_on_screen = on,
                "brush_cursor" => p.brush_cursor = on,
                "cursor_shows" => p.cursor_shows = pick(&SHOWS, value).unwrap_or(p.cursor_shows),
                "cursor_style" => p.cursor_style = pick(&STYLES, value).unwrap_or(p.cursor_style),
                "rapid_undo_delay" => {
                    if let Some(v) = number {
                        p.rapid_undo_delay = v.clamp(RAPID_UNDO_MIN_S, RAPID_UNDO_MAX_S);
                    }
                }
                "selection_visibility" => p.selection_visibility = unit.unwrap_or(p.selection_visibility),
                "stabilization" => p.stabilization = unit.unwrap_or(0.0),
                "motion_filtering" => p.motion_filtering = unit.unwrap_or(0.0),
                "motion_expression" => p.motion_expression = unit.unwrap_or(0.0),
                "pressure_smoothing" => p.pressure_smoothing = unit.unwrap_or(0.0),
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
        p
    }

    pub fn keep(&self) {
        let curve: Vec<String> = self.curve.iter().map(|[x, y]| format!("{x},{y}")).collect();
        let lines = [
            format!("right_handed {}", self.right_handed),
            format!("brush_on_screen {}", self.brush_on_screen),
            format!("brush_cursor {}", self.brush_cursor),
            format!("cursor_shows {}", place(&SHOWS, self.cursor_shows)),
            format!("cursor_style {}", place(&STYLES, self.cursor_style)),
            format!("rapid_undo_delay {}", self.rapid_undo_delay),
            format!("selection_visibility {}", self.selection_visibility),
            format!("stabilization {}", self.stabilization),
            format!("motion_filtering {}", self.motion_filtering),
            format!("motion_expression {}", self.motion_expression),
            format!("pressure_smoothing {}", self.pressure_smoothing),
            format!("curve {}", curve.join(",")),
            format!("actions_tab {}", self.actions_tab),
        ];
        crate::files::keep_text("interface", &(lines.join("\n") + "\n"));
    }

    /// The artwork follows them.
    pub fn apply(&self, session: &CanvasSession) {
        session.set_brush_scaling(self.brush_on_screen);
        session.set_selection_visibility(self.selection_visibility);
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
