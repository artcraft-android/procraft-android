//! The Actions panel, as the Android app's (ui/ActionsPanel.kt): six tabs (Add, Canvas, Share,
//! Video, Prefs, Help) of grouped rows. Rows the desktop can't do yet show dimmed, as the Android
//! app's own rows still to come do. Share writes the artwork to a file.

use egui::{Align2, Color32, Rect, ScrollArea, Sense, Ui, UiBuilder, pos2, vec2};

use crate::files::Format;
use crate::icons;
use crate::look::{Look, Weight, argb, highlight, mix, pc, text};
use crate::panel::{self, PANEL_TITLE};

const ACTIONS_WIDTH: f32 = 390.0;
const TAB_IDLE: u32 = 0xFFA3A3A3;
const ACTIONS_GROUP: u32 = 0x420C0C0C;
const ACTIONS_SEPARATOR: u32 = 0x06FFFFFF;
const ACTIONS_ROW: u32 = 0xFFA8A8A8;
const ACTIONS_DISABLED: u32 = 0xFF5E5E5E;
const SWITCH_FIXED: u32 = 0xFF404040;
const SWITCH_FIXED_KNOB: u32 = 0xFF525252;
const ROW_HEIGHT: f32 = 44.0;
const GROUP_GAP: f32 = 20.0;
const HEADING_HEIGHT: f32 = 45.0;
/// The panel ends this far above the window's bottom edge.
const BOTTOM_MARGIN: f32 = 25.0;

/// What a row does, for the app to carry out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    InsertFile,
    Flip { horizontal: bool },
    Share(Format),
    About,
}

enum Row {
    /// A button row, dimmed without anything to do; its value on the right.
    Button(&'static str, Option<&'static str>, Option<Act>),
    /// A switch row: shown as it is (the desktop can't change it yet).
    Switch(&'static str, bool),
    Heading(&'static str),
}

const TABS: [(&str, &icons::Icon); 6] = [
    ("Add", &icons::TAB_ADD),
    ("Canvas", &icons::TAB_CANVAS),
    ("Share", &icons::TAB_SHARE),
    ("Video", &icons::TAB_VIDEO),
    ("Prefs", &icons::TAB_PREFS),
    ("Help", &icons::TAB_HELP),
];

/// The groups of rows of tab `tab`.
fn groups(tab: usize) -> Vec<Vec<Row>> {
    use Row::{Button, Heading, Switch};
    match tab {
        0 => vec![
            vec![
                Button("Insert a file", None, Some(Act::InsertFile)),
                Button("Insert a photo", None, None),
                Button("Take a photo", None, None),
                Button("Add text", None, None),
            ],
            vec![
                Button("Cut", None, None),
                Button("Copy", None, None),
                Button("Copy canvas", None, None),
                Button("Paste", None, None),
            ],
        ],
        1 => vec![
            vec![Button("Crop & Resize", None, None)],
            vec![
                Switch("Animation Assist", false),
                Switch("Page Assist", false),
                Switch("Drawing Guide", false),
                Button("Edit Drawing Guide", None, None),
            ],
            vec![Switch("Reference", false)],
            vec![
                Button("Flip horizontal", None, Some(Act::Flip { horizontal: true })),
                Button("Flip vertical", None, Some(Act::Flip { horizontal: false })),
            ],
            vec![Button("Canvas information", None, None)],
        ],
        2 => vec![
            vec![
                Heading("Share Image"),
                Button("ProCraft", None, Some(Act::Share(Format::Procraft))),
                Button("Procreate", None, Some(Act::Share(Format::Package))),
                Button("PSD", None, Some(Act::Share(Format::Psd))),
                Button("PDF", None, Some(Act::Share(Format::Pdf))),
                Button("JPEG", None, None),
                Button("PNG", None, Some(Act::Share(Format::Png))),
                Button("TIFF", None, Some(Act::Share(Format::Tiff))),
            ],
            vec![
                Heading("Share Layers"),
                Button("PDF", None, Some(Act::Share(Format::LayersPdf))),
                Button("PNG Files", None, Some(Act::Share(Format::LayerPngs))),
                Button("Animated GIF", None, None),
                Button("Animated PNG", None, None),
                Button("Animated MP4", None, None),
                Button("Animated HEVC", None, None),
            ],
        ],
        3 => vec![
            vec![Button("Time-lapse Replay", None, None), Switch("Time-lapse Recording", true)],
            vec![Button("Export time-lapse video", None, None)],
        ],
        4 => vec![
            vec![
                Switch("Light interface", false),
                Switch("Right-hand interface", false),
                Button("Brush scaling", Some("Canvas"), None),
                Switch("Project canvas", false),
                Switch("Brush cursor", true),
                Button("Advanced cursor settings", None, None),
            ],
            vec![Button("Pressure and Smoothing", None, None), Button("Gesture controls", None, None)],
            vec![Switch("Size & Opacity sidebar", true)],
        ],
        _ => vec![vec![Button("About ProCraft", None, Some(Act::About)), Button("Licences", None, None)]],
    }
}

#[derive(Default)]
pub struct Actions {
    tab: usize,
}

impl Actions {
    /// Shows the panel hanging from `button`; returns where it is (points) and the row tapped.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        button: Rect,
        backdrops: &mut Vec<f32>,
    ) -> (Rect, Option<Act>) {
        let screen = ctx.content_rect();
        let (left, arrow_x) =
            panel::place(ACTIONS_WIDTH, button.center().x, screen, pc::SIDE_BAR_WIDTH + panel::POPOVER_GAP);
        let top = screen.top() + pc::BAR_HEIGHT;
        let groups = groups(self.tab);
        let rows: f32 = groups
            .iter()
            .map(|g| {
                g.iter()
                    .map(|r| if matches!(r, Row::Heading(_)) { HEADING_HEIGHT } else { ROW_HEIGHT + 1.0 })
                    .sum::<f32>()
            })
            .sum::<f32>()
            + GROUP_GAP * (groups.len().saturating_sub(1)) as f32;
        let content = panel::ARROW_HEIGHT + 22.0 + 34.0 + 60.0 + 20.0 + rows + 11.0;
        let height = content.min(screen.bottom() - BOTTOM_MARGIN - top);
        let rect = Rect::from_min_size(pos2(left, top), vec2(ACTIONS_WIDTH, height));
        let mut tapped = None;
        egui::Area::new(egui::Id::unique("actions panel")).fixed_pos(rect.min).order(egui::Order::Foreground).show(
            ctx,
            |ui| {
                ui.allocate_rect(rect, Sense::click());
                panel::frame(ui, look, "actions frame", rect, arrow_x, backdrops);
                let body = panel::body(rect);
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
                    let cell =
                        Rect::from_min_size(pos2(body.left() + 10.0 + 64.0 * i as f32, tabs_top), vec2(54.0, 60.0));
                    let response = ui.interact(cell, ui.scope_id().with(("tab", i)), Sense::click());
                    if response.clicked() {
                        self.tab = i;
                    }
                    let lit = i == self.tab || response.is_pointer_button_down_on();
                    // a 50 frame centred 24 down
                    look.icon(
                        ui,
                        icon,
                        pos2(cell.center().x, cell.top() + 24.0),
                        50.0,
                        argb(if lit { pc::ACCENT } else { TAB_IDLE }),
                    );
                    let ink = if lit { Color32::WHITE } else { argb(TAB_IDLE) };
                    text(
                        ui.painter(),
                        pos2(cell.center().x, cell.top() + 41.0),
                        Align2::CENTER_TOP,
                        label,
                        12.0,
                        Weight::Medium,
                        ink,
                    );
                }
                let list = Rect::from_min_max(
                    pos2(body.left() + 12.0, tabs_top + 60.0 + 20.0),
                    pos2(body.right() - 12.0, body.bottom() - 11.0),
                );
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
                                if let Some(act) = group_view(ui, group, g) {
                                    tapped = Some(act);
                                }
                            }
                        });
                });
            },
        );
        (rect, tapped)
    }
}

