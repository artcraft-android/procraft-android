//! The Actions panel, as the Android app's (ui/ActionsPanel.kt): six tabs (Add, Canvas, Share,
//! Video, Prefs, Help) of grouped rows, and the pages Prefs opens (Advanced cursor, Pressure and
//! Smoothing). Rows the desktop can't do yet show dimmed, as the Android app's own rows still to
//! come do. Share writes the artwork to a file.

use egui::{Align2, Color32, Id, Order, Pos2, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};

use crate::animated_export::Animated;
use crate::files::Format;
use crate::icons;
use crate::look::{Look, Weight, argb, highlight, mix, pc, text};
use crate::menu::{Glass, Menu};
use crate::panel::{self, PANEL_TITLE};
use crate::prefs::{CursorShows, CursorStyle, Prefs, RAPID_UNDO_MAX_S, RAPID_UNDO_MIN_S};

const ACTIONS_WIDTH: f32 = 390.0;
const TAB_IDLE: u32 = 0xFFA3A3A3;
const ACTIONS_GROUP: u32 = 0x420C0C0C;
const ACTIONS_SEPARATOR: u32 = 0x06FFFFFF;
const ACTIONS_ROW: u32 = 0xFFA8A8A8;
const ACTIONS_DISABLED: u32 = 0xFF5E5E5E;
const SWITCH_FIXED: u32 = 0xFF404040;
const SWITCH_FIXED_KNOB: u32 = 0xFF525252;
const SLIDE_LABEL: u32 = 0xFFC1C1C1;
const SLIDE_VALUE: u32 = 0xFFEDEDED;
const SLIDE_TRACK: u32 = 0xFF414141;
const CURSOR_HEADING: u32 = 0xFFB5B5B5;
const CURVE_FILL: u32 = 0xFF262626;
const CURVE_GRID: u32 = 0xFF353535;
const CURVE_EDGE: u32 = 0xFF666666;
const NODE_MENU_FILL: u32 = 0xFF101010;
const NODE_MENU_WIDTH: f32 = 70.0;
const NODE_MENU_ROW: f32 = 36.0;
/// Brush scaling's menu: its fill and rim, a radio's ring.
const CHOICE_MENU: Glass = Glass { fill: 0xFF5A5C5B, edge: Some(0x33FFFFFF), frost: false, corner: 26.0 };
const RADIO_RING: u32 = 0xFF606060;
const ROW_HEIGHT: f32 = 44.0;
const CHOICE_HEIGHT: f32 = 42.0;
const SLIDE_HEIGHT: f32 = 63.0;
const GROUP_GAP: f32 = 20.0;
const HEADING_HEIGHT: f32 = 45.0;
/// The panel ends this far above the window's bottom edge, and its rows scroll past this tall.
const BOTTOM_MARGIN: f32 = 25.0;
const MOST_ROWS: f32 = 760.0;

/// What a row does. Those the panel does itself (a page, a menu, a setting) never reach the app;
/// [`Act::Prefs`] tells it a setting changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    InsertFile,
    InsertPhoto,
    Flip { horizontal: bool },
    Share(Format),
    AnimatedExport(Animated),
    About,
    Licences,
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
    RightHanded,
    BrushCursor,
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
    Stabilization,
    MotionFiltering,
    MotionExpression,
}

enum Row {
    /// A button row, dimmed without anything to do; its value on the right.
    Button(&'static str, Option<&'static str>, Option<Act>),
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
}

