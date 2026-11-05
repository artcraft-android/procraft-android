//! The colour panel floating: its grabber dragged out onto the canvas makes it a window of its
//! own, 260 wide, moved by its grabber and closed by its Close (the colour button then opens the
//! panel again). The same colour, mode and Harmony kind as the panel; no swatches, history or
//! palette under the pickers, the modes as icons under them.

use egui::{Align2, Color32, Pos2, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use procraft_core::CanvasSession;

use super::{
    Colours, EMPTY_SLOT, GRABBER, HarmonyKind, Mode, PALETTE_TILE, colours_in_p3, hex_pad, hue_slider, of_hex,
    rgb_to_hsv, srgb, take_rgb,
};
use crate::compact_colours::{tabs_row, value_view};
use crate::icons;
use crate::look::{Look, Weight, argb, pc, text};
use crate::panel::{self, PANEL_TITLE};

pub(super) const WIDTH: f32 = 260.0;
/// Where the grabber is held from: its middle.
pub(super) const GRAB: Vec2 = vec2(WIDTH / 2.0, 7.5);
const TABS: f32 = 52.0;
/// Groups and fields: white over the frosted window.
const GROUP: u32 = 0x0FFFFFFF;
const FIELD: u32 = 0x0FFFFFFF;
/// Close: a disc of white over the window with its cross cut out of it (drawn darker).
const CLOSE_DISC: u32 = 0x26FFFFFF;
const CLOSE_CROSS: u32 = 0x4D000000;
/// A palette's swatches: six to a row, 40 square; or, as cards, named tiles two to a row.
const SWATCH: f32 = 40.0;
const ACROSS: usize = 6;
const TILES_ACROSS: usize = 2;

impl Colours {
    /// The panel floats from `at` (the window's top-left), the pointer holding its grabber.
    pub(super) fn float_at(&mut self, at: Pos2) {
        self.floating = Some(at);
        self.floating_grab = Some(GRAB);
    }

    /// The floating window, while there is one: returns where it is.
    pub fn show_floating(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        backdrops: &mut Vec<f32>,
    ) -> Option<Rect> {
        let mut at = self.floating?;
        let height = if self.mode == Mode::Disc { 351.0 } else { 390.0 };
        // held by its grabber, it follows the pointer until let go
        let (pointer, down) = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        if let Some(grab) = self.floating_grab {
            match pointer {
                Some(p) if down => at = p - grab,
                _ => self.floating_grab = None,
            }
        }
        // kept on the screen
        let screen = ctx.content_rect();
        at.x = at.x.clamp(screen.left(), (screen.right() - WIDTH).max(screen.left()));
        at.y = at.y.clamp(screen.top(), (screen.bottom() - height).max(screen.top()));
        self.floating = Some(at);
        let rect = Rect::from_min_size(at, vec2(WIDTH, height));
        let before = self.hsv;
        let p3 = colours_in_p3(session.profile());
        // Escape closes it, as its Close does (a menu of its, or the hex keypad, takes it first)
        let mut closed = !self.modal() && crate::ui::escape(ctx);
        let mut hex_row = Rect::NOTHING;
        let mut kind_button = Rect::NOTHING;
        egui::Area::new(egui::Id::unique("floating colours")).fixed_pos(rect.min).order(egui::Order::Foreground).show(
            ctx,
            |ui| {
                // the canvas under it takes no touch
                ui.allocate_rect(rect, Sense::click_and_drag());
                panel::floating_frame(ui, look, "floating colours", rect, backdrops);
                let grabber = Rect::from_min_size(pos2(rect.center().x - 20.0, rect.top() + 5.5), vec2(40.0, 4.0));
                ui.painter().rect_filled(grabber, 2, argb(GRABBER));
                let handle = Rect::from_center_size(pos2(rect.center().x, rect.top() + 15.0), vec2(150.0, 30.0));
                let held = ui.interact(handle, ui.scope_id().with("floating grabber"), Sense::drag());
                if held.drag_started()
                    && let Some(p) = held.interact_pointer_pos()
                {
                    self.floating_grab = Some(p - at);
                }
                let title = if self.mode == Mode::Palettes { "Palettes" } else { "Colors" };
                let ink = argb(PANEL_TITLE);
                text(ui.painter(), pos2(at.x + 18.0, at.y + 24.0), Align2::LEFT_CENTER, title, 24.0, Weight::Bold, ink);
                let close = Rect::from_min_size(pos2(at.x + 216.0, at.y + 4.0), vec2(40.0, 40.0));
                close_button(ui, close);
                closed |= ui.interact(close, ui.scope_id().with("floating close"), Sense::click()).clicked();
                match self.mode {
                    Mode::Disc => {
                        let disc = Rect::from_min_size(pos2(at.x + 10.0, at.y + 53.0), vec2(240.0, 240.0));
                        self.model_disc(ui, disc);
                    }
                    Mode::Classic => {
                        let square = Rect::from_min_size(pos2(at.x + 6.0, at.y + 50.0), vec2(248.0, 248.0));
                        self.pickers.square(ui, &mut self.hsv, square);
                        let row = Rect::from_min_size(pos2(at.x + 10.0, at.y + 298.0), vec2(240.0, 40.0));
                        hue_slider(ui, row, "floating hue", &mut self.hsv);
                    }
                    Mode::Harmony => {
                        // the kind (its menu the panel's), the wheel, then the brightness
                        let label = self.harmony.label();
                        let at_label = pos2(at.x + 18.0, at.y + 58.0);
                        let shown =
                            text(ui.painter(), at_label, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, ink);
                        kind_button =
                            Rect::from_min_max(pos2(shown.left(), at.y + 44.0), pos2(shown.right(), at.y + 72.0));
                        if ui
                            .interact(kind_button, ui.scope_id().with("floating harmony kind"), Sense::click())
                            .clicked()
                        {
                            self.harmony_menu.open();
                        }
                        let wheel = Rect::from_min_size(pos2(at.x + 24.0, at.y + 75.0), vec2(212.0, 212.0));
                        self.pickers.harmony(ui, &mut self.hsv, wheel, self.harmony, p3);
                        let row = Rect::from_min_size(pos2(at.x + 10.0, at.y + 298.0), vec2(240.0, 40.0));
                        self.pickers.harmony_brightness(ui, row, &mut self.hsv, p3);
                    }
                    Mode::Value => {
                        hex_row = value_view(
                            ui,
                            (at.x + 10.0, at.y + 48.0),
                            &mut self.hsv,
                            &mut self.floating_rgb,
                            &mut self.hex,
                            (GROUP, FIELD),
                        );
                    }
                    Mode::Palettes => self.floating_palettes(ui, look, session, rect),
                }
                let tabs = Rect::from_min_size(pos2(at.x, rect.bottom() - TABS), vec2(WIDTH, TABS));
                if let Some(mode) = tabs_row(ui, look, tabs, self.mode) {
                    self.mode = mode;
                    self.hex = None;
                }
            },
        );
        if self.mode == Mode::Harmony && kind_button != Rect::NOTHING {
            let menu = Rect::from_min_size(
                pos2(kind_button.left() - 2.0, kind_button.bottom() + 8.0),
                vec2(195.0, 9.0 * 2.0 + 38.0 * HarmonyKind::ALL.len() as f32),
            );
            self.harmony_menu_in(ctx, look, menu, backdrops);
        }
        if self.mode == Mode::Palettes {
            self.palette_menu(ctx, look, backdrops);
        }
        if let Some(rgb) = hex_pad(ctx, look, &mut self.hex, hex_row) {
            take_rgb(&mut self.hsv, rgb);
        }
        if self.hsv != before {
            self.send(session);
        }
        if closed {
            self.floating = None;
            self.floating_grab = None;
            self.hex = None;
            self.harmony_menu.close();
        }
        Some(rect)
    }

    /// Palettes floating: Compact or Cards, then each palette as a card (the default one ticked, a
    /// tick choosing it) of swatches six to a row, or of named tiles two to a row, a tap taking a
    /// colour.
    fn floating_palettes(&mut self, ui: &mut Ui, look: &mut Look, session: &CanvasSession, rect: Rect) {
        let switch = Rect::from_min_size(pos2(rect.left() + 10.0, rect.top() + 48.0), vec2(240.0, 36.0));
        ui.painter().rect_filled(switch, 9, argb(GROUP));
        for (i, label) in ["Compact", "Cards"].into_iter().enumerate() {
            let half = Rect::from_min_size(pos2(switch.left() + 120.0 * i as f32, switch.top()), vec2(120.0, 36.0));
            let on = self.palette_cards == (i == 1);
            if on {
                ui.painter().rect_filled(half, 9, argb(pc::ACCENT));
            }
            let (ink, weight) =
                if on { (Color32::WHITE, Weight::SemiBold) } else { (argb(PANEL_TITLE), Weight::Regular) };
            text(ui.painter(), half.center(), Align2::CENTER_CENTER, label, 15.0, weight, ink);
            if ui.interact(half, ui.scope_id().with(("floating palettes view", i)), Sense::click()).clicked() {
                self.palette_cards = i == 1;
            }
        }
        let list = Rect::from_min_size(pos2(rect.left(), rect.top() + 90.0), vec2(rect.width(), 246.0));
        let palettes = self.palettes.list();
        let default = self.palettes.default_palette() as usize;
        let mut taken = None;
        let mut menu = None;
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            ScrollArea::vertical().id_salt("floating palettes").show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
                for (n, p) in palettes.iter().enumerate() {
                    let filled: Vec<(usize, u32)> =
                        p.colors.iter().enumerate().filter_map(|(slot, c)| c.map(|c| (slot, c))).collect();
                    let body = if self.palette_cards {
                        filled.len().div_ceil(TILES_ACROSS) as f32 * PALETTE_TILE
                    } else {
                        SWATCH * p.colors.len().div_ceil(ACROSS).max(1) as f32
                    };
                    let rows = p.colors.len().div_ceil(ACROSS).max(1);
                    let (line, _) = ui.allocate_exact_size(vec2(list.width(), 44.0 + body), Sense::hover());
                    let card = Rect::from_min_max(
                        pos2(line.left() + 10.0, line.top()),
                        pos2(line.right() - 10.0, line.bottom()),
                    );
                    ui.painter().rect_filled(card, 10, argb(GROUP));
                    // the default palette ticked, the name, then its menu's button
                    let tick = pos2(card.left() + 19.0, card.top() + 21.5);
                    if n == default {
                        ui.painter().circle_filled(tick, 10.5, argb(pc::ACCENT));
                        look.icon(ui, &icons::TICK, tick, 13.0, Color32::WHITE);
                    }
                    let ink = argb(PANEL_TITLE);
                    let name_at = pos2(card.left() + if n == default { 35.0 } else { 12.0 }, tick.y);
                    text(ui.painter(), name_at, Align2::LEFT_CENTER, &p.name, 15.0, Weight::SemiBold, ink);
                    let more = Rect::from_center_size(pos2(card.right() - 24.0, tick.y), vec2(40.0, 40.0));
                    look.icon(ui, &icons::MORE, more.center(), 20.0, ink);
                    if ui.interact(more, ui.scope_id().with(("floating palette more", n)), Sense::click()).clicked() {
                        menu = Some((n as u32, more));
                    }
                    if self.palette_cards {
                        let tiles = Rect::from_min_size(pos2(card.left(), card.top() + 44.0), vec2(card.width(), body));
                        if let Some(c) = floating_tiles(ui, n, &filled, tiles) {
                            taken = Some(c);
                        }
                        continue;
                    }
                    for (slot, rgb) in p.colors.iter().enumerate() {
                        let (row, col) = (slot / ACROSS, slot % ACROSS);
                        let cell = Rect::from_min_size(
                            pos2(card.left() + SWATCH * col as f32, card.top() + 44.0 + SWATCH * row as f32),
                            vec2(SWATCH, SWATCH),
                        );
                        // the bottom row's outer corners round with the card
                        let last = row + 1 == rows;
                        let corners = egui::CornerRadius {
                            nw: 0,
                            ne: 0,
                            sw: if last && col == 0 { 10 } else { 0 },
                            se: if last && col == ACROSS - 1 { 10 } else { 0 },
                        };
                        let fill = rgb.map_or(argb(EMPTY_SLOT), |c| {
                            srgb([
                                (c >> 16 & 0xFF) as f32 / 255.0,
                                (c >> 8 & 0xFF) as f32 / 255.0,
                                (c & 0xFF) as f32 / 255.0,
                            ])
                        });
                        ui.painter().rect_filled(cell, corners, fill);
                        if let Some(c) = *rgb
                            && ui
                                .interact(cell, ui.scope_id().with(("floating swatch", n, slot)), Sense::click())
                                .clicked()
                        {
                            taken = Some(c);
                        }
                    }
                }
            });
        });
        if let Some(at) = menu {
            self.palette_menu_for = Some(at);
            self.palette_menu.open();
        }
        if let Some(c) = taken {
            take_rgb(&mut self.hsv, c);
            self.send(session);
        }
    }
}

