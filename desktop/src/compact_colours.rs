//! The compact colour picker other screens open (a gradient map's stop, Custom Canvas's
//! background), as the Android app's (ui/ColorPanel.kt, CompactColors): "Colors", a picker per
//! mode and the modes as icons, 260 wide; no swatches, history or palette. It edits a colour of its
//! own, which the screen that opened it takes as it changes.

use egui::{Align2, Color32, Rect, ScrollArea, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use procraft_core::{HueControl, Palettes, SLIDER_HUES, hue_at, hue_position};

use crate::colours::{
    self, EMPTY_SLOT, HarmonyKind, Mode, Pickers, SATURATION_GREY, hex_pad, hex_row, hsv_to_rgb, hue_slider, paint_row,
    rgb_of, set, slider_row, srgb, take_rgb, to_hex,
};
use crate::look::{Look, Weight, argb, pc, text};
use crate::panel::{self, PANEL_TITLE};

const WIDTH: f32 = 260.0;
const FILL: u32 = 0xFF1E1E1E;
const GROUP: u32 = 0xFF292929;
const HEX_FIELD: u32 = 0xFF343434;
/// "Colors" (9 above it, 5 below), and the tabs under the picker.
const TITLE: f32 = 42.0;
pub const TABS: f32 = 52.0;

/// What the picker did this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picked {
    /// The colour changed to this (0xRRGGBB).
    Colour(u32),
    /// A tap off it closed it.
    Closed,
}

pub struct CompactColours {
    /// The colour, as hue, saturation and brightness of its sRGB values (0..1).
    pub hsv: [f32; 3],
    mode: Mode,
    pickers: Pickers,
    /// Value shows red, green and blue (else hue, saturation and brightness); the hex keypad's
    /// digits typed, while it is open.
    rgb_shown: bool,
    hex: Option<String>,
    /// Its arrow under it (pointing down at what it colours), else above it.
    pub arrow_below: bool,
}

impl CompactColours {
    /// One for `rgb` (0xRRGGBB); a grey opens at `grey_hue` (0..1).
    pub fn new(rgb: u32, grey_hue: f32) -> CompactColours {
        let mut hsv = [grey_hue, 0.0, 0.0];
        take_rgb(&mut hsv, rgb);
        CompactColours {
            hsv,
            mode: Mode::Disc,
            pickers: Pickers::default(),
            rgb_shown: false,
            hex: None,
            arrow_below: true,
        }
    }

    pub fn rgb(&self) -> u32 {
        to_hex(hsv_to_rgb(self.hsv))
    }

    /// The hex keypad is open: it takes the keys.
    pub fn takes_keys(&self) -> bool {
        self.hex.is_some()
    }

    /// How tall the mode shown's picker is.
    fn body_height(&self) -> f32 {
        match self.mode {
            Mode::Disc => 4.0 + 240.0 + 6.0,
            Mode::Classic => 248.0 + 40.0,
            Mode::Harmony => 10.0 + 200.0 + 8.0 + 36.0,
            Mode::Value => 36.0 + 6.0 + 188.0 + 6.0 + 44.0 + 10.0,
            Mode::Palettes => 240.0,
        }
    }

    /// Its size, the arrow under it included.
    pub fn size(&self) -> Vec2 {
        vec2(WIDTH, TITLE + self.body_height() + TABS + panel::ARROW_HEIGHT)
    }

