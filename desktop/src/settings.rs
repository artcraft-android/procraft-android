//! Help's Advanced settings: the app's own settings, apart from the canvas, on a page of their own
//! over everything, as the Android app's (SettingsActivity.kt): an artwork opening as it was left,
//! fitting the canvas inside the interface, no time-lapse for new artworks, simplified undos, the
//! maximum speed distance, what an artwork dragged out becomes, the touch companion, colour names,
//! feedback sounds and tap assistance. Kept with the Prefs tab's.

use egui::{Align2, Color32, Id, Order, Rect, ScrollArea, Sense, Stroke, UiBuilder, Vec2, pos2, vec2};

use crate::look::{Weight, argb, text};
use crate::prefs::{DRAG_FORMATS, DRAG_IMAGE_FORMATS, Prefs};

const BACK: u32 = 0xFF000000;
const GROUP: u32 = 0xFF1C1C1E;
const INK: u32 = 0xFFFFFFFF;
const DIM: u32 = 0xFF8E8E93;
const LINE: u32 = 0xFF38383A;
const BLUE: u32 = 0xFF0A84FF;
/// The page's column at most this wide, its contents this far in.
const WIDTH: f32 = 640.0;
const MARGIN: f32 = 24.0;

/// A switch the page sets.
#[derive(Clone, Copy)]
enum Setting {
    OrientationMemory,
    FitInside,
    NoTimeLapse,
    SimplifiedUndos,
    TouchCompanion,
    ColourNames,
    FeedbackSounds,
    TapAssistance,
}

impl Setting {
    fn of(self, p: &mut Prefs) -> &mut bool {
        match self {
            Setting::OrientationMemory => &mut p.orientation_memory,
            Setting::FitInside => &mut p.fit_inside,
            Setting::NoTimeLapse => &mut p.no_time_lapse,
            Setting::SimplifiedUndos => &mut p.simplified_undos,
            Setting::TouchCompanion => &mut p.touch_companion,
            Setting::ColourNames => &mut p.colour_names,
            Setting::FeedbackSounds => &mut p.feedback_sounds,
            Setting::TapAssistance => &mut p.tap_assistance,
        }
    }
}

/// A choice the page sets, one of its options.
#[derive(Clone, Copy)]
enum Pick {
    DragFormat,
    DragImageFormat,
}

impl Pick {
    fn of(self, p: &mut Prefs) -> &mut usize {
        match self {
            Pick::DragFormat => &mut p.drag_format,
            Pick::DragImageFormat => &mut p.drag_image_format,
        }
    }

    fn options(self) -> &'static [&'static str] {
        match self {
            Pick::DragFormat => &DRAG_FORMATS,
            Pick::DragImageFormat => &DRAG_IMAGE_FORMATS,
        }
    }
}