/// The groups of rows of tab `tab`.
fn groups(tab: usize, showing: &Showing, prefs: &Prefs) -> Vec<Vec<Row>> {
    use Row::{Button, Heading, Slide, Switch};
    match tab {
        0 => vec![
            vec![
                Button("Insert a file", None, Some(Act::InsertFile)),
                Button("Insert a photo", None, Some(Act::InsertPhoto)),
                Button("Take a photo", None, None),
                Button("Add text", None, None),
            ],
            vec![
                Button("Cut", None, Some(Act::Cut)),
                Button("Copy", None, Some(Act::Copy)),
                Button("Copy canvas", None, Some(Act::CopyCanvas)),
                Button("Paste", None, showing.can_paste.then_some(Act::Paste)),
            ],
        ],
        1 => vec![
            vec![Button("Crop & Resize", None, Some(Act::Crop))],
            vec![
                Switch("Animation Assist", showing.animation, Some(Act::AnimationAssist)),
                Switch("Page Assist", showing.pages, Some(Act::PageAssist)),
                Switch("Drawing Guide", showing.guide, Some(Act::DrawingGuide)),
                Button("Edit Drawing Guide", None, Some(Act::EditGuide)),
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
            vec![
                vec![
                    Switch("Light interface", false, None),
                    Switch("Right-hand interface", prefs.right_handed, Some(Act::RightHanded)),
                    Button(
                        "Brush scaling",
                        Some(if prefs.brush_on_screen { "Screen" } else { "Canvas" }),
                        Some(Act::ScalingMenu),
                    ),
                    Switch("Project canvas", false, None),
                    Switch("Brush cursor", prefs.brush_cursor, Some(Act::BrushCursor)),
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
                ],
                vec![Switch("Size & Opacity sidebar", showing.side_bar, Some(Act::SideBar))],
            ]
        }
        _ => vec![
            vec![Button("About ProCraft", None, Some(Act::About)), Button("Licences", None, Some(Act::Licences))],
            vec![Button("Advanced settings", None, None)],
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
    page: Page,
    /// Brush scaling's menu, and the row it hangs from.
    scaling: Menu,
    scaling_row: Option<Rect>,
    /// The pressure curve: the node pressed (its index, whether it was just added and whether it
    /// moved), and the node whose menu shows.
    curve_press: Option<(usize, bool, bool)>,
    node_menu: Option<usize>,
    /// The frame the node menu opened in (the press that opened it closes nothing).
    node_menu_frame: u64,
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
        let groups = groups(tab, showing, prefs);
        let header = 12.0 + 44.0;
        let content = panel::ARROW_HEIGHT
            + match self.page {
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
            };
        let height = content.min(screen.bottom() - BOTTOM_MARGIN - top);
        let rect = Rect::from_min_size(pos2(left, top), vec2(ACTIONS_WIDTH, height));
        let mut tapped = None;
        egui::Area::new(Id::unique("actions panel")).fixed_pos(rect.min).order(Order::Foreground).show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click());
            panel::frame(ui, look, "actions frame", rect, arrow_x, backdrops);
            let body = panel::body(rect);
            tapped = match self.page {
                Page::Tabs => self.tabs(ui, look, body, &groups),
                Page::Cursor => self.cursor_page(ui, look, body, prefs),
                Page::Pressure => self.pressure_page(ui, look, body, prefs),
            };
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
            Some(Tapped::Act(Act::RightHanded)) => {
                prefs.right_handed = !prefs.right_handed;
                Some(Act::Prefs)
            }
            Some(Tapped::Act(Act::BrushCursor)) => {
                prefs.brush_cursor = !prefs.brush_cursor;
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
        if self.node_menu(ctx, prefs) {
            act = Some(Act::Prefs);
        }
        (rect, act)
    }

    /// One of its menus is open: a press elsewhere only closes it.
    pub fn modal(&self) -> bool {
        self.scaling.is_open() || self.node_menu.is_some()
    }

    /// The panel's pages go back to the tabs (as it closes).
    pub fn closed(&mut self) {
        self.page = Page::Tabs;
        self.scaling.close();
        self.node_menu = None;
        self.curve_press = None;
    }

    /// The tabs page: the title, the tabs, and the open tab's groups.
    fn tabs(&mut self, ui: &mut Ui, look: &mut Look, body: Rect, groups: &[Vec<Row>]) -> Option<Tapped> {
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
        for (i, (label, icon)) in TABS.iter().enumerate() {
            let cell = Rect::from_min_size(pos2(body.left() + 10.0 + 64.0 * i as f32, tabs_top), vec2(54.0, 60.0));
            let response = ui.interact(cell, ui.scope_id().with(("tab", i)), Sense::click());
            if response.clicked() {
                self.tab = Some(i);
            }
            let lit = Some(i) == self.tab || response.is_pointer_button_down_on();
            // a 50 frame centred 24 down
            let tint = argb(if lit { pc::ACCENT } else { TAB_IDLE });
            look.icon(ui, icon, pos2(cell.center().x, cell.top() + 24.0), 50.0, tint);
            let ink = if lit { Color32::WHITE } else { argb(TAB_IDLE) };
            let at = pos2(cell.center().x, cell.top() + 41.0);
            text(ui.painter(), at, Align2::CENTER_TOP, label, 12.0, Weight::Medium, ink);
        }
        let list = Rect::from_min_max(
            pos2(body.left() + 12.0, tabs_top + 60.0 + 20.0),
            pos2(body.right() - 12.0, body.bottom() - 11.0),
        );
        let mut tapped = None;
        self.scaling_row = None;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ScrollArea::vertical()
                .id_salt(("actions rows", self.tab))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for (g, group) in groups.iter().enumerate() {
                        if g > 0 {
                            ui.add_space(GROUP_GAP);
                        }
                        let (t, cells) = group_view(ui, group, g);
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
                text(ui.painter(), at, Align2::LEFT_CENTER, label, 14.5, Weight::Regular, Color32::WHITE);
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
            self.node_menu = None;
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
                    tapped = group_view(ui, &smoothing_rows(prefs), 0).0;
                    ui.add_space(16.0);
                    // the stroke always starts at the pointer's tip here
                    group_view(ui, &[Row::Fixed("Tip Attachment", true)], 1);
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
                    if self.pressure_curve(ui, square, &mut prefs.curve) {
                        tapped = Some(Tapped::Act(Act::Prefs));
                    }
                    ui.add_space(27.0);
                    let (t, _) = group_view(ui, &[Row::Button("Reset all", None, Some(Act::ResetInput))], 2);
                    tapped = tapped.or(t);
                    ui.add_space(13.0);
                });
        });
        tapped
    }

    /// The pressure curve in `rect`: a press off the nodes adds one there, a node is dragged
    /// between its neighbours, and a tap on a node shows its menu. True when it changed.
    fn pressure_curve(&mut self, ui: &Ui, rect: Rect, curve: &mut Vec<[f32; 2]>) -> bool {
        let response = ui.interact(rect, ui.scope_id().with("pressure curve"), Sense::click_and_drag());
        let to_curve = |p: Pos2| {
            [
                ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
                (1.0 - (p.y - rect.top()) / rect.height()).clamp(0.0, 1.0),
            ]
        };
        let mut changed = false;
        let down = response.is_pointer_button_down_on();
        if down
            && self.curve_press.is_none()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            self.node_menu = None;
            let at = to_curve(origin);
            let near = curve.iter().position(|p| (p[0] - at[0]).hypot(p[1] - at[1]) < 0.05);
            let index = near.unwrap_or_else(|| {
                curve.push(at);
                curve.sort_by(|a, b| a[0].total_cmp(&b[0]));
                changed = true;
                curve.iter().position(|p| *p == at).unwrap_or(0)
            });
            self.curve_press = Some((index, near.is_none(), false));
        } else if response.clicked()
            && self.curve_press.is_none()
            && let Some(p) = response.interact_pointer_pos()
        {
            // pressed and let go within a frame: on a node its menu, elsewhere a node added
            let at = to_curve(p);
            match curve.iter().position(|p| (p[0] - at[0]).hypot(p[1] - at[1]) < 0.05) {
                Some(index) => {
                    self.node_menu = Some(index);
                    self.node_menu_frame = ui.ctx().cumulative_frame_nr();
                }
                None => {
                    curve.push(at);
                    curve.sort_by(|a, b| a[0].total_cmp(&b[0]));
                    changed = true;
                }
            }
        }
        if let Some((index, _, moved)) = self.curve_press.as_mut() {
            if down
                && response.dragged()
                && let Some(p) = response.interact_pointer_pos()
            {
                *moved = true;
                let [x, y] = to_curve(p);
                let lo = index.checked_sub(1).and_then(|i| curve.get(i)).map_or(0.0, |p| p[0]);
                let hi = curve.get(*index + 1).map_or(1.0, |p| p[0]);
                if let Some(node) = curve.get_mut(*index) {
                    *node = [x.clamp(lo, hi.max(lo)), y];
                    changed = true;
                }
            }
            if !down {
                // a tap on a node shows what can be done with it
                if let Some((index, false, false)) = self.curve_press {
                    self.node_menu = Some(index);
                    self.node_menu_frame = ui.ctx().cumulative_frame_nr();
                }
                self.curve_press = None;
            }
        }
        let painter = ui.painter();
        painter.rect_filled(rect, 0, argb(CURVE_FILL));
        for k in 1..4 {
            let f = k as f32 / 4.0;
            let x = rect.left() + rect.width() * f;
            let y = rect.top() + rect.height() * f;
            painter.line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(1.0, argb(CURVE_GRID)));
            painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, argb(CURVE_GRID)));
        }
        painter.rect_stroke(rect, 0, Stroke::new(1.0, argb(CURVE_EDGE)), egui::StrokeKind::Inside);
        let samples = procraft_core::curve_samples(curve.iter().flatten().copied().collect(), 129);
        let last = samples.len().saturating_sub(1).max(1) as f32;
        let line: Vec<Pos2> = samples
            .iter()
            .enumerate()
            .map(|(i, v)| pos2(rect.left() + rect.width() * i as f32 / last, rect.top() + (1.0 - v) * rect.height()))
            .collect();
        painter.add(egui::Shape::line(line, Stroke::new(1.5, Color32::WHITE)));
        let place = |[x, y]: [f32; 2]| pos2(rect.left() + x * rect.width(), rect.top() + (1.0 - y) * rect.height());
        for p in curve.iter() {
            painter.circle_filled(place(*p), 7.0, Color32::WHITE);
            painter.circle_filled(place(*p), 5.0, argb(pc::ACCENT));
        }
        // where the node menu goes: centred over its node, its bottom 11 above it
        if let Some(i) = self.node_menu
            && let Some(p) = curve.get(i)
        {
            let rows = if i == 0 || i + 1 == curve.len() { 1.0 } else { 2.0 };
            let at = place(*p);
            let menu = Rect::from_min_size(
                pos2(at.x - NODE_MENU_WIDTH / 2.0, at.y - 11.0 - NODE_MENU_ROW * rows),
                vec2(NODE_MENU_WIDTH, NODE_MENU_ROW * rows),
            );
            ui.ctx().data_mut(|d| d.insert_temp(Id::unique("node menu at"), menu));
        }
        changed
    }

    /// The menu over a curve node, while it shows: Delete (a node between the ends) and Reset (the
    /// curve straight again). A press elsewhere closes it. True when the curve changed.
    fn node_menu(&mut self, ctx: &egui::Context, prefs: &mut Prefs) -> bool {
        let Some(index) = self.node_menu else { return false };
        let Some(menu) = ctx.data(|d| d.get_temp::<Rect>(Id::unique("node menu at"))) else { return false };
        if self.page != Page::Pressure || index >= prefs.curve.len() {
            self.node_menu = None;
            return false;
        }
        let end = index == 0 || index + 1 == prefs.curve.len();
        let rows: &[&str] = if end { &["Reset"] } else { &["Delete", "Reset"] };
        let mut picked = None;
        egui::Area::new(Id::unique("curve node menu")).fixed_pos(menu.min).order(Order::Tooltip).show(ctx, |ui| {
            ui.allocate_rect(menu, Sense::click());
            let shadow =
                egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(0x80) };
            ui.painter().add(shadow.as_shape(menu, 12.0));
            ui.painter().rect_filled(menu, 12, argb(NODE_MENU_FILL));
            for (k, label) in rows.iter().enumerate() {
                let row = Rect::from_min_size(
                    pos2(menu.left(), menu.top() + NODE_MENU_ROW * k as f32),
                    vec2(menu.width(), NODE_MENU_ROW),
                );
                text(ui.painter(), row.center(), Align2::CENTER_CENTER, label, 17.0, Weight::Regular, Color32::WHITE);
                if ui.interact(row, ui.scope_id().with(("node", k)), Sense::click()).clicked() {
                    picked = Some(*label);
                }
            }
        });
        let pressed_elsewhere =
            ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !menu.contains(p)));
        match picked {
            Some("Delete") => {
                prefs.curve.remove(index);
                self.node_menu = None;
                true
            }
            Some(_) => {
                prefs.curve = crate::prefs::straight();
                self.node_menu = None;
                true
            }
            None => {
                if pressed_elsewhere && ctx.cumulative_frame_nr() != self.node_menu_frame {
                    self.node_menu = None;
                }
                false
            }
        }
    }
}

