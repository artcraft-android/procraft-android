//! The colour panel, as the Android app's (ui/ColorPanel.kt): the current and second colours, the
//! disc picker (a hue ring round a saturation/brightness disc), the history, the default palette's
//! swatches and the mode tabs.

use egui::{Align2, Color32, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, pos2, vec2};
use std::sync::Arc;

use procraft_core::{CanvasSession, ColourHistory, Palettes};

use crate::colour_drop::{ColourDrop, Touch};
use crate::icons;
use crate::look::{Look, Weight, argb, pc, text};
use crate::panel::{self, PANEL_TITLE};

const PANEL_WIDTH: f32 = 376.0;
const HEADER_HEIGHT: f32 = 64.0;
const RING_OUTER: f32 = 182.0;
const RING_INNER: f32 = 124.0;
const DISC_RADIUS: f32 = 105.0;
const PICKER: f32 = 364.0;
const SWATCH_EDGE: u32 = 0xFF5A5A5A;
const GRABBER: u32 = 0x7AC6C6C6;
const SET_CELL: u32 = 0x2E5B5B5B;
const CLEAR_TEXT: u32 = 0xFFB5B5B5;
const EMPTY_SWATCH: u32 = 0xFF2A2A2A;
const EMPTY_SLOT: u32 = 0xFF555555;
/// The history's strip: its last colours, as many as it keeps.
const HISTORY_SHOWN: usize = 10;

/// The disc's ring is not linear in hue: the hues (degrees) at 49 evenly spaced points round it,
/// anticlockwise from the right, as the Android app has them; between them they interpolate.
const RING_HUES: [f32; 49] = [
    0.0, 0.2, 0.2, 0.2, 4.5, 13.2, 18.9, 23.7, 27.9, 32.4, 36.4, 40.9, 44.6, 49.1, 53.1, 57.2, 61.4, 74.2, 88.6, 114.8,
    120.5, 120.5, 120.5, 120.5, 134.8, 149.9, 162.2, 173.4, 180.0, 186.9, 195.4, 203.4, 211.4, 219.4, 227.5, 235.3,
    242.1, 251.1, 258.5, 268.0, 276.5, 285.3, 293.6, 300.2, 303.3, 318.2, 333.7, 351.2, 360.0,
];

/// The colours the panel shows and changes.
pub struct Colours {
    /// The colour painted with, as hue, saturation and brightness of its sRGB values (0..1).
    pub hsv: [f32; 3],
    /// The second colour (secondary colour dynamics), sRGB.
    pub secondary: [f32; 3],
    /// The colours painted with lately, kept by the shared core (each canvas adds to it).
    pub history: Arc<ColourHistory>,
    /// The palettes, kept by the shared core (the default one shows here).
    palettes: Arc<Palettes>,
    /// The disc's picture for its hue, and the ring's.
    disc: Option<(u32, f32, TextureHandle)>,
    ring: Option<(f32, TextureHandle)>,
    /// What a drag that started on the picker moves: the ring (true) or the disc.
    on_ring: Option<bool>,
}

impl Default for Colours {
    fn default() -> Colours {
        Colours {
            hsv: [0.0, 0.0, 0.0],
            secondary: [1.0, 1.0, 1.0],
            history: ColourHistory::new(),
            palettes: Palettes::open(crate::files::app_dir().join("palettes.json").to_string_lossy().into_owned()),
            disc: None,
            ring: None,
            on_ring: None,
        }
    }
}

impl Colours {
    /// The colour painted with, sRGB.
    pub fn rgb(&self) -> [f32; 3] {
        hsv_to_rgb(self.hsv)
    }

    pub fn set_rgb(&mut self, session: &CanvasSession, rgb: u32) {
        let [h, s, v] = rgb_to_hsv(of_hex(rgb));
        // a grey keeps the hue it had
        self.hsv = [if s > 0.0 { h } else { self.hsv[0] }, s, v];
        self.send(session);
    }

    /// The last colour in the history other than the current one, back; false if there is none.
    pub fn previous(&mut self, session: &CanvasSession) -> bool {
        let Some(before) = self.history.previous(to_hex(self.rgb())) else { return false };
        self.set_rgb(session, before);
        true
    }

    /// The current and second colours trade places.
    pub fn swap(&mut self, session: &CanvasSession) {
        let was = self.rgb();
        let [h, s, v] = rgb_to_hsv(self.secondary);
        self.hsv = [if s > 0.0 { h } else { self.hsv[0] }, s, v];
        self.secondary = was;
        self.send(session);
    }