/// A row of the page: a switch with its name and what it does, the speed distance's slider with
/// what it does, or a choice with its name (its value beside it opening a menu of the options).
enum Row {
    Toggle(Setting, &'static str, &'static str),
    SpeedDistance(&'static str),
    Choice(Pick, &'static str),
}

/// The groups of rows, each under its heading (none for "").
const GROUPS: [(&str, &[Row]); 5] = [
    (
        "",
        &[
            Row::Toggle(
                Setting::OrientationMemory,
                "Canvas Orientation Memory",
                "An artwork opens zoomed and turned as it was left.",
            ),
            Row::Toggle(
                Setting::FitInside,
                "Fit Canvas Inside Interface",
                "A fitted canvas keeps clear of the top bar and the sidebar.",
            ),
        ],
    ),
    (
        "",
        &[
            Row::Toggle(Setting::NoTimeLapse, "Disable Time-lapse", "New artworks don't record their time-lapse."),
            Row::Toggle(
                Setting::SimplifiedUndos,
                "Simplified Undos",
                "Strokes made in an adjustment undo together once it is left.",
            ),
        ],
    ),
    (
        "Maximum Speed Distance",
        &[Row::SpeedDistance("How far the pen moves before a brush's speed dynamics count it as fast.")],
    ),
    (
        "Drag and Drop export",
        &[
            Row::Choice(Pick::DragFormat, "Preferred file format"),
            Row::Choice(Pick::DragImageFormat, "Preferred image format"),
        ],
    ),
    (
        "Accessibility",
        &[
            Row::Toggle(
                Setting::TouchCompanion,
                "Single Touch Gestures Companion",
                "A floating bar of undo, redo, zoom, move and fit for one finger.",
            ),
            Row::Toggle(
                Setting::ColourNames,
                "Color Description Notifications",
                "A message names the colour once it stops changing.",
            ),
            Row::Toggle(
                Setting::FeedbackSounds,
                "Feedback Sounds",
                "Undo, redo, tools and panels click as they are used.",
            ),
            Row::Toggle(
                Setting::TapAssistance,
                "Tap Assistance - Initial Touch Location Support",
                "A tap lands where the finger first touched, even if it drifts before lifting.",
            ),
        ],
    ),
];

/// The speed distance's range.
const SPEED_MIN: f32 = 0.25;
const SPEED_MAX: f32 = 1.0;

/// What the page did.
pub enum Said {
    /// A setting changed (kept already).
    Changed,
    Licences,
    Done,
}

/// The page over the window; what was done on it.
pub fn show(ctx: &egui::Context, prefs: &mut Prefs) -> Option<Said> {
    let screen = ctx.content_rect();
    let mut said = None;
    egui::Area::new(Id::unique("advanced settings"))
        .fixed_pos(screen.min)
        .order(Order::Foreground)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.allocate_rect(screen, Sense::click());
            ui.painter().rect_filled(screen, 0, argb(BACK));
            let width = screen.width().min(WIDTH);
            let column =
                Rect::from_min_size(pos2(screen.center().x - width / 2.0, screen.top()), vec2(width, screen.height()));
            let mut page = ui.new_child(UiBuilder::new().max_rect(column));
            ScrollArea::vertical().id_salt("advanced settings").show(&mut page, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.add_space(MARGIN);
                let (title, _) = ui.allocate_exact_size(vec2(width, 36.0), Sense::hover());
                let inner = title.shrink2(vec2(MARGIN, 0.0));
                text(ui.painter(), inner.left_center(), Align2::LEFT_CENTER, "ProCraft", 30.0, Weight::Bold, argb(INK));
                let done = text(
                    ui.painter(),
                    inner.right_center() - vec2(8.0, 0.0),
                    Align2::RIGHT_CENTER,
                    "Done",
                    17.0,
                    Weight::Regular,
                    argb(BLUE),
                );
                if ui.interact(done.expand(8.0), ui.scope_id().with("done"), Sense::click()).clicked() {
                    said = Some(Said::Done);
                }
                for (k, (heading, rows)) in GROUPS.iter().enumerate() {
                    ui.add_space(24.0);
                    let (h, _) = ui.allocate_exact_size(vec2(width, 16.0), Sense::hover());
                    let words = heading.to_uppercase();
                    text(
                        ui.painter(),
                        h.left_center() + vec2(MARGIN + 16.0, 0.0),
                        Align2::LEFT_CENTER,
                        &words,
                        13.0,
                        Weight::Regular,
                        argb(DIM),
                    );
                    ui.add_space(6.0);
                    let inside = width - 2.0 * MARGIN;
                    let tall: Vec<f32> = rows.iter().map(|r| row_height(ui, r, inside)).collect();
                    let total = tall.iter().sum::<f32>() + (tall.len().saturating_sub(1)) as f32;
                    let (group, _) = ui.allocate_exact_size(vec2(width, total), Sense::hover());
                    let group = group.shrink2(vec2(MARGIN, 0.0));
                    ui.painter().rect_filled(group, 12, argb(GROUP));
                    let mut y = group.top();
                    for (i, (row, h)) in rows.iter().zip(&tall).enumerate() {
                        if i > 0 {
                            let line = Rect::from_min_max(pos2(group.left() + 16.0, y), pos2(group.right(), y + 1.0));
                            ui.painter().rect_filled(line, 0, argb(LINE));
                            y += 1.0;
                        }
                        let at = Rect::from_min_size(pos2(group.left(), y), vec2(group.width(), *h));
                        if draw_row(ui, row, at, prefs, (k, i)) {
                            said = Some(Said::Changed);
                        }
                        y += h;
                    }
                }
                // the licences of what the app is made with
                ui.add_space(24.0);
                let (row, response) = ui.allocate_exact_size(vec2(width, 49.0), Sense::click());
                let row = row.shrink2(vec2(MARGIN, 0.0));
                ui.painter().rect_filled(row, 12, argb(GROUP));
                let cy = row.center().y;
                text(
                    ui.painter(),
                    pos2(row.left() + 16.0, cy),
                    Align2::LEFT_CENTER,
                    "Licenses",
                    17.0,
                    Weight::Regular,
                    argb(INK),
                );
                text(
                    ui.painter(),
                    pos2(row.right() - 16.0, cy),
                    Align2::RIGHT_CENTER,
                    "›",
                    20.0,
                    Weight::Regular,
                    argb(DIM),
                );
                if response.clicked() {
                    said = Some(Said::Licences);
                }
                ui.add_space(MARGIN);
            });
        });
    if crate::ui::escape(ctx) {
        said = Some(Said::Done);
    }
    said
}

