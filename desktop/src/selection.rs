//! The selection tool, as the Android app's (ui/SelectionUi.kt): its floating toolbar (Automatic,
//! Freehand, Rectangle and Ellipse; Add, Remove, Invert, Copy & Paste, Feather, Save & Load, Color
//! Fill, Clear) and popovers, and what shows while a selection is made (the rectangle or ellipse
//! dragged out and the freehand path, dashed; the crosshair and the bar while a colour
//! selection's threshold is slid). The shared touches make the selections.

use std::collections::HashMap;

use egui::{
    Align2, Color32, Id, LayerId, Order, Pos2, Rect, Sense, Shape, Stroke, TextureHandle, TextureOptions, pos2, vec2,
};
use procraft_core::{CanvasSession, SelectKind, SelectTouch, SelectionDisplay, SelectionJoin};

use crate::icons;
use crate::look::{Look, Themed, Weight, argb, text};
use crate::transform::{
    Button, DASH, DASH_GAP, MENU_ARROW, MENU_GROUP, MENU_TEXT, MENU_WIDTH, Tapped, button_tip, floating_toolbar,
    menu_slider, menu_title, popover_frame,
};

/// The selection toolbar's buttons with room, and where the window is narrow.
const BUTTON_ROOMY: f32 = 83.0;
const BUTTON_NARROW: f32 = 70.0;
const FEATHER_WIDTH: f32 = 250.0;
const START_DOT: u32 = 0xFFC7C7C7;
const MESSAGE_BAR: Themed = Themed(0xFF606060, 0xFFD0D0D0);
const MESSAGE_TEXT: Themed = Themed(0xFFB5B5B5, 0xFF3B3B3B);
const KINDS: [SelectKind; 4] =
    [SelectKind::Automatic, SelectKind::Freehand, SelectKind::Rectangle, SelectKind::Ellipse];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Popup {
    None,
    Feather,
    Saved,
}

pub struct Selection {
    /// The tool is open, how it selects, and whether new selections take away.
    pub on: bool,
    kind: SelectKind,
    removes: bool,
    /// The colour selection's threshold, kept from one to the next; while slid, where and how far.
    threshold: f32,
    sliding: Option<(Pos2, f32)>,
    /// A rectangle or ellipse being dragged out (window pixels).
    drag: Option<(Pos2, Pos2)>,
    /// Whether there is a selection; Color Fill on; the feather's amount; the popover open; the
    /// selections kept and their pictures.
    has: bool,
    color_fill: bool,
    feather: f32,
    popup: Popup,
    saved: u32,
    pictures: HashMap<u32, TextureHandle>,
}

impl Selection {
    /// The icon of how it selects now, for the pointer over the canvas.
    pub fn kind_icon(&self) -> &'static icons::Icon {
        match self.kind {
            SelectKind::Automatic => &icons::AUTOMATIC,
            SelectKind::Freehand => &icons::FREEHAND,
            SelectKind::Rectangle => &icons::RECTANGLE_SHAPE,
            SelectKind::Ellipse => &icons::ELLIPSE_SHAPE,
        }
    }
}

impl Default for Selection {
    fn default() -> Selection {
        Selection {
            on: false,
            kind: SelectKind::Freehand,
            removes: false,
            threshold: 0.5,
            sliding: None,
            drag: None,
            has: false,
            color_fill: false,
            feather: 0.0,
            popup: Popup::None,
            saved: 0,
            pictures: HashMap::new(),
        }
    }
}

impl Selection {
    /// The tool opens (each time it opens, shapes add to the selection).
    pub fn open(&mut self, session: &CanvasSession) {
        self.removes = false;
        self.on = true;
        self.has = session.has_selection();
        self.touches(session);
        self.look(session);
    }

    /// The tool closes; a freehand path drawn is closed into a selection.
    pub fn close(&mut self, session: &CanvasSession) {
        if !self.on {
            return;
        }
        session.close_freehand();
        session.set_touch_selecting(None);
        self.on = false;
        self.set_popup(session, Popup::None);
        self.drag = None;
        self.sliding = None;
        self.look(session);
    }

    /// The touches select as the tool is set: selecting by colour always adds.
    fn touches(&self, session: &CanvasSession) {
        let join =
            if self.removes && self.kind != SelectKind::Automatic { SelectionJoin::Remove } else { SelectionJoin::Add };
        session.set_touch_selecting(Some(SelectTouch { kind: self.kind, join, threshold: self.threshold }));
    }

