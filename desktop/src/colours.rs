//! The colour panel, as the Android app's (ui/ColorPanel.kt): the current and second colours, a
//! picker per mode (Disc: a hue ring round a saturation/brightness disc; Classic: a
//! saturation/brightness square and three sliders; Harmony: a hue/saturation wheel with the
//! colour's companions; Value: number sliders and the hex code), the history, the default
//! palette's swatches and the mode tabs.

use egui::{Align2, Color32, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, UiBuilder, pos2, vec2};
use std::sync::Arc;

use procraft_core::{
    CanvasSession, ColourHistory, ColourModel, ColourPlace, HueControl, Palettes, Plugins, SLIDER_HUES, colours_in_p3,
    harmony_wheel, hue_at, hue_position, oklab_of, oklab_slid, oklab_to_rgb, oklab_tracks, p3_hsv_of, p3_hsv_to_rgb,
};

use crate::colour_drop::{ColourDrop, Touch};
use crate::icons;
use crate::look::{Look, Shade, Themed, Weight, argb, pc, text};
use crate::panel::{self, PANEL_TITLE};

mod floating;

const PANEL_WIDTH: f32 = 376.0;
/// How far the grabber is dragged before the panel floats, points.
const UNDOCK_DRAG: f32 = 30.0;
const HEADER_HEIGHT: f32 = 64.0;
const RING_OUTER: f32 = 182.0;
const RING_INNER: f32 = 124.0;
const DISC_RADIUS: f32 = 105.0;
const PICKER: f32 = 364.0;
const SWATCH_EDGE: Themed = Themed(0xFF5A5A5A, 0xFFC0C0C0);
const MODEL_BUTTON: Themed = Themed(0xFF3A3A3A, 0xFFDCDCDC);
const GRABBER: Themed = Themed(0x7AC6C6C6, 0xFFB0B0B0);
const SET_CELL: Themed = Themed(0x2E5B5B5B, 0xFFD7D7D7);
const CLEAR_TEXT: Themed = Themed(0xFFB5B5B5, 0xFF4A4A4A);
const EMPTY_SWATCH: Themed = Themed(0xFF2A2A2A, 0xFFD9D9D9);
pub(crate) const EMPTY_SLOT: Themed = Themed(0xFF555555, 0xFFC8C8C8);
/// The history's strip: its last colours, as many as it keeps.
const HISTORY_SHOWN: usize = 10;
/// Classic's square, Harmony's wheel (its radius), and their rows.
const SQUARE: f32 = 344.0;
const HARMONY_RADIUS: f32 = 149.0;
const SLIDER_ROW: f32 = 36.0;
pub(crate) const SATURATION_GREY: u32 = 0xFF7F7F7F;
/// Value's groups of sliders and its hex row.
const VALUE_GROUP: Themed = Themed(0xFF434343, 0xFFD5D5D5);
const VALUE_GROUP_HEIGHT: f32 = 167.0;
const HEX_ROW: Themed = Themed(0xFF414141, 0xFFD8D8D8);
const HEX_FIELD: Themed = Themed(0xFF4B4B4B, 0xFFCFCFCF);
/// The hex keypad beside the panel.
const HEX_PAD: (f32, f32) = (259.0, 340.0);
const HEX_PAD_FILL: u32 = 0xFF616161;
const HEX_PAD_EDGE: u32 = 0xFF262626;
const HEX_KEY_FILL: u32 = 0xFF6C6C6C;
const HEX_KEY_HELD: u32 = 0xFF8A8A8A;
const HEX_KEY_INK: u32 = 0xFFEEEEEE;
const HEX_BACK_INK: u32 = 0xFFBABABA;
/// Harmony's kinds menu.
const HARMONY_MENU_FILL: Themed = Themed(0xFF5A5C5B, 0xFFF0F0F0);
/// Palettes: the Compact/Cards switch, then the list of palettes, each a card.
const PALETTE_LIST: f32 = 577.0;
/// Windows shorter than this get the short panel: the history or the palette (as its "•••" menu
/// says), not both, and the pickers tighter (see [`Shape`]).
const SHORT_SCREEN: f32 = 900.0;
/// Short, the classic square takes the height left over: the window less the bar and arrow (56),
/// the header (64), the sliders (108), the palette (142), the tabs (70) and a 25 margin.
const SHORT_CLASSIC_REST: f32 = 465.0;
/// The palettes' list takes the height left over, up to its measured height: the screen less the
/// bar and arrow (56), the header (64), the Compact and Cards switch (44), the tabs (70) and a 25
/// margin.
const PALETTES_REST: f32 = 259.0;
/// What the panel leaves under it, as the short classic panel does: its picker scrolls in a window
/// too short for that.
const PANEL_MARGIN: f32 = 17.0;
/// The short panel's menu: its width, edge and corners.
const SHORT_MENU_WIDTH: f32 = 252.0;
const SHORT_MENU_EDGE: u32 = 0x24FFFFFF;
const SHORT_MENU_CORNER: f32 = 17.0;

/// The pickers' sizes, full or short: Classic's square's height, Harmony's wheel (its radius, the
/// space above it and under it), Value's groups (their height, the gap between rows and above the
/// first), and the palettes' list.
#[derive(Clone, Copy, Debug)]
struct Shape {
    square: f32,
    harmony_radius: f32,
    harmony_top: f32,
    harmony_gap: f32,
    value_group: f32,
    value_gap: f32,
    value_top: f32,
    palette_list: f32,
}

impl Shape {
    fn of(short: bool, screen_height: f32) -> Shape {
        if short {
            Shape {
                square: (screen_height - SHORT_CLASSIC_REST).clamp(150.0, SQUARE),
                harmony_radius: HARMONY_RADIUS - 1.0,
                harmony_top: 4.0,
                harmony_gap: 10.0,
                // the rows 51 apart instead of 53
                value_group: 151.0,
                value_gap: 7.0,
                value_top: 3.0,
                palette_list: (screen_height - PALETTES_REST).clamp(120.0, 458.0),
            }
        } else {
            Shape {
                square: SQUARE,
                harmony_radius: HARMONY_RADIUS,
                harmony_top: 10.0,
                harmony_gap: 31.0,
                value_group: VALUE_GROUP_HEIGHT,
                value_gap: 9.0,
                value_top: 9.0,
                palette_list: PALETTE_LIST,
            }
        }
    }
}
pub(super) const PALETTE_TILE: f32 = 120.0;
/// A swatch held this long is taken out of its palette.
const SWATCH_HOLD: std::time::Duration = std::time::Duration::from_millis(400);

/// A grid of nine squares (Create new palette), as the Android app draws it.
const MENU_GRID: icons::Icon = icons::Icon {
    name: "menu-grid",
    width: 40.0,
    height: 40.0,
    parts: &[
        icons::Part {
            d: "M12.3 11.5h3.4a0.8 0.8 0 0 1 0.8 0.8v3.4a0.8 0.8 0 0 1-0.8 0.8h-3.4a0.8 0.8 0 0 1-0.8-0.8v-3.4a0.8 0.8 0 0 1 0.8-0.8z",
            stroke: 0.0,
            fill: true,
            even_odd: false,
        },
        icons::Part {
            d: "M18.5 11.5h3.4a0.8 0.8 0 0 1 0.8 0.8v3.4a0.8 0.8 0 0 1-0.8 0.8h-3.4a0.8 0.8 0 0 1-0.8-0.8v-3.4a0.8 0.8 0 0 1 0.8-0.8z",
            stroke: 0.0,
            fill: true,
            even_odd: false,
        },
        icons::Part {
            d: "M24.7 11.5h3.4a0.8 0.8 0 0 1 0.8 0.8v3.4a0.8 0.8 0 0 1-0.8 0.8h-3.4a0.8 0.8 0 0 1-0.8-0.8v-3.4a0.8 0.8 0 0 1 0.8-0.8z",
            stroke: 0.0,
            fill: true,
            even_odd: false,
        },
        icons::Part {
            d: "M12.3 17.7h3.4a0.8 0.8 0 0 1 0.8 0.8v3.4a0.8 0.8 0 0 1-0.8 0.8h-3.4a0.8 0.8 0 0 1-0.8-0.8v-3.4a0.8 0.8 0 0 1 0.8-0.8z",
            stroke: 0.0,
            fill: true,
            even_odd: false,
        },
        icons::Part {
            d: "M18.5 17.7h3.4a0.8 0.8 0 0 1 0.8 0.8v3.4a0.8 0.8 0 0 1-0.8 0.8h-3.4a0.8 0.8 0 0 1-0.8-0.8v-3.4a0.8 0.8 0 0 1 0.8-0.8z",
            stroke: 0.0,
            fill: true,
            even_odd: false,
        },
        icons::Part {
            d: "M24.7 17.7h3.4a0.8 0.8 0 0 1 0.8 0.8v3.4a0.8 0.8 0 0 1-0.8 0.8h-3.4a0.8 0.8 0 0 1-0.8-0.8v-3.4a0.8 0.8 0 0 1 0.8-0.8z",
            stroke: 0.0,
            fill: true,
            even_odd: false,
        },
        icons::Part {
            d: "M12.3 23.9h3.4a0.8 0.8 0 0 1 0.8 0.8v3.4a0.8 0.8 0 0 1-0.8 0.8h-3.4a0.8 0.8 0 0 1-0.8-0.8v-3.4a0.8 0.8 0 0 1 0.8-0.8z",
            stroke: 0.0,
            fill: true,
            even_odd: false,
        },
        icons::Part {
            d: "M18.5 23.9h3.4a0.8 0.8 0 0 1 0.8 0.8v3.4a0.8 0.8 0 0 1-0.8 0.8h-3.4a0.8 0.8 0 0 1-0.8-0.8v-3.4a0.8 0.8 0 0 1 0.8-0.8z",
            stroke: 0.0,
            fill: true,
            even_odd: false,
        },
        icons::Part {
            d: "M24.7 23.9h3.4a0.8 0.8 0 0 1 0.8 0.8v3.4a0.8 0.8 0 0 1-0.8 0.8h-3.4a0.8 0.8 0 0 1-0.8-0.8v-3.4a0.8 0.8 0 0 1 0.8-0.8z",
            stroke: 0.0,
            fill: true,
            even_odd: false,
        },
    ],
};

/// The panel's modes, as its tabs list them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Disc,
    Classic,
    Harmony,
    Value,
    Palettes,
}

/// Which colours go with the colour in Harmony: the hues turned from it (degrees).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HarmonyKind {
    #[default]
    Complementary,
    SplitComplementary,
    Analogous,
    Triadic,
    Tetradic,
}

impl HarmonyKind {
    const ALL: [HarmonyKind; 5] = [
        HarmonyKind::Complementary,
        HarmonyKind::SplitComplementary,
        HarmonyKind::Analogous,
        HarmonyKind::Triadic,
        HarmonyKind::Tetradic,
    ];

    fn label(self) -> &'static str {
        match self {
            HarmonyKind::Complementary => "Complementary",
            HarmonyKind::SplitComplementary => "Split Complementary",
            HarmonyKind::Analogous => "Analogous",
            HarmonyKind::Triadic => "Triadic",
            HarmonyKind::Tetradic => "Tetradic",
        }
    }

    fn turns(self) -> &'static [f32] {
        match self {
            HarmonyKind::Complementary => &[180.0],
            HarmonyKind::SplitComplementary => &[150.0, 210.0],
            HarmonyKind::Analogous => &[30.0, -30.0],
            HarmonyKind::Triadic => &[120.0, 240.0],
            HarmonyKind::Tetradic => &[90.0, 180.0, 270.0],
        }
    }
}

