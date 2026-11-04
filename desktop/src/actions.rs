//! The Actions panel, as the Android app's (ui/ActionsPanel.kt): six tabs (Add, Canvas, Share,
//! Video, Prefs, Help; on a model, 3D in Video's place) of grouped rows, and the pages Prefs opens (Advanced cursor, Pressure and
//! Smoothing). Rows the desktop can't do yet show dimmed, as the Android app's own rows still to
//! come do. Share writes the artwork to a file.

use std::collections::HashMap;
use std::time::Instant;

use egui::{Align2, Color32, Id, Order, Pos2, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};

use crate::animated_export::Animated;
use crate::files::Format;
use crate::icons;
use crate::look::{Look, Themed, Weight, argb, cubic_bezier, highlight, mix, pc, text};
use crate::menu::{Glass, Menu};
use crate::panel::{self, PANEL_TITLE};
use crate::prefs::{CursorShows, CursorStyle, Prefs, RAPID_UNDO_MAX_S, RAPID_UNDO_MIN_S};

const ACTIONS_WIDTH: f32 = 390.0;
const TAB_IDLE: Themed = Themed(0xFFA3A3A3, 0xFF404040);
const ACTIONS_GROUP: Themed = Themed(0x420C0C0C, 0xFFE8E8E8);
const ACTIONS_SEPARATOR: Themed = Themed(0x06FFFFFF, 0xFFE2E2E2);
const ACTIONS_ROW: Themed = Themed(0xFFA8A8A8, 0xFF393939);
const ACTIONS_DISABLED: Themed = Themed(0xFF5E5E5E, 0xFFABABAB);
const SWITCH_FIXED: Themed = Themed(0xFF404040, 0xFFD2D2D2);
const SWITCH_FIXED_KNOB: Themed = Themed(0xFF525252, 0xFFE9E9E9);
const SLIDE_LABEL: Themed = Themed(0xFFC1C1C1, 0xFF3D3D3D);
const SLIDE_VALUE: Themed = Themed(0xFFEDEDED, 0xFF3D3D3D);
const SLIDE_TRACK: Themed = Themed(0xFF414141, 0xFFBEBEBE);
const CURSOR_HEADING: Themed = Themed(0xFFB5B5B5, 0xFF404040);
/// Brush scaling's menu: its fill and rim, a radio's ring.
const CHOICE_MENU: Glass =
    Glass { fill: Themed(0xFF5A5C5B, 0xFFF0F0F0), edge: Some(0x33FFFFFF), frost: false, corner: 26.0, shadow: false };
const RADIO_RING: Themed = Themed(0xFF606060, 0xFFB0B0B0);
const ROW_HEIGHT: f32 = 44.0;
const CHOICE_HEIGHT: f32 = 42.0;
const SLIDE_HEIGHT: f32 = 63.0;
const GROUP_GAP: f32 = 20.0;
const HEADING_HEIGHT: f32 = 45.0;
/// The panel ends this far above the window's bottom edge, and its rows scroll past this tall.
const BOTTOM_MARGIN: f32 = 25.0;
const MOST_ROWS: f32 = 760.0;
/// A row with a private action: how far a swipe to the left slides it open, that action's button
/// and its look, and how long it takes to settle open or closed (seconds).
const PRIVATE_SLIDE: f32 = 248.0;
const PRIVATE_WIDTH: f32 = 245.0;
const PRIVATE_FILL: u32 = 0xFF454545;
const PRIVATE_INK: u32 = 0xFFA8A8A8;
const PRIVATE_SETTLE_S: f32 = 0.3;
/// A page pushed in or going back, and the rows fading out and in as the tab changes (seconds).
const PAGE_SLIDE_S: f32 = 0.45;
const TAB_OUT_S: f32 = 0.1;
const TAB_IN_S: f32 = 0.2;

/// What a row does. Those the panel does itself (a page, a menu, a setting) never reach the app;
/// [`Act::Prefs`] tells it a setting changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    InsertFile,
    InsertPhoto,
    /// Insert a file or a photo, or Paste, swiped open: what they bring goes on a private layer.
    InsertPrivateFile,
    InsertPrivatePhoto,
    PastePrivate,
    /// On a model: its 2D texture shown flat, strokes reaching through it, the lighting studio.
    ModelFlat,
    PaintThrough,
    LightingStudio,
    AddText,
    Flip {
        horizontal: bool,
    },
    Share(Format),
    AnimatedExport(Animated),
    /// A model's turntable as an animation.
    ModelAnimated(Animated),
    About,
    Licences,
    AdvancedSettings,
    /// The keyboard shortcuts' list.
    Shortcuts,
    GestureControls,
    CanvasInfo,
    AnimationAssist,
    PageAssist,
    TimeLapseReplay,
    TimeLapseRecording,
    ExportTimeLapse,
    Reference,
    DrawingGuide,
    EditGuide,
    Crop,
    Cut,
    Copy,
    CopyCanvas,
    Paste,
    SideBar,
    Prefs,
    Page(Page),
    ScalingMenu,
    LightInterface,
    RightHanded,
    BrushCursor,
    ProjectCanvas,
    /// The eyedropper picks from the current layer, or the canvas as composited.
    EyedropperLayer,
    ResetInput,
}

/// The panel's pages: the tabs, or one Prefs opened.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Tabs,
    Pressure,
    Cursor,
}

/// The settings a slider row sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Slid {
    RapidUndo,
    SelectionVisibility,
    FillGap,
    Stabilization,
    MotionFiltering,
    MotionExpression,
}