    pub fn send(&self, session: &CanvasSession) {
        let [r, g, b] = self.rgb();
        session.set_color(r, g, b);
        let [r, g, b] = self.secondary;
        session.set_secondary_color(r, g, b);
    }

    /// Shows the panel hanging from `button`; returns where it is (points).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        drop: &mut ColourDrop,
        button: Rect,
        backdrops: &mut Vec<f32>,
    ) -> Rect {
        let screen = ctx.content_rect();
        let (left, arrow_x) =
            panel::place(PANEL_WIDTH, button.center().x, screen, pc::SIDE_BAR_WIDTH + panel::POPOVER_GAP);
        let top = screen.top() + pc::BAR_HEIGHT;
        // the disc (8 above, 17 below), the history (44 and 36), the palette (44 and three rows of
        // 36) and the tabs (70)
        let height = panel::ARROW_HEIGHT + HEADER_HEIGHT + 8.0 + PICKER + 17.0 + 80.0 + 44.0 + 108.0 + 8.0 + 70.0;
        let rect = Rect::from_min_size(pos2(left, top), vec2(PANEL_WIDTH, height));
        egui::Area::new(egui::Id::unique("colour panel")).fixed_pos(rect.min).order(egui::Order::Foreground).show(
            ctx,
            |ui| {
                ui.allocate_rect(rect, Sense::click());
                panel::frame(ui, look, "colour frame", rect, arrow_x, backdrops);
                let body = panel::body(rect);
                panel::header(ui, look, body, "Colors", 20.0, false, false);
                // the current colour (edged) and the second, which swaps with it when tapped
                let swatch = vec2(43.0, 25.0);
                let second = Rect::from_min_size(pos2(body.right() - 20.0 - swatch.x, body.top() + 21.5), swatch);
                let first = second.translate(vec2(-(swatch.x + 7.0), 0.0));
                ui.painter().rect_filled(first, 6, srgb(self.rgb()));
                ui.painter().rect_stroke(first, 6, egui::Stroke::new(1.0, argb(SWATCH_EDGE)), egui::StrokeKind::Inside);
                ui.painter().rect_filled(second, 6, srgb(self.secondary));
                if ui.interact(second, ui.scope_id().with("swap"), Sense::click()).clicked() {
                    self.swap(session);
                }
                let grabber = Rect::from_center_size(pos2(body.center().x, body.top() + 6.0), vec2(40.0, 4.0));
                ui.painter().rect_filled(grabber, 2, argb(GRABBER));

                let picker = Rect::from_min_size(
                    pos2(body.left() + 6.0, body.top() + HEADER_HEIGHT + 8.0),
                    vec2(PICKER, PICKER),
                );
                self.disc_picker(ui, session, picker);

                let mut y = picker.bottom() + 17.0;
                // the history: its title, Clear, and the last ten colours
                let ink = argb(PANEL_TITLE);
                text(
                    ui.painter(),
                    pos2(body.left() + 21.0, y + 13.0),
                    Align2::LEFT_TOP,
                    "History",
                    14.0,
                    Weight::SemiBold,
                    ink,
                );
                let clear = Rect::from_min_size(pos2(body.right() - 16.0 - 86.0, y + 11.0), vec2(86.0, 22.5));
                ui.painter().rect_filled(clear, 7, argb(SET_CELL));
                text(
                    ui.painter(),
                    clear.center(),
                    Align2::CENTER_CENTER,
                    "Clear",
                    13.0,
                    Weight::Regular,
                    argb(CLEAR_TEXT),
                );
                if ui.interact(clear, ui.scope_id().with("clear history"), Sense::click()).clicked() {
                    self.history.clear();
                }
                y += 44.0;
                let strip = Rect::from_min_size(pos2(body.left() + 8.0, y), vec2(body.width() - 16.0, 36.0));
                let kept = self.history.colours();
                let history: Vec<Option<u32>> = (0..HISTORY_SHOWN).map(|i| kept.get(i).copied()).collect();
                if let Some((Some(c), _)) =
                    swatch_row(ui, strip, &history, EMPTY_SWATCH, [8.0, 8.0, 8.0, 8.0], "history")
                {
                    self.set_rgb(session, c);
                }
                y += 36.0;
                let index = self.palettes.default_palette();
                let palette = self.palettes.list().into_iter().nth(index as usize);
                let name = palette.as_ref().map_or("", |p| p.name.as_str());
                text(
                    ui.painter(),
                    pos2(body.left() + 21.0, y + 13.0),
                    Align2::LEFT_TOP,
                    name,
                    14.0,
                    Weight::SemiBold,
                    ink,
                );
                let colors = palette.map(|p| p.colors).unwrap_or_default();
                y += 44.0;
                for row in 0..3 {
                    let cells: Vec<Option<u32>> =
                        (0..10).map(|c| colors.get(row * 10 + c).copied().flatten()).collect();
                    let line = Rect::from_min_size(pos2(body.left() + 8.0, y), vec2(body.width() - 16.0, 36.0));
                    let corners = match row {
                        0 => [8.0, 8.0, 0.0, 0.0],
                        2 => [0.0, 0.0, 8.0, 8.0],
                        _ => [0.0; 4],
                    };
                    // a filled swatch tapped gives its colour, held is taken out, and dragged out
                    // fills where it is let go (off this panel); an empty one tapped keeps the
                    // current colour there
                    for (i, (cell, rgb)) in paint_row(ui, line, &cells, EMPTY_SLOT, corners).into_iter().enumerate() {
                        let slot = (row * 10 + i) as u32;
                        let id = ui.scope_id().with(("palette swatch", slot));
                        match rgb {
                            Some(c) => match drop.source(ui, session, id, cell, c, true, true, Some(rect)) {
                                Some(Touch::Tap) => self.set_rgb(session, c),
                                Some(Touch::Hold) => self.palettes.set_swatch(index, slot, None),
                                Some(Touch::Dragged) | None => {}
                            },
                            // not the swatch's id: a hold that took a colour out doesn't tap
                            // the slot it leaves when let go
                            None => {
                                let id = ui.scope_id().with(("palette slot", slot));
                                if ui.interact(cell, id, Sense::click()).clicked() {
                                    self.palettes.set_swatch(index, slot, Some(to_hex(self.rgb())));
                                }
                            }
                        }
                    }
                    y += 36.0;
                }
                mode_tabs(ui, look, Rect::from_min_max(pos2(body.left(), body.bottom() - 70.0), body.max));
            },
        );
        rect
    }

    /// The disc picker, 364 square: a hue ring (radii 124 to 182, its hues running anticlockwise
    /// from the right) round the saturation/brightness disc (radius 105).
    fn disc_picker(&mut self, ui: &Ui, session: &CanvasSession, rect: Rect) {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let centre = rect.center();
        let response = ui.interact(rect, ui.scope_id().with("disc"), Sense::click_and_drag());
        if let Some(p) = response.interact_pointer_pos() {
            let d = p - centre;
            let ring = *self.on_ring.get_or_insert(d.length() > (DISC_RADIUS + RING_INNER) / 2.0);
            if ring {
                let turn = ((-d.y).atan2(d.x) / std::f32::consts::TAU + 1.0) % 1.0;
                self.hsv[0] = hue_at(turn);
            } else {
                let mut uv = d / DISC_RADIUS;
                if uv.length() > 1.0 {
                    uv /= uv.length();
                }
                let (x, y) = disc_to_square(uv.x, uv.y);
                self.hsv[1] = (x + 1.0) / 2.0;
                self.hsv[2] = (1.0 - y) / 2.0;
            }
            self.send(session);
        }
        if !response.is_pointer_button_down_on() {
            self.on_ring = None;
        }
        // the ring, drawn once for the screen's density
        if self.ring.as_ref().is_none_or(|(at, _)| *at != ppp) {
            let side = (PICKER * ppp).round() as usize;
            let image = ring_image(side, ppp);
            self.ring = Some((ppp, ctx.load_texture("hue ring", image, TextureOptions::LINEAR)));
        }
        let hue_key = (self.hsv[0] * 10_000.0).round() as u32;
        if self.disc.as_ref().is_none_or(|(h, at, _)| *h != hue_key || *at != ppp) {
            let side = (DISC_RADIUS * 2.0 * ppp).round() as usize;
            let image = disc_image(self.hsv[0], side);
            self.disc = Some((hue_key, ppp, ctx.load_texture("colour disc", image, TextureOptions::LINEAR)));
        }
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let painter = ui.painter();
        if let Some((_, t)) = &self.ring {
            painter.image(t.id(), rect, uv, Color32::WHITE);
        }
        if let Some((_, _, t)) = &self.disc {
            painter.image(
                t.id(),
                Rect::from_center_size(centre, vec2(DISC_RADIUS * 2.0, DISC_RADIUS * 2.0)),
                uv,
                Color32::WHITE,
            );
        }
        // the ring's knob sits on the middle of the ring, where the hue is
        let turn = hue_position(self.hsv[0]) * std::f32::consts::TAU;
        let mid = (RING_INNER + RING_OUTER) / 2.0;
        knob(ui, centre + vec2(turn.cos() * mid, -turn.sin() * mid), 26.2, srgb(hsv_to_rgb([self.hsv[0], 1.0, 1.0])));
        let (x, y) = square_to_disc(self.hsv[1] * 2.0 - 1.0, 1.0 - self.hsv[2] * 2.0);
        knob(ui, centre + vec2(x, y) * DISC_RADIUS, 10.75, srgb(self.rgb()));
    }
}