/// The colours the panel shows and changes.
pub struct Colours {
    /// The colour painted with, as hue, saturation and brightness of its sRGB values (0..1).
    pub hsv: [f32; 3],
    /// The colour not in use: masks' (black at first) while a layer of another kind is current,
    /// the brush colour while a mask is.
    other: [f32; 3],
    /// Whether a mask is current, the colour then the one masks paint with.
    on_mask: bool,
    /// The second colour (secondary colour dynamics), sRGB.
    pub secondary: [f32; 3],
    /// The colours painted with lately, kept by the shared core (each canvas adds to it).
    pub history: Arc<ColourHistory>,
    /// The palettes, kept by the shared core (the default one shows here).
    pub palettes: Arc<Palettes>,
    pickers: Pickers,
    /// The mode shown.
    pub mode: Mode,
    pub harmony: HarmonyKind,
    harmony_menu: crate::menu::Menu,
    /// The hex keypad is open: the digits typed on it since.
    hex: Option<String>,
    /// Palettes: shown as cards of named tiles (else compact grids); one renamed in place (which,
    /// the name typed, focused yet); the menu of one (which, and where its button is); the plus's
    /// menu; a swatch held (which palette and slot, since when, whether the hold has acted).
    palette_cards: bool,
    renaming_palette: Option<(u32, String, bool)>,
    palette_menu: crate::menu::Menu,
    palette_menu_for: Option<(u32, Rect)>,
    new_palette_menu: crate::menu::Menu,
    new_palette_button: Rect,
    swatch_press: Option<(u32, usize, std::time::Instant, bool)>,
    /// The short panel's "•••" (where it is) and its menu.
    short_dots: Rect,
    /// How far the picker is scrolled, in a window too short for all of it.
    scrolled: f32,
    short_menu: crate::menu::Menu,
    /// Floating: where the window is (its top-left), where the pointer holds its grabber while
    /// it moves, and whether its Value shows red, green and blue.
    pub floating: Option<Pos2>,
    floating_grab: Option<egui::Vec2>,
    floating_rgb: bool,
    /// The colour model's button left of the swatches, its menu, and a model chosen there (for
    /// the preferences to keep).
    model_button: Rect,
    model_menu: crate::menu::Menu,
    pub model_chosen: Option<ColourModel>,
    /// Where the colour stands in the model as last set or worked out: kept while that is the
    /// colour, so a grey or black keeps its hue and saturation.
    place: ColourPlace,
    /// The panel picks the background's colour (the Layers panel's Background color row tapped):
    /// titled so, with a tick that closes it, and its colour sent to no brush (the interface takes
    /// it to the background); and the tick tapped.
    pub background: bool,
    pub background_done: bool,
}

impl Default for Colours {
    fn default() -> Colours {
        let kept = crate::files::kept_colours().unwrap_or(([0.59, 1.0, 1.0], [0.0; 3]));
        Colours {
            // as last left, else the blue a first start has (as Android's)
            hsv: kept.0,
            other: kept.1,
            on_mask: false,
            secondary: [1.0, 1.0, 1.0],
            history: ColourHistory::new(),
            palettes: Palettes::open(crate::files::app_dir().join("palettes.json").to_string_lossy().into_owned()),
            pickers: Pickers::default(),
            mode: Mode::default(),
            harmony: HarmonyKind::default(),
            harmony_menu: crate::menu::Menu::default(),
            hex: None,
            palette_cards: false,
            renaming_palette: None,
            palette_menu: crate::menu::Menu::default(),
            palette_menu_for: None,
            new_palette_menu: crate::menu::Menu::default(),
            new_palette_button: Rect::NOTHING,
            swatch_press: None,
            short_dots: Rect::NOTHING,
            scrolled: 0.0,
            short_menu: crate::menu::Menu::default(),
            floating: None,
            floating_grab: None,
            floating_rgb: false,
            model_button: Rect::NOTHING,
            model_menu: crate::menu::Menu::default(),
            model_chosen: None,
            place: ColourPlace::default(),
            background: false,
            background_done: false,
        }
    }
}

impl Colours {
    /// The colour model the disc, Classic and the Value sliders work in.
    pub fn model(&self) -> ColourModel {
        self.pickers.model
    }

    pub fn set_model(&mut self, model: ColourModel) {
        self.pickers.model = model;
    }

    /// The colour in the colour model: as last set while it is still the colour, else worked
    /// out afresh (a grey keeping the hue it had).
    fn model_coords(&self) -> [f32; 3] {
        self.place.coords_of(self.model(), self.hsv, to_hex(self.rgb()))
    }

    /// The colour set to `coords` in the colour model.
    fn set_model_coords(&mut self, coords: [f32; 3]) {
        let model = self.model();
        self.hsv = model.to_hsv(coords, self.hsv[0]);
        if model != ColourModel::Hsb {
            self.place.keep_coords(model, coords, to_hex(self.rgb()));
        }
    }

    /// The disc picker in `rect`, in the colour model; whether it changed the colour.
    pub(crate) fn model_disc(&mut self, ui: &Ui, rect: Rect) -> bool {
        let mut coords = self.model_coords();
        let changed = self.pickers.disc(ui, &mut coords, rect);
        if changed {
            self.set_model_coords(coords);
        }
        changed
    }

    /// Whether Classic shows a plugin's triangle in place of its square.
    pub fn set_triangle(&mut self, on: bool) {
        self.pickers.triangle = on;
    }

    /// The picker plugin on, if any, and the session its picture is drawn with: Classic's square is
    /// its.
    pub fn set_picker(&mut self, picker: Option<(Arc<Plugins>, Arc<CanvasSession>)>) {
        self.pickers.picker = picker;
    }

    /// Classic's square in `rect`, in the colour model; whether it changed the colour.
    pub(crate) fn model_square(&mut self, ui: &Ui, rect: Rect) -> bool {
        let mut coords = self.model_coords();
        let changed = self.pickers.square(ui, &mut coords, rect);
        if changed {
            self.set_model_coords(coords);
        }
        changed
    }

    /// Classic's slider for channel `channel` (0 the hue) in `rect`, in the colour model; whether
    /// it changed the colour.
    pub(crate) fn model_slider(&mut self, ui: &Ui, rect: Rect, id: &str, channel: usize) -> bool {
        let model = self.model();
        let coords = self.model_coords();
        let tracks = classic_tracks(model, coords);
        let Some((place, colours)) = tracks.get(channel) else { return false };
        let Some(to) = slider_row(ui, rect, id, *place, colours) else { return false };
        let slid = model.slid(coords, channel, to);
        let changed = slid != coords;
        if changed {
            self.set_model_coords(slid);
        }
        changed
    }

    /// How much room HSY′'s sliders take under the disc (none for another model).
    fn luma_sliders_height(&self) -> f32 {
        if self.model() == ColourModel::Hsy { SLIDER_ROW * 2.0 + 4.0 } else { 0.0 }
    }

    /// HSY′'s saturation and luma sliders, from `top` across `body`; whether they changed the
    /// colour.
    fn luma_sliders(&mut self, ui: &Ui, body: Rect, top: f32) -> bool {
        let model = ColourModel::Hsy;
        let coords = self.model_coords();
        let mut changed = None;
        for (i, letter) in [(1usize, "S"), (2, "Y\u{2032}")] {
            let row = Rect::from_min_size(
                pos2(body.left() + 8.0, top + SLIDER_ROW * (i - 1) as f32),
                vec2(body.width() - 16.0, SLIDER_ROW),
            );
            let ink = argb(PANEL_TITLE);
            text(
                ui.painter(),
                pos2(row.left() + 13.0, row.center().y),
                Align2::LEFT_CENTER,
                letter,
                15.0,
                Weight::Regular,
                ink,
            );
            let value = coords.get(i).copied().unwrap_or(0.0);
            let shown = format!("{}%", (value * 100.0).round() as i32);
            text(
                ui.painter(),
                pos2(row.right() - 10.0, row.center().y),
                Align2::RIGHT_CENTER,
                &shown,
                15.0,
                Weight::Regular,
                ink,
            );
            let gradient: Vec<Color32> = model.along(coords, i, 12).into_iter().map(|c| srgb(of_hex(c))).collect();
            let track = Rect::from_min_max(pos2(row.left() + 40.0, row.top()), pos2(row.right() - 56.0, row.bottom()));
            if let Some(to) = slider_row(ui, track, ("luma slider", i), value, &gradient) {
                changed = Some((i, to));
            }
        }
        let Some((i, to)) = changed else { return false };
        let mut c = coords;
        if let Some(slot) = c.get_mut(i) {
            *slot = to;
        }
        self.set_model_coords(c);
        true
    }