enum Row {
    /// A button row, dimmed without anything to do; its value on the right.
    Button(&'static str, Option<&'static str>, Option<Act>),
    /// A button row a swipe to the left slides open, a second button coming in from the right
    /// that does it privately: the two labels and what each does (none: dimmed).
    Private(&'static str, &'static str, Option<Act>, Option<Act>),
    /// A switch row, on or off; what a tap does, or none where the desktop can't change it yet
    /// (shown dimmed, as it is).
    Switch(&'static str, bool, Option<Act>),
    /// A switch that stays as it is, shown as set.
    Fixed(&'static str, bool),
    /// A slider row this tall: its name, the value as shown, where it is (0..1), and what it sets.
    Slide(&'static str, String, f32, f32, Slid),
    Heading(&'static str),
}

impl Row {
    fn height(&self) -> f32 {
        match self {
            Row::Heading(_) => HEADING_HEIGHT,
            Row::Slide(.., tall, _) => *tall,
            _ => ROW_HEIGHT,
        }
    }
}

/// The groups of a model's tab `tab` (Add, Canvas, Share or 3D).
fn model_groups(tab: usize, showing: &Showing) -> Vec<Vec<Row>> {
    use Row::{Button, Heading, Switch};
    // showing the 2D texture, the rest is for the model
    let shown = !showing.model_flat;
    match tab {
        0 => vec![
            vec![
                Button("Insert a file", None, Some(Act::InsertFile)),
                Button("Insert a photo", None, Some(Act::InsertPhoto)),
                Button("Take a photo", None, None),
            ],
            vec![
                Button("Cut", None, Some(Act::Cut)),
                Button("Copy", None, Some(Act::Copy)),
                Button("Copy canvas", None, Some(Act::CopyCanvas)),
                Button("Paste", None, showing.can_paste.then_some(Act::Paste)),
            ],
        ],
        1 => vec![
            vec![Switch("Reference", showing.reference, Some(Act::Reference))],
            vec![Button("Canvas information", None, Some(Act::CanvasInfo))],
        ],
        2 => vec![
            vec![
                Heading("Share Model"),
                // the layered package doesn't hold a model yet
                Button("ProCraft", None, None),
                Button("Procreate", None, None),
                Button("USDZ", None, Some(Act::Share(Format::ModelUsdz))),
                Button("OBJ", None, Some(Act::Share(Format::ModelObj))),
            ],
            vec![
                Heading("Share Image"),
                Button("JPEG", None, Some(Act::Share(Format::ModelJpeg))),
                Button("PNG", None, Some(Act::Share(Format::ModelRender))),
                Button("TIFF", None, Some(Act::Share(Format::ModelTiff))),
                Button("Animated GIF", None, Some(Act::ModelAnimated(Animated::Gif))),
                Button("Animated PNG", None, Some(Act::ModelAnimated(Animated::Png))),
                Button("Animated MP4", None, Some(Act::ModelAnimated(Animated::Mp4))),
                Button("Animated HEVC", None, Some(Act::ModelAnimated(Animated::Hevc))),
            ],
            vec![Heading("Share Textures"), Button("PNG", None, Some(Act::Share(Format::ModelTextures)))],
        ],
        _ => vec![
            vec![
                Switch("Show 2D texture", showing.model_flat, Some(Act::ModelFlat)),
                Switch("Paint through mesh", showing.paint_through, shown.then_some(Act::PaintThrough)),
                Button("Edit lighting & environment", None, shown.then_some(Act::LightingStudio)),
            ],
            vec![Button("View in AR", None, None)],
        ],
    }
}

/// What a tap in a group did.
#[derive(Clone, Copy)]
enum Tapped {
    Act(Act),
    Slid(Slid, f32),
}

const TABS: [(&str, &icons::Icon); 6] = [
    ("Add", &icons::TAB_ADD),
    ("Canvas", &icons::TAB_CANVAS),
    ("Share", &icons::TAB_SHARE),
    ("Video", &icons::TAB_VIDEO),
    ("Prefs", &icons::TAB_PREFS),
    ("Help", &icons::TAB_HELP),
];

/// On a model, its own tab takes Video's place.
const MODEL_TABS: [(&str, &icons::Icon); 6] = [
    ("Add", &icons::TAB_ADD),
    ("Canvas", &icons::TAB_CANVAS),
    ("Share", &icons::TAB_SHARE),
    ("3D", &icons::TAB3_D),
    ("Prefs", &icons::TAB_PREFS),
    ("Help", &icons::TAB_HELP),
];

/// What the rows show besides the settings: something to paste, and Animation or Page Assist,
/// Reference, the drawing guide and the side bar on.
pub struct Showing {
    pub can_paste: bool,
    pub animation: bool,
    pub pages: bool,
    pub time_lapse: bool,
    pub reference: bool,
    pub guide: bool,
    pub side_bar: bool,
    /// The artwork paints on a model: whether its 2D texture shows flat, and strokes reach
    /// through it.
    pub model: bool,
    pub model_flat: bool,
    pub paint_through: bool,
}

/// The groups of rows of tab `tab`.
fn groups(tab: usize, showing: &Showing, prefs: &Prefs) -> Vec<Vec<Row>> {
    use Row::{Button, Heading, Private, Slide, Switch};
    if showing.model && tab <= 3 {
        return model_groups(tab, showing);
    }
    match tab {
        0 => vec![
            vec![
                // swiped to the left, the first three and Paste put what they bring on a private
                // layer
                Private("Insert a file", "Insert a private file", Some(Act::InsertFile), Some(Act::InsertPrivateFile)),
                Private(
                    "Insert a photo",
                    "Insert a private photo",
                    Some(Act::InsertPhoto),
                    Some(Act::InsertPrivatePhoto),
                ),
                Private("Take a photo", "Take a private photo", None, None),
                Button("Add text", None, Some(Act::AddText)),
            ],
            vec![
                Button("Cut", None, Some(Act::Cut)),
                Button("Copy", None, Some(Act::Copy)),
                Button("Copy canvas", None, Some(Act::CopyCanvas)),
                Private(
                    "Paste",
                    "Paste Private",
                    showing.can_paste.then_some(Act::Paste),
                    showing.can_paste.then_some(Act::PastePrivate),
                ),
            ],
        ],
        1 => vec![
            vec![Button("Crop & Resize", None, Some(Act::Crop))],
            vec![
                Switch("Animation Assist", showing.animation, Some(Act::AnimationAssist)),
                Switch("Page Assist", showing.pages, Some(Act::PageAssist)),
                Switch("Drawing Guide", showing.guide, Some(Act::DrawingGuide)),
                // greyed until the guide is on
                Button("Edit Drawing Guide", None, showing.guide.then_some(Act::EditGuide)),
            ],
            vec![Switch("Reference", showing.reference, Some(Act::Reference))],
            vec![
                Button("Flip horizontal", None, Some(Act::Flip { horizontal: true })),
                Button("Flip vertical", None, Some(Act::Flip { horizontal: false })),
            ],
            vec![Button("Canvas information", None, Some(Act::CanvasInfo))],
        ],
        2 => vec![
            vec![
                Heading("Share Image"),
                Button("ProCraft", None, Some(Act::Share(Format::Procraft))),
                Button("Procreate", None, Some(Act::Share(Format::Package))),
                Button("PSD", None, Some(Act::Share(Format::Psd))),
                Button("PDF", None, Some(Act::Share(Format::Pdf))),
                Button("JPEG", None, Some(Act::Share(Format::Jpeg))),
                Button("PNG", None, Some(Act::Share(Format::Png))),
                Button("TIFF", None, Some(Act::Share(Format::Tiff))),
            ],
            vec![
                Heading("Share Layers"),
                Button("PDF", None, Some(Act::Share(Format::LayersPdf))),
                Button("PNG Files", None, Some(Act::Share(Format::LayerPngs))),
                Button("Animated GIF", None, Some(Act::AnimatedExport(Animated::Gif))),
                Button("Animated PNG", None, Some(Act::AnimatedExport(Animated::Png))),
                Button("Animated MP4", None, Some(Act::AnimatedExport(Animated::Mp4))),
                Button("Animated HEVC", None, Some(Act::AnimatedExport(Animated::Hevc))),
            ],
        ],
        3 => vec![
            vec![
                Button("Time-lapse Replay", None, Some(Act::TimeLapseReplay)),
                Switch("Time-lapse Recording", showing.time_lapse, Some(Act::TimeLapseRecording)),
            ],
            vec![Button("Export time-lapse video", None, Some(Act::ExportTimeLapse))],
        ],
        4 => {
            let delay = prefs.rapid_undo_delay;
            let visibility = prefs.selection_visibility;
            let gap = prefs.fill_gap;
            vec![
                vec![
                    Switch("Light interface", prefs.light_interface, Some(Act::LightInterface)),
                    Switch("Right-hand interface", prefs.right_handed, Some(Act::RightHanded)),
                    Button(
                        "Brush scaling",
                        Some(if prefs.brush_on_screen { "Screen" } else { "Canvas" }),
                        Some(Act::ScalingMenu),
                    ),
                    Switch("Project canvas", prefs.project_canvas, Some(Act::ProjectCanvas)),
                    Switch("Brush cursor", prefs.brush_cursor, Some(Act::BrushCursor)),
                    Switch("Eyedropper picks from layer", prefs.eyedropper_layer, Some(Act::EyedropperLayer)),
                    Button("Advanced cursor settings", None, Some(Act::Page(Page::Cursor))),
                ],
                vec![
                    Button("Pressure and Smoothing", None, Some(Act::Page(Page::Pressure))),
                    Button("Gesture controls", None, Some(Act::GestureControls)),
                ],
                vec![
                    Slide(
                        "Rapid undo delay",
                        format!("{delay:.2}s"),
                        (delay - RAPID_UNDO_MIN_S) / (RAPID_UNDO_MAX_S - RAPID_UNDO_MIN_S),
                        SLIDE_HEIGHT,
                        Slid::RapidUndo,
                    ),
                    Slide(
                        "Selection mask visibility",
                        format!("{:.1}%", visibility * 100.0),
                        visibility,
                        SLIDE_HEIGHT,
                        Slid::SelectionVisibility,
                    ),
                    Slide(
                        "ColorDrop gap closing",
                        if gap == 0 { "Off".to_owned() } else { format!("{gap} px") },
                        gap as f32 / procraft_engine::fill::MOST_GAP as f32,
                        SLIDE_HEIGHT,
                        Slid::FillGap,
                    ),
                ],
                vec![Switch("Size & Opacity sidebar", showing.side_bar, Some(Act::SideBar))],
            ]
        }
        _ => vec![
            vec![Button("About ProCraft", None, Some(Act::About)), Button("Licences", None, Some(Act::Licences))],
            vec![
                Button("Keyboard shortcuts", None, Some(Act::Shortcuts)),
                Button("Advanced settings", None, Some(Act::AdvancedSettings)),
            ],
        ],
    }
}

/// Pressure and Smoothing's sliders.
fn smoothing_rows(prefs: &Prefs) -> Vec<Row> {
    let percent = |v: f32| format!("{}%", (v * 100.0).round());
    vec![
        Row::Slide("Stabilization", percent(prefs.stabilization), prefs.stabilization, 64.0, Slid::Stabilization),
        Row::Slide(
            "Motion Filtering",
            percent(prefs.motion_filtering),
            prefs.motion_filtering,
            64.0,
            Slid::MotionFiltering,
        ),
        Row::Slide(
            "Motion Filtering Expression",
            percent(prefs.motion_expression),
            prefs.motion_expression,
            64.0,
            Slid::MotionExpression,
        ),
    ]
}

/// A group's height: its heading, its rows and the lines between them (none between sliders).
fn group_height(rows: &[Row]) -> f32 {
    let mut height = 0.0;
    let mut last: Option<&Row> = None;
    for row in rows {
        if let Row::Heading(_) = row {
            height += HEADING_HEIGHT;
            continue;
        }
        if last.is_some_and(|l| !(matches!(l, Row::Slide(..)) && matches!(row, Row::Slide(..)))) {
            height += 1.0;
        }
        height += row.height();
        last = Some(row);
    }
    height
}

/// A choice group's height: its rows and the lines between those beside no chosen one.
fn choices_height(n: usize, chosen: usize) -> f32 {
    let lines = (1..n).filter(|&i| i != chosen && i - 1 != chosen).count();
    CHOICE_HEIGHT * n as f32 + lines as f32
}

#[derive(Default)]
pub struct Actions {
    /// The tab open (kept between runs in the settings).
    tab: Option<usize>,
    /// The tab whose rows show, and since when they change: the old rows fade out, then the new
    /// ones fade in.
    shown_tab: Option<usize>,
    tab_fade: Option<(Instant, usize)>,
    page: Page,
    /// The page last shown, and a page pushed in or going back: the page it leaves, since when,
    /// and whether it went forward.
    last_page: Page,
    page_move: Option<(Page, Instant, bool)>,
    /// Brush scaling's menu, and the row it hangs from.
    scaling: Menu,
    scaling_row: Option<Rect>,
    /// Pressure and Smoothing's curve.
    curve: crate::curve::CurveEditor,
    /// The artwork paints on a model: its tabs have 3D in Video's place.
    model_tabs: bool,
    /// The rows with a private action swiped (by group and row), until the tab changes or the
    /// panel closes.
    swipes: Swipes,
}

type Swipes = HashMap<(usize, usize), Swipe>;

/// A private row's slide: under a drag, where it is; otherwise settling from `from` towards open
/// or closed since `since` (seconds).
#[derive(Clone, Copy, Debug, Default)]
struct Swipe {
    dragging: Option<f32>,
    from: f32,
    open: bool,
    since: f64,
}

impl Swipe {
    /// How far it is slid open at `now` (0 closed, [`PRIVATE_SLIDE`] open).
    fn at(&self, now: f64) -> f32 {
        if let Some(d) = self.dragging {
            return d;
        }
        let to = if self.open { PRIVATE_SLIDE } else { 0.0 };
        let k = ((now - self.since) as f32 / PRIVATE_SETTLE_S).clamp(0.0, 1.0);
        self.from + (to - self.from) * crate::look::spring(PRIVATE_SETTLE_S, 0.8, 0.2, k)
    }

    /// Settling open or closed from where it is.
    fn settle(&mut self, open: bool, now: f64) {
        self.from = self.at(now);
        self.dragging = None;
        self.open = open;
        self.since = now;
    }
}

impl Actions {
    /// Shows the panel hanging from `button`; returns where it is (points) and what a row asked. A setting changed in `prefs` returns
    /// [`Act::Prefs`].
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        button: Rect,
        showing: &Showing,
        prefs: &mut Prefs,
        backdrops: &mut Vec<f32>,
    ) -> (Rect, Option<Act>) {
        let screen = ctx.content_rect();
        let (left, arrow_x) = panel::place(ctx, ACTIONS_WIDTH, button.center().x);
        let top = screen.top() + pc::BAR_HEIGHT;
        let tab = *self.tab.get_or_insert(prefs.actions_tab);
        let rows_alpha = self.tab_fade(ctx, tab);
        let groups = groups(self.shown_tab.unwrap_or(tab), showing, prefs);
        self.model_tabs = showing.model;
        let header = 12.0 + 44.0;
        let content_of = |page: Page| {
            panel::ARROW_HEIGHT
                + match page {
                    Page::Tabs => {
                        let rows: f32 = groups.iter().map(|g| group_height(g)).sum::<f32>()
                            + GROUP_GAP * (groups.len().saturating_sub(1)) as f32;
                        22.0 + 34.0 + 60.0 + 20.0 + rows.min(MOST_ROWS) + 11.0
                    }
                    Page::Cursor => {
                        let heading = 21.0 + 8.0;
                        header
                            + 8.0
                            + 6.0
                            + heading
                            + choices_height(3, prefs.cursor_shows as usize)
                            + 28.0
                            + heading
                            + choices_height(3, prefs.cursor_style as usize)
                            + 13.0
                    }
                    Page::Pressure => {
                        let curve = ACTIONS_WIDTH - 24.0 - 14.0;
                        let rows = 20.0
                            + group_height(&smoothing_rows(prefs))
                            + 16.0
                            + ROW_HEIGHT
                            + 24.0
                            + 19.0
                            + curve
                            + 27.0
                            + ROW_HEIGHT
                            + 13.0;
                        header + rows.min(MOST_ROWS)
                    }
                }
        };
        // a page pushed in slides in from the right as the one it covers slides off to the left;
        // going back, the page goes at once as the one under it slides back in; the panel's
        // height follows
        let moving = self.page_move.map(|(from, since, forward)| {
            let t = since.elapsed().as_secs_f32() / PAGE_SLIDE_S;
            (from, cubic_bezier(0.25, 0.1, 0.25, 1.0, t), forward, t >= 1.0)
        });
        let content = match moving {
            Some((from, e, _, _)) => content_of(from) + (content_of(self.page) - content_of(from)) * e,
            None => content_of(self.page),
        };
        if moving.is_some_and(|m| m.3) {
            self.page_move = None;
        } else if moving.is_some() {
            ctx.request_repaint();
        }
        let height = content.min(screen.bottom() - BOTTOM_MARGIN - top);
        let rect = Rect::from_min_size(pos2(left, top), vec2(ACTIONS_WIDTH, height));
        let mut tapped = None;
        let page = self.page;
        egui::Area::new(Id::unique("actions panel")).fixed_pos(rect.min).order(Order::Foreground).show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click());
            panel::frame(ui, look, "actions frame", rect, arrow_x, backdrops);
            let body = panel::body(rect);
            ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
                ui.set_clip_rect(body.intersect(ui.clip_rect()));
                let width = body.width();
                let (shift, leaving) = match moving {
                    Some((from, e, true, false)) => (width * (1.0 - e), Some((from, -width * e))),
                    Some((_, e, false, false)) => (-width * (1.0 - e), None),
                    _ => (0.0, None),
                };
                // the page it leaves, only seen
                if let Some((from, dx)) = leaving {
                    let _ = self.page_view(ui, look, from, body.translate(vec2(dx, 0.0)), &groups, prefs, rows_alpha);
                }
                tapped = self.page_view(ui, look, page, body.translate(vec2(shift, 0.0)), &groups, prefs, rows_alpha);
                if leaving.is_some() || shift != 0.0 {
                    tapped = None;
                }
            });
        });
        // what the panel does itself
        let mut act = match tapped {
            Some(Tapped::Act(Act::Page(page))) => {
                self.page = page;
                None
            }
            Some(Tapped::Act(Act::ScalingMenu)) => {
                self.scaling.open();
                None
            }
            Some(Tapped::Act(Act::LightInterface)) => {
                prefs.light_interface = !prefs.light_interface;
                Some(Act::Prefs)
            }
            Some(Tapped::Act(Act::RightHanded)) => {
                prefs.right_handed = !prefs.right_handed;
                Some(Act::Prefs)
            }
            Some(Tapped::Act(Act::BrushCursor)) => {
                prefs.brush_cursor = !prefs.brush_cursor;
                Some(Act::Prefs)
            }
            Some(Tapped::Act(Act::ProjectCanvas)) => {
                prefs.project_canvas = !prefs.project_canvas;
                Some(Act::Prefs)
            }
            Some(Tapped::Act(Act::EyedropperLayer)) => {
                prefs.eyedropper_layer = !prefs.eyedropper_layer;
                Some(Act::Prefs)
            }
            Some(Tapped::Act(Act::ResetInput)) => {
                prefs.reset_input();
                Some(Act::Prefs)
            }
            Some(Tapped::Act(a)) => Some(a),
            Some(Tapped::Slid(which, at)) => {
                let at = at.clamp(0.0, 1.0);
                match which {
                    Slid::RapidUndo => {
                        prefs.rapid_undo_delay = RAPID_UNDO_MIN_S + at * (RAPID_UNDO_MAX_S - RAPID_UNDO_MIN_S);
                    }
                    Slid::SelectionVisibility => prefs.selection_visibility = at,
                    Slid::FillGap => prefs.fill_gap = (at * procraft_engine::fill::MOST_GAP as f32).round() as u32,
                    Slid::Stabilization => prefs.stabilization = at,
                    Slid::MotionFiltering => prefs.motion_filtering = at,
                    Slid::MotionExpression => prefs.motion_expression = at,
                }
                Some(Act::Prefs)
            }
            None => None,
        };
        // another tab, kept
        if let Some(tab) = self.tab.filter(|t| *t != prefs.actions_tab) {
            prefs.actions_tab = tab;
            act = act.or(Some(Act::Prefs));
        }
        if self.scaling_menu(ctx, look, prefs, backdrops) {
            act = Some(Act::Prefs);
        }
        if self.page != Page::Pressure {
            self.curve.close();
        }
        if self.curve.menu(ctx, &mut prefs.curve) {
            act = Some(Act::Prefs);
        }
        // another page: it slides in
        if self.page != self.last_page {
            self.page_move = Some((self.last_page, Instant::now(), self.page != Page::Tabs));
            self.last_page = self.page;
            ctx.request_repaint();
        }
        (rect, act)
    }

    /// Page `page` of the panel in `body`, the tabs' rows shown `rows_alpha`.
    #[allow(clippy::too_many_arguments)]
    fn page_view(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        page: Page,
        body: Rect,
        groups: &[Vec<Row>],
        prefs: &mut Prefs,
        rows_alpha: f32,
    ) -> Option<Tapped> {
        match page {
            Page::Tabs => self.tabs(ui, look, body, groups, rows_alpha),
            Page::Cursor => self.cursor_page(ui, look, body, prefs),
            Page::Pressure => self.pressure_page(ui, look, body, prefs),
        }
    }

    /// How much the rows show as the tab changes to `tab`: the old ones fade out (0.1 s), then
    /// the new ones fade in (0.2 s).
    fn tab_fade(&mut self, ctx: &egui::Context, tab: usize) -> f32 {
        let shown = *self.shown_tab.get_or_insert(tab);
        // another tab (also while the rows fade): they fade out from here
        if self.tab_fade.map_or(shown != tab, |(_, to)| to != tab) {
            self.tab_fade = Some((Instant::now(), tab));
        }
        let Some((since, _)) = self.tab_fade else { return 1.0 };
        ctx.request_repaint();
        let t = since.elapsed().as_secs_f32();
        let ease = |k: f32| cubic_bezier(0.42, 0.0, 0.58, 1.0, k);
        if t < TAB_OUT_S {
            return 1.0 - ease(t / TAB_OUT_S);
        }
        if self.shown_tab != Some(tab) {
            self.swipes.clear();
        }
        self.shown_tab = Some(tab);
        let k = (t - TAB_OUT_S) / TAB_IN_S;
        if k >= 1.0 {
            self.tab_fade = None;
            return 1.0;
        }
        ease(k)
    }

    /// One of its menus is open: a press elsewhere only closes it.
    pub fn modal(&self) -> bool {
        self.scaling.is_open() || self.curve.menu_open()
    }

    /// The panel's pages go back to the tabs (as it closes).
    pub fn closed(&mut self) {
        self.page = Page::Tabs;
        self.last_page = Page::Tabs;
        self.page_move = None;
        self.tab_fade = None;
        self.shown_tab = self.tab;
        self.scaling.close();
        self.curve.close();
        self.swipes.clear();
    }

    /// The tabs page: the title, the tabs, and the open tab's groups.
    fn tabs(&mut self, ui: &mut Ui, look: &mut Look, body: Rect, groups: &[Vec<Row>], alpha: f32) -> Option<Tapped> {
        text(
            ui.painter(),
            pos2(body.left() + 24.0, body.top() + 22.0),
            Align2::LEFT_TOP,
            "Actions",
            24.0,
            Weight::Bold,
            argb(PANEL_TITLE),
        );
        // the tabs: 54 wide, 10 apart; a tab lights up as soon as it is touched
        let tabs_top = body.top() + 22.0 + 34.0;
        let tabs = if self.model_tabs { &MODEL_TABS } else { &TABS };
        for (i, (label, icon)) in tabs.iter().enumerate() {
            let cell = Rect::from_min_size(pos2(body.left() + 10.0 + 64.0 * i as f32, tabs_top), vec2(54.0, 60.0));
            let response = ui.interact(cell, ui.scope_id().with(("tab", i)), Sense::click());
            if response.clicked() {
                self.tab = Some(i);
            }
            let lit = Some(i) == self.tab || response.is_pointer_button_down_on();
            // a 50 frame centred 24 down
            let tint = if lit { argb(pc::ACCENT) } else { argb(TAB_IDLE) };
            look.icon(ui, icon, pos2(cell.center().x, cell.top() + 24.0), 50.0, tint);
            let ink = if lit { argb(pc::TEXT) } else { argb(TAB_IDLE) };
            let at = pos2(cell.center().x, cell.top() + 41.0);
            text(ui.painter(), at, Align2::CENTER_TOP, label, 12.0, Weight::Medium, ink);
        }
        let list = Rect::from_min_max(
            pos2(body.left() + 12.0, tabs_top + 60.0 + 20.0),
            pos2(body.right() - 12.0, body.bottom() - 11.0),
        );
        let mut tapped = None;
        self.scaling_row = None;
        let shown_tab = self.shown_tab;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ui.multiply_opacity(alpha);
            ScrollArea::vertical()
                .id_salt(("actions rows", shown_tab))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for (g, group) in groups.iter().enumerate() {
                        if g > 0 {
                            ui.add_space(GROUP_GAP);
                        }
                        let (t, cells) = group_view(ui, group, g, &mut self.swipes);
                        tapped = tapped.or(t);
                        // Brush scaling's menu hangs beside its row
                        if let Some(at) = group
                            .iter()
                            .filter(|r| !matches!(r, Row::Heading(_)))
                            .position(|r| matches!(r, Row::Button(_, _, Some(Act::ScalingMenu))))
                        {
                            self.scaling_row = cells.get(at).copied();
                        }
                    }
                });
        });
        tapped
    }

    /// Brush scaling's menu, while open: relative to the canvas or to the screen, the chosen one
    /// marked; 254 wide, its left edge 54 in from the row's right end. True when one was picked.
    fn scaling_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        prefs: &mut Prefs,
        backdrops: &mut Vec<f32>,
    ) -> bool {
        let Some(row) = self.scaling_row else {
            self.scaling.close();
            return false;
        };
        let rect = Rect::from_min_size(pos2(row.right() - 54.0, row.top() - 8.0), vec2(254.0, 8.0 + 40.0 * 2.0 + 8.0));
        let mut picked = None;
        let chosen = usize::from(prefs.brush_on_screen);
        self.scaling.show(ctx, "brush scaling", rect, rect.min, CHOICE_MENU, backdrops, |ui| {
            for (i, label) in ["Relative to canvas", "Relative to screen"].into_iter().enumerate() {
                let line = Rect::from_min_size(
                    pos2(rect.left(), rect.top() + 8.0 + 40.0 * i as f32),
                    vec2(rect.width(), 40.0),
                );
                let at = pos2(line.left() + 20.0, line.center().y);
                text(ui.painter(), at, Align2::LEFT_CENTER, label, 14.5, Weight::Regular, argb(pc::TEXT));
                radio(ui, look, pos2(line.right() - 19.0 - 9.0, line.center().y), i == chosen);
                if ui.interact(line, ui.scope_id().with(("scaling", i)), Sense::click()).clicked() {
                    picked = Some(i == 1);
                }
            }
        });
        let Some(on_screen) = picked else { return false };
        prefs.brush_on_screen = on_screen;
        self.scaling.close();
        true
    }

    /// A page's header: the back chevron and its title. True when Back was tapped.
    fn page_header(&mut self, ui: &Ui, look: &mut Look, body: Rect, title: &str) -> bool {
        let back = Rect::from_min_size(pos2(body.left() + 8.0, body.top() + 12.0 + 5.0), vec2(44.0, 34.0));
        look.icon(ui, &icons::CHEVRON_LEFT, back.center(), 22.0, argb(PANEL_TITLE));
        let at = pos2(back.right(), body.top() + 12.0 + 22.0);
        text(ui.painter(), at, Align2::LEFT_CENTER, title, 22.0, Weight::Bold, argb(PANEL_TITLE));
        let tapped = ui.interact(back, ui.scope_id().with("back"), Sense::click()).clicked();
        if tapped {
            self.page = Page::Tabs;
            self.curve.close();
        }
        tapped
    }

    /// Advanced cursor settings: when the brush cursor shows, and how it is drawn.
    fn cursor_page(&mut self, ui: &mut Ui, look: &mut Look, body: Rect, prefs: &mut Prefs) -> Option<Tapped> {
        if self.page_header(ui, look, body, "Advanced cursor") {
            return None;
        }
        let (left, right) = (body.left() + 12.0, body.right() - 12.0);
        let mut y = body.top() + 12.0 + 44.0 + 8.0;
        let mut changed = false;
        for (k, heading) in ["Brush cursor visibility", "Brush outline style"].into_iter().enumerate() {
            y += if k == 0 { 6.0 } else { 28.0 };
            let ink = argb(CURSOR_HEADING);
            text(ui.painter(), pos2(left + 14.0, y), Align2::LEFT_TOP, heading, 17.0, Weight::Bold, ink);
            y += 21.0 + 8.0;
            let (labels, chosen) = if k == 0 {
                (["Show while hovering", "Show while painting", "Show both"], prefs.cursor_shows as usize)
            } else {
                (["High contrast", "Active color", "Per brush"], prefs.cursor_style as usize)
            };
            let group = Rect::from_min_size(pos2(left, y), vec2(right - left, choices_height(3, chosen)));
            if let Some(i) = choice_group(ui, group, &labels, chosen, k) {
                if k == 0 {
                    prefs.cursor_shows = match i {
                        0 => CursorShows::Hovering,
                        1 => CursorShows::Painting,
                        _ => CursorShows::Both,
                    };
                } else {
                    prefs.cursor_style = match i {
                        0 => CursorStyle::Contrast,
                        1 => CursorStyle::Colour,
                        _ => CursorStyle::Brush,
                    };
                }
                changed = true;
            }
            y = group.bottom();
        }
        changed.then_some(Tapped::Act(Act::Prefs))
    }

    /// Pressure and Smoothing: the app's stabilization, motion filtering and its expression, Tip
    /// Attachment, the pressure curve every stroke goes through, and Reset all.
    fn pressure_page(&mut self, ui: &mut Ui, look: &mut Look, body: Rect, prefs: &mut Prefs) -> Option<Tapped> {
        if self.page_header(ui, look, body, "Pressure & Smoothing") {
            return None;
        }
        let list = Rect::from_min_max(
            pos2(body.left() + 12.0, body.top() + 12.0 + 44.0),
            pos2(body.right() - 12.0, body.bottom()),
        );
        let mut tapped = None;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ScrollArea::vertical()
                .id_salt("pressure rows")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    let width = ui.available_width();
                    ui.add_space(20.0);
                    tapped = group_view(ui, &smoothing_rows(prefs), 0, &mut self.swipes).0;
                    ui.add_space(16.0);
                    // the stroke always starts at the pointer's tip here
                    group_view(ui, &[Row::Fixed("Tip Attachment", true)], 1, &mut self.swipes);
                    ui.add_space(24.0);
                    let (label, _) = ui.allocate_exact_size(vec2(width, 19.0), Sense::hover());
                    text(
                        ui.painter(),
                        pos2(label.left() + 13.0, label.center().y),
                        Align2::LEFT_CENTER,
                        "App Pressure Sensitivity",
                        13.0,
                        Weight::SemiBold,
                        argb(SLIDE_LABEL),
                    );
                    let (band, _) = ui.allocate_exact_size(vec2(width, width - 14.0), Sense::hover());
                    let square = band.shrink2(vec2(7.0, 0.0));
                    if self.curve.show(ui, square, &mut prefs.curve, &crate::curve::ACTIONS, "pressure") {
                        tapped = Some(Tapped::Act(Act::Prefs));
                    }
                    ui.add_space(27.0);
                    let reset = [Row::Button("Reset all", None, Some(Act::ResetInput))];
                    let (t, _) = group_view(ui, &reset, 2, &mut self.swipes);
                    tapped = tapped.or(t);
                    ui.add_space(13.0);
                });
        });
        tapped
    }
}

