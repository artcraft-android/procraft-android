//! The QuickMenu, as the Android app's (ui/QuickMenuUi.kt): its name in the middle of where it was
//! opened and its six actions round it, spreading out as it opens. An action tapped runs and
//! closes it, a tap anywhere else closes it, and a button held lists the actions it can be set to.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, Id, Order, Pos2, Rect, ScrollArea, Sense, Shape, Stroke, UiBuilder, pos2, vec2};
use procraft_core::{
    BrushInfo, CanvasSession, QuickAction, QuickMenuSet, quick_action_label, quick_actions_listed,
    quick_adjustments_listed, quick_menu_active_after, quick_menu_added, quick_menus, quick_menus_saved,
};

use crate::icons;
use crate::look::{Look, ROW_TRACKING, Weight, argb, cubic_bezier, pc, text, text_tracked};

// glass: the buttons a dark tint over the canvas with a faint rim, the middle one a light tint
const BUTTON_FILL: u32 = 0x9C282828;
const BUTTON_EDGE: u32 = 0x40FFFFFF;
const BUTTON_INK: u32 = 0xFFF7F7F7;
const TITLE_FILL: u32 = 0x94CFCFCF;
const TITLE_INK: u32 = 0xFF0F0F0F;
const BUTTON: (f32, f32) = (171.0, 48.0);
/// Where the buttons' centres are from the middle, clockwise from the top.
const SLOTS: [(f32, f32); 6] =
    [(0.0, -69.0), (191.0, -39.0), (191.0, 39.0), (0.0, 69.0), (-191.0, 39.0), (-191.0, -39.0)];
const REACH: (f32, f32) = (191.0, 69.0);
const OPEN_SECS: f32 = 0.15;
const CLOSE_SECS: f32 = 0.12;
const HOLD: Duration = Duration::from_millis(400);

// the picker (as measured): a dark panel 220 wide, corners 30, an arrow 14 tall; capsule rows 47
// tall on a 50 pitch, 8.5 in from its sides, under a 55.5 tall title
const PICKER_WIDTH: f32 = 220.0;
const PICKER_CORNER: f32 = 30.0;
const PICKER_ARROW: f32 = 14.0;
const PICKER_HEADER: f32 = 55.5;
const ROW_HEIGHT: f32 = 47.0;
const ROW_GAP: f32 = 3.0;
const ROW_PITCH: f32 = ROW_HEIGHT + ROW_GAP;
const PICKER_INSET: f32 = 8.5;
/// The rows' labels: their size, and how far below the row's middle (as measured by ink).
const ROW_TEXT: f32 = 15.1;
const ROW_TEXT_DROP: f32 = 0.75;
const PICKER_HEIGHT: f32 = PICKER_HEADER + ROW_PITCH * 6.0 - ROW_GAP + 6.0;
const PICKER_MARGIN: f32 = 8.0;
/// How far above the panel's foot its rows stop: where its corners have turned in past the rows.
const CORNER_CLEAR: f32 = 9.5;
/// How far below the list's top the chosen row shows.
const CHOSEN_AT: f32 = 89.0;
const PICKER_FILL: u32 = 0xFF373737;
const ROW_FILL: u32 = 0xFF2E2E2E;
const ROW_INK: u32 = 0xFFC5C5C5;
const PICKER_TITLE_INK: u32 = 0xFFC4C4C4;

/// A page of a held button's picker: the actions, the adjustments, the brush sets, a set's brushes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Page {
    Actions,
    Adjustments,
    BrushSets,
    Brushes(usize),
}

/// What a picker's row tapped does: opens a list, or sets the button.
#[derive(Clone, Copy)]
enum Tap {
    Go(Page),
    Action(QuickAction),
    Brush(u32, u32),
}

/// What a tapped button does: its action (the menu opened at the point), or its brush given to the
/// current tool.
pub enum Ran {
    Action(QuickAction, Pos2),
    Brush(BrushInfo),
}