/// A group: led by its heading (above it, not a row of it), its rows on a dark rounded wash with
/// faint lines between them. Returns the row tapped.
fn group_view(ui: &mut Ui, rows: &[Row], g: usize) -> Option<Act> {
    let width = ui.available_width();
    let mut rows = rows.iter().peekable();
    if let Some(Row::Heading(label)) = rows.peek() {
        let (band, _) = ui.allocate_exact_size(vec2(width, HEADING_HEIGHT), Sense::hover());
        text(ui.painter(), band.min + vec2(13.0, 7.0), Align2::LEFT_TOP, label, 14.0, Weight::Bold, argb(PANEL_TITLE));
        rows.next();
    }
    let rows: Vec<&Row> = rows.collect();
    let height = ROW_HEIGHT * rows.len() as f32 + (rows.len().saturating_sub(1)) as f32;
    let (area, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter().rect_filled(area, 22, argb(ACTIONS_GROUP));
    let mut tapped = None;
    let mut y = area.top();
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(area.left(), y), vec2(width, 1.0)),
                0,
                argb(ACTIONS_SEPARATOR),
            );
            y += 1.0;
        }
        let cell = Rect::from_min_size(pos2(area.left(), y), vec2(width, ROW_HEIGHT));
        // the first and last rows follow the group's rounded corners
        let corner = |at: bool| if at { 22 } else { 0 };
        let corners = egui::CornerRadius {
            nw: corner(i == 0),
            ne: corner(i == 0),
            sw: corner(i + 1 == rows.len()),
            se: corner(i + 1 == rows.len()),
        };
        match row {
            Row::Button(label, value, act) => {
                let response = ui.interact(cell, ui.scope_id().with(("row", g, i)), Sense::click());
                // a button row turns blue while it is held, its text white
                let lit = if act.is_some() {
                    highlight(ui.ctx(), response.id, response.is_pointer_button_down_on())
                } else {
                    0.0
                };
                if lit > 0.0 {
                    ui.painter().rect_filled(cell, corners, argb(pc::ACCENT).gamma_multiply(lit));
                }
                let idle = argb(if act.is_some() { ACTIONS_ROW } else { ACTIONS_DISABLED });
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
                    text(
                        ui.painter(),
                        pos2(cell.right() - 16.0, cell.center().y),
                        Align2::RIGHT_CENTER,
                        v,
                        15.0,
                        Weight::Regular,
                        shown,
                    );
                }
                if response.clicked() {
                    tapped = *act;
                }
            }
            Row::Switch(label, on) => {
                text(
                    ui.painter(),
                    pos2(cell.left() + 16.0, cell.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    17.0,
                    Weight::Regular,
                    argb(ACTIONS_DISABLED),
                );
                // a 63 x 28 switch, shown as it is: the desktop can't change it yet
                let track =
                    Rect::from_min_size(pos2(cell.right() - 8.0 - 63.0, cell.center().y - 14.0), vec2(63.0, 28.0));
                ui.painter().rect_filled(track, 14, argb(SWITCH_FIXED));
                let knob =
                    Rect::from_min_size(track.min + vec2(2.0 + if *on { 22.0 } else { 0.0 }, 2.0), vec2(37.0, 24.0));
                ui.painter().rect_filled(knob, 12, argb(SWITCH_FIXED_KNOB));
            }
            Row::Heading(_) => {}
        }
        y += ROW_HEIGHT;
    }
    tapped
}