/// A radio mark centred at `at`: blue with a tick when chosen, else a grey ring.
pub(crate) fn radio(ui: &Ui, look: &mut Look, at: Pos2, chosen: bool) {
    if chosen {
        ui.painter().circle_filled(at, 9.0, argb(pc::ACCENT));
        look.icon(ui, &icons::TICK, at, 12.0, Color32::WHITE);
    } else {
        ui.painter().circle_stroke(at, 9.0 - 0.75, Stroke::new(1.5, argb(RADIO_RING)));
    }
}

/// Choices as rows of a rounded group in `rect`, the chosen one a full blue row. Returns the one
/// tapped.
fn choice_group(ui: &Ui, rect: Rect, labels: &[&str], chosen: usize, g: usize) -> Option<usize> {
    ui.painter().rect_filled(rect, 18, argb(ACTIONS_GROUP));
    let mut tapped = None;
    let mut y = rect.top();
    for (i, label) in labels.iter().enumerate() {
        if i > 0 && i != chosen && i - 1 != chosen {
            let line = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 1.0));
            ui.painter().rect_filled(line, 0, argb(ACTIONS_SEPARATOR));
            y += 1.0;
        }
        let row = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), CHOICE_HEIGHT));
        if i == chosen {
            let corner = |at: bool| if at { 18 } else { 0 };
            let corners = egui::CornerRadius {
                nw: corner(i == 0),
                ne: corner(i == 0),
                sw: corner(i + 1 == labels.len()),
                se: corner(i + 1 == labels.len()),
            };
            ui.painter().rect_filled(row, corners, argb(pc::ACCENT));
        }
        let ink = if i == chosen { Color32::WHITE } else { argb(PANEL_TITLE) };
        let at = pos2(row.left() + 16.0, row.center().y);
        text(ui.painter(), at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, ink);
        if ui.interact(row, ui.scope_id().with(("choice", g, i)), Sense::click()).clicked() {
            tapped = Some(i);
        }
        y += CHOICE_HEIGHT;
    }
    tapped
}