    /// The colour model's button: its name in a small box, a tap on which opens its menu.
    fn model_button(&mut self, ui: &Ui, rect: Rect) {
        self.model_button = rect;
        ui.painter().rect_filled(rect, 6, argb(MODEL_BUTTON));
        let ink = argb(PANEL_TITLE);
        text(
            ui.painter(),
            pos2(rect.left() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            self.model().label(),
            13.0,
            Weight::Regular,
            ink,
        );
        // a small downward chevron, as a menu's button has
        let c = pos2(rect.right() - 9.0, rect.center().y);
        let stroke = egui::Stroke::new(1.4, ink);
        ui.painter().line_segment([c + vec2(-3.5, -1.8), c + vec2(0.0, 1.8)], stroke);
        ui.painter().line_segment([c + vec2(0.0, 1.8), c + vec2(3.5, -1.8)], stroke);
        if ui.interact(rect, ui.scope_id().with("colour model"), Sense::click()).clicked() {
            self.model_menu.open();
        }
    }

    /// The colour models as a menu under their button, the one in use ticked.
    fn model_menu(&mut self, ctx: &egui::Context, look: &mut Look, backdrops: &mut Vec<f32>) {
        let width = 160.0;
        let rect = Rect::from_min_size(
            pos2(self.model_button.right() - width, self.model_button.bottom() + 6.0),
            vec2(width, 9.0 * 2.0 + 38.0 * ColourModel::ALL.len() as f32),
        );
        let glass =
            crate::menu::Glass { fill: HARMONY_MENU_FILL, edge: None, frost: false, corner: 18.0, shadow: false };
        let current = self.model();
        let mut chosen = None;
        self.model_menu.show(ctx, "colour models", rect, self.model_button.center_bottom(), glass, backdrops, |ui| {
            for (i, model) in ColourModel::ALL.into_iter().enumerate() {
                let row = Rect::from_min_size(
                    pos2(rect.left(), rect.top() + 9.0 + 38.0 * i as f32),
                    vec2(rect.width(), 38.0),
                );
                let ink = Color32::WHITE;
                text(
                    ui.painter(),
                    pos2(row.left() + 18.0, row.center().y),
                    Align2::LEFT_CENTER,
                    model.label(),
                    16.0,
                    Weight::Regular,
                    ink,
                );
                if model == current {
                    look.icon(ui, &icons::TICK, pos2(row.right() - 16.0 - 9.0, row.center().y), 18.0, ink);
                }
                if ui.interact(row, ui.scope_id().with(("colour model", i)), Sense::click()).clicked() {
                    chosen = Some(model);
                }
            }
        });
        if let Some(model) = chosen {
            self.set_model(model);
            self.model_chosen = Some(model);
            self.model_menu.close();
        }
    }

    /// The colour painted with, sRGB.
    pub fn rgb(&self) -> [f32; 3] {
        hsv_to_rgb(self.hsv)
    }

    pub fn set_rgb(&mut self, session: &CanvasSession, rgb: u32) {
        take_rgb(&mut self.hsv, rgb);
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

    /// A mask became current, or stopped being: the colour trades places with the other kind's,
    /// as masks paint with a colour of their own.
    pub fn follow_mask(&mut self, session: &CanvasSession, on_mask: bool) {
        if on_mask == self.on_mask {
            return;
        }
        self.on_mask = on_mask;
        std::mem::swap(&mut self.hsv, &mut self.other);
        self.send(session);
    }

    /// The brush colour and masks', whichever is in use.
    pub fn brush_and_mask(&self) -> ([f32; 3], [f32; 3]) {
        if self.on_mask { (self.other, self.hsv) } else { (self.hsv, self.other) }
    }

    pub fn send(&self, session: &CanvasSession) {
        // picking the background's colour, the brush keeps its own
        if self.background {
            return;
        }
        let [r, g, b] = self.rgb();
        session.set_color(r, g, b);
        let [r, g, b] = self.secondary;
        session.set_secondary_color(r, g, b);
    }

    /// Shows the panel hanging from `button`; returns where it is (points). In a short window it
    /// shows the history or the palette, as `history_shown` says (its menu sets it).
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        drop: &mut ColourDrop,
        button: Rect,
        history_shown: &mut bool,
        backdrops: &mut Vec<f32>,
    ) -> Rect {
        let screen = ctx.content_rect();
        let (left, arrow_x) = panel::place(ctx, PANEL_WIDTH, button.center().x);
        let top = screen.top() + pc::BAR_HEIGHT;
        // the header (with Harmony's kind under its title), the picker, the history (44 and 36),
        // the palette (44 and three rows of 36) and the tabs (70); short, the history or the
        // palette (34 and its rows) and the pickers tighter
        let short = screen.height() < SHORT_SCREEN;
        let shape = Shape::of(short, screen.height());
        let has_palette = (self.palettes.default_palette() as usize) < self.palettes.list().len();
        let history = *history_shown || !has_palette;
        let header = match self.mode {
            Mode::Harmony => HEADER_HEIGHT + 14.0,
            // short, the disc reaches up into the header's last 7
            Mode::Disc if short => HEADER_HEIGHT - 7.0,
            _ => HEADER_HEIGHT,
        };
        let picker_height = match self.mode {
            Mode::Disc if short => PICKER + 3.0 + self.luma_sliders_height(),
            Mode::Disc => 8.0 + PICKER + 17.0 + self.luma_sliders_height(),
            Mode::Palettes => 8.0 + PICKER + 17.0,
            Mode::Classic => shape.square + SLIDER_ROW * 3.0,
            Mode::Harmony => shape.harmony_top + shape.harmony_radius * 2.0 + shape.harmony_gap + SLIDER_ROW,
            Mode::Value => shape.value_group * 2.0 + 6.0 + 6.0 + 44.0 + if short { 2.0 } else { 0.0 },
        };
        let bottom = match (short, history) {
            (false, _) => 80.0 + 44.0 + 108.0,
            (true, true) => 44.0 + 36.0,
            (true, false) => 34.0 + 108.0,
        };
        let whole = if self.mode == Mode::Palettes {
            panel::ARROW_HEIGHT + HEADER_HEIGHT + 36.0 + 8.0 + shape.palette_list + 70.0
        } else {
            panel::ARROW_HEIGHT + header + picker_height + bottom + 8.0 + 70.0
        };
        let height = whole.min(screen.height() - pc::BAR_HEIGHT - PANEL_MARGIN);
        let overflow = whole - height;
        self.scrolled = self.scrolled.min(overflow);
        let mut hex_row = Rect::NOTHING;
        let rect = Rect::from_min_size(pos2(left, top), vec2(PANEL_WIDTH, height));
        egui::Area::new(egui::Id::unique("colour panel")).fixed_pos(rect.min).order(egui::Order::Foreground).show(
            ctx,
            |ui| {
                ui.allocate_rect(rect, Sense::click());
                panel::frame(ui, look, "colour frame", rect, arrow_x, backdrops);
                let body = panel::body(rect);
                if self.background {
                    // titled so on every page, with a tick 12 in from the right that closes it
                    panel::header(ui, look, body, "Background", 20.0, false, false);
                    let tick =
                        Rect::from_min_size(pos2(body.right() - 12.0 - 44.0, body.top() + 19.0), vec2(44.0, 44.0));
                    ui.painter().circle_filled(tick.center(), 22.0, argb(pc::ACCENT));
                    look.icon(ui, &icons::DONE_TICK, tick.center(), 40.0, Color32::WHITE);
                    if ui.interact(tick, ui.scope_id().with("background done"), Sense::click()).clicked() {
                        self.background_done = true;
                    }
                } else if self.mode == Mode::Palettes {
                    panel::header(ui, look, body, "Palettes", 20.0, false, false);
                    // the plus: a new palette, from a file or a picture, or the examples back
                    let plus =
                        Rect::from_min_size(pos2(body.right() - 17.0 - 40.0, body.top() + 12.0), vec2(40.0, 41.0));
                    look.icon(ui, &icons::PLUS_MEDIUM, plus.center(), 41.0, argb(panel::PANEL_GLYPH));
                    self.new_palette_button = plus;
                    if ui.interact(plus, ui.scope_id().with("new palette"), Sense::click()).clicked() {
                        self.new_palette_menu.open();
                    }
                } else {
                    panel::header(ui, look, body, "Colors", 20.0, false, false);
                }
                // the current colour (edged) and the second, which swaps with it when tapped
                let swatch = vec2(43.0, 25.0);
                let second = Rect::from_min_size(pos2(body.right() - 20.0 - swatch.x, body.top() + 21.5), swatch);
                let first = second.translate(vec2(-(swatch.x + 7.0), 0.0));
                if self.mode != Mode::Palettes && !self.background {
                    // the colour model, for the pages that work in one
                    if self.mode != Mode::Harmony {
                        let model =
                            Rect::from_min_size(pos2(first.left() - 8.0 - 70.0, first.top()), vec2(70.0, swatch.y));
                        self.model_button(ui, model);
                    }
                    ui.painter().rect_filled(first, 6, srgb(self.rgb()));
                    ui.painter().rect_stroke(
                        first,
                        6,
                        egui::Stroke::new(1.0, argb(SWATCH_EDGE)),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().rect_filled(second, 6, srgb(self.secondary));
                    if ui.interact(second, ui.scope_id().with("swap"), Sense::click()).clicked() {
                        self.swap(session);
                    }
                }
                let grabber = Rect::from_center_size(pos2(body.center().x, body.top() + 6.0), vec2(40.0, 4.0));
                ui.painter().rect_filled(grabber, 2, argb(GRABBER));
                // dragged out by its grabber, the panel floats as a window of its own (not while
                // it picks the background's colour)
                let handle = Rect::from_center_size(pos2(body.center().x, body.top() + 15.0), vec2(150.0, 30.0));
                let held = ui.interact(handle, ui.scope_id().with("undock"), Sense::drag());
                if !self.background
                    && held.dragged()
                    && let (Some(p), Some(from)) = (held.interact_pointer_pos(), ui.input(|i| i.pointer.press_origin()))
                    && p.distance(from) > UNDOCK_DRAG
                {
                    self.float_at(p - floating::GRAB);
                }

                let top = body.top() + header;
                if self.mode == Mode::Harmony {
                    self.harmony_kind_button(ui, body);
                }
                // on a window too short for it all the picker scrolls, by the wheel or a drag
                // between its controls, the tabs staying
                let clip = ui.clip_rect();
                let top = if overflow > 0.0 {
                    let view = Rect::from_min_max(pos2(body.left(), top), pos2(body.right(), body.bottom() - 70.0));
                    let dragged = ui.interact(view, ui.scope_id().with("scroll"), Sense::drag()).drag_delta().y;
                    let wheel = if ui.rect_contains_pointer(view) {
                        ui.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta.y))
                    } else {
                        0.0
                    };
                    self.scrolled = (self.scrolled - dragged - wheel).clamp(0.0, overflow);
                    ui.set_clip_rect(clip.intersect(view));
                    top - self.scrolled
                } else {
                    top
                };
                match self.mode {
                    Mode::Palettes => self.palettes_view(ui, look, session, body, top, shape.palette_list),
                    Mode::Disc => {
                        let at = pos2(body.left() + 6.0, top + if short { 0.0 } else { 8.0 });
                        let picker = Rect::from_min_size(at, vec2(PICKER, PICKER));
                        if self.model_disc(ui, picker) {
                            self.send(session);
                        }
                        // HSY′: its saturation and luma on sliders under the wheel
                        if self.model() == ColourModel::Hsy && self.luma_sliders(ui, body, picker.bottom() + 4.0) {
                            self.send(session);
                        }
                    }
                    Mode::Classic => self.classic(ui, session, body, top, shape.square),
                    Mode::Harmony => self.harmony(ui, session, body, top, shape),
                    Mode::Value => hex_row = self.value(ui, session, body, top, shape),
                }

                if self.mode != Mode::Palettes && short {
                    // the history or the palette, as the "•••" beside its title says
                    let y = top + picker_height;
                    let band = if history { 44.0 } else { 34.0 };
                    if history {
                        let at = pos2(body.left() + 21.0, y + 13.0);
                        text(ui.painter(), at, Align2::LEFT_TOP, "History", 14.0, Weight::SemiBold, argb(PANEL_TITLE));
                        self.history_strip(ui, session, body, y + band);
                    } else {
                        let at = pos2(body.left() + 21.0, y + 8.0);
                        let name = self.palette_name();
                        text(ui.painter(), at, Align2::LEFT_TOP, &name, 14.0, Weight::SemiBold, argb(PANEL_TITLE));
                        self.palette_rows(ui, session, drop, rect, body, y + band);
                    }
                    // three dots in a 44 x 34 box 9 in from the right, level with the title
                    let dots =
                        Rect::from_min_size(pos2(body.right() - 9.0 - 44.0, y + (band - 34.0) / 2.0), vec2(44.0, 34.0));
                    for i in -1..=1 {
                        ui.painter().circle_filled(dots.center() + vec2(i as f32 * 6.5, 0.0), 1.8, argb(PANEL_TITLE));
                    }
                    self.short_dots = dots;
                    if ui.interact(dots, ui.scope_id().with("short menu"), Sense::click()).clicked() {
                        self.short_menu.open();
                    }
                } else if self.mode != Mode::Palettes {
                    let mut y = top + picker_height;
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
                    self.history_strip(ui, session, body, y);
                    y += 36.0;
                    let name = self.palette_name();
                    text(
                        ui.painter(),
                        pos2(body.left() + 21.0, y + 13.0),
                        Align2::LEFT_TOP,
                        &name,
                        14.0,
                        Weight::SemiBold,
                        ink,
                    );
                    y += 44.0;
                    self.palette_rows(ui, session, drop, rect, body, y);
                }
                ui.set_clip_rect(clip);
                let tabs = Rect::from_min_max(pos2(body.left(), body.bottom() - 70.0), body.max);
                if let Some(mode) = mode_tabs(ui, look, tabs, self.mode) {
                    self.mode = mode;
                    self.hex = None;
                    self.scrolled = 0.0;
                }
            },
        );
        if self.mode == Mode::Harmony {
            self.harmony_menu(ctx, look, rect, backdrops);
        }
        if self.mode != Mode::Palettes {
            self.model_menu(ctx, look, backdrops);
        }
        if self.mode == Mode::Value
            && let Some(rgb) = hex_pad(ctx, look, &mut self.hex, hex_row)
        {
            self.set_rgb(session, rgb);
        }
        if self.mode == Mode::Palettes {
            self.palette_menu(ctx, look, backdrops);
            self.new_palette_menu(ctx, look, backdrops);
        } else if short {
            self.short_menu(ctx, look, history, history_shown, backdrops);
        }
        rect
    }

    /// The last ten colours painted with, in a strip 36 tall from `y`; one tapped is taken.
    fn history_strip(&mut self, ui: &Ui, session: &CanvasSession, body: Rect, y: f32) {
        let strip = Rect::from_min_size(pos2(body.left() + 8.0, y), vec2(body.width() - 16.0, 36.0));
        let kept = self.history.colours();
        let history: Vec<Option<u32>> = (0..HISTORY_SHOWN).map(|i| kept.get(i).copied()).collect();
        if let Some((Some(c), _)) = swatch_row(ui, strip, &history, EMPTY_SWATCH, [8.0, 8.0, 8.0, 8.0], "history") {
            self.set_rgb(session, c);
        }
    }

    /// The active palette's name.
    fn palette_name(&self) -> String {
        let index = self.palettes.default_palette() as usize;
        self.palettes.list().into_iter().nth(index).map(|p| p.name).unwrap_or_default()
    }