    /// Shows it at `rect` (as big as [`Self::size`]), its arrow under it pointing at `tip`, over
    /// everything; Harmony with the colour panel's `harmony` kind, Palettes with its `palettes`.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        rect: Rect,
        tip: f32,
        palettes: &Palettes,
        harmony: HarmonyKind,
        p3: bool,
    ) -> Option<Picked> {
        let screen = ctx.content_rect();
        let before = self.hsv;
        let mut closed = false;
        let mut value_hex = Rect::NOTHING;
        egui::Area::new(egui::Id::unique("compact colours")).fixed_pos(screen.min).order(egui::Order::Tooltip).show(
            ctx,
            |ui| {
                // allocated, so the canvas under it takes no touch
                closed = ui.allocate_rect(screen, Sense::click_and_drag()).clicked();
                ui.interact(rect, ui.scope_id().with("on"), Sense::click_and_drag());
                let body = panel::solid_frame(
                    ui,
                    look,
                    "compact colours",
                    rect,
                    tip - rect.left(),
                    self.arrow_below,
                    panel::POPOVER_CORNER,
                    FILL,
                );
                text(
                    ui.painter(),
                    pos2(body.left() + 18.0, body.top() + 9.0),
                    Align2::LEFT_TOP,
                    "Colors",
                    23.0,
                    Weight::Bold,
                    argb(PANEL_TITLE),
                );
                let top = body.top() + TITLE;
                value_hex = self.picker(ui, body.left(), top, palettes, harmony, p3);
                let tabs = Rect::from_min_size(pos2(body.left(), top + self.body_height()), vec2(WIDTH, TABS));
                self.tabs(ui, look, tabs);
            },
        );
        self.keypad(ctx, look, value_hex);
        if closed {
            Some(Picked::Closed)
        } else if self.hsv != before {
            Some(Picked::Colour(self.rgb()))
        } else {
            None
        }
    }

    /// The mode shown's picker, 260 wide from (`left`, `top`); Harmony with the colour panel's
    /// `harmony` kind (for a canvas in P3, `p3`, or sRGB), Palettes with its `palettes`. Returns
    /// Value's hex row (else nothing).
    pub fn picker(
        &mut self,
        ui: &mut Ui,
        left: f32,
        top: f32,
        palettes: &Palettes,
        harmony: HarmonyKind,
        p3: bool,
    ) -> Rect {
        match self.mode {
            Mode::Disc => {
                let disc = Rect::from_min_size(pos2(left + 10.0, top + 4.0), vec2(240.0, 240.0));
                self.pickers.disc(ui, &mut self.hsv, disc);
            }
            Mode::Classic => {
                let square = Rect::from_min_size(pos2(left + 6.0, top), vec2(248.0, 248.0));
                self.pickers.square(ui, &mut self.hsv, square);
                // the hue only, 240 wide under the square
                let row = Rect::from_min_size(pos2(left + 10.0, square.bottom()), vec2(240.0, 36.0));
                hue_slider(ui, row, "compact hue", &mut self.hsv);
            }
            Mode::Harmony => {
                let wheel = Rect::from_min_size(pos2(left + 30.0, top + 10.0), vec2(200.0, 200.0));
                self.pickers.harmony(ui, &mut self.hsv, wheel, harmony, p3);
                let row = Rect::from_min_size(pos2(left + 10.0, wheel.bottom() + 8.0), vec2(240.0, 36.0));
                self.pickers.harmony_brightness(ui, row, &mut self.hsv, p3);
            }
            Mode::Value => return self.value(ui, left + 10.0, top),
            Mode::Palettes => {
                let list = Rect::from_min_size(pos2(left, top), vec2(WIDTH, 240.0));
                self.palettes(ui, list, palettes);
            }
        }
        Rect::NOTHING
    }

    /// The modes as icons at `rect` (260 by [`TABS`]); a tap shows that mode.
    pub fn tabs(&mut self, ui: &Ui, look: &mut Look, rect: Rect) {
        if let Some(mode) = tabs_row(ui, look, rect, self.mode) {
            self.mode = mode;
            self.hex = None;
        }
    }

    /// The hex keypad over Value's hex row `hex_row`, while it is open.
    pub fn keypad(&mut self, ctx: &egui::Context, look: &mut Look, hex_row: Rect) {
        if let Some(rgb) = hex_pad(ctx, look, &mut self.hex, hex_row) {
            take_rgb(&mut self.hsv, rgb);
        }
    }

    /// Value: HSB or RGB (a switch picks), then the hex code; 240 wide from `left`. Returns the
    /// hex row.
    fn value(&mut self, ui: &Ui, left: f32, top: f32) -> Rect {
        value_view(ui, (left, top), &mut self.hsv, &mut self.rgb_shown, &mut self.hex, (GROUP, HEX_FIELD))
    }

    /// The palettes' swatches, one grid after another (10 across, rounded 8), a tap taking a
    /// colour.
    fn palettes(&mut self, ui: &mut Ui, rect: Rect, palettes: &Palettes) {
        let list = palettes.list();
        let mut taken = None;
        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ScrollArea::vertical().id_salt("compact palettes").show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
                ui.add_space(4.0 - 10.0);
                for (n, p) in list.iter().enumerate() {
                    let (line, _) = ui.allocate_exact_size(vec2(rect.width(), 72.0), Sense::hover());
                    for row in 0..3 {
                        let cells: Vec<Option<u32>> =
                            (0..10).map(|c| p.colors.get(row * 10 + c).copied().flatten()).collect();
                        let strip = Rect::from_min_size(
                            pos2(line.left() + 10.0, line.top() + 24.0 * row as f32),
                            vec2(line.width() - 20.0, 24.0),
                        );
                        let corners = match row {
                            0 => [8.0, 8.0, 0.0, 0.0],
                            2 => [0.0, 0.0, 8.0, 8.0],
                            _ => [0.0; 4],
                        };
                        for (i, (cell, rgb)) in
                            paint_row(ui, strip, &cells, EMPTY_SLOT, corners).into_iter().enumerate()
                        {
                            let id = ui.scope_id().with(("compact swatch", n, row, i));
                            if let Some(c) = rgb
                                && ui.interact(cell, id, Sense::click()).clicked()
                            {
                                taken = Some(c);
                            }
                        }
                    }
                }
                ui.add_space(4.0);
            });
        });
        if let Some(c) = taken {
            take_rgb(&mut self.hsv, c);
        }
    }
}