/// A group: led by its heading (above it, not a row of it), its rows on a dark rounded wash with
/// faint lines between them (none between sliders). Returns what was tapped, and where each row
/// is.
fn group_view(ui: &mut Ui, rows: &[Row], g: usize, swipes: &mut Swipes) -> (Option<Tapped>, Vec<Rect>) {
    let width = ui.available_width();
    let mut rows = rows.iter().peekable();
    if let Some(Row::Heading(label)) = rows.peek() {
        let (band, _) = ui.allocate_exact_size(vec2(width, HEADING_HEIGHT), Sense::hover());
        text(ui.painter(), band.min + vec2(13.0, 7.0), Align2::LEFT_TOP, label, 14.0, Weight::Bold, argb(PANEL_TITLE));
        rows.next();
    }
    let rows: Vec<&Row> = rows.collect();
    let (area, _) = ui.allocate_exact_size(vec2(width, group_height_of(&rows)), Sense::hover());
    ui.painter().rect_filled(area, 22, argb(ACTIONS_GROUP));
    let mut tapped = None;
    let mut cells = Vec::new();
    let mut y = area.top();
    for (i, row) in rows.iter().enumerate() {
        let after_slide = i > 0 && matches!(rows.get(i - 1), Some(Row::Slide(..)));
        if i > 0 && !(after_slide && matches!(row, Row::Slide(..))) {
            let line = Rect::from_min_size(pos2(area.left(), y), vec2(width, 1.0));
            ui.painter().rect_filled(line, 0, argb(ACTIONS_SEPARATOR));
            y += 1.0;
        }
        let cell = Rect::from_min_size(pos2(area.left(), y), vec2(width, row.height()));
        cells.push(cell);
        // the first and last rows follow the group's rounded corners
        let corner = |at: bool| if at { 22 } else { 0 };
        let corners = egui::CornerRadius {
            nw: corner(i == 0),
            ne: corner(i == 0),
            sw: corner(i + 1 == rows.len()),
            se: corner(i + 1 == rows.len()),
        };
        let label_at = pos2(cell.left() + 16.0, cell.center().y);
        let switch_at = pos2(cell.right() - 8.0 - 63.0, cell.center().y - 14.0);
        match row {
            Row::Button(label, value, act) => {
                let response = ui.interact(cell, ui.scope_id().with(("row", g, i)), Sense::click());
                button_row(ui, &response, cell, corners, label, *value, act.is_some());
                if response.clicked() {
                    tapped = act.map(Tapped::Act);
                }
            }
            Row::Private(label, private, act, private_act) => {
                if let Some(t) = private_row(ui, cell, corners, (label, private), (*act, *private_act), swipes, (g, i))
                {
                    tapped = Some(t);
                }
            }
            Row::Switch(label, on, Some(act)) => {
                text(ui.painter(), label_at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, argb(ACTIONS_ROW));
                crate::transform::switch(ui, switch_at, *on, label);
                if ui.interact(cell, ui.scope_id().with(("switch", g, i)), Sense::click()).clicked() {
                    tapped = Some(Tapped::Act(*act));
                }
            }
            Row::Switch(label, on, None) | Row::Fixed(label, on) => {
                let ink = if matches!(row, Row::Fixed(..)) { ACTIONS_ROW } else { ACTIONS_DISABLED };
                text(ui.painter(), label_at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, argb(ink));
                // a 63 x 28 switch, shown as it is: it can't be changed here
                let track = Rect::from_min_size(switch_at, vec2(63.0, 28.0));
                ui.painter().rect_filled(track, 14, argb(SWITCH_FIXED));
                let knob =
                    Rect::from_min_size(track.min + vec2(2.0 + if *on { 22.0 } else { 0.0 }, 2.0), vec2(37.0, 24.0));
                ui.painter().rect_filled(knob, 12, argb(SWITCH_FIXED_KNOB));
            }
            Row::Slide(label, value, fraction, _, which) => {
                if let Some(at) = slide_row(ui, cell, label, value, *fraction, (g, i)) {
                    tapped = Some(Tapped::Slid(*which, at));
                }
            }
            Row::Heading(_) => {}
        }
        y += row.height();
    }
    (tapped, cells)
}