pub struct QuickMenu {
    /// Where it was opened (points; kept while it draws back in), whether it is open, and since
    /// when it has been opening or closing (from how far open).
    at: Option<Pos2>,
    open: bool,
    since: Instant,
    from: f32,
    /// The button whose action is being chosen, the picker's page, and whether its list is still
    /// to be scrolled to the chosen row.
    choosing: Option<usize>,
    page: Page,
    scroll_to: bool,
    /// A button pressed and when (held long enough, it opens the picker and is not tapped).
    press: Option<(usize, Instant, bool)>,
    /// The menus (each six buttons, clockwise from the top), the one kept as active, and the one
    /// the ring shows this time (a menu just added shows without becoming the active one).
    menus: Vec<QuickMenuSet>,
    active: usize,
    shown_menu: usize,
    /// The menus' popover (the middle tapped): open, the menus added while it shows (listed first,
    /// newest first), the row blue, and a row swiped (how far).
    menus_open: bool,
    fresh: Vec<usize>,
    marked: Option<usize>,
    swipe: Option<(usize, f32)>,
}

impl Default for QuickMenu {
    fn default() -> QuickMenu {
        let (kept, active) = crate::files::kept_quick_menus();
        let menus = quick_menus(kept, crate::files::kept_quick_menu());
        let active = active.min(menus.len() - 1);
        QuickMenu {
            at: None,
            open: false,
            since: Instant::now(),
            from: 0.0,
            choosing: None,
            page: Page::Actions,
            scroll_to: false,
            press: None,
            menus,
            active,
            shown_menu: active,
            menus_open: false,
            fresh: Vec::new(),
            marked: None,
            swipe: None,
        }
    }
}

impl QuickMenu {
    /// Opens it at `at` (points), or closes it if it is open.
    pub fn toggle(&mut self, at: Pos2) {
        if self.open {
            self.close();
            return;
        }
        self.at = Some(at);
        self.open = true;
        self.since = Instant::now();
        self.from = 0.0;
        self.choosing = None;
        self.shown_menu = self.active;
    }

    pub fn close(&mut self) {
        if self.open {
            self.from = self.shown();
            self.open = false;
            self.since = Instant::now();
            self.choosing = None;
            self.close_menus();
        }
    }

    fn close_menus(&mut self) {
        self.menus_open = false;
        self.fresh.clear();
        self.swipe = None;
    }

    /// The buttons the ring shows.
    fn slots(&self) -> &[QuickAction] {
        self.menus.get(self.shown_menu).map_or(&[], |m| &m.slots)
    }

    fn keep(&self) {
        crate::files::keep_quick_menus(&quick_menus_saved(self.menus.clone()), self.active);
    }

    /// How far open, 0..1: spreading out as it opens, drawing back in as it closes.
    fn shown(&self) -> f32 {
        let t = self.since.elapsed().as_secs_f32();
        if self.open {
            cubic_bezier(0.25, 0.1, 0.25, 1.0, t / OPEN_SECS)
        } else {
            self.from * (1.0 - cubic_bezier(0.42, 0.0, 1.0, 1.0, t / CLOSE_SECS))
        }
    }