/// The modes as a row of icons, the one shown blue; returns the one tapped.
pub(crate) fn tabs_row(ui: &Ui, look: &mut Look, rect: Rect, mode: Mode) -> Option<Mode> {
    let w = rect.width() / colours::MODE_TABS.len() as f32;
    let mut chosen = None;
    for (i, (icon, _, m)) in colours::MODE_TABS.into_iter().enumerate() {
        let tab = Rect::from_min_size(pos2(rect.left() + w * i as f32, rect.top()), vec2(w, rect.height()));
        if ui.interact(tab, ui.scope_id().with(("compact tab", i)), Sense::click()).clicked() {
            chosen = Some(m);
        }
        // the tabs' 36 glyphs, a little smaller here
        let ink = if m == mode { argb(pc::ACCENT) } else { argb(PANEL_TITLE) };
        look.icon(ui, icon, tab.center(), 33.0, ink);
    }
    chosen
}

/// Value's picker for `hsv`: HSB or RGB (a switch picks, `rgb_shown`), then the hex code (`hex`
/// the keypad's digits while it is open); 240 wide from `at`, its groups filled `fills` (the
/// groups', the hex field's). Returns the hex row.
pub(crate) fn value_view(
    ui: &Ui,
    (left, top): (f32, f32),
    hsv: &mut [f32; 3],
    rgb_shown: &mut bool,
    hex: &mut Option<String>,
    (group_fill, field_fill): (u32, u32),
) -> Rect {
    let switch = Rect::from_min_size(pos2(left, top), vec2(240.0, 36.0));
    ui.painter().rect_filled(switch, 9, argb(group_fill));
    for (i, label) in ["HSB", "RGB"].into_iter().enumerate() {
        let half = Rect::from_min_size(pos2(left + 120.0 * i as f32, top), vec2(120.0, 36.0));
        let on = *rgb_shown == (i == 1);
        if on {
            ui.painter().rect_filled(half, 9, argb(pc::ACCENT));
        }
        let (ink, weight) = if on { (Color32::WHITE, Weight::SemiBold) } else { (argb(PANEL_TITLE), Weight::Regular) };
        text(ui.painter(), half.center(), Align2::CENTER_CENTER, label, 15.0, weight, ink);
        if ui.interact(half, ui.scope_id().with(("value switch", i)), Sense::click()).clicked() {
            *rgb_shown = i == 1;
        }
    }
    let group = Rect::from_min_size(pos2(left, switch.bottom() + 6.0), vec2(240.0, 188.0));
    ui.painter().rect_filled(group, 10, argb(group_fill));
    let rgb = to_hex(hsv_to_rgb(*hsv));
    let [r, g, b] = [rgb >> 16 & 0xFF, rgb >> 8 & 0xFF, rgb & 0xFF];
    let now = *hsv;
    for i in 0..3 {
        let row =
            Rect::from_min_size(pos2(group.left() + 12.0, group.top() + 10.0 + 62.0 * i as f32), vec2(216.0, 44.0));
        let (letter, value, colours, shown): (&str, f32, Vec<Color32>, String) = match (*rgb_shown, i) {
            (false, 0) => (
                "H",
                hue_position(HueControl::Slider, now[0]),
                SLIDER_HUES.iter().map(|h| srgb(hsv_to_rgb([h / 360.0, 1.0, 1.0]))).collect(),
                format!("{}\u{b0}", (now[0] * 360.0).round() as i32),
            ),
            (false, 1) => (
                "S",
                now[1],
                vec![argb(SATURATION_GREY), srgb(hsv_to_rgb([now[0], 1.0, 1.0]))],
                format!("{}%", (now[1] * 100.0).round() as i32),
            ),
            (false, _) => {
                ("B", now[2], vec![Color32::BLACK, Color32::WHITE], format!("{}%", (now[2] * 100.0).round() as i32))
            }
            (true, 0) => ("R", r as f32 / 255.0, vec![rgb_of(0, g, b), rgb_of(255, g, b)], r.to_string()),
            (true, 1) => ("G", g as f32 / 255.0, vec![rgb_of(r, 0, b), rgb_of(r, 255, b)], g.to_string()),
            _ => ("B", b as f32 / 255.0, vec![rgb_of(r, g, 0), rgb_of(r, g, 255)], b.to_string()),
        };
        let ink = argb(PANEL_TITLE);
        let y = row.center().y;
        text(ui.painter(), pos2(row.left(), y), Align2::LEFT_CENTER, letter, 15.0, Weight::Regular, ink);
        text(ui.painter(), pos2(row.right(), y), Align2::RIGHT_CENTER, &shown, 15.0, Weight::Regular, ink);
        let track = Rect::from_min_max(pos2(row.left() + 20.0, y - 18.0), pos2(row.right() - 48.0, y + 18.0));
        let Some(to) = slider_row(ui, track, ("compact value", *rgb_shown, i), value, &colours) else {
            continue;
        };
        let byte = (to * 255.0).round() as u32;
        match (*rgb_shown, i) {
            (false, 0) => _ = set(&mut hsv[0], hue_at(HueControl::Slider, to)),
            (false, 1) => _ = set(&mut hsv[1], to),
            (false, _) => _ = set(&mut hsv[2], to),
            (true, 0) => take_rgb(hsv, byte << 16 | g << 8 | b),
            (true, 1) => take_rgb(hsv, r << 16 | byte << 8 | b),
            _ => take_rgb(hsv, r << 16 | g << 8 | byte),
        }
    }
    let row = Rect::from_min_size(pos2(left, group.bottom() + 6.0), vec2(240.0, 44.0));
    hex_row(ui, row, rgb, hex, group_fill, field_fill);
    row
}