/// A button row at `cell`: it turns blue while it is held (`response`), its text white; dimmed
/// unless it does something (`acts`). Its value, if any, on the right.
fn button_row(
    ui: &Ui,
    response: &egui::Response,
    cell: Rect,
    corners: egui::CornerRadius,
    label: &str,
    value: Option<&str>,
    acts: bool,
) {
    let held = response.is_pointer_button_down_on() && !response.dragged();
    let lit = if acts { highlight(ui.ctx(), response.id, held) } else { 0.0 };
    if lit > 0.0 {
        ui.painter().rect_filled(cell, corners, argb(pc::ACCENT).gamma_multiply(lit));
    }
    let idle = if acts { argb(ACTIONS_ROW) } else { argb(ACTIONS_DISABLED) };
    let ink = mix(idle, Color32::WHITE, lit);
    text(
        ui.painter(),
        pos2(cell.left() + 16.0, cell.center().y),
        Align2::LEFT_CENTER,
        label,
        17.0,
        Weight::Regular,
        ink,
    );
    if let Some(v) = value {
        let shown = mix(argb(TAB_IDLE), Color32::WHITE, lit);
        let at = pos2(cell.right() - 16.0, cell.center().y);
        text(ui.painter(), at, Align2::RIGHT_CENTER, v, 15.0, Weight::Regular, shown);
    }
}