    /// Draws it while it shows. Returns what a button tapped does. A button set to a brush asks the
    /// library to be kept (`keep_brushes`), as it keeps the brush's key.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        keep_brushes: &mut bool,
    ) -> Option<Ran> {
        let at = self.at?;
        let shown = self.shown();
        if !self.open && shown <= 0.0 {
            self.at = None;
            return None;
        }
        // Escape closes the picker if it is open, else the menu, as a tap elsewhere does
        if self.open && crate::ui::escape(ctx) {
            if self.choosing.is_some() {
                self.choosing = None;
            } else if self.menus_open {
                self.close_menus();
            } else {
                self.close();
            }
        }
        if self.since.elapsed().as_secs_f32() < OPEN_SECS.max(CLOSE_SECS) {
            ctx.request_repaint();
        }
        let screen = ctx.content_rect();
        // kept on the screen
        let c = pos2(
            at.x.clamp(
                REACH.0 + BUTTON.0 / 2.0,
                (screen.right() - REACH.0 - BUTTON.0 / 2.0).max(REACH.0 + BUTTON.0 / 2.0),
            ),
            at.y.clamp(REACH.1 + BUTTON.1, (screen.bottom() - REACH.1 - BUTTON.1).max(REACH.1 + BUTTON.1)),
        );
        let alpha = shown.clamp(0.0, 1.0);
        let fade = |argb_colour: u32| argb(argb_colour).gamma_multiply(alpha);
        let mut ran = None;
        egui::Area::new(Id::unique("quick menu")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            // a tap elsewhere closes the picker or the menus if open, else the menu
            if ui.allocate_rect(screen, Sense::click()).clicked() {
                if self.choosing.is_some() {
                    self.choosing = None;
                } else if self.menus_open {
                    self.close_menus();
                } else {
                    self.close();
                }
            }
            let title = Rect::from_center_size(c, vec2(BUTTON.0, BUTTON.1));
            ui.painter().rect_filled(title, BUTTON.1 / 2.0, fade(TITLE_FILL));
            let name = self.menus.get(self.shown_menu).map_or("", |m| m.name.as_str());
            text(ui.painter(), c, Align2::CENTER_CENTER, name, 16.0, Weight::SemiBold, fade(TITLE_INK));
            // the middle tapped lists the menus
            if self.open
                && self.choosing.is_none()
                && ui.interact(title, ui.scope_id().with("quick title"), Sense::click()).clicked()
            {
                if self.menus_open {
                    self.close_menus();
                } else {
                    self.menus_open = true;
                    self.marked = Some(self.active);
                }
            }
            let slots = self.slots().to_vec();
            let brushes: Vec<Option<BrushInfo>> = (0..slots.len()).map(|i| self.slot_brush(session, i)).collect();
            for (i, (&(dx, dy), &action)) in SLOTS.iter().zip(&slots).enumerate() {
                let centre = c + vec2(dx, dy) * shown;
                let button = Rect::from_center_size(centre, vec2(BUTTON.0, BUTTON.1));
                ui.painter().rect_filled(button, BUTTON.1 / 2.0, fade(BUTTON_FILL));
                let edge = Stroke::new(1.0, fade(BUTTON_EDGE));
                ui.painter().rect_stroke(button, BUTTON.1 / 2.0, edge, egui::StrokeKind::Inside);
                let brush = brushes.get(i).cloned().flatten();
                let label = brush.as_ref().map_or_else(|| quick_action_label(action), |b| b.name.clone());
                // on one line, cut by the pill where it is too long (from its left)
                let painter = ui.painter().with_clip_rect(button.intersect(ui.clip_rect()));
                let (at, align) = if crate::look::text_width(&painter, &label, 16.0, Weight::SemiBold) > button.width()
                {
                    (button.left_center(), Align2::LEFT_CENTER)
                } else {
                    (centre, Align2::CENTER_CENTER)
                };
                text(&painter, at, align, &label, 16.0, Weight::SemiBold, fade(BUTTON_INK));
                if !self.open {
                    continue;
                }
                let response = ui.interact(button, ui.scope_id().with(("quick button", i)), Sense::click());
                // held, it lists the actions it can be set to (and is not tapped)
                if response.is_pointer_button_down_on() && !self.menus_open {
                    let (_, since, held) = self.press.get_or_insert((i, Instant::now(), false));
                    if !*held && since.elapsed() >= HOLD {
                        *held = true;
                        self.choosing = Some(i);
                        let adjusts = slots.get(i).copied().is_some_and(procraft_core::quick_action_adjusts);
                        self.page = if adjusts { Page::Adjustments } else { Page::Actions };
                        self.scroll_to = true;
                    } else if !*held {
                        ctx.request_repaint_after(HOLD.saturating_sub(since.elapsed()));
                    }
                }
                let held = self.press.is_some_and(|(p, _, held)| p == i && held);
                if response.clicked() && !held && self.choosing.is_none() && !self.menus_open {
                    ran = Some(brush.map_or(Ran::Action(action, at), Ran::Brush));
                }
                if !response.is_pointer_button_down_on() && self.press.is_some_and(|(p, _, _)| p == i) {
                    self.press = None;
                }
            }
        });
        if let Some(i) = self.choosing {
            let (dx, dy) = SLOTS.get(i).copied().unwrap_or_default();
            let centre_x = c.x + dx;
            let top = c.y + dy - BUTTON.1 / 2.0;
            let brush = self.slot_brush(session, i);
            let picked = self.picker(ctx, look, session, i, brush, (centre_x, top, top + BUTTON.1), screen);
            let set = match picked {
                Some(Tap::Action(a)) => Some((a, 0)),
                Some(Tap::Brush(set, index)) => {
                    let key = session.quick_brush_key(set, index);
                    *keep_brushes |= key != 0;
                    (key != 0).then_some((QuickAction::Brushes, key))
                }
                _ => None,
            };
            if let Some((action, key)) = set {
                if let Some(menu) = self.menus.get_mut(self.shown_menu) {
                    menu.brushes.resize(menu.slots.len(), 0);
                    if let (Some(slot), Some(brush)) = (menu.slots.get_mut(i), menu.brushes.get_mut(i)) {
                        *slot = action;
                        *brush = key;
                    }
                }
                self.keep();
                self.choosing = None;
            }
        }
        if self.menus_open && self.open {
            self.menus_popover(ctx, Rect::from_center_size(c, vec2(BUTTON.0, BUTTON.1)), screen);
        }
        if ran.is_some() {
            self.close();
        }
        ran
    }

    /// The brush button `i` of the ring gives the current tool, if it is set to one still in the
    /// library.
    fn slot_brush(&self, session: &CanvasSession, i: usize) -> Option<BrushInfo> {
        let menu = self.menus.get(self.shown_menu)?;
        let key = menu.brushes.get(i).copied().unwrap_or(0);
        if menu.slots.get(i) != Some(&QuickAction::Brushes) || key == 0 {
            return None;
        }
        session.quick_brush(key)
    }

    /// A held button's picker, hung from the button by an arrow (above it when there is no room
    /// below; `button` its centre, top and bottom): every action by name, the button's own blue,
    /// scrolled to it. Adjustments opens their own list; Select brush the brush sets, then a set's
    /// brushes. Returns what a row tapped sets the button to.
    #[allow(clippy::too_many_arguments)]
    fn picker(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        slot: usize,
        brush: Option<BrushInfo>,
        button: (f32, f32, f32),
        screen: Rect,
    ) -> Option<Tap> {
        let (centre_x, button_top, button_bottom) = button;
        let current = self.slots().get(slot).copied().unwrap_or(QuickAction::None);
        let below = button_bottom + PICKER_ARROW + PICKER_HEIGHT <= screen.bottom() - PICKER_MARGIN;
        let left = (centre_x - PICKER_WIDTH / 2.0)
            .clamp(screen.left() + PICKER_MARGIN, (screen.right() - PICKER_WIDTH - PICKER_MARGIN).max(PICKER_MARGIN));
        let top = if below { button_bottom + PICKER_ARROW } else { button_top - PICKER_ARROW - PICKER_HEIGHT };
        let panel = Rect::from_min_size(pos2(left, top), vec2(PICKER_WIDTH, PICKER_HEIGHT));
        // the page's title, where its way back goes, its rows (what each says, whether it opens a
        // list, what a tap does) and the one blue (in a list of its own, the button's, else the first)
        let sets =
            if matches!(self.page, Page::BrushSets | Page::Brushes(_)) { session.brush_library() } else { Vec::new() };
        let (title, back) = match self.page {
            Page::Actions => ("Set Action", None),
            Page::Adjustments => ("Adjustments", Some(Page::Actions)),
            Page::BrushSets => ("Brush Set", Some(Page::Actions)),
            Page::Brushes(_) => ("Brushes", Some(Page::BrushSets)),
        };
        let rows: Vec<(String, bool, Tap)> = match self.page {
            Page::Actions => quick_actions_listed()
                .into_iter()
                .map(|row| match row {
                    None => ("Adjustments".to_owned(), true, Tap::Go(Page::Adjustments)),
                    Some(QuickAction::Brushes) => {
                        (quick_action_label(QuickAction::Brushes), true, Tap::Go(Page::BrushSets))
                    }
                    Some(a) => (quick_action_label(a), false, Tap::Action(a)),
                })
                .collect(),
            Page::Adjustments => {
                quick_adjustments_listed().into_iter().map(|a| (quick_action_label(a), false, Tap::Action(a))).collect()
            }
            Page::BrushSets => {
                sets.iter().enumerate().map(|(s, set)| (set.name.clone(), true, Tap::Go(Page::Brushes(s)))).collect()
            }
            Page::Brushes(s) => sets.get(s).map_or_else(Vec::new, |set| {
                set.brushes.iter().map(|b| (b.name.clone(), false, Tap::Brush(b.set, b.index))).collect()
            }),
        };
        let chosen = match self.page {
            Page::Actions => quick_actions_listed().iter().position(|&r| r == Some(current)),
            Page::Adjustments => Some(quick_adjustments_listed().iter().position(|&a| a == current).unwrap_or(0)),
            Page::BrushSets => Some(brush.as_ref().map_or(0, |b| b.set as usize)),
            Page::Brushes(s) => Some(brush.as_ref().filter(|b| b.set as usize == s).map_or(0, |b| b.index as usize)),
        };
        let mut picked = None;
        egui::Area::new(Id::unique("quick picker")).fixed_pos(screen.min).order(Order::Tooltip).show(ctx, |ui| {
            // the arrow, its point on the button
            let tip = if below { button_bottom } else { button_top };
            let base = if below { tip + PICKER_ARROW + 1.0 } else { tip - PICKER_ARROW - 1.0 };
            let arrow =
                vec![pos2(centre_x, tip), pos2(centre_x + PICKER_ARROW, base), pos2(centre_x - PICKER_ARROW, base)];
            ui.painter().add(Shape::convex_polygon(arrow, argb(PICKER_FILL), Stroke::NONE));
            let shadow =
                egui::epaint::Shadow { offset: [0, 4], blur: 24, spread: 0, color: Color32::from_black_alpha(0x40) };
            ui.painter().add(shadow.as_shape(panel, PICKER_CORNER));
            ui.painter().rect_filled(panel, PICKER_CORNER, argb(PICKER_FILL));
            ui.allocate_rect(panel, Sense::click());
            // the title; in a list of its own a way back
            let header = Rect::from_min_size(panel.min, vec2(PICKER_WIDTH, PICKER_HEADER));
            let ink = argb(PICKER_TITLE_INK);
            if let Some(back) = back {
                let at = pos2(header.left() + 13.0 + 12.0, header.top() + 15.0 + 12.0);
                look.icon(ui, &icons::CHEVRON_LEFT, at, 24.0, ink);
                let at = pos2(header.left() + 13.0 + 24.0 + 4.0, header.top() + 15.0);
                text(ui.painter(), at, Align2::LEFT_TOP, title, 19.3, Weight::Bold, ink);
                if ui.interact(header, ui.scope_id().with("picker back"), Sense::click()).clicked() {
                    picked = Some(Tap::Go(back));
                }
            } else {
                let at = pos2(header.left() + 21.0, header.top() + 15.0);
                text(ui.painter(), at, Align2::LEFT_TOP, title, 19.3, Weight::Bold, ink);
            }
            // the rows end where the panel's corners start to turn, so none shows past them
            let list = Rect::from_min_max(pos2(panel.left(), header.bottom()), panel.max - vec2(0.0, CORNER_CLEAR));
            let mut area = ScrollArea::vertical()
                .id_salt(("quick picker list", self.page))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
            if std::mem::take(&mut self.scroll_to) {
                // the chosen row a little below the list's top
                let past = ROW_PITCH * 2.0 - CHOSEN_AT;
                let offset = chosen.filter(|&k| k >= 2).map_or(0.0, |k| (k - 2) as f32 * ROW_PITCH + past);
                area = area.vertical_scroll_offset(offset);
            }
            ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
                area.show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, ROW_GAP);
                    for (k, (label, more, tap)) in rows.iter().enumerate() {
                        let (outer, response) = ui.allocate_exact_size(vec2(list.width(), ROW_HEIGHT), Sense::click());
                        let r = outer.shrink2(vec2(PICKER_INSET, 0.0));
                        let on = Some(k) == chosen;
                        let fill = if on { argb(pc::ACCENT) } else { argb(ROW_FILL) };
                        ui.painter().rect_filled(r, ROW_HEIGHT / 2.0, fill);
                        let weight = if on { Weight::SemiBold } else { Weight::Regular };
                        let colour = if on { Color32::WHITE } else { argb(ROW_INK) };
                        let at = pos2(r.left() + 16.0, r.center().y + ROW_TEXT_DROP);
                        text_tracked(
                            ui.painter(),
                            at,
                            Align2::LEFT_CENTER,
                            label,
                            ROW_TEXT,
                            weight,
                            colour,
                            ROW_TRACKING,
                        );
                        if *more {
                            let at = pos2(r.right() - 16.0 - 9.0, r.center().y);
                            look.icon(ui, &icons::CHEVRON_RIGHT, at, 18.0, colour);
                        }
                        if response.clicked() {
                            picked = Some(*tap);
                        }
                    }
                    ui.add_space(6.0);
                });
            });
        });
        // a list opened shows in its place
        if let Some(Tap::Go(page)) = picked {
            self.page = page;
            self.scroll_to = true;
            return None;
        }
        picked
    }
}