/// What a row says under its name or slider, wrapped `width` wide.
fn detail(ui: &egui::Ui, words: &str, width: f32) -> std::sync::Arc<egui::Galley> {
    crate::look::wrapped(ui.painter(), words, 13.0, Weight::Regular, argb(DIM), width)
}

/// How tall `row` is, `width` wide.
fn row_height(ui: &egui::Ui, row: &Row, width: f32) -> f32 {
    match row {
        // a line of the name over what it does, 10 above and below; the switch beside them
        Row::Toggle(_, _, what) => 10.0 + 21.0 + detail(ui, what, width - 32.0 - 63.0 - 12.0).size().y + 10.0,
        // the slider 44 tall over what it does, 6 above and below and 4 under the words
        Row::SpeedDistance(what) => 6.0 + 44.0 + detail(ui, what, width - 32.0).size().y + 4.0 + 6.0,
        // a line of the name, 14 above and below
        Row::Choice(..) => 14.0 + 21.0 + 14.0,
    }
}

/// A choice's menu, under its value: the options 48 tall, 12 in, 8 above and below them, the
/// chosen one blue. Returns the one tapped; a tap elsewhere closes it (`close`).
fn choice_menu(ui: &egui::Ui, value: Rect, options: &[&str], chosen: usize, close: &mut bool) -> Option<usize> {
    let widest = options
        .iter()
        .map(|o| ui.painter().layout_no_wrap((*o).into(), Weight::Regular.font(17.0), argb(INK)).size().x)
        .fold(0.0, f32::max);
    let size = vec2((widest + 24.0).max(112.0), 8.0 + 48.0 * options.len() as f32 + 8.0);
    let menu = Rect::from_min_size(value.left_bottom(), size);
    let mut tapped = None;
    egui::Area::new(Id::unique("settings choice menu")).fixed_pos(menu.min).order(Order::Tooltip).show(
        ui.ctx(),
        |ui| {
            let shadow =
                egui::epaint::Shadow { offset: [0, 3], blur: 12, spread: 0, color: Color32::from_black_alpha(0x60) };
            ui.painter().add(shadow.as_shape(menu, 4));
            ui.painter().rect_filled(menu, 4, argb(GROUP));
            for (i, option) in options.iter().enumerate() {
                let item = Rect::from_min_size(
                    pos2(menu.left(), menu.top() + 8.0 + 48.0 * i as f32),
                    vec2(menu.width(), 48.0),
                );
                let ink = if i == chosen { argb(BLUE) } else { argb(INK) };
                text(
                    ui.painter(),
                    pos2(item.left() + 12.0, item.center().y),
                    Align2::LEFT_CENTER,
                    option,
                    17.0,
                    Weight::Regular,
                    ink,
                );
                if ui.interact(item, ui.scope_id().with(("choice", i)), Sense::click()).clicked() {
                    tapped = Some(i);
                }
            }
        },
    );
    let elsewhere =
        ui.ctx().input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !menu.contains(p)));
    *close = tapped.is_some() || elsewhere;
    tapped
}