/// A row of equal swatches in `rect` (its corners rounded as given); returns the one tapped, with
/// its place.
fn swatch_row(
    ui: &Ui,
    rect: Rect,
    cells: &[Option<u32>],
    empty: u32,
    [tl, tr, br, bl]: [f32; 4],
    id: impl std::hash::Hash + std::fmt::Debug,
) -> Option<(Option<u32>, usize)> {
    let mut tapped = None;
    for (i, (cell, c)) in paint_row(ui, rect, cells, empty, [tl, tr, br, bl]).into_iter().enumerate() {
        if ui.interact(cell, ui.scope_id().with((&id, i)), Sense::click()).clicked() {
            tapped = Some((c, i));
        }
    }
    tapped
}

/// Draws a row of equal swatches in `rect` (its corners rounded as given); returns each one's
/// place and colour.
fn paint_row(
    ui: &Ui,
    rect: Rect,
    cells: &[Option<u32>],
    empty: u32,
    [tl, tr, br, bl]: [f32; 4],
) -> Vec<(Rect, Option<u32>)> {
    let n = cells.len().max(1);
    let w = rect.width() / n as f32;
    let mut placed = Vec::with_capacity(cells.len());
    for (i, c) in cells.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(rect.left() + w * i as f32, rect.top()), vec2(w, rect.height()));
        let first = i == 0;
        let last = i + 1 == n;
        let corners = egui::CornerRadius {
            nw: if first { tl as u8 } else { 0 },
            sw: if first { bl as u8 } else { 0 },
            ne: if last { tr as u8 } else { 0 },
            se: if last { br as u8 } else { 0 },
        };
        let fill = c.map_or(argb(empty), |rgb| srgb(of_hex(rgb)));
        // a hair over, so the cells meet without a seam
        ui.painter().rect_filled(cell.expand2(vec2(0.25, 0.0)), corners, fill);
        placed.push((cell, *c));
    }
    placed
}