/// Close: a disc with a cross cut out of it.
fn close_button(ui: &Ui, rect: Rect) {
    let c = rect.center();
    ui.painter().circle_filled(c, 8.5, argb(CLOSE_DISC));
    let stroke = Stroke::new(1.3, argb(CLOSE_CROSS));
    for (a, b) in [(vec2(-3.2, -3.2), vec2(3.2, 3.2)), (vec2(3.2, -3.2), vec2(-3.2, 3.2))] {
        ui.painter().line_segment([c + a, c + b], stroke);
    }
}

/// A palette's colours as named tiles two to a row at `at`, 120 square, the block's corners
/// rounded (as measured); dark lettering on bright colours. The colour tapped, if one was.
fn floating_tiles(ui: &Ui, n: usize, filled: &[(usize, u32)], at: Rect) -> Option<u32> {
    let mut taken = None;
    let rows = filled.len().div_ceil(TILES_ACROSS);
    for (k, &(slot, rgb)) in filled.iter().enumerate() {
        let (row, col) = (k / TILES_ACROSS, k % TILES_ACROSS);
        let tile = Rect::from_min_size(
            at.min + vec2(PALETTE_TILE * col as f32, PALETTE_TILE * row as f32),
            vec2(PALETTE_TILE, PALETTE_TILE),
        );
        let (first, last) = (row == 0, row + 1 == rows);
        let (left, right) = (col == 0, col + 1 == TILES_ACROSS || k + 1 == filled.len());
        let round = |on: bool| if on { 10 } else { 0 };
        let corners = egui::CornerRadius {
            nw: round(first && left),
            ne: round(first && right),
            sw: round(last && left),
            se: round(last && right),
        };
        ui.painter().rect_filled(tile, corners, srgb(of_hex(rgb)));
        let lettering = if rgb_to_hsv(of_hex(rgb))[2] >= 0.85 { Color32::BLACK } else { Color32::WHITE };
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &procraft_core::colour_name(rgb),
            0.0,
            egui::TextFormat {
                font_id: Weight::Regular.font(15.0),
                color: lettering,
                line_height: Some(17.0),
                ..Default::default()
            },
        );
        job.wrap.max_width = PALETTE_TILE - 16.0;
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(pos2(tile.left() + 8.0, tile.bottom() - 10.0 - galley.size().y), galley, lettering);
        if ui.interact(tile, ui.scope_id().with(("floating tile", n, slot)), Sense::click()).clicked() {
            taken = Some(rgb);
        }
    }
    taken
}