/// A row with a private action at `cell`: a swipe to the left slides it open (let go past halfway
/// it stays open), its private button coming in from the right; a tap on the row closes it again,
/// one on the button does it privately. Returns what was tapped.
fn private_row(
    ui: &Ui,
    cell: Rect,
    corners: egui::CornerRadius,
    (label, private): (&str, &str),
    (act, private_act): (Option<Act>, Option<Act>),
    swipes: &mut Swipes,
    at: (usize, usize),
) -> Option<Tapped> {
    let now = ui.input(|i| i.time);
    let id = ui.scope_id().with(("private row", at));
    let response = ui.interact(cell, id, Sense::click_and_drag());
    let swipe = swipes.entry(at).or_default();
    if response.drag_started() {
        swipe.dragging = Some(swipe.at(now));
    }
    if let Some(d) = swipe.dragging.as_mut() {
        *d = (*d - response.drag_delta().x).clamp(0.0, PRIVATE_SLIDE);
    }
    if response.drag_stopped() {
        let open = swipe.at(now) > PRIVATE_SLIDE / 2.0;
        swipe.settle(open, now);
    }
    let slide = swipe.at(now);
    if swipe.dragging.is_none() && (now - swipe.since) < f64::from(PRIVATE_SETTLE_S) {
        ui.ctx().request_repaint();
    }
    let painter = ui.painter_at(cell);
    if slide <= 0.0 {
        button_row(ui, &response, cell, corners, label, None, act.is_some());
        return response.clicked().then_some(act).flatten().map(Tapped::Act);
    }
    // slid aside, the row only closes again when tapped
    let ink = if act.is_some() { argb(ACTIONS_ROW) } else { argb(ACTIONS_DISABLED) };
    let label_at = pos2(cell.left() + 16.0 - slide, cell.center().y);
    text(&painter, label_at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, ink);
    let left = cell.right() - PRIVATE_WIDTH + (PRIVATE_SLIDE - slide);
    let button = Rect::from_min_size(pos2(left, cell.top()), vec2(PRIVATE_WIDTH, cell.height()));
    // rounded 13, its outer corners as round as the group's where it meets them
    let round = egui::CornerRadius { nw: 13, sw: 13, ne: corners.ne.max(13), se: corners.se.max(13) };
    painter.rect_filled(button, round, argb(PRIVATE_FILL));
    let ink = if private_act.is_some() { argb(PRIVATE_INK) } else { argb(ACTIONS_DISABLED) };
    text(&painter, button.center(), Align2::CENTER_CENTER, private, 17.0, Weight::Regular, ink);
    let pressed = ui.interact(button.intersect(cell), id.with("private"), Sense::click()).clicked();
    if pressed && private_act.is_some() {
        *swipe = Swipe::default();
        return private_act.map(Tapped::Act);
    }
    if response.clicked() {
        swipe.settle(false, now);
    }
    None
}