/// The mode tabs: Disc, Classic, Harmony, Value and Palettes (only Disc so far).
fn mode_tabs(ui: &Ui, look: &mut Look, rect: Rect) {
    let tabs = [
        (&icons::MODE_DISC, "Disc"),
        (&icons::MODE_CLASSIC, "Classic"),
        (&icons::MODE_HARMONY, "Harmony"),
        (&icons::MODE_VALUE, "Value"),
        (&icons::MODE_PALETTES, "Palettes"),
    ];
    let w = rect.width() / tabs.len() as f32;
    for (i, (icon, label)) in tabs.into_iter().enumerate() {
        let selected = i == 0;
        let x = rect.left() + w * (i as f32 + 0.5);
        // a 36 frame centred 27 down
        look.icon(
            ui,
            icon,
            pos2(x, rect.top() + 9.0 + 18.0),
            36.0,
            if selected { argb(pc::ACCENT) } else { argb(PANEL_TITLE) },
        );
        let ink = if selected { Color32::WHITE } else { argb(PANEL_TITLE) };
        text(ui.painter(), pos2(x, rect.top() + 41.0), Align2::CENTER_TOP, label, 13.0, Weight::Medium, ink);
    }
}

/// A colour knob: `fill` inside a thin white ring, over a soft shadow.
fn knob(ui: &Ui, at: Pos2, r: f32, fill: Color32) {
    let shadow = egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(0x59) };
    ui.painter().add(shadow.as_shape(Rect::from_center_size(at, vec2(r * 2.0, r * 2.0)), r));
    ui.painter().circle_filled(at, r, fill);
    ui.painter().circle_stroke(at, r - 0.6, egui::Stroke::new(1.2, Color32::WHITE));
}