    /// The selection shows tinted while selecting by colour, else striped.
    fn look(&self, session: &CanvasSession) {
        let tint = self.on && self.kind == SelectKind::Automatic;
        session.set_selection_display(if tint { SelectionDisplay::Tint } else { SelectionDisplay::Stripes });
    }

    /// Nothing selected any more (a freehand path being drawn closed first), as Deselect does.
    pub fn clear(&mut self, session: &CanvasSession) {
        session.close_freehand();
        session.select_none();
        self.has = false;
        self.look(session);
    }

    /// A rectangle or ellipse is being dragged out.
    pub fn dragged(&mut self, session: &CanvasSession, a: Pos2, b: Pos2) {
        self.drag = Some((a, b));
        self.set_popup(session, Popup::None);
    }

    /// Another popover, or none: the feather popover closing keeps its feathering as one undo step.
    fn set_popup(&mut self, session: &CanvasSession, p: Popup) {
        if self.popup == Popup::Feather && p != Popup::Feather {
            session.select_feather_done();
        }
        self.popup = p;
    }

    /// A colour selection's threshold slid (kept when `done`).
    pub fn slid(&mut self, session: &CanvasSession, at: Pos2, threshold: f32, done: bool) {
        if done {
            self.sliding = None;
            self.threshold = threshold;
            self.touches(session);
        } else {
            self.sliding = Some((at, threshold));
        }
    }

    /// A selection was made: Color Fill fills it.
    pub fn changed(&mut self, session: &CanvasSession) {
        self.drag = None;
        self.has = session.has_selection();
        if self.color_fill && self.has {
            session.fill_selection();
        }
        self.look(session);
    }