// the menus' popover (as measured): the picker's panel, 249 tall, 17 off the middle with its
// arrow's tip 5.5 off it; rows from 54 under its top, 8 in; a plus 21 across and 3.6 thick, 30 from
// its right and 28 from its top; a row swiped 70 shows a red Delete 63 x 42, 4.5 in from its end
const MENUS_HEIGHT: f32 = 249.0;
const MENUS_OFF: f32 = 17.0;
const MENUS_TIP: f32 = 5.5;
const MENUS_ROWS_AT: f32 = 54.0;
const MENUS_INSET: f32 = 8.0;
const PLUS: f32 = 21.0;
const PLUS_WEIGHT: f32 = 3.6;
const PLUS_INK: u32 = 0xFFC0C0C0;
const SWIPE_OPEN: f32 = 70.0;
const DELETE: (f32, f32) = (63.0, 42.0);
const DELETE_IN: f32 = 4.5;
const DELETE_FILL: u32 = 0xFFFE4346;

impl QuickMenu {
    /// The menus, hung from the middle (below it where it fits, else above, else beside): a plus
    /// adds one and shows it, a row tapped makes it the active one, a row swiped left shows Delete
    /// (swiped all the way, it goes at once). Menus added while it shows are listed first.
    fn menus_popover(&mut self, ctx: &egui::Context, middle: Rect, screen: Rect) {
        let size = vec2(PICKER_WIDTH, MENUS_HEIGHT);
        let inside = screen.shrink(PICKER_MARGIN);
        let c = middle.center();
        let places = [
            Rect::from_min_size(pos2(c.x - size.x / 2.0, middle.bottom() + MENUS_OFF), size),
            Rect::from_min_size(pos2(c.x - size.x / 2.0, middle.top() - MENUS_OFF - size.y), size),
            Rect::from_min_size(pos2(middle.right() + MENUS_OFF, c.y - size.y / 2.0), size),
            Rect::from_min_size(pos2(middle.left() - MENUS_OFF - size.x, c.y - size.y / 2.0), size),
        ];
        let [below, ..] = places;
        let (side, panel) =
            places.into_iter().enumerate().find(|(_, r)| inside.contains_rect(*r)).unwrap_or((0, below));
        let a = PICKER_ARROW;
        let arrow = match side {
            0 => [
                pos2(c.x, middle.bottom() + MENUS_TIP),
                pos2(c.x + a, panel.top() + 1.0),
                pos2(c.x - a, panel.top() + 1.0),
            ],
            1 => [
                pos2(c.x, middle.top() - MENUS_TIP),
                pos2(c.x - a, panel.bottom() - 1.0),
                pos2(c.x + a, panel.bottom() - 1.0),
            ],
            2 => [
                pos2(middle.right() + MENUS_TIP, c.y),
                pos2(panel.left() + 1.0, c.y - a),
                pos2(panel.left() + 1.0, c.y + a),
            ],
            _ => [
                pos2(middle.left() - MENUS_TIP, c.y),
                pos2(panel.right() - 1.0, c.y + a),
                pos2(panel.right() - 1.0, c.y - a),
            ],
        };
        let mut chose = None;
        let mut delete = None;
        let mut add = false;
        egui::Area::new(Id::unique("quick menus")).fixed_pos(screen.min).order(Order::Tooltip).show(ctx, |ui| {
            ui.painter().add(Shape::convex_polygon(arrow.to_vec(), argb(PICKER_FILL), Stroke::NONE));
            let shadow =
                egui::epaint::Shadow { offset: [0, 4], blur: 24, spread: 0, color: Color32::from_black_alpha(0x40) };
            ui.painter().add(shadow.as_shape(panel, PICKER_CORNER));
            ui.painter().rect_filled(panel, PICKER_CORNER, argb(PICKER_FILL));
            ui.allocate_rect(panel, Sense::click());
            let at = pos2(panel.left() + 21.0, panel.top() + 16.0);
            text(ui.painter(), at, Align2::LEFT_TOP, "QuickMenu", 19.3, Weight::Bold, argb(PICKER_TITLE_INK));
            let plus = pos2(panel.right() - 30.0, panel.top() + 28.0);
            for bar in [vec2(PLUS, PLUS_WEIGHT), vec2(PLUS_WEIGHT, PLUS)] {
                ui.painter().rect_filled(Rect::from_center_size(plus, bar), PLUS_WEIGHT / 2.0, argb(PLUS_INK));
            }
            if ui
                .interact(Rect::from_center_size(plus, vec2(44.0, 44.0)), ui.scope_id().with("add"), Sense::click())
                .clicked()
            {
                add = true;
            }
            // the menus added this time first, newest first, then the rest as kept
            let order: Vec<usize> =
                self.fresh.iter().copied().chain((0..self.menus.len()).filter(|i| !self.fresh.contains(i))).collect();
            let deletable = self.menus.len() > 1;
            let list = Rect::from_min_max(
                pos2(panel.left(), panel.top() + MENUS_ROWS_AT),
                panel.max - vec2(0.0, CORNER_CLEAR),
            );
            ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
                ScrollArea::vertical()
                    .id_salt("quick menus list")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(0.0, ROW_GAP);
                        for &i in &order {
                            let Some(menu) = self.menus.get(i) else { continue };
                            let (outer, response) =
                                ui.allocate_exact_size(vec2(list.width(), ROW_HEIGHT), Sense::click_and_drag());
                            let r = outer.shrink2(vec2(MENUS_INSET, 0.0));
                            let mut offset = self.swipe.filter(|s| s.0 == i).map_or(0.0, |s| s.1);
                            if deletable && response.dragged() {
                                offset = (offset + response.drag_delta().x).clamp(-r.width(), 0.0);
                                self.swipe = Some((i, offset));
                            }
                            if response.drag_stopped() && self.swipe.is_some_and(|s| s.0 == i) {
                                // past most of the row it goes; past half the Delete it stays open
                                self.swipe = if offset < -r.width() * 0.6 {
                                    delete = Some(i);
                                    None
                                } else if offset < -SWIPE_OPEN / 2.0 {
                                    Some((i, -SWIPE_OPEN))
                                } else {
                                    None
                                };
                                offset = self.swipe.map_or(0.0, |s| s.1);
                            }
                            let moved = r.translate(vec2(offset, 0.0));
                            let on = self.marked == Some(i);
                            let fill = if on { argb(pc::ACCENT) } else { argb(ROW_FILL) };
                            ui.painter().rect_filled(moved, ROW_HEIGHT / 2.0, fill);
                            let (weight, colour) =
                                if on { (Weight::SemiBold, Color32::WHITE) } else { (Weight::Regular, argb(ROW_INK)) };
                            let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(list));
                            let at = pos2(moved.left() + 16.0, moved.center().y + ROW_TEXT_DROP);
                            text_tracked(
                                &painter,
                                at,
                                Align2::LEFT_CENTER,
                                &menu.name,
                                ROW_TEXT,
                                weight,
                                colour,
                                ROW_TRACKING,
                            );
                            if offset < 0.0 {
                                let button = Rect::from_min_size(
                                    pos2(r.right() - DELETE_IN - DELETE.0, r.center().y - DELETE.1 / 2.0),
                                    vec2(DELETE.0, DELETE.1),
                                );
                                // shown as far as the row has moved off it
                                let bare = Rect::from_min_max(pos2(moved.right() + 2.5, r.top()), r.max);
                                let painter = painter.with_clip_rect(painter.clip_rect().intersect(bare));
                                painter.rect_filled(button, DELETE.1 / 2.0, argb(DELETE_FILL));
                                let white = Color32::WHITE;
                                text(
                                    &painter,
                                    button.center(),
                                    Align2::CENTER_CENTER,
                                    "Delete",
                                    13.0,
                                    Weight::SemiBold,
                                    white,
                                );
                                let tapped =
                                    ui.interact(button, ui.scope_id().with(("delete", i)), Sense::click()).clicked();
                                if tapped && offset <= -SWIPE_OPEN {
                                    delete = Some(i);
                                    continue;
                                }
                            }
                            if response.clicked() {
                                // a tap closes a row left open, else chooses the menu
                                if self.swipe.is_some() {
                                    self.swipe = None;
                                } else {
                                    chose = Some(i);
                                }
                            }
                        }
                        ui.add_space(6.0);
                    });
            });
        });
        if add {
            self.menus = quick_menu_added(std::mem::take(&mut self.menus));
            let new = self.menus.len() - 1;
            // it shows, but the active one stays until a row is chosen
            self.fresh.insert(0, new);
            self.shown_menu = new;
            self.marked = None;
            self.swipe = None;
            self.keep();
        } else if let Some(i) = delete {
            self.delete_menu(i);
        } else if let Some(i) = chose {
            self.active = i;
            self.shown_menu = i;
            self.marked = Some(i);
            self.keep();
        }
    }

    /// Deletes menu `i` (never the last one left); the ring shows the one before it if it showed it.
    fn delete_menu(&mut self, i: usize) {
        if self.menus.len() < 2 || i >= self.menus.len() {
            return;
        }
        self.menus.remove(i);
        let after = |k: usize| quick_menu_active_after(k as u32, i as u32) as usize;
        self.active = after(self.active);
        self.shown_menu = after(self.shown_menu);
        self.fresh.retain(|&k| k != i);
        for k in &mut self.fresh {
            if *k > i {
                *k -= 1;
            }
        }
        self.marked = None;
        self.swipe = None;
        self.keep();
    }
}