/// The hue (0..1) at `position` (0..1) round the ring.
fn hue_at(position: f32) -> f32 {
    let p = position.clamp(0.0, 1.0) * (RING_HUES.len() - 1) as f32;
    let i = (p as usize).min(RING_HUES.len() - 2);
    match (RING_HUES.get(i), RING_HUES.get(i + 1)) {
        (Some(a), Some(b)) => (a + (b - a) * (p - i as f32)) / 360.0,
        _ => 0.0,
    }
}

/// Where `hue` (0..1) sits (0..1) round the ring.
fn hue_position(hue: f32) -> f32 {
    let h = hue.clamp(0.0, 1.0) * 360.0;
    for (i, w) in RING_HUES.windows(2).enumerate() {
        if let [a, b] = w
            && h <= *b
            && b > a
        {
            return (i as f32 + ((h - a) / (b - a)).clamp(0.0, 1.0)) / (RING_HUES.len() - 1) as f32;
        }
    }
    1.0
}

/// A point of the square (-1..1) on the disc, and back.
fn square_to_disc(x: f32, y: f32) -> (f32, f32) {
    (x * (1.0 - y * y / 2.0).sqrt(), y * (1.0 - x * x / 2.0).sqrt())
}

fn disc_to_square(u: f32, v: f32) -> (f32, f32) {
    let r2 = 2.0 * 2f32.sqrt();
    let half = |a: f32, b: f32, s: f32| {
        0.5 * (2.0 + a * a - b * b + r2 * s).max(0.0).sqrt() - 0.5 * (2.0 + a * a - b * b - r2 * s).max(0.0).sqrt()
    };
    (half(u, v, u).clamp(-1.0, 1.0), half(v, u, v).clamp(-1.0, 1.0))
}

/// The hue ring, `side` pixels square (at `ppp` pixels a point), with soft edges.
fn ring_image(side: usize, ppp: f32) -> egui::ColorImage {
    let half = side as f32 / 2.0;
    let (inner, outer) = (RING_INNER * ppp, RING_OUTER * ppp);
    let mut pixels = vec![Color32::TRANSPARENT; side * side];
    for (k, px) in pixels.iter_mut().enumerate() {
        let (i, j) = ((k % side) as f32 + 0.5 - half, (k / side) as f32 + 0.5 - half);
        let r = i.hypot(j);
        let cover = ((r - inner + 0.5).clamp(0.0, 1.0) * (outer - r + 0.5).clamp(0.0, 1.0)).clamp(0.0, 1.0);
        if cover <= 0.0 {
            continue;
        }
        let turn = ((-j).atan2(i) / std::f32::consts::TAU + 1.0) % 1.0;
        *px = srgb(hsv_to_rgb([hue_at(turn), 1.0, 1.0])).gamma_multiply(cover);
    }
    egui::ColorImage::new([side, side], pixels)
}

/// The saturation/brightness disc for `hue`, `side` pixels square, with a soft edge.
fn disc_image(hue: f32, side: usize) -> egui::ColorImage {
    let half = side as f32 / 2.0;
    let mut pixels = vec![Color32::TRANSPARENT; side * side];
    for (k, px) in pixels.iter_mut().enumerate() {
        let u = ((k % side) as f32 + 0.5) / half - 1.0;
        let v = ((k / side) as f32 + 0.5) / half - 1.0;
        let edge = ((1.0 - u.hypot(v)) * half + 0.5).clamp(0.0, 1.0);
        if edge <= 0.0 {
            continue;
        }
        let (x, y) = disc_to_square(u, v);
        *px = srgb(hsv_to_rgb([hue, (x + 1.0) / 2.0, (1.0 - y) / 2.0])).gamma_multiply(edge);
    }
    egui::ColorImage::new([side, side], pixels)
}

/// A colour's sRGB values (0..1) as a colour to paint with.
pub fn srgb([r, g, b]: [f32; 3]) -> Color32 {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(byte(r), byte(g), byte(b))
}

pub fn to_hex(rgb: [f32; 3]) -> u32 {
    let c = srgb(rgb);
    u32::from(c.r()) << 16 | u32::from(c.g()) << 8 | u32::from(c.b())
}

pub fn of_hex(v: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((v >> shift) & 0xff) as f32 / 255.0)
}

/// Hue, saturation and brightness (0..1) as sRGB values.
pub fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

/// sRGB values as hue, saturation and brightness (0..1).
pub fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [h / 6.0, if max > 0.0 { d / max } else { 0.0 }, max]
}