/// Row `row` at `at`; whether its setting changed (and was kept).
fn draw_row(ui: &egui::Ui, row: &Row, at: Rect, prefs: &mut Prefs, id: (usize, usize)) -> bool {
    match row {
        Row::Toggle(setting, name, what) => {
            let words =
                Rect::from_min_max(at.min + vec2(16.0, 10.0), pos2(at.right() - 16.0 - 63.0 - 12.0, at.bottom()));
            text(ui.painter(), words.min, Align2::LEFT_TOP, name, 17.0, Weight::Regular, argb(INK));
            let galley = detail(ui, what, words.width());
            ui.painter().galley(words.min + vec2(0.0, 21.0), galley, argb(DIM));
            let on = *setting.of(prefs);
            crate::transform::switch(ui, pos2(at.right() - 16.0 - 63.0, at.center().y - 14.0), on, ("settings", id));
            if ui.interact(at, ui.scope_id().with(("settings row", id)), Sense::click()).clicked() {
                *setting.of(prefs) = !on;
                prefs.keep();
                return true;
            }
            false
        }
        Row::SpeedDistance(what) => {
            // a thin track and a round knob; pressed or dragged anywhere along it
            let slider = Rect::from_min_max(at.min + vec2(16.0, 6.0), pos2(at.right() - 16.0, at.top() + 6.0 + 44.0));
            let r = 14.0;
            let (x0, x1, y) = (slider.left() + r, slider.right() - r, slider.center().y);
            let response = ui.interact(slider, ui.scope_id().with(("speed distance", id)), Sense::click_and_drag());
            let mut changed = false;
            if response.is_pointer_button_down_on()
                && let Some(p) = response.interact_pointer_pos()
            {
                let part = ((p.x - x0) / (x1 - x0)).clamp(0.0, 1.0);
                prefs.speed_distance = SPEED_MIN + part * (SPEED_MAX - SPEED_MIN);
                changed = true;
            }
            if response.drag_stopped() || response.clicked() {
                prefs.keep();
            }
            let part = ((prefs.speed_distance - SPEED_MIN) / (SPEED_MAX - SPEED_MIN)).clamp(0.0, 1.0);
            let x = x0 + (x1 - x0) * part;
            let painter = ui.painter();
            painter.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(4.0, argb(LINE)));
            painter.circle_filled(pos2(x0, y), 2.0, argb(BLUE));
            painter.circle_filled(pos2(x1, y), 2.0, argb(LINE));
            painter.line_segment([pos2(x0, y), pos2(x, y)], Stroke::new(4.0, argb(BLUE)));
            painter.circle_filled(pos2(x, y), r, Color32::WHITE);
            let galley = detail(ui, what, slider.width());
            painter.galley(pos2(slider.left(), slider.bottom()), galley, argb(DIM));
            changed
        }
        Row::Choice(pick, name) => {
            let cy = at.center().y;
            text(ui.painter(), pos2(at.left() + 16.0, cy), Align2::LEFT_CENTER, name, 17.0, Weight::Regular, argb(INK));
            let options = pick.options();
            let chosen = (*pick.of(prefs)).min(options.len().saturating_sub(1));
            let shown = options.get(chosen).copied().unwrap_or_default();
            let value = text(
                ui.painter(),
                pos2(at.right() - 16.0, cy),
                Align2::RIGHT_CENTER,
                shown,
                17.0,
                Weight::Regular,
                argb(DIM),
            );
            // which choice's menu is open (one at a time)
            let key = Id::unique("settings choice open");
            let open = ui.ctx().data(|d| d.get_temp::<Option<(usize, usize)>>(key)).flatten();
            if open == Some(id) {
                let mut close = false;
                let tapped = choice_menu(ui, value, options, chosen, &mut close);
                if close {
                    ui.ctx().data_mut(|d| d.insert_temp(key, None::<(usize, usize)>));
                }
                if let Some(i) = tapped {
                    *pick.of(prefs) = i;
                    prefs.keep();
                    return true;
                }
            } else if ui.interact(at, ui.scope_id().with(("settings row", id)), Sense::click()).clicked() {
                ui.ctx().data_mut(|d| d.insert_temp(key, Some(id)));
            }
            false
        }
    }
}