    /// The active palette's three rows of ten from `y`, in the panel at `panel`: a filled swatch
    /// tapped gives its colour, held is taken out, and dragged out fills where it is let go (off
    /// this panel); an empty one tapped keeps the current colour there.
    fn palette_rows(
        &mut self,
        ui: &Ui,
        session: &CanvasSession,
        drop: &mut ColourDrop,
        panel: Rect,
        body: Rect,
        mut y: f32,
    ) {
        let index = self.palettes.default_palette();
        let colors = self.palettes.list().into_iter().nth(index as usize).map(|p| p.colors).unwrap_or_default();
        for row in 0..3 {
            let cells: Vec<Option<u32>> = (0..10).map(|c| colors.get(row * 10 + c).copied().flatten()).collect();
            let line = Rect::from_min_size(pos2(body.left() + 8.0, y), vec2(body.width() - 16.0, 36.0));
            let corners = match row {
                0 => [8.0, 8.0, 0.0, 0.0],
                2 => [0.0, 0.0, 8.0, 8.0],
                _ => [0.0; 4],
            };
            for (i, (cell, rgb)) in paint_row(ui, line, &cells, EMPTY_SLOT, corners).into_iter().enumerate() {
                let slot = (row * 10 + i) as u32;
                let id = ui.scope_id().with(("palette swatch", slot));
                match rgb {
                    Some(c) => match drop.source(ui, session, id, cell, c, true, true, Some(panel)) {
                        Some(Touch::Tap) => self.set_rgb(session, c),
                        Some(Touch::Hold) => self.palettes.set_swatch(index, slot, None),
                        Some(Touch::Dragged) | None => {}
                    },
                    // not the swatch's id: a hold that took a colour out doesn't tap the slot it
                    // leaves when let go
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
    }

    /// The short panel's menu under its "•••": Color history or Active palette (a radio each,
    /// setting `history_shown`), and with the history shown, Clear history. Its right edge is 16
    /// left of the dots, its top 32 under their box's.
    fn short_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        history: bool,
        history_shown: &mut bool,
        backdrops: &mut Vec<f32>,
    ) {
        let height = 6.0 + 40.0 * 2.0 + if history { 11.0 + 1.0 + 10.0 + 40.0 + 9.5 } else { 7.0 };
        let dots = self.short_dots;
        let rect = Rect::from_min_size(pos2(dots.left() - 246.0, dots.top() + 32.0), vec2(SHORT_MENU_WIDTH, height));
        let glass = crate::menu::Glass {
            fill: crate::menu::OVER_PANEL,
            edge: Some(SHORT_MENU_EDGE),
            frost: false,
            corner: SHORT_MENU_CORNER,
            shadow: false,
        };
        let mut chosen = None;
        self.short_menu.show(ctx, "short colour menu", rect, rect.right_top(), glass, backdrops, |ui| {
            for (i, (label, on)) in [("Color history", true), ("Active palette", false)].into_iter().enumerate() {
                let row = Rect::from_min_size(
                    pos2(rect.left(), rect.top() + 6.0 + 40.0 * i as f32),
                    vec2(rect.width(), 40.0),
                );
                let at = pos2(row.left() + 20.0, row.center().y);
                text(ui.painter(), at, Align2::LEFT_CENTER, label, 18.0, Weight::Regular, argb(crate::menu::INK));
                crate::actions::radio(ui, look, pos2(row.right() - 19.0 - 9.0, row.center().y), on == history);
                if ui.interact(row, ui.scope_id().with(("short menu", i)), Sense::click()).clicked() {
                    chosen = Some(Some(on));
                }
            }
            if history {
                let rule = Rect::from_min_max(
                    pos2(rect.left() + 16.0, rect.top() + 6.0 + 80.0 + 11.0),
                    pos2(rect.right() - 15.0, rect.top() + 6.0 + 80.0 + 12.0),
                );
                ui.painter().rect_filled(rule, 0, argb(pc::GLASS_MENU_EDGE));
                let row = Rect::from_min_size(pos2(rect.left(), rule.bottom() + 10.0), vec2(rect.width(), 40.0));
                let danger = argb(crate::menu::DANGER);
                // a 41 frame centred 29.5 in
                look.icon(ui, &icons::SWEEP, pos2(row.left() + 29.5, row.center().y), 41.0, danger);
                let at = pos2(row.left() + 9.0 + 41.0 + 2.0, row.center().y);
                text(ui.painter(), at, Align2::LEFT_CENTER, "Clear history", 18.0, Weight::Regular, danger);
                if ui.interact(row, ui.scope_id().with("short clear"), Sense::click()).clicked() {
                    chosen = Some(None);
                }
            }
        });
        match chosen {
            Some(Some(on)) => *history_shown = on,
            Some(None) => self.history.clear(),
            None => return,
        }
        self.short_menu.close();
    }
}

/// The pictures the pickers draw with, kept while they still fit, and what a drag on the disc
/// moves. Each set of pickers (the panel's, a compact picker's) has its own.
#[derive(Default)]
pub struct Pickers {
    /// The colour model the disc works in.
    pub model: ColourModel,
    /// The disc's picture for its hue, and the ring's (each with its size in pixels and the model
    /// it shows).
    disc: Option<(u32, usize, ColourModel, TextureHandle)>,
    ring: Option<(usize, ColourModel, TextureHandle)>,
    /// What a drag that started on the disc picker moves: the ring (true) or the disc.
    on_ring: Option<bool>,
    /// Classic's square for its hue (with the model it shows), and Harmony's wheel for its
    /// brightness.
    square: Option<(u32, usize, ColourModel, TextureHandle)>,
    wheel: Option<(u32, usize, TextureHandle)>,
    /// A plugin has Classic show a triangle in place of its square (HSB), or a picker plugin draws
    /// it (its picture and where touches land); the picture for its hue and size, and whether a
    /// picker plugin's.
    pub triangle: bool,
    pub picker: Option<(Arc<Plugins>, Arc<CanvasSession>)>,
    triangle_picture: Option<(u32, usize, bool, TextureHandle)>,
    /// Harmony's hue, saturation and brightness (Display P3) as dragged to, with the colour they
    /// made: kept while that is the colour, not worked out again from it (a dark or greyish
    /// colour's would wander, and black's be lost).
    harmony_held: Option<(u32, [f32; 3])>,
}

impl Pickers {
    /// The disc picker in `rect` (364 square, or smaller and scaled alike): a hue ring (radii 124
    /// to 182, its hues running anticlockwise from the right) round the saturation/brightness disc
    /// (radius 105). Returns whether it changed `hsv`.
    pub fn disc(&mut self, ui: &Ui, hsv: &mut [f32; 3], rect: Rect) -> bool {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let k = rect.width() / PICKER;
        let (disc_radius, ring_inner, ring_outer) = (DISC_RADIUS * k, RING_INNER * k, RING_OUTER * k);
        let centre = rect.center();
        let before = *hsv;
        let response = ui.interact(rect, ui.scope_id().with("disc"), Sense::click_and_drag());
        if let Some(p) = response.interact_pointer_pos() {
            let d = p - centre;
            let ring = *self.on_ring.get_or_insert(d.length() > (disc_radius + ring_inner) / 2.0);
            if ring {
                let turn = ((-d.y).atan2(d.x) / std::f32::consts::TAU + 1.0) % 1.0;
                hsv[0] = self.model.ring_hue(turn);
            } else {
                // (HSY′'s disc is Krita's wheel: hue round it and saturation out, worked out on
                // linear light at the colour's luma; the ring and sliders on the encoded values)
                *hsv = self.model.disc_pick(*hsv, [d.x / disc_radius, d.y / disc_radius]);
            }
        }
        if !response.is_pointer_button_down_on() {
            self.on_ring = None;
        }
        // the ring, drawn once for its size
        let side = (rect.width() * ppp).round() as usize;
        let model = self.model;
        if self.ring.as_ref().is_none_or(|(at, m, _)| *at != side || *m != model) {
            let image = ring_image(side, ppp * k, model);
            self.ring = Some((side, model, ctx.load_texture("hue ring", image, TextureOptions::LINEAR)));
        }
        // the disc is drawn for its hue (HSY′'s, all hues, for its linear luma)
        let hue_key = model.disc_key(*hsv);
        let disc_side = (disc_radius * 2.0 * ppp).round() as usize;
        if self.disc.as_ref().is_none_or(|(h, at, m, _)| *h != hue_key || *at != disc_side || *m != model) {
            let image = picture_image(&model.disc_picture(*hsv, disc_side as u32));
            self.disc =
                Some((hue_key, disc_side, model, ctx.load_texture("colour disc", image, TextureOptions::LINEAR)));
        }
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let painter = ui.painter();
        if let Some((_, _, t)) = &self.ring {
            painter.image(t.id(), rect, uv, Color32::WHITE);
        }
        if let Some((_, _, _, t)) = &self.disc {
            painter.image(
                t.id(),
                Rect::from_center_size(centre, vec2(disc_radius * 2.0, disc_radius * 2.0)),
                uv,
                Color32::WHITE,
            );
        }
        // the ring's knob sits on the middle of the ring, where the hue is
        let turn = model.ring_turn(hsv[0]) * std::f32::consts::TAU;
        let mid = (ring_inner + ring_outer) / 2.0;
        knob(ui, centre + vec2(turn.cos() * mid, -turn.sin() * mid), 26.2 * k, srgb(model.pure(hsv[0])));
        let [x, y] = model.disc_point(*hsv);
        knob(ui, centre + vec2(x, y) * disc_radius, 10.75, srgb(model.to_rgb(*hsv)));
        *hsv != before
    }

    /// Classic's saturation/brightness square in `square` (rounded 10), saturation across and
    /// brightness up; returns whether it changed `hsv`.
    pub fn square(&mut self, ui: &Ui, hsv: &mut [f32; 3], square: Rect) -> bool {
        let model = self.model;
        // a plugin's: its picker, or a triangle, in its place
        if (self.triangle || self.picker.is_some()) && model == ColourModel::Hsb {
            return self.triangle(ui, hsv, square);
        }
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let before = *hsv;
        let response = ui.interact(square, ui.scope_id().with("classic square"), Sense::click_and_drag());
        if let Some(p) = response.interact_pointer_pos() {
            hsv[1] = ((p.x - square.left()) / square.width()).clamp(0.0, 1.0);
            hsv[2] = 1.0 - ((p.y - square.top()) / square.height()).clamp(0.0, 1.0);
        }
        let hue_key = (hsv[0] * 10_000.0).round() as u32;
        let side = (square.width() * ppp).round() as usize;
        if self.square.as_ref().is_none_or(|(h, at, m, _)| *h != hue_key || *at != side || *m != model) {
            let image = if model == ColourModel::Hsb {
                square_image(hsv[0], side)
            } else {
                model_square_image(model, hsv[0], side)
            };
            let texture = ctx.load_texture("classic square", image, TextureOptions::LINEAR);
            self.square = Some((hue_key, side, model, texture));
        }
        if let Some((_, _, _, t)) = &self.square {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            let shape = egui::epaint::RectShape::filled(square, 10, Color32::WHITE).with_texture(t.id(), uv);
            ui.painter().add(egui::Shape::Rect(shape));
        }
        // the knob may hang over the square's edge
        let at = pos2(square.left() + hsv[1] * square.width(), square.top() + (1.0 - hsv[2]) * square.height());
        knob(ui, at, 10.75, srgb(model.to_rgb(*hsv)));
        *hsv != before
    }

    /// A plugin's picker in Classic's square `area`, as big as fits: a picker plugin's picture,
    /// touches and knob, or else a triangle (saturation and brightness between the pure hue at the
    /// right, white at the top left and black at the bottom left); returns whether it changed `hsv`.
    fn triangle(&mut self, ui: &Ui, hsv: &mut [f32; 3], area: Rect) -> bool {
        let side = area.width().min(area.height());
        let rect = Rect::from_center_size(area.center(), vec2(side, side));
        let ctx = ui.ctx().clone();
        let before = *hsv;
        let picker = self.picker.clone();
        let pick = |x: f32, y: f32| match &picker {
            Some((plugins, _)) => plugins.picker_pick(x, y),
            None => Some(procraft_core::triangle_sv(x, y)),
        };
        let response = ui.interact(rect, ui.scope_id().with("classic triangle"), Sense::click_and_drag());
        if let Some(p) = response.interact_pointer_pos()
            && let Some([s, v]) = pick((p.x - rect.left()) / side, (p.y - rect.top()) / side).as_deref()
        {
            hsv[1] = *s;
            hsv[2] = *v;
        }
        let hue_key = (hsv[0] * 10_000.0).round() as u32;
        let px = (side * ctx.pixels_per_point()).round() as usize;
        let plugin = picker.is_some();
        let stale = |(h, at, by, _): &(u32, usize, bool, TextureHandle)| *h != hue_key || *at != px || *by != plugin;
        if self.triangle_picture.as_ref().is_none_or(stale) {
            let rgba = match &picker {
                Some((_, session)) => session.picker_picture(hsv[0], px as u32),
                None => Some(procraft_core::triangle_picture(hsv[0], px as u32)),
            };
            // (a picture that couldn't be drawn shows none; its plugin goes off)
            self.triangle_picture = rgba.filter(|rgba| rgba.len() == px * px * 4).map(|rgba| {
                let image = egui::ColorImage::from_rgba_premultiplied([px, px], &rgba);
                (hue_key, px, plugin, ctx.load_texture("classic triangle", image, TextureOptions::LINEAR))
            });
        }
        if let Some((_, _, _, t)) = &self.triangle_picture {
            ui.painter().image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        let at = match &picker {
            Some((plugins, _)) => plugins.picker_point(hsv[1], hsv[2]),
            None => Some(procraft_core::triangle_point(hsv[1], hsv[2])),
        };
        if let Some([x, y]) = at.as_deref() {
            knob(ui, pos2(rect.left() + x * side, rect.top() + y * side), 10.75, srgb(ColourModel::Hsb.to_rgb(*hsv)));
        }
        *hsv != before
    }

    /// Harmony's wheel in `wheel` (hue round it, saturation out, in Display P3) with a knob for
    /// the colour and one for each of `kind`'s companions, for a canvas whose colours are in P3
    /// (`p3`) or sRGB; returns whether it changed `hsv`.
    pub fn harmony(&mut self, ui: &Ui, hsv: &mut [f32; 3], wheel: Rect, kind: HarmonyKind, p3: bool) -> bool {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let rgb = to_hex(hsv_to_rgb(*hsv));
        let [h, sat, v] = self.harmony_hsv(rgb, p3);
        let before = *hsv;
        let response = ui.interact(wheel, ui.scope_id().with("harmony wheel"), Sense::click_and_drag());
        if let Some(p) = response.interact_pointer_pos() {
            let u = ((p.x - wheel.left()) / wheel.width()).clamp(0.0, 1.0) * 2.0 - 1.0;
            let w = ((p.y - wheel.top()) / wheel.height()).clamp(0.0, 1.0) * 2.0 - 1.0;
            let turn = ((-w).atan2(u) / std::f32::consts::TAU + 1.0) % 1.0;
            self.harmony_set(hsv, [turn, u.hypot(w).min(1.0), v], p3);
        }
        // the brightness and the space in one key
        let v_key = (v * 10_000.0).round() as u32 * 2 + u32::from(p3);
        let side = (wheel.width() * ppp).round() as usize;
        if self.wheel.as_ref().is_none_or(|(k, at, _)| *k != v_key || *at != side) {
            let image = egui::ColorImage::from_rgba_premultiplied([side, side], &harmony_wheel(v, side as u32, p3));
            self.wheel = Some((v_key, side, ctx.load_texture("harmony wheel", image, TextureOptions::LINEAR)));
        }
        if let Some((_, _, t)) = &self.wheel {
            ui.painter().image(t.id(), wheel, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        let r = wheel.width() / 2.0;
        let at = |turn: f32| {
            wheel.center()
                + vec2((turn * std::f32::consts::TAU).cos() * sat * r, -(turn * std::f32::consts::TAU).sin() * sat * r)
        };
        for t in kind.turns() {
            let hue = ((h + t / 360.0) % 1.0 + 1.0) % 1.0;
            knob(ui, at(hue), 10.75, srgb(of_hex(p3_hsv_to_rgb(hue, sat, v, p3))));
        }
        knob(ui, at(h), 14.5, srgb(of_hex(rgb)));
        *hsv != before
    }

    /// Harmony's brightness slider (the colour's value in Display P3, the canvas's colours in P3
    /// or not as `p3` says); returns whether it changed `hsv`.
    pub fn harmony_brightness(&mut self, ui: &Ui, rect: Rect, hsv: &mut [f32; 3], p3: bool) -> bool {
        let [h, sat, v] = self.harmony_hsv(to_hex(hsv_to_rgb(*hsv)), p3);
        let Some(b) = slider_row(ui, rect, "harmony brightness", v, &[Color32::BLACK, Color32::WHITE]) else {
            return false;
        };
        let before = *hsv;
        self.harmony_set(hsv, [h, sat, b], p3);
        *hsv != before
    }

    /// Harmony's hue, saturation and brightness for colour `rgb`: as dragged to while it is that
    /// colour, else worked out from it.
    fn harmony_hsv(&self, rgb: u32, p3: bool) -> [f32; 3] {
        match self.harmony_held {
            Some((held, p3_hsv)) if held == rgb => p3_hsv,
            _ => p3_hsv_of(rgb, p3),
        }
    }

    /// `hsv` set to the colour of Harmony's `p3_hsv`, which is kept with it.
    fn harmony_set(&mut self, hsv: &mut [f32; 3], p3_hsv: [f32; 3], p3: bool) {
        let [h, s, v] = p3_hsv;
        take_rgb(hsv, p3_hsv_to_rgb(h, s, v, p3));
        self.harmony_held = Some((to_hex(hsv_to_rgb(*hsv)), p3_hsv));
    }
}

impl Colours {
    /// Classic: the saturation/brightness square (344, rounded 10), then sliders for the hue,
    /// saturation and brightness, all in the colour model (its third channel for the brightness).
    /// Short, the square is `side` tall (still 344 wide).
    fn classic(&mut self, ui: &Ui, session: &CanvasSession, body: Rect, top: f32, side: f32) {
        let square = Rect::from_min_size(pos2(body.left() + 16.0, top), vec2(SQUARE, side));
        let mut changed = self.model_square(ui, square);
        let row = |i: f32| {
            Rect::from_min_size(
                pos2(body.left() + 20.0, square.bottom() + SLIDER_ROW * i),
                vec2(body.width() - 40.0, SLIDER_ROW),
            )
        };
        changed |= self.model_slider(ui, row(0.0), "classic hue", 0);
        changed |= self.model_slider(ui, row(1.0), "classic saturation", 1);
        changed |= self.model_slider(ui, row(2.0), "classic brightness", 2);
        if changed {
            self.send(session);
        }
    }

    /// Harmony's kind under the title; a tap lists the kinds.
    fn harmony_kind_button(&mut self, ui: &Ui, body: Rect) {
        let at = pos2(body.left() + 20.0, body.top() + 50.0 + 14.0);
        let shown =
            text(ui.painter(), at, Align2::LEFT_CENTER, self.harmony.label(), 17.0, Weight::Regular, argb(PANEL_TITLE));
        let button = Rect::from_min_max(pos2(shown.left(), body.top() + 50.0), pos2(shown.right(), body.top() + 78.0));
        if ui.interact(button, ui.scope_id().with("harmony kind"), Sense::click()).clicked() {
            self.harmony_menu.open();
        }
    }

    /// Harmony's kinds: a menu left of the panel, its top level with the wheel; the kind shown
    /// ticked.
    fn harmony_menu(&mut self, ctx: &egui::Context, look: &mut Look, panel: Rect, backdrops: &mut Vec<f32>) {
        let body = panel::body(panel);
        let rect = Rect::from_min_size(
            pos2(body.left() + 20.0 - 204.0, body.top() + 50.0 + 36.0),
            vec2(195.0, 9.0 * 2.0 + 38.0 * HarmonyKind::ALL.len() as f32),
        );
        self.harmony_menu_in(ctx, look, rect, backdrops);
    }

    /// Harmony's kinds as a menu at `rect`, the kind shown ticked.
    fn harmony_menu_in(&mut self, ctx: &egui::Context, look: &mut Look, rect: Rect, backdrops: &mut Vec<f32>) {
        let glass =
            crate::menu::Glass { fill: HARMONY_MENU_FILL, edge: None, frost: false, corner: 24.0, shadow: false };
        let current = self.harmony;
        let mut chosen = None;
        self.harmony_menu.show(ctx, "harmony kinds", rect, rect.right_top(), glass, backdrops, |ui| {
            for (i, kind) in HarmonyKind::ALL.into_iter().enumerate() {
                let row = Rect::from_min_size(
                    pos2(rect.left(), rect.top() + 9.0 + 38.0 * i as f32),
                    vec2(rect.width(), 38.0),
                );
                let ink = Color32::WHITE;
                text(
                    ui.painter(),
                    pos2(row.left() + 20.0, row.center().y),
                    Align2::LEFT_CENTER,
                    kind.label(),
                    17.0,
                    Weight::Regular,
                    ink,
                );
                if kind == current {
                    look.icon(ui, &icons::TICK, pos2(row.right() - 18.0 - 9.0, row.center().y), 18.0, ink);
                }
                if ui.interact(row, ui.scope_id().with(("harmony", i)), Sense::click()).clicked() {
                    chosen = Some(kind);
                }
            }
        });
        if let Some(kind) = chosen {
            self.harmony = kind;
            self.harmony_menu.close();
        }
    }

    /// Harmony: a 298 wheel (hue round it, saturation out, in Display P3) with a knob for the
    /// colour and one for each of its companions, then a brightness slider.
    fn harmony(&mut self, ui: &Ui, session: &CanvasSession, body: Rect, top: f32, shape: Shape) {
        let r = shape.harmony_radius;
        let wheel = Rect::from_min_size(
            pos2(body.left() + PANEL_WIDTH / 2.0 - r, top + shape.harmony_top),
            vec2(r * 2.0, r * 2.0),
        );
        let p3 = colours_in_p3(session.profile());
        let mut changed = self.pickers.harmony(ui, &mut self.hsv, wheel, self.harmony, p3);
        let row = Rect::from_min_size(
            pos2(body.left() + 20.0, wheel.bottom() + shape.harmony_gap),
            vec2(body.width() - 40.0, SLIDER_ROW),
        );
        changed |= self.pickers.harmony_brightness(ui, row, &mut self.hsv, p3);
        if changed {
            self.send(session);
        }
    }

    /// Value: the hue, saturation and brightness, then the red, green and blue, each a number
    /// slider; then the hex code, a tap on which opens the keypad. Returns the hex row.
    fn value(&mut self, ui: &Ui, session: &CanvasSession, body: Rect, top: f32, shape: Shape) -> Rect {
        let left = body.left() + 8.0;
        let width = body.width() - 16.0;
        let rgb = to_hex(self.rgb());
        let [r, g, b] = [rgb >> 16 & 0xFF, rgb >> 8 & 0xFF, rgb & 0xFF];
        let (group_height, gap) = (shape.value_group, shape.value_gap);
        // the first group in the colour model (HSB's as it was; another's gradients worked out)
        let model = self.model();
        let coords = self.model_coords();
        let letters = model.letters();
        let tracks = classic_tracks(model, coords);
        // OKLab's sliders are its own lightness, a and b (a and b ±OKLAB_AB across them)
        let lab = oklab_of(self.rgb());
        let lab_tracks = oklab_tracks(lab);
        let lab_track = |i: usize| -> Option<(f32, Vec<Color32>)> {
            lab_tracks.get(i).map(|t| (t.place, t.colours.iter().map(|&c| srgb(of_hex(c))).collect()))
        };
        for (k, group_top) in [top, top + group_height + 6.0].into_iter().enumerate() {
            let group = Rect::from_min_size(pos2(left, group_top), vec2(width, group_height));
            ui.painter().rect_filled(group, 10, argb(VALUE_GROUP));
            for i in 0..3 {
                let y = group_top + shape.value_top + (44.0 + gap) * i as f32;
                let row = Rect::from_min_size(pos2(left, y), vec2(width, 44.0));
                let (letter, value, colours, shown): (&str, f32, Vec<Color32>, String) = match (k, i) {
                    (0, 0) if model == ColourModel::Oklab => {
                        let (place, colours) = lab_track(0).unwrap_or_default();
                        ("L", place, colours, format!("{}", (lab[0] * 100.0).round() as i32))
                    }
                    (0, j) if model == ColourModel::Oklab => {
                        let v = lab.get(j).copied().unwrap_or(0.0);
                        let letter = if j == 1 { "a" } else { "b" };
                        let (place, colours) = lab_track(j).unwrap_or_default();
                        (letter, place, colours, format!("{v:+.2}"))
                    }
                    (0, j) => {
                        let (place, colours) = tracks.get(j).cloned().unwrap_or_default();
                        let shown = match j {
                            0 => format!("{}\u{b0}", (coords[0] * 360.0).round() as i32),
                            _ => format!("{}%", (coords.get(j).copied().unwrap_or(0.0) * 100.0).round() as i32),
                        };
                        (letters.get(j).copied().unwrap_or(""), place, colours, shown)
                    }
                    (_, 0) => ("R", r as f32 / 255.0, vec![rgb_of(0, g, b), rgb_of(255, g, b)], r.to_string()),
                    (_, 1) => ("G", g as f32 / 255.0, vec![rgb_of(r, 0, b), rgb_of(r, 255, b)], g.to_string()),
                    _ => ("B", b as f32 / 255.0, vec![rgb_of(r, g, 0), rgb_of(r, g, 255)], b.to_string()),
                };
                let ink = argb(PANEL_TITLE);
                text(
                    ui.painter(),
                    pos2(row.left() + 13.0, row.center().y),
                    Align2::LEFT_CENTER,
                    letter,
                    15.0,
                    Weight::Regular,
                    ink,
                );
                text(
                    ui.painter(),
                    pos2(row.left() + 304.0 + 22.5, row.center().y),
                    Align2::CENTER_CENTER,
                    &shown,
                    15.0,
                    Weight::Regular,
                    ink,
                );
                let track =
                    Rect::from_center_size(pos2(row.left() + 32.0 + 132.0, row.center().y), vec2(264.0, SLIDER_ROW));
                let Some(to) = slider_row(ui, track, ("value", k, i), value, &colours) else { continue };
                let byte = (to * 255.0).round() as u32;
                match (k, i) {
                    (0, _) if model == ColourModel::Oklab => {
                        let [h, s, v] = rgb_to_hsv(oklab_to_rgb(oklab_slid(lab, i, to)));
                        self.hsv = [if s > 0.0 { h } else { self.hsv[0] }, s, v];
                    }
                    (0, _) => self.set_model_coords(model.slid(coords, i, to)),
                    (_, 0) => self.set_rgb(session, byte << 16 | g << 8 | b),
                    (_, 1) => self.set_rgb(session, r << 16 | byte << 8 | b),
                    _ => self.set_rgb(session, r << 16 | g << 8 | byte),
                }
                if k == 0 {
                    self.send(session);
                }
            }
        }
        // the hex code
        let row = Rect::from_min_size(pos2(left, top + group_height * 2.0 + 6.0 + 6.0), vec2(width, 44.0));
        hex_row(ui, row, rgb, &mut self.hex, HEX_ROW, HEX_FIELD);
        row
    }
}

/// The hex code's row, filled `fill`: "Hexadecimal", and the code in a field filled `field_fill`, a
/// tap on which opens the keypad (`typed`: the digits typed on it since; none while it is shut).
pub(crate) fn hex_row(
    ui: &Ui,
    row: Rect,
    rgb: u32,
    typed: &mut Option<String>,
    fill: impl Shade,
    field_fill: impl Shade,
) {
    ui.painter().rect_filled(row, 10, argb(fill));
    let ink = argb(PANEL_TITLE);
    text(
        ui.painter(),
        pos2(row.left() + 13.0, row.center().y),
        Align2::LEFT_CENTER,
        "Hexadecimal",
        16.0,
        Weight::Regular,
        ink,
    );
    let field = Rect::from_min_size(pos2(row.right() - 8.0 - 84.5, row.center().y - 14.0), vec2(84.5, 28.0));
    ui.painter().rect_filled(field, 7, argb(field_fill));
    if typed.is_some() {
        ui.painter().rect_stroke(field, 7, egui::Stroke::new(1.0, argb(pc::ACCENT)), egui::StrokeKind::Inside);
    }
    let code = typed.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| format!("{rgb:06X}"));
    text(ui.painter(), field.center(), Align2::CENTER_CENTER, &format!("#{code}"), 14.0, Weight::Regular, ink);
    if ui.interact(field, ui.scope_id().with("hex field"), Sense::click()).clicked() {
        *typed = Some(String::new());
    }
}

/// The hex keypad, left of the panel, its middle level with the hex `row`: D E F, A B C, 7 8 9,
/// 4 5 6, 1 2 3, then a wide 0 and backspace, while `typed` holds the digits typed on it. Six
/// digits typed give the colour, returned (a seventh starts a new code); a tap off it closes it.
/// The keyboard's digits type on it too.
pub(crate) fn hex_pad(ctx: &egui::Context, look: &mut Look, typed: &mut Option<String>, row: Rect) -> Option<u32> {
    typed.as_ref()?;
    let screen = ctx.content_rect();
    let x = (row.left() - 12.0 - HEX_PAD.0).max(screen.left());
    let y = (row.center().y - HEX_PAD.1 / 2.0).clamp(screen.top(), (screen.bottom() - HEX_PAD.1).max(screen.top()));
    let pad = Rect::from_min_size(pos2(x, y), vec2(HEX_PAD.0, HEX_PAD.1));
    let mut key: Option<char> = None;
    let mut back = false;
    let mut close = false;
    egui::Area::new(egui::Id::unique("hex pad")).fixed_pos(screen.min).order(egui::Order::Tooltip).show(ctx, |ui| {
        // allocated, so the canvas under it takes no touch
        if ui.allocate_rect(screen, Sense::click_and_drag()).clicked() {
            close = true;
        }
        ui.interact(pad, ui.scope_id().with("pad"), Sense::click_and_drag());
        ui.painter().rect_filled(pad, 30, argb(HEX_PAD_FILL));
        ui.painter().rect_stroke(pad, 30, egui::Stroke::new(1.0, argb(HEX_PAD_EDGE)), egui::StrokeKind::Inside);
        let inner = pad.shrink(10.0);
        let rows = ["DEF", "ABC", "789", "456", "123"];
        for (j, keys) in rows.iter().enumerate() {
            for (i, c) in keys.chars().enumerate() {
                let rect = Rect::from_min_size(inner.min + vec2(81.0 * i as f32, 54.0 * j as f32), vec2(77.0, 50.0));
                if hex_key(ui, rect, ("hex key", c)) {
                    key = Some(c);
                }
                let ink = argb(HEX_KEY_INK);
                text(ui.painter(), rect.center(), Align2::CENTER_CENTER, &c.to_string(), 21.0, Weight::Regular, ink);
            }
        }
        let last = inner.top() + 54.0 * 5.0;
        let zero = Rect::from_min_size(pos2(inner.left(), last), vec2(158.0, 50.0));
        if hex_key(ui, zero, ("hex key", '0')) {
            key = Some('0');
        }
        text(ui.painter(), zero.center(), Align2::CENTER_CENTER, "0", 21.0, Weight::Regular, argb(HEX_KEY_INK));
        let backspace = Rect::from_min_size(pos2(inner.left() + 162.0, last), vec2(77.0, 50.0));
        back = hex_key(ui, backspace, "hex back");
        look.icon(ui, &icons::BACKSPACE, backspace.center(), 24.0, argb(HEX_BACK_INK));
    });
    ctx.input(|i| {
        for e in &i.events {
            match e {
                egui::Event::Text(t) => {
                    key = t.chars().find(char::is_ascii_hexdigit).map(|c| c.to_ascii_uppercase()).or(key)
                }
                egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => back = true,
                egui::Event::Key { key: egui::Key::Enter, pressed: true, .. } => close = true,
                _ => {}
            }
        }
    });
    close |= crate::ui::escape(ctx);
    if close {
        *typed = None;
        return None;
    }
    let code = typed.as_mut()?;
    if back {
        code.pop();
    }
    let c = key?;
    if code.len() >= 6 {
        code.clear();
    }
    code.push(c);
    if code.len() == 6 { u32::from_str_radix(code, 16).ok() } else { None }
}

impl Colours {
    /// The hex keypad is open: the keyboard types on it, not the app's shortcuts.
    pub fn takes_keys(&self) -> bool {
        self.mode == Mode::Value && self.hex.is_some()
    }

    /// A menu of the panel's is up: a press elsewhere only closes it.
    pub fn modal(&self) -> bool {
        self.palette_menu.is_open()
            || self.new_palette_menu.is_open()
            || self.short_menu.is_open()
            || self.harmony_menu.is_open()
            || self.model_menu.is_open()
            || self.takes_keys()
    }

    /// Palettes: the Compact/Cards switch, then every palette as a card, the default ticked: its
    /// name (tapped, renamed in place), its menu, and its swatches as a grid or as named tiles.
    fn palettes_view(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        session: &CanvasSession,
        body: Rect,
        top: f32,
        list_height: f32,
    ) {
        let switch = Rect::from_min_size(pos2(body.left() + 8.5, top), vec2(body.width() - 17.0, 36.0));
        ui.painter().rect_filled(switch, 9, argb(SET_CELL));
        let inner = switch.shrink(2.0);
        let half = inner.width() / 2.0;
        for (k, (icon, label)) in
            [(&icons::CARD_COMPACT, "Compact"), (&icons::CARD_STACK, "Cards")].into_iter().enumerate()
        {
            let cards = k == 1;
            let on = self.palette_cards == cards;
            let cell =
                Rect::from_min_size(pos2(inner.left() + half * k as f32, inner.top()), vec2(half, inner.height()));
            if on {
                ui.painter().rect_filled(cell, 7, argb(pc::ACCENT));
            }
            let ink = if on { Color32::WHITE } else { argb(PANEL_TITLE) };
            let weight = if on { Weight::SemiBold } else { Weight::Regular };
            let words = ui.painter().layout_no_wrap(label.into(), weight.font(15.0), ink).size().x;
            let left = cell.center().x - (21.0 + 6.0 + words) / 2.0;
            look.icon(ui, icon, pos2(left + 10.5, cell.center().y), 21.0, ink);
            text(ui.painter(), pos2(left + 27.0, cell.center().y), Align2::LEFT_CENTER, label, 15.0, weight, ink);
            if ui.interact(cell, ui.scope_id().with(("palette view", k)), Sense::click()).clicked() {
                self.palette_cards = cards;
            }
        }
        let list = Rect::from_min_size(pos2(body.left(), switch.bottom() + 8.0), vec2(body.width(), list_height));
        let palettes = self.palettes.list();
        let default = self.palettes.default_palette();
        let pad = if self.palette_cards { 8.0 } else { 8.5 };
        ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("palettes")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for (i, p) in palettes.iter().enumerate() {
                        let filled: Vec<(usize, u32)> =
                            p.colors.iter().enumerate().filter_map(|(slot, c)| c.map(|c| (slot, c))).collect();
                        let body_height = if self.palette_cards {
                            filled.len().div_ceil(3) as f32 * PALETTE_TILE
                        } else {
                            36.0 * 3.0
                        };
                        let (outer, _) = ui.allocate_exact_size(vec2(list.width(), 44.0 + body_height), Sense::hover());
                        let card = outer.shrink2(vec2(pad, 0.0));
                        self.palette_card(ui, look, session, i as u32, p, &filled, card, i as u32 == default);
                        ui.add_space(8.0);
                    }
                });
        });
    }