/// A radio mark centred at `at`: blue with a tick when chosen, else a grey ring.
fn radio(ui: &Ui, look: &mut Look, at: Pos2, chosen: bool) {
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
fn group_view(ui: &mut Ui, rows: &[Row], g: usize) -> (Option<Tapped>, Vec<Rect>) {
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
                // a button row turns blue while it is held, its text white
                let held = response.is_pointer_button_down_on();
                let lit = if act.is_some() { highlight(ui.ctx(), response.id, held) } else { 0.0 };
                if lit > 0.0 {
                    ui.painter().rect_filled(cell, corners, argb(pc::ACCENT).gamma_multiply(lit));
                }
                let idle = argb(if act.is_some() { ACTIONS_ROW } else { ACTIONS_DISABLED });
                let ink = mix(idle, Color32::WHITE, lit);
                text(ui.painter(), label_at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, ink);
                if let Some(v) = value {
                    let shown = mix(argb(TAB_IDLE), Color32::WHITE, lit);
                    let at = pos2(cell.right() - 16.0, cell.center().y);
                    text(ui.painter(), at, Align2::RIGHT_CENTER, v, 15.0, Weight::Regular, shown);
                }
                if response.clicked() {
                    tapped = act.map(Tapped::Act);
                }
            }
            Row::Switch(label, on, Some(act)) => {
                text(ui.painter(), label_at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, argb(ACTIONS_ROW));
                crate::transform::switch(ui, switch_at, *on);
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