    /// Draws what shows while selecting and the toolbar with its popover; does what was asked.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession, bar_top: f32) {
        if !self.on {
            return;
        }
        self.overlay(ctx, session, bar_top);
        let drawing = !session.freehand_path().is_empty();
        let (has, joins) = (self.has, self.kind != SelectKind::Automatic);
        let segments: Vec<(&icons::Icon, &str, bool)> = KINDS
            .iter()
            .map(|&k| {
                let (icon, label) = match k {
                    SelectKind::Automatic => (&icons::AUTOMATIC, "Automatic"),
                    SelectKind::Freehand => (&icons::FREEHAND, "Freehand"),
                    SelectKind::Rectangle => (&icons::RECTANGLE_SHAPE, "Rectangle"),
                    SelectKind::Ellipse => (&icons::ELLIPSE_SHAPE, "Ellipse"),
                };
                (icon, label, self.kind == k)
            })
            .collect();
        let buttons = [
            Button::new(&icons::SELECT_ADD, "Add", 44.0).active(!joins || !self.removes),
            Button::new(&icons::SELECT_REMOVE, "Remove", 44.0).active(joins && self.removes).enabled(joins),
            Button::new(&icons::INVERT, "Invert", 44.0).enabled(has),
            Button::new(&icons::SELECT_COPY_PASTE, "Copy & Paste", 44.0).enabled(has || drawing),
            Button::new(&icons::FEATHER, "Feather", 44.0).active(self.popup == Popup::Feather).enabled(has || drawing),
            Button::new(&icons::SAVE_LOAD, "Save & Load", 44.0)
                .active(self.popup == Popup::Saved)
                .enabled(has || drawing),
            Button::new(&icons::COLOR_FILL, "Color Fill", 37.0).active(self.color_fill),
            Button::new(&icons::CLEAR_SELECTION, "Clear", 44.0).enabled(has || drawing),
        ];
        let tapped =
            floating_toolbar(ctx, look, "selection toolbar", (BUTTON_ROOMY, BUTTON_NARROW), &segments, &buttons, 41.0);
        match tapped {
            Some(Tapped::Background) => self.set_popup(session, Popup::None),
            Some(Tapped::Segment(k)) => {
                session.close_freehand();
                self.kind = KINDS.get(k).copied().unwrap_or(SelectKind::Freehand);
                self.touches(session);
                self.changed(session);
            }
            Some(Tapped::Button(0)) => {
                self.removes = false;
                self.touches(session);
            }
            Some(Tapped::Button(1)) => {
                self.removes = true;
                self.touches(session);
            }
            Some(Tapped::Button(2)) => {
                session.select_invert();
                self.has = session.has_selection();
                self.look(session);
            }
            Some(Tapped::Button(3)) => {
                session.close_freehand();
                if session.copy_selection_to_layer().is_some() {
                    session.select_none();
                }
                self.has = session.has_selection();
                self.look(session);
            }
            Some(Tapped::Button(4)) => self.open_popup(ctx, session, Popup::Feather),
            Some(Tapped::Button(5)) => self.open_popup(ctx, session, Popup::Saved),
            Some(Tapped::Button(6)) => {
                self.color_fill = !self.color_fill;
                if self.color_fill && self.has {
                    session.fill_selection();
                }
            }
            Some(Tapped::Button(_)) => {
                session.set_touch_selecting(None);
                self.touches(session);
                session.select_none();
                self.has = false;
                self.look(session);
            }
            None => {}
        }
        // Escape closes the popover
        if self.popup != Popup::None && crate::ui::escape(ctx) {
            self.set_popup(session, Popup::None);
        }
        self.popover(ctx, look, session);
    }

    fn open_popup(&mut self, ctx: &egui::Context, session: &CanvasSession, p: Popup) {
        self.set_popup(session, if self.popup == p { Popup::None } else { p });
        if self.popup == Popup::Feather {
            self.feather = 0.0;
        }
        self.saved = session.saved_selection_count();
        if self.popup == Popup::Saved {
            self.pictures.clear();
            for i in 0..self.saved {
                if let Some(t) = session.saved_selection_thumbnail(i, 128, 92) {
                    let image =
                        egui::ColorImage::from_rgba_premultiplied([t.width as usize, t.height as usize], &t.rgba);
                    self.pictures
                        .insert(i, ctx.load_texture(format!("saved selection {i}"), image, TextureOptions::LINEAR));
                }
            }
        }
    }

    /// The popover over the toolbar: Feather's amount, or the selections kept.
    fn popover(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession) {
        if self.popup == Popup::None {
            return;
        }
        let index = if self.popup == Popup::Feather { 4 } else { 5 };
        let (tip_x, tip_y) = button_tip(ctx, (BUTTON_ROOMY, BUTTON_NARROW), index, 8);
        let width = if self.popup == Popup::Feather { FEATHER_WIDTH } else { MENU_WIDTH };
        let height = if self.popup == Popup::Feather { 40.0 + 4.0 + 59.0 + 16.0 } else { 40.0 + 8.0 + 172.0 } + 20.0;
        let screen = ctx.content_rect();
        let left = (tip_x - width / 2.0).clamp(screen.left(), screen.right() - width);
        let rect = Rect::from_min_size(pos2(left, tip_y - MENU_ARROW - height), vec2(width, height));
        egui::Area::new(Id::unique("selection popover")).fixed_pos(rect.min).order(Order::Tooltip).show(ctx, |ui| {
            popover_frame(ui, rect, tip_x, tip_y);
            let inner = rect.shrink(10.0);
            if self.popup == Popup::Feather {
                menu_title(ui, inner, "Feather");
                let group =
                    Rect::from_min_size(pos2(inner.left(), inner.top() + 44.0), vec2(inner.width(), 59.0 + 16.0));
                ui.painter().rect_filled(group, 13, argb(MENU_GROUP));
                let amount = self.feather;
                let shown =
                    if amount <= 0.0 { "None".to_owned() } else { format!("{}%", (amount * 100.0).round() as i32) };
                let row = Rect::from_min_size(group.min + vec2(0.0, 8.0), vec2(group.width(), 59.0));
                if let Some(v) = menu_slider(ui, row, "Amount", amount, &shown) {
                    self.feather = v;
                    session.select_feather(v);
                }
                return;
            }
            // the selections kept: a new one, and each to put back
            let ink = argb(MENU_TEXT);
            text(
                ui.painter(),
                pos2(inner.left() + 14.0, inner.top() + 20.0),
                Align2::LEFT_CENTER,
                "Selections",
                24.0,
                Weight::Bold,
                ink,
            );
            let add = Rect::from_min_size(pos2(inner.right() - 40.0, inner.top() + 2.0), vec2(40.0, 36.0));
            look.icon(ui, &icons::ADD, add.center(), 40.0, ink);
            if ui.interact(add, ui.scope_id().with("save selection"), Sense::click()).clicked() {
                self.saved = session.save_selection();
                self.open_popup_refresh(ctx, session);
            }
            for i in 0..self.saved {
                let row = Rect::from_min_size(
                    pos2(inner.left(), inner.top() + 48.0 + 52.0 * i as f32),
                    vec2(inner.width(), 51.0),
                );
                if row.bottom() > inner.bottom() {
                    break;
                }
                ui.painter().rect_filled(row, 13, argb(MENU_GROUP));
                let thumb = Rect::from_min_size(row.min + vec2(0.0, 2.5), vec2(64.0, 46.0));
                let corners = egui::CornerRadius { nw: 13, sw: 13, ne: 0, se: 0 };
                ui.painter().rect_filled(thumb, corners, Color32::BLACK);
                if let Some(t) = self.pictures.get(&i) {
                    let size = t.size_vec2();
                    let k = (thumb.width() / size.x.max(1.0)).min(thumb.height() / size.y.max(1.0));
                    let shown = Rect::from_center_size(thumb.center(), size * k);
                    // rounded with the box where it reaches the box's left side
                    let corners = if shown.left() <= thumb.left() + 0.5 { corners } else { egui::CornerRadius::ZERO };
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    let picture =
                        egui::epaint::RectShape::filled(shown, corners, Color32::WHITE).with_texture(t.id(), uv);
                    ui.painter().add(picture);
                }
                let label = format!("Selection {}", i + 1);
                text(
                    ui.painter(),
                    pos2(thumb.right() + 14.0, row.center().y),
                    Align2::LEFT_CENTER,
                    &label,
                    16.0,
                    Weight::Regular,
                    ink,
                );
                if ui.interact(row, ui.scope_id().with(("saved selection", i)), Sense::click()).clicked() {
                    session.load_selection(i);
                    self.has = session.has_selection();
                    self.look(session);
                }
            }
        });
    }

    /// The kept selections' pictures read again (one was added).
    fn open_popup_refresh(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        self.popup = Popup::None;
        self.open_popup(ctx, session, Popup::Saved);
    }

    /// While a selection is made: the rectangle or ellipse dragged and the freehand path (dashed,
    /// its first point a dot to close it on), and the crosshair and bar while a colour selection's
    /// threshold is slid.
    fn overlay(&self, ctx: &egui::Context, session: &CanvasSession, bar_top: f32) {
        let ppp = ctx.pixels_per_point();
        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::unique("selection overlay")));
        if let Some((a, b)) = self.drag {
            let (a, b) = (a / ppp, b / ppp);
            let r = Rect::from_two_pos(a, b);
            let outline: Vec<Pos2> = if self.kind == SelectKind::Ellipse {
                (0..=96)
                    .map(|k| {
                        let t = k as f32 / 96.0 * std::f32::consts::TAU;
                        pos2(r.center().x + t.cos() * r.width() / 2.0, r.center().y + t.sin() * r.height() / 2.0)
                    })
                    .collect()
            } else {
                vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()]
            };
            dashed(&painter, &outline);
        }
        let path: Vec<Pos2> =
            session.freehand_path().as_chunks::<2>().0.iter().map(|&[x, y]| pos2(x, y) / ppp).collect();
        if let Some(&first) = path.first() {
            dashed(&painter, &path);
            painter.circle_filled(first, 5.5, argb(START_DOT));
        }
        if let Some((at, threshold)) = self.sliding {
            // a thin cross where the colour is picked
            let p = at / ppp;
            let arm = 8.0;
            painter.line_segment([p - vec2(arm, 0.0), p + vec2(arm, 0.0)], Stroke::new(1.0, argb(DASH)));
            painter.line_segment([p - vec2(0.0, arm), p + vec2(0.0, arm)], Stroke::new(1.0, argb(DASH)));
            let screen = ctx.content_rect();
            let band = Rect::from_min_size(pos2(screen.left(), bar_top), vec2(screen.width(), 24.0));
            painter.rect_filled(band, 0, argb(MESSAGE_BAR));
            let reading = format!("Selection Threshold {:.1}%", (threshold * 1000.0).round() / 10.0);
            text(&painter, band.center(), Align2::CENTER_CENTER, &reading, 14.0, Weight::Regular, argb(MESSAGE_TEXT));
        }
    }
}

/// A path drawn as the selection's outline: thin dark dashes over light ones.
fn dashed(painter: &egui::Painter, points: &[Pos2]) {
    painter.extend(Shape::dashed_line_with_offset(points, Stroke::new(0.75, argb(DASH_GAP)), &[5.0], &[5.0], 5.0));
    painter.extend(Shape::dashed_line_with_offset(points, Stroke::new(0.75, argb(DASH)), &[5.0], &[5.0], 0.0));
}