    /// One palette's card at `card`.
    #[allow(clippy::too_many_arguments)]
    fn palette_card(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        session: &CanvasSession,
        i: u32,
        p: &procraft_core::PaletteInfo,
        filled: &[(usize, u32)],
        card: Rect,
        default: bool,
    ) {
        let painter = ui.painter_at(card);
        painter.rect_filled(card, 12, argb(SET_CELL));
        let ink = argb(PANEL_TITLE);
        let mid = card.top() + 22.0;
        if default {
            let disc = pos2(card.left() + 11.5 + 10.0, mid);
            painter.circle_filled(disc, 10.0, argb(pc::ACCENT));
            look.icon(ui, &icons::TICK, disc, 14.0, Color32::WHITE);
        }
        let name_left = card.left() + if default { 11.5 + 20.0 + 4.0 } else { 12.0 };
        let more = Rect::from_min_size(pos2(card.right() - 8.5 - 30.0, card.top()), vec2(30.0, 44.0));
        if let Some((_, typed, focused)) = self.renaming_palette.as_mut().filter(|(r, _, _)| *r == i) {
            let field = Rect::from_min_max(pos2(name_left, mid - 10.0), pos2(more.left() - 4.0, mid + 10.0));
            let mut output = egui::TextEdit::singleline(typed)
                .font(Weight::Regular.font(14.0))
                .text_color(ink)
                .frame(egui::Frame::NONE)
                .margin(vec2(0.0, 0.0))
                .desired_width(field.width())
                .show(&mut ui.new_child(UiBuilder::new().max_rect(field)));
            if !*focused {
                output.response.request_focus();
                let all = egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(typed.chars().count()),
                );
                output.state.cursor.set_char_range(Some(all));
                output.state.store(ui.ctx(), output.response.id);
                *focused = true;
            } else if output.response.lost_focus() {
                let name = typed.clone();
                self.renaming_palette = None;
                self.palettes.rename(i, name);
            }
        } else {
            let shown = text(&painter, pos2(name_left, mid), Align2::LEFT_CENTER, &p.name, 14.0, Weight::SemiBold, ink);
            // a tap on the name renames it in place
            if ui.interact(shown, ui.scope_id().with(("palette name", i)), Sense::click()).clicked() {
                self.renaming_palette = Some((i, p.name.clone(), false));
            }
        }
        look.icon(ui, &icons::MORE, more.center(), 20.0, ink);
        if ui.interact(more, ui.scope_id().with(("palette more", i)), Sense::click()).clicked() {
            self.palette_menu_for = Some((i, more));
            self.palette_menu.open();
        }
        let below = Rect::from_min_max(pos2(card.left(), card.top() + 44.0), card.max);
        if self.palette_cards {
            // named tiles, three across; dark lettering on bright colours
            for (k, &(slot, rgb)) in filled.iter().enumerate() {
                let tile = Rect::from_min_size(
                    below.min + vec2(PALETTE_TILE * (k % 3) as f32, PALETTE_TILE * (k / 3) as f32),
                    vec2(PALETTE_TILE, PALETTE_TILE),
                );
                let bottom_corners = k / 3 == (filled.len() - 1) / 3;
                let corner = |on: bool| if on { 12 } else { 0 };
                let corners = egui::CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: corner(bottom_corners && k % 3 == 0),
                    se: corner(bottom_corners && (k % 3 == 2 || k + 1 == filled.len())),
                };
                painter.rect_filled(tile, corners, srgb(of_hex(rgb)));
                let bright = rgb_to_hsv(of_hex(rgb))[2] >= 0.85;
                let lettering = if bright { Color32::BLACK } else { Color32::WHITE };
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
                let galley = painter.layout_job(job);
                // its baseline 11.5 above the bottom, as measured
                painter.galley(pos2(tile.left() + 8.0, tile.bottom() - 8.0 - galley.size().y), galley, lettering);
                self.swatch_touch(ui, session, tile, i, slot, Some(rgb));
            }
        } else {
            let cells: Vec<Option<u32>> = p.colors.iter().take(30).copied().collect();
            for row in 0..3 {
                let line =
                    Rect::from_min_size(pos2(below.left(), below.top() + 36.0 * row as f32), vec2(below.width(), 36.0));
                let corners = match row {
                    0 => [8.0, 8.0, 0.0, 0.0],
                    2 => [0.0, 0.0, 12.0, 12.0],
                    _ => [0.0; 4],
                };
                let part = cells.get(row * 10..row * 10 + 10).unwrap_or(&[]);
                for (c, (cell, rgb)) in paint_row(ui, line, part, EMPTY_SLOT, corners).into_iter().enumerate() {
                    self.swatch_touch(ui, session, cell, i, row * 10 + c, rgb);
                }
            }
        }
    }

    /// A palette's swatch at `rect`: tapped, its colour is painted with (an empty one keeps the
    /// current colour); held, it is taken out.
    fn swatch_touch(
        &mut self,
        ui: &Ui,
        session: &CanvasSession,
        rect: Rect,
        palette: u32,
        slot: usize,
        rgb: Option<u32>,
    ) {
        let response = ui.interact(rect, ui.scope_id().with(("palette swatch", palette, slot)), Sense::click());
        let held = self.swatch_press.is_some_and(|(p, s, _, done)| p == palette && s == slot && done);
        if response.is_pointer_button_down_on() && rgb.is_some() {
            let (p, s, since, done) =
                self.swatch_press.get_or_insert((palette, slot, std::time::Instant::now(), false));
            if (*p, *s) == (palette, slot) && !*done {
                let t = since.elapsed();
                if t >= SWATCH_HOLD {
                    *done = true;
                    self.palettes.set_swatch(palette, slot as u32, None);
                } else {
                    ui.ctx().request_repaint_after(SWATCH_HOLD - t);
                }
            }
        }
        if response.clicked() && !held {
            match rgb {
                Some(c) => self.set_rgb(session, c),
                None => self.palettes.set_swatch(palette, slot as u32, Some(to_hex(self.rgb()))),
            }
        }
        if !response.is_pointer_button_down_on()
            && self.swatch_press.is_some_and(|(p, s, _, _)| p == palette && s == slot)
        {
            self.swatch_press = None;
        }
    }

    /// A palette's menu, left of its button: Set as active (unless it is), Duplicate, Share;
    /// Delete (unless it is the only one).
    fn palette_menu(&mut self, ctx: &egui::Context, look: &mut Look, backdrops: &mut Vec<f32>) {
        let Some((i, button)) = self.palette_menu_for else { return };
        let ink = crate::menu::INK;
        let mut entries = Vec::new();
        if self.palettes.default_palette() != i {
            entries.push(crate::menu::Entry::Item(&icons::MENU_CHECK_CIRCLE, "Set as active", ink));
        }
        entries.push(crate::menu::Entry::Item(&icons::MENU_DUPLICATE, "Duplicate", ink));
        entries.push(crate::menu::Entry::Item(&icons::MENU_SHARE, "Share", ink));
        if self.palettes.list().len() > 1 {
            entries.push(crate::menu::DIVIDER);
            entries.push(crate::menu::Entry::Item(&icons::MENU_TRASH, "Delete", crate::menu::DANGER));
        }
        let size = vec2(crate::menu::LIBRARY_WIDTH, crate::menu::entries_height(&entries));
        let rect = Rect::from_min_size(pos2(button.left() - 5.0 - size.x, button.top() + 25.0), size);
        let mut tapped = None;
        self.palette_menu.show(
            ctx,
            "palette menu",
            rect,
            rect.right_top(),
            crate::menu::OVER_A_PANEL,
            backdrops,
            |ui| {
                tapped = crate::menu::entries(ui, look, rect, &entries);
            },
        );
        let Some(crate::menu::Entry::Item(_, label, _)) = tapped.and_then(|t| entries.get(t)) else { return };
        self.palette_menu.close();
        match *label {
            "Set as active" => self.palettes.choose_default(i),
            "Duplicate" => self.palettes.duplicate(i),
            "Share" => {
                let name = self.palettes.list().get(i as usize).map(|p| p.name.clone()).unwrap_or_default();
                if let Some(bytes) = self.palettes.swatches_file(i) {
                    crate::files::save_file(&bytes, &name, "swatches", "Palette");
                }
            }
            _ => self.palettes.delete(i),
        }
    }

    /// The palettes' plus menu, hanging below it, their right edges in line: a new palette, one from
    /// a file (a palette's, or a picture's colours) or a picture, and the example palettes back.
    /// (Taking a photo for one is the tablet's.)
    fn new_palette_menu(&mut self, ctx: &egui::Context, look: &mut Look, backdrops: &mut Vec<f32>) {
        let ink = crate::menu::INK;
        let entries = [
            crate::menu::Entry::Item(&MENU_GRID, "Create new palette", ink),
            crate::menu::Entry::Item(&icons::MENU_FOLDER, "New from file", ink),
            crate::menu::Entry::Item(&icons::MENU_PHOTO, "New from photos", ink),
            crate::menu::DIVIDER,
            crate::menu::Entry::Item(&icons::MENU_RESTORE, "Restore example palettes", ink),
        ];
        let plus = self.new_palette_button;
        let size = vec2(crate::menu::LIBRARY_WIDTH, crate::menu::entries_height(&entries));
        let rect = Rect::from_min_size(pos2(plus.left() - size.x, plus.bottom()), size);
        let mut tapped = None;
        self.new_palette_menu.show(
            ctx,
            "new palette menu",
            rect,
            rect.right_top(),
            crate::menu::OVER_A_PANEL,
            backdrops,
            |ui| {
                tapped = crate::menu::entries(ui, look, rect, &entries);
            },
        );
        let Some(i) = tapped else { return };
        self.new_palette_menu.close();
        match i {
            0 => self.palettes.add(),
            1 => {
                let Some((bytes, name)) = crate::files::pick_file("New palette from a file") else { return };
                if !self.palettes.import_file(bytes.clone(), Some(name)) {
                    self.palette_from_picture(&bytes);
                }
            }
            2 => {
                let Some((bytes, _)) = crate::files::pick_file("New palette from a picture") else { return };
                self.palette_from_picture(&bytes);
            }
            _ => self.palettes.restore_examples(),
        }
    }

    /// A palette of a picture's main colours (a PNG's), light to dark, added at the end.
    fn palette_from_picture(&mut self, bytes: &[u8]) {
        let Some(pixels) = crate::files::picture_pixels(bytes) else {
            crate::files::say("This picture can't be read.");
            return;
        };
        if !self.palettes.add_from_pixels(pixels) {
            crate::files::say("This picture can't be read.");
        }
    }
}