/// The rows' height in a group, with the lines between them (none between sliders).
fn group_height_of(rows: &[&Row]) -> f32 {
    rows.iter().map(|r| r.height()).sum::<f32>()
        + rows.windows(2).filter(|w| !matches!(w, [Row::Slide(..), Row::Slide(..)])).count() as f32
}

/// A slider row at `cell`: the name and value on top, the bar under them, blue up to a small white
/// knob; dragged or tapped along, it goes there. Returns where it was set (0..1), while held.
fn slide_row(ui: &Ui, cell: Rect, label: &str, value: &str, fraction: f32, id: (usize, usize)) -> Option<f32> {
    let inset = 16.0;
    let response = ui.interact(cell, ui.scope_id().with(("slide", id)), Sense::click_and_drag());
    let set = (response.is_pointer_button_down_on() || response.clicked())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|p| ((p.x - cell.left() - inset) / (cell.width() - 2.0 * inset)).clamp(0.0, 1.0));
    let painter = ui.painter();
    let (name_at, value_at) = (pos2(cell.left() + 16.0, cell.top() + 9.0), pos2(cell.right() - 19.0, cell.top() + 9.0));
    text(painter, name_at, Align2::LEFT_TOP, label, 17.0, Weight::Regular, argb(SLIDE_LABEL));
    text(painter, value_at, Align2::RIGHT_TOP, value, 17.0, Weight::Regular, argb(SLIDE_VALUE));
    let y = cell.bottom() - 17.5;
    let (from, to) = (cell.left() + inset, cell.right() - inset);
    let x = from + fraction.clamp(0.0, 1.0) * (to - from);
    painter.rect_filled(Rect::from_min_max(pos2(from, y - 1.5), pos2(to, y + 1.5)), 0, argb(SLIDE_TRACK));
    painter.rect_filled(Rect::from_min_max(pos2(from, y - 1.5), pos2(x, y + 1.5)), 0, argb(pc::ACCENT));
    painter.rect_filled(Rect::from_center_size(pos2(x, y), vec2(16.0, 10.0)), 5, Color32::WHITE);
    set
}