/// A key of the hex keypad at `rect`, lighter while held; true when tapped.
fn hex_key(ui: &Ui, rect: Rect, id: impl std::hash::Hash + std::fmt::Debug) -> bool {
    let response = ui.interact(rect, ui.scope_id().with(id), Sense::click());
    let fill = if response.is_pointer_button_down_on() { HEX_KEY_HELD } else { HEX_KEY_FILL };
    ui.painter().rect_filled(rect, 21, argb(fill));
    response.clicked()
}

/// A slider row (36 tall): a 4 tall track through `colours` and a pill showing the colour under
/// it; a press anywhere on the row moves the pill there. Returns the value set, while held.
pub(crate) fn slider_row(
    ui: &Ui,
    rect: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: f32,
    colours: &[Color32],
) -> Option<f32> {
    let response = ui.interact(rect, ui.scope_id().with(id), Sense::click_and_drag());
    let y = rect.center().y;
    let track = Rect::from_min_max(pos2(rect.left(), y - 2.0), pos2(rect.right(), y + 2.0));
    gradient(ui.painter(), track, colours);
    let pill = vec2(16.0, 10.0);
    let x = (rect.left() + value * rect.width()).clamp(rect.left() + pill.x / 2.0, rect.right() - pill.x / 2.0);
    ui.painter().rect_filled(Rect::from_center_size(pos2(x, y), pill), pill.y / 2.0, colour_along(colours, value));
    let p = response.interact_pointer_pos()?;
    Some(((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0))
}

/// A horizontal gradient through evenly spaced `colours` across `rect` (its ends rounded), mixed as
/// the colours are written, not as light.
fn gradient(painter: &egui::Painter, rect: Rect, colours: &[Color32]) {
    let (Some(&first), Some(&last)) = (colours.first(), colours.last()) else { return };
    let r = rect.height() / 2.0;
    painter.circle_filled(pos2(rect.left() + r, rect.center().y), r, first);
    painter.circle_filled(pos2(rect.right() - r, rect.center().y), r, last);
    let inner = rect.shrink2(vec2(r, 0.0));
    let steps = 64;
    let mut mesh = egui::Mesh::default();
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        let x = inner.left() + inner.width() * t;
        let c = colour_along(colours, t);
        mesh.colored_vertex(pos2(x, rect.top()), c);
        mesh.colored_vertex(pos2(x, rect.bottom()), c);
        if k > 0 {
            let i = (k * 2) as u32;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i, i + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The colour `t` (0..1) along evenly spaced `colours`, mixed as they are written.
fn colour_along(colours: &[Color32], t: f32) -> Color32 {
    let n = colours.len();
    if n == 0 {
        return Color32::TRANSPARENT;
    }
    let p = t.clamp(0.0, 1.0) * (n - 1) as f32;
    let i = (p as usize).min(n.saturating_sub(2));
    let (Some(a), Some(b)) = (colours.get(i), colours.get((i + 1).min(n - 1))) else { return Color32::TRANSPARENT };
    let f = p - i as f32;
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * f).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

pub(crate) fn rgb_of(r: u32, g: u32, b: u32) -> Color32 {
    Color32::from_rgb(r as u8, g as u8, b as u8)
}

/// Classic's square for `hue`, `side` pixels: white to the pure hue across, darkening to black
/// down, mixed as written.
fn square_image(hue: f32, side: usize) -> egui::ColorImage {
    let pure = srgb(hsv_to_rgb([hue, 1.0, 1.0]));
    let mut pixels = vec![Color32::BLACK; side * side];
    let step = (side.max(2) - 1) as f32;
    for (k, px) in pixels.iter_mut().enumerate() {
        let (x, y) = ((k % side) as f32 / step, (k / side) as f32 / step);
        let across = colour_along(&[Color32::WHITE, pure], x);
        let dark = 1.0 - y;
        let shade = |c: u8| (f32::from(c) * dark).round() as u8;
        *px = Color32::from_rgb(shade(across.r()), shade(across.g()), shade(across.b()));
    }
    egui::ColorImage::new([side, side], pixels)
}

/// A picker's picture from the core (premultiplied RGBA, square) as an image.
fn picture_image(rgba: &[u8]) -> egui::ColorImage {
    let side = ((rgba.len() / 4) as f64).sqrt() as usize;
    egui::ColorImage::from_rgba_premultiplied([side, side], rgba.get(..side * side * 4).unwrap_or_default())
}

/// Classic's square for a model other than HSB (the core's: small, as a hue drag redraws it every
/// frame), saturation across and the model's third channel up.
fn model_square_image(model: ColourModel, hue: f32, side: usize) -> egui::ColorImage {
    picture_image(&model.square_picture(hue, side as u32))
}

/// Classic's three sliders in `model` at `coords` (the core's): where each stands, and the colours
/// along it.
fn classic_tracks(model: ColourModel, coords: [f32; 3]) -> [(f32, Vec<Color32>); 3] {
    model.tracks(coords).map(|t| (t.place, t.colours.into_iter().map(|c| srgb(of_hex(c))).collect()))
}

/// A row of equal swatches in `rect` (its corners rounded as given); returns the one tapped, with
/// its place.
fn swatch_row(
    ui: &Ui,
    rect: Rect,
    cells: &[Option<u32>],
    empty: impl Shade,
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
pub(crate) fn paint_row(
    ui: &Ui,
    rect: Rect,
    cells: &[Option<u32>],
    empty: impl Shade,
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

/// The modes, as their tabs list them.
pub(crate) const MODE_TABS: [(&icons::Icon, &str, Mode); 5] = [
    (&icons::MODE_DISC, "Disc", Mode::Disc),
    (&icons::MODE_CLASSIC, "Classic", Mode::Classic),
    (&icons::MODE_HARMONY, "Harmony", Mode::Harmony),
    (&icons::MODE_VALUE, "Value", Mode::Value),
    (&icons::MODE_PALETTES, "Palettes", Mode::Palettes),
];

/// The mode tabs: Disc, Classic, Harmony, Value and Palettes.
fn mode_tabs(ui: &Ui, look: &mut Look, rect: Rect, mode: Mode) -> Option<Mode> {
    let w = rect.width() / MODE_TABS.len() as f32;
    let mut chosen = None;
    for (i, (icon, label, m)) in MODE_TABS.into_iter().enumerate() {
        let selected = m == mode;
        let tab = Rect::from_min_size(pos2(rect.left() + w * i as f32, rect.top()), vec2(w, rect.height()));
        if ui.interact(tab, ui.scope_id().with(("mode tab", i)), Sense::click()).clicked() {
            chosen = Some(m);
        }
        let x = rect.left() + w * (i as f32 + 0.5);
        // a 36 frame centred 27 down
        look.icon(
            ui,
            icon,
            pos2(x, rect.top() + 9.0 + 18.0),
            36.0,
            if selected { argb(pc::ACCENT) } else { argb(PANEL_TITLE) },
        );
        let ink = if selected { argb(pc::MENU_INK) } else { argb(PANEL_TITLE) };
        text(ui.painter(), pos2(x, rect.top() + 41.0), Align2::CENTER_TOP, label, 13.0, Weight::Medium, ink);
    }
    chosen
}

/// `hsv` set to `rgb` (0xRRGGBB); a grey keeps the hue it had.
pub(crate) fn take_rgb(hsv: &mut [f32; 3], rgb: u32) {
    let [h, s, v] = rgb_to_hsv(of_hex(rgb));
    *hsv = [if s > 0.0 { h } else { hsv[0] }, s, v];
}

/// `slot` set to `v`; whether that changed it.
pub(crate) fn set(slot: &mut f32, v: f32) -> bool {
    let changed = *slot != v;
    *slot = v;
    changed
}

/// The hue slider (its hues as the classic slider's); returns whether it changed `hsv`.
pub(crate) fn hue_slider(ui: &Ui, rect: Rect, id: &str, hsv: &mut [f32; 3]) -> bool {
    let hues: Vec<Color32> = SLIDER_HUES.iter().map(|h| srgb(hsv_to_rgb([h / 360.0, 1.0, 1.0]))).collect();
    slider_row(ui, rect, id, hue_position(HueControl::Slider, hsv[0]), &hues)
        .is_some_and(|v| set(&mut hsv[0], hue_at(HueControl::Slider, v)))
}

/// A colour knob: `fill` inside a thin white ring, over a soft shadow.
fn knob(ui: &Ui, at: Pos2, r: f32, fill: Color32) {
    let shadow = egui::epaint::Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(0x59) };
    ui.painter().add(shadow.as_shape(Rect::from_center_size(at, vec2(r * 2.0, r * 2.0)), r));
    ui.painter().circle_filled(at, r, fill);
    ui.painter().circle_stroke(at, r - 0.6, egui::Stroke::new(1.2, Color32::WHITE));
}

/// The hue ring, `side` pixels square (at `ppp` pixels a point), with soft edges, its hues as
/// `model` shows them at their purest.
fn ring_image(side: usize, ppp: f32, model: ColourModel) -> egui::ColorImage {
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
        *px = srgb(model.pure(model.ring_hue(turn))).gamma_multiply(cover);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harmony_keeps_its_hue_and_saturation_through_black() {
        let mut pickers = Pickers::default();
        let mut hsv = [0.6, 0.8, 0.9];
        let [h, s, _] = pickers.harmony_hsv(to_hex(hsv_to_rgb(hsv)), false);
        // its brightness down to black and up again
        pickers.harmony_set(&mut hsv, [h, s, 0.0], false);
        assert_eq!(to_hex(hsv_to_rgb(hsv)), 0);
        assert_eq!(pickers.harmony_hsv(0, false), [h, s, 0.0]);
        pickers.harmony_set(&mut hsv, [h, s, 0.9], false);
        let [h2, s2, _] = p3_hsv_of(to_hex(hsv_to_rgb(hsv)), false);
        assert!((h2 - h).abs() < 0.01 && (s2 - s).abs() < 0.02, "{h2} {s2} (was {h} {s})");
        // a colour set elsewhere is worked out afresh
        assert_eq!(pickers.harmony_hsv(0xFF0000, false), p3_hsv_of(0xFF0000, false));
    }

    #[test]
    fn classic_s_sliders_are_hsb_s_as_they_were_and_follow_another_model() {
        let [hue, saturation, brightness] = classic_tracks(ColourModel::Hsb, [0.3, 0.4, 0.5]);
        assert_eq!(hue.0, hue_position(HueControl::Slider, 0.3));
        let slider: Vec<Color32> = SLIDER_HUES.iter().map(|h| srgb(hsv_to_rgb([h / 360.0, 1.0, 1.0]))).collect();
        assert_eq!(hue.1, slider);
        assert_eq!(saturation, (0.4, vec![argb(SATURATION_GREY), srgb(hsv_to_rgb([0.3, 1.0, 1.0]))]));
        assert_eq!(brightness, (0.5, vec![Color32::BLACK, Color32::WHITE]));
        // the same colour stands elsewhere in OKLab, its hue evenly along the slider
        let coords = ColourModel::Oklab.of_rgb([0.8, 0.2, 0.1], 0.0);
        let [hue, saturation, lightness] = classic_tracks(ColourModel::Oklab, coords);
        assert_eq!((hue.0, saturation.0, lightness.0), (coords[0], coords[1], coords[2]));
        assert_eq!(lightness.1.first(), Some(&Color32::BLACK));
        assert_eq!(lightness.1.last(), Some(&Color32::WHITE));
        assert_ne!(coords, rgb_to_hsv([0.8, 0.2, 0.1]));
    }

    #[test]
    fn classic_s_square_shows_the_model() {
        let image = |model| model_square_image(model, 0.0, 16);
        for model in [ColourModel::Hls, ColourModel::Hsy, ColourModel::Oklab] {
            let square = image(model);
            let at = |x: usize, y: usize| square.pixels.get(y * 16 + x).copied();
            assert_eq!(at(0, 0), Some(Color32::WHITE), "{model:?}");
            assert_eq!(at(15, 15), Some(Color32::BLACK), "{model:?}");
        }
        // its picture stays small however big the square is drawn
        let most = procraft_core::MODEL_SQUARE as usize;
        assert_eq!(model_square_image(ColourModel::Oklab, 0.2, 4000).size, [most, most]);
        // HLS's middle row is the pure hue at the right, where HSB's top row has it
        let hls = image(ColourModel::Hls);
        let pure = hls.pixels.get(7 * 16 + 15).copied().unwrap_or_default();
        assert!(pure.r() > 220 && pure.g() < 40 && pure.b() < 40, "{pure:?}");
    }

    #[test]
    fn a_gradient_is_mixed_as_written_and_hues_go_round_their_stops() {
        assert_eq!(colour_along(&[Color32::BLACK, Color32::WHITE], 0.5), Color32::from_gray(128));
        assert_eq!(colour_along(&[Color32::BLACK, Color32::WHITE], 1.0), Color32::WHITE);
        for hue in [0.1, 0.35, 0.8] {
            let back = hue_at(HueControl::Slider, hue_position(HueControl::Slider, hue));
            assert!((back - hue).abs() < 1e-3, "{hue} came back {back}");
        }
    }

    #[test]
    fn picking_the_background_the_panel_says_so_and_leaves_the_brush_s_colour() {
        use egui::{Event, Modifiers, PointerButton, RawInput};
        let (ctx, mut look, mut drop) = (egui::Context::default(), Look::default(), ColourDrop::default());
        crate::look::install_fonts(&ctx);
        let session = CanvasSession::new(64, 64);
        let mut colours = Colours { mode: Mode::Disc, ..Colours::default() };
        colours.send(&session);
        let brush = session.color();
        colours.background = true;
        // a colour taken while it picks the background goes to no brush
        colours.set_rgb(&session, 0x3366CC);
        assert_eq!(session.color(), brush);
        assert_eq!(to_hex(colours.rgb()), 0x3366CC);
        // its tick, 12 in from the right, level with the title, closes it
        let button = Rect::from_min_size(pos2(1000.0, 10.0), vec2(40.0, 40.0));
        let (mut tick, mut history) = (Pos2::ZERO, true);
        let press = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        for step in 0..6 {
            let events = match step {
                2 => vec![Event::PointerMoved(tick)],
                3 => vec![press(tick, true)],
                4 => vec![press(tick, false)],
                _ => Vec::new(),
            };
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 1100.0))),
                time: Some(f64::from(step) / 60.0),
                events,
                ..RawInput::default()
            };
            let mut output = ctx.clone().run_ui(raw, |ui| {
                let shown =
                    colours.show(ui.ctx(), &mut look, &session, &mut drop, button, &mut history, &mut Vec::new());
                let body = panel::body(shown);
                tick = pos2(body.right() - 12.0 - 22.0, body.top() + 19.0 + 22.0);
            });
            output.textures_delta.clear();
        }
        assert!(colours.background_done);
        assert_eq!(session.color(), brush);
        // picking done, the brush's colour goes to the brush again
        colours.background = false;
        colours.set_rgb(&session, 0xCC3366);
        assert_ne!(session.color(), brush);
    }
}
